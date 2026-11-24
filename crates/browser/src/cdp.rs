// Part of Agent Record Adapter (see NOTICE).
//! The proxy's decisions about single DevTools messages, kept free of I/O so
//! they are tested without a browser.
//!
//! agent-browser talks to the browser through one browser-level WebSocket
//! with flattened sessions (`sessionId` on every page command), so every
//! page command passes here.

use serde_json::{Map, Value, json};

/// Commands that would bring the browser's window or tab to the front. The
/// proxy answers them itself with an empty success, so the person's window
/// stays in front. agent-browser sends `Page.bringToFront` after switching or
/// opening a tab and `Target.activateTarget` to revive a tab that stopped
/// answering (`cli/src/native/browser.rs`, `actions.rs`).
pub const SWALLOWED: &[&str] = &["Page.bringToFront", "Target.activateTarget"];

/// What the proxy does with one message from agent-browser.
#[derive(Debug, PartialEq)]
pub enum ClientIntercept {
    /// Send this text on to the browser.
    Forward(String),
    /// Do not send it; answer agent-browser with this text instead.
    Answer(String),
}

/// Decides what happens to `text`, a message agent-browser sent.
pub fn intercept_client_message(text: &str) -> ClientIntercept {
    let Ok(Value::Object(mut message)) = serde_json::from_str::<Value>(text) else {
        return ClientIntercept::Forward(text.to_owned());
    };
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    if SWALLOWED.contains(&method) {
        return ClientIntercept::Answer(empty_answer(&message));
    }
    if method == "Target.createTarget" {
        // A new tab or window opens behind the person's windows. Chrome's
        // `background` asks for exactly that; agent-browser leaves it unset.
        let params = message
            .entry("params")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(params) = params {
            params.insert("background".into(), Value::Bool(true));
            return ClientIntercept::Forward(Value::Object(message).to_string());
        }
    }
    ClientIntercept::Forward(text.to_owned())
}

/// The success answer, with no result, to a command: `{"id", "result": {}}`
/// plus the command's `sessionId` when it had one (flattened sessions answer
/// with it).
pub fn empty_answer(command: &Map<String, Value>) -> String {
    let mut answer = Map::new();
    answer.insert(
        "id".into(),
        command.get("id").cloned().unwrap_or(Value::Null),
    );
    answer.insert("result".into(), json!({}));
    if let Some(session) = command.get("sessionId") {
        answer.insert("sessionId".into(), session.clone());
    }
    Value::Object(answer).to_string()
}

/// The `sessionId` of a message, if it has one.
pub fn session_of(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    value.get("sessionId")?.as_str().map(str::to_owned)
}

/// The session of a page that was just attached (`Target.attachedToTarget`
/// for a target of type `page`, flattened sessions).
pub fn attached_page_session(event: &Value) -> Option<String> {
    if event.get("method")?.as_str()? != "Target.attachedToTarget" {
        return None;
    }
    let params = event.get("params")?;
    (params.get("targetInfo")?.get("type")?.as_str()? == "page")
        .then(|| params.get("sessionId")?.as_str().map(str::to_owned))
        .flatten()
}

/// The command that makes the page of `session` behave as the focused page
/// (focus and blur events, `document.hasFocus()`, text input) while its
/// window stays behind: Chrome's `Emulation.setFocusEmulationEnabled`, the
/// stand-in for the `Page.bringToFront` the proxy does not send.
pub fn focus_emulation(session: &str, id: i64) -> Value {
    json!({ "id": id, "method": "Emulation.setFocusEmulationEnabled",
            "params": { "enabled": true }, "sessionId": session })
}

/// Points the browser's discovery answers (`/json/version`, `/json/list`) at
/// the proxy: every `host:chrome_port` becomes `127.0.0.1:proxy_port`, in the
/// WebSocket URLs and in the DevTools front-end links alike.
pub fn rewrite_discovery_body(body: &str, chrome_port: u16, proxy_port: u16) -> String {
    let to = format!("127.0.0.1:{proxy_port}");
    ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .fold(body.to_owned(), |body, host| {
            body.replace(&format!("{host}:{chrome_port}"), &to)
        })
}

/// An action agent-browser sent that a recording keeps.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentAction {
    /// A mouse button went down at a point of the page (CSS pixels of the
    /// viewport), `count` clicks in a row.
    Click {
        x: f64,
        y: f64,
        button: String,
        count: u64,
    },
    /// A tap of a touch screen at a point of the page.
    Tap { x: f64, y: f64 },
    /// A key went down. `text` is what it types, if anything.
    Key {
        key: String,
        modifiers: u64,
        text: Option<String>,
    },
    /// Text typed into the focused field at once (`Input.insertText`).
    InsertText { text: String },
    /// A value written into the element `object_id` (agent-browser's `fill`
    /// of an input it sets through the value setter).
    SetValue { object_id: String, value: String },
    /// Options chosen in the element `object_id` (agent-browser's `select`
    /// of a `<select>` or an ARIA combobox/listbox), by their values or names.
    Select {
        object_id: String,
        values: Vec<String>,
    },
    /// The page was sent to an address.
    Navigate { url: String },
    /// Files were put in a file input.
    Upload { files: Vec<String> },
}

impl AgentAction {
    /// The text this action writes, if it writes any.
    pub fn written_text(&self) -> Option<&str> {
        match self {
            AgentAction::Key {
                text: Some(text), ..
            } => Some(text),
            AgentAction::InsertText { text } | AgentAction::SetValue { value: text, .. } => {
                Some(text)
            }
            _ => None,
        }
    }
}

/// The action `message` (sent by agent-browser) performs, if a recording
/// keeps it, with the page session it targets.
pub fn agent_action(message: &Value) -> Option<(AgentAction, Option<String>)> {
    let method = message.get("method")?.as_str()?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let session = message
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let text = |key: &str| params.get(key).and_then(Value::as_str).map(str::to_owned);
    let number = |key: &str| params.get(key).and_then(Value::as_f64);
    let action = match method {
        "Input.dispatchMouseEvent" if text("type").as_deref() == Some("mousePressed") => {
            AgentAction::Click {
                x: number("x")?,
                y: number("y")?,
                button: text("button").unwrap_or_else(|| "left".into()),
                count: params
                    .get("clickCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(1),
            }
        }
        "Input.dispatchTouchEvent" if text("type").as_deref() == Some("touchStart") => {
            let point = params.get("touchPoints")?.get(0)?;
            AgentAction::Tap {
                x: point.get("x")?.as_f64()?,
                y: point.get("y")?.as_f64()?,
            }
        }
        "Input.dispatchKeyEvent"
            if matches!(text("type").as_deref(), Some("keyDown" | "rawKeyDown")) =>
        {
            AgentAction::Key {
                key: text("key").or_else(|| text("code")).unwrap_or_default(),
                modifiers: params.get("modifiers").and_then(Value::as_u64).unwrap_or(0),
                text: text("text").filter(|t| !t.is_empty()),
            }
        }
        "Input.insertText" => AgentAction::InsertText {
            text: text("text")?,
        },
        "Page.navigate" => AgentAction::Navigate { url: text("url")? },
        "DOM.setFileInputFiles" => AgentAction::Upload {
            files: params
                .get("files")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        },
        "Runtime.callFunctionOn" if is_option_select(&params) => AgentAction::Select {
            object_id: text("objectId")?,
            values: params
                .get("arguments")?
                .get(0)?
                .get("value")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        },
        "Runtime.callFunctionOn" if is_value_setter(&params) => AgentAction::SetValue {
            object_id: text("objectId")?,
            value: params
                .get("arguments")?
                .get(0)?
                .get("value")?
                .as_str()?
                .to_owned(),
        },
        _ => return None,
    };
    Some((action, session))
}

/// Whether a `Runtime.callFunctionOn` is agent-browser's `select`
/// (`select_option` in `cli/src/native/interaction.rs`): a function of the
/// values (and how to match them) that works on a `<select>` or a listbox.
fn is_option_select(params: &Value) -> bool {
    params
        .get("functionDeclaration")
        .and_then(Value::as_str)
        .is_some_and(|code| {
            code.contains("function(vals, by)") && code.contains("HTMLSelectElement")
        })
}

/// Whether a `Runtime.callFunctionOn` is agent-browser's `fill` of an input
/// through the native value setter (`set_value_input` in
/// `cli/src/native/interaction.rs`), which takes the value as its argument.
fn is_value_setter(params: &Value) -> bool {
    params
        .get("functionDeclaration")
        .and_then(Value::as_str)
        .is_some_and(|code| code.contains(".prototype, 'value').set"))
}

/// The JavaScript function that describes an element for a recording, called
/// with the element as `this`. It names no value of a secret field: it only
/// says that the field is secure.
pub const DESCRIBE_FUNCTION: &str = r#"function() {
  let e = this;
  while (e && e.shadowRoot && e.shadowRoot.activeElement) { e = e.shadowRoot.activeElement; }
  if (!e || e.nodeType !== 1) { return { url: location.href, title: document.title }; }
  const attr = (n) => e.getAttribute(n) || '';
  const type = (e.type || '').toLowerCase();
  const autocomplete = attr('autocomplete').toLowerCase();
  const secure = type === 'password' || /password|one-time-code|cc-number|cc-csc/.test(autocomplete);
  let label = attr('aria-label') || attr('placeholder') || attr('title');
  if (!label && e.labels && e.labels.length) { label = e.labels[0].innerText; }
  if (!label && !secure) { label = (e.innerText || e.value || '').toString(); }
  return {
    url: location.href, title: document.title,
    tag: e.tagName.toLowerCase(), role: attr('role'), type, id: e.id || '',
    name: attr('name'), label: (label || '').trim().slice(0, 120), autocomplete, secure
  };
}"#;

/// The expression that describes the element under a point of the page.
pub fn describe_point_expression(x: f64, y: f64) -> String {
    format!("({DESCRIBE_FUNCTION}).call(document.elementFromPoint({x}, {y}))")
}

/// The expression that describes the focused element of the page.
pub fn describe_focused_expression() -> String {
    format!("({DESCRIBE_FUNCTION}).call(document.activeElement)")
}

/// The probe the proxy sends the browser, with its own `id`, to describe the
/// element of `action` before it runs. `None` for actions with no element.
pub fn probe_for(action: &AgentAction, session: Option<&str>, id: i64) -> Option<Value> {
    let (method, params) = match action {
        AgentAction::Click { x, y, .. } | AgentAction::Tap { x, y } => (
            "Runtime.evaluate",
            json!({ "expression": describe_point_expression(*x, *y), "returnByValue": true }),
        ),
        AgentAction::Key { .. } | AgentAction::InsertText { .. } => (
            "Runtime.evaluate",
            json!({ "expression": describe_focused_expression(), "returnByValue": true }),
        ),
        AgentAction::SetValue { object_id, .. } | AgentAction::Select { object_id, .. } => (
            "Runtime.callFunctionOn",
            json!({ "objectId": object_id, "functionDeclaration": DESCRIBE_FUNCTION,
                    "returnByValue": true }),
        ),
        AgentAction::Upload { .. } => (
            "Runtime.evaluate",
            json!({ "expression": describe_focused_expression(), "returnByValue": true }),
        ),
        AgentAction::Navigate { .. } => return None,
    };
    let mut probe = json!({ "id": id, "method": method, "params": params });
    if let Some(session) = session {
        probe["sessionId"] = json!(session);
    }
    Some(probe)
}

/// The element description out of a probe's answer, if it gave one.
pub fn probe_description(answer: &Value) -> Option<Map<String, Value>> {
    answer
        .get("result")?
        .get("result")?
        .get("value")?
        .as_object()
        .cloned()
}

/// One line of a recording's agent inbox for `action`, in the shape the
/// recorder reads (`{"kind", "at_ms", "spot", "fields"}`), with
/// `"driver": "browser"` and `"by": "agent"`.
///
/// `element` is what the probe said about the element. A value is kept only
/// when the probe answered and the field is not secure: a password field, or
/// a field the probe could not describe, gets `"secure": true` (or
/// `"value_withheld": true`) and never the text.
pub fn inbox_line(action: &AgentAction, element: Option<&Map<String, Value>>, at_ms: u64) -> Value {
    let mut fields = Map::new();
    fields.insert("driver".into(), json!("browser"));
    fields.insert("by".into(), json!("agent"));
    if let Some(element) = element {
        for key in ["url", "title"] {
            if let Some(value) = element.get(key) {
                fields.insert(key.into(), value.clone());
            }
        }
        let mut described = element.clone();
        described.remove("url");
        described.remove("title");
        described.remove("secure");
        if !described.is_empty() {
            fields.insert("element".into(), Value::Object(described));
        }
    }
    let secure = element
        .and_then(|e| e.get("secure"))
        .and_then(Value::as_bool);
    let mut spot = Value::Null;
    let kind = match action {
        AgentAction::Click {
            x,
            y,
            button,
            count,
        } => {
            fields.insert("button".into(), json!(button));
            fields.insert("count".into(), json!(count));
            fields.insert("point".into(), json!({ "x": x, "y": y }));
            spot = json!([x, y]);
            "click"
        }
        AgentAction::Tap { x, y } => {
            fields.insert("button".into(), json!("left"));
            fields.insert("count".into(), json!(1));
            fields.insert("point".into(), json!({ "x": x, "y": y }));
            spot = json!([x, y]);
            "click"
        }
        AgentAction::Key {
            key,
            modifiers,
            text,
        } => {
            let printable = text
                .as_deref()
                .is_some_and(|t| !t.chars().any(char::is_control));
            if printable && *modifiers & !8 == 0 {
                write_text(&mut fields, text.as_deref().unwrap_or(""), secure, "keys");
                "type"
            } else {
                fields.insert("keys".into(), json!(keys_name(key, *modifiers)));
                "press"
            }
        }
        AgentAction::InsertText { text } => {
            write_text(&mut fields, text, secure, "keys");
            "type"
        }
        AgentAction::SetValue { value, .. } => {
            write_text(&mut fields, value, secure, "value");
            "type"
        }
        AgentAction::Select { values, .. } => {
            fields.insert("value".into(), json!(values.join(", ")));
            fields.insert("values".into(), json!(values));
            "select"
        }
        AgentAction::Navigate { url } => {
            fields.insert("url".into(), json!(url));
            "navigate"
        }
        AgentAction::Upload { files } => {
            fields.insert("files".into(), json!(files));
            "upload"
        }
    };
    json!({ "kind": kind, "at_ms": at_ms, "spot": spot, "fields": fields })
}

fn write_text(fields: &mut Map<String, Value>, text: &str, secure: Option<bool>, source: &str) {
    match secure {
        Some(false) => {
            let key = if source == "value" { "value" } else { "typed" };
            fields.insert(key.into(), json!(text));
            fields.insert("source".into(), json!(source));
        }
        Some(true) => {
            fields.insert("secure".into(), json!(true));
        }
        None => {
            fields.insert("value_withheld".into(), json!(true));
        }
    }
}

/// `ctrl+shift+a`-style name of a key with DevTools modifiers (Alt=1,
/// Ctrl=2, Meta=4, Shift=8).
pub fn keys_name(key: &str, modifiers: u64) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (bit, name) in [(2, "ctrl"), (1, "alt"), (8, "shift"), (4, "meta")] {
        if modifiers & bit != 0 {
            parts.push(name.into());
        }
    }
    parts.push(key.to_lowercase());
    parts.join("+")
}

#[cfg(test)]
#[path = "cdp_tests.rs"]
mod tests;
