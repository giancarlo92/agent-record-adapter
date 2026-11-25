// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! agent-desktop's own actions in a running recording.
//!
//! A recording hears the person through Windows' mouse and keyboard hooks. An
//! agent's `click`, `set-value` or `select` goes through accessibility or
//! window messages instead, so the hooks never see it, and a task an agent did
//! while it recorded ended with no steps. Now each element action that
//! succeeds while a background recording runs is described here, with the
//! recorder's own description of an element (window, role, name, ancestors,
//! match count), and appended to that recording's agent inbox; the recording
//! reads the inbox and writes the action as it would write the person's, with
//! `by: "agent"`. A secret written by `secret fill` is noted as a secure
//! field with the secret's name, never its value.

use std::io::Write;
use std::path::Path;

use agent_desktop_core::commands::record_background::{
    running_agent_inboxes, secret_being_written,
};
use agent_desktop_core::{
    Action, Direction, KeyCombo, Modifier, NativeHandle, RecordRequest, WindowInfo,
};
use serde_json::{Map, Value, json};

use super::record_element::{FieldValue, Observer};
use crate::tree::element::{UIAElement, uia_element};

/// An element action about to run while a background recording runs, its
/// element described now: a click on a dialog's Open button closes the
/// dialog, and after it there is nothing left to describe.
pub(crate) struct Noted {
    inboxes: Vec<std::path::PathBuf>,
    observer: Observer,
    element: UIAElement,
    fields: Map<String, Value>,
    spot: Option<(i32, i32)>,
}

/// Describes the element of `handle` before an action on it. `None` when no
/// background recording runs.
pub(crate) fn before(handle: &NativeHandle) -> Option<Noted> {
    let inboxes = running_agent_inboxes();
    if inboxes.is_empty() {
        return None;
    }
    let element = UIAElement::from(uia_element(handle).ok()?.0.clone());
    let mut observer = observer()?;
    let target = observer.of_element(element.clone());
    let spot = Observer::spot_of(&target);
    let fields = observer.describe(&target);
    Some(Noted {
        inboxes,
        observer,
        element,
        fields,
        spot,
    })
}

impl Noted {
    /// Notes `action`, which succeeded, in every background recording running.
    pub(crate) fn after(self, action: &Action) {
        let Some(line) = describe(self.observer, self.element, self.fields, self.spot, action)
        else {
            return;
        };
        for inbox in &self.inboxes {
            append(inbox, &line);
        }
    }
}

/// A click agent-desktop is about to send to a point of a window
/// (`click-at`), described now: the click may open a dialog (a file picker)
/// that covers what was under the point.
pub(crate) struct NotedPoint {
    inboxes: Vec<std::path::PathBuf>,
    line: String,
}

/// Describes the point (`screen`, in screen pixels) of window `top` before
/// a click there; `window_point` is the point as `click-at` was given it.
/// `None` when no background recording runs.
pub(crate) fn before_point(
    top: isize,
    screen: (i32, i32),
    window_point: (i32, i32),
    double: bool,
) -> Option<NotedPoint> {
    let inboxes = running_agent_inboxes();
    if inboxes.is_empty() {
        return None;
    }
    let mut observer = observer()?;
    let target = observer.at_point_in(top, screen.0, screen.1);
    let mut fields = observer.describe(&target);
    click(&mut fields, "left", if double { 2 } else { 1 });
    fields.insert("by".into(), json!("agent"));
    fields.insert("point".into(), json!({ "x": screen.0, "y": screen.1 }));
    fields.insert(
        "window_point".into(),
        json!({ "x": window_point.0, "y": window_point.1 }),
    );
    let line = json!({
        "kind": "click",
        "at_ms": now_ms(),
        "spot": [screen.0, screen.1],
        "fields": fields,
    });
    Some(NotedPoint {
        inboxes,
        line: serde_json::to_string(&line).ok()?,
    })
}

impl NotedPoint {
    /// Notes the click, which was sent, in every background recording running.
    pub(crate) fn after(self) {
        for inbox in &self.inboxes {
            append(inbox, &self.line);
        }
    }
}

/// A command agent-desktop is about to run on a whole window (`menu`,
/// `close-window`), the window described now: closing it leaves nothing to
/// describe.
pub(crate) struct NotedWindow {
    inboxes: Vec<std::path::PathBuf>,
    kind: &'static str,
    fields: Map<String, Value>,
    spot: Option<(i32, i32)>,
}

/// Describes `window` before a menu command runs in it. `None` when no
/// background recording runs.
pub(crate) fn before_menu(window: &WindowInfo) -> Option<NotedWindow> {
    noted_window(window, "click")
}

/// Describes `window` before it is closed (`close-window`). `None` when no
/// background recording runs.
pub(crate) fn before_close(window: &WindowInfo) -> Option<NotedWindow> {
    noted_window(window, "close")
}

fn noted_window(window: &WindowInfo, kind: &'static str) -> Option<NotedWindow> {
    let inboxes = running_agent_inboxes();
    if inboxes.is_empty() {
        return None;
    }
    let mut observer = observer()?;
    let (fields, spot) = describe_window(&mut observer, window);
    Some(NotedWindow {
        inboxes,
        kind,
        fields,
        spot,
    })
}

impl NotedWindow {
    /// Notes the menu command that ran, by its path (File, Save as...), as a
    /// click on its menu item.
    pub(crate) fn after_menu(mut self, path: &[String]) {
        let Some(name) = path.last() else {
            return;
        };
        self.fields.insert(
            "element".into(),
            json!({ "role": "menuitem", "name": name }),
        );
        self.fields.insert("menu_path".into(), json!(path));
        self.fields.insert("button".into(), json!("left"));
        self.fields.insert("count".into(), json!(1));
        self.write();
    }

    /// Notes that the window was closed (or asked to save first).
    pub(crate) fn after_close(self) {
        self.write();
    }

    fn write(mut self) {
        self.fields.insert("by".into(), json!("agent"));
        let line = json!({
            "kind": self.kind,
            "at_ms": now_ms(),
            "spot": self.spot.map(|(x, y)| json!([x, y])),
            "fields": self.fields,
        });
        if let Ok(line) = serde_json::to_string(&line) {
            for inbox in &self.inboxes {
                append(inbox, &line);
            }
        }
    }
}

/// The recorder's description of a whole window, and its middle.
fn describe_window(
    observer: &mut Observer,
    window: &WindowInfo,
) -> (Map<String, Value>, Option<(i32, i32)>) {
    let handle: isize = window
        .id
        .strip_prefix("w-")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(0);
    let spot = window
        .bounds
        .as_ref()
        .map(|b| ((b.x + b.width / 2.0) as i32, (b.y + b.height / 2.0) as i32));
    let target = observer.of_window(handle, spot.unwrap_or((0, 0)));
    (observer.describe(&target), spot)
}

/// Notes a key agent-desktop pressed in a window (`press <key> --window-id`),
/// on whatever has the focus there, in every background recording running.
pub(crate) fn note_window_key(window: &WindowInfo, combo: &KeyCombo) {
    let inboxes = running_agent_inboxes();
    if inboxes.is_empty() {
        return;
    }
    let Some(mut observer) = observer() else {
        return;
    };
    let handle: isize = window
        .id
        .strip_prefix("w-")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(0);
    let spot = window
        .bounds
        .as_ref()
        .map(|b| ((b.x + b.width / 2.0) as i32, (b.y + b.height / 2.0) as i32));
    // In a browser, the page's focused control is what the key went to (a
    // radio button for Space); the window alone when no control has it.
    let focused = crate::actions::chromium_input::is_chromium_top(window)
        .and_then(crate::actions::chromium_input::page_focus)
        .filter(|element| {
            element
                .0
                .get_control_type()
                .is_ok_and(|kind| kind != uiautomation::types::ControlType::Document)
        });
    let target = match focused {
        Some(element) => observer.of_element(element),
        None => observer.of_window(handle, spot.unwrap_or((0, 0))),
    };
    let mut fields = observer.describe(&target);
    fields.insert("keys".into(), json!(keys_of(combo)));
    fields.insert("by".into(), json!("agent"));
    let line = json!({
        "kind": "press",
        "at_ms": now_ms(),
        "spot": spot.map(|(x, y)| json!([x, y])),
        "fields": fields,
    });
    if let Ok(line) = serde_json::to_string(&line) {
        for inbox in inboxes {
            append(&inbox, &line);
        }
    }
}

/// Keys spelled and ordered as the recorder spells a person's (`ctrl+return`).
fn keys_of(combo: &KeyCombo) -> String {
    let mut parts: Vec<String> = [
        (Modifier::Ctrl, "ctrl"),
        (Modifier::Alt, "alt"),
        (Modifier::Shift, "shift"),
        (Modifier::Meta, "cmd"),
    ]
    .into_iter()
    .filter(|(modifier, _)| combo.modifiers.contains(modifier))
    .map(|(_, name)| name.to_owned())
    .collect();
    let key = combo.key.to_lowercase();
    parts.push(match key.as_str() {
        "enter" => "return".to_owned(),
        "esc" => "escape".to_owned(),
        _ => key,
    });
    parts.join("+")
}

fn observer() -> Option<Observer> {
    let request = RecordRequest {
        app: None,
        window_id: None,
        display: None,
        max_duration_ms: 0,
        stop_file: None,
        include_injected: false,
        video: None,
        indicator: false,
    };
    Observer::new(&request).ok()
}

fn describe(
    observer: Observer,
    element: UIAElement,
    mut fields: Map<String, Value>,
    spot: Option<(i32, i32)>,
    action: &Action,
) -> Option<String> {
    let kind = match action {
        Action::Click => click(&mut fields, "left", 1),
        // A tree item opened or closed: a replay opens or closes it again (a
        // click alone only selects it).
        Action::Expand | Action::Collapse => {
            fields.insert("expand".into(), json!(matches!(action, Action::Expand)));
            click(&mut fields, "left", 1)
        }
        Action::DoubleClick => click(&mut fields, "left", 2),
        Action::TripleClick => click(&mut fields, "left", 3),
        Action::RightClick => click(&mut fields, "right", 1),
        Action::Check | Action::Uncheck | Action::Toggle => {
            let state = observer.state_after(&element);
            if !state.is_empty() {
                fields.insert("state_after".into(), Value::Object(state));
            }
            click(&mut fields, "left", 1)
        }
        Action::SetValue(_) | Action::TypeText(_) | Action::Clear => {
            match secret_being_written() {
                Some(name) => {
                    fields.insert("secure".into(), json!(true));
                    fields.insert("secret".into(), json!(name));
                }
                None => match observer.value(&element) {
                    FieldValue::Secret => {
                        fields.insert("secure".into(), json!(true));
                    }
                    FieldValue::Text(text) => {
                        fields.insert("value".into(), json!(text));
                        fields.insert("source".into(), json!("value"));
                    }
                    // A field that does not say what it holds: the value the
                    // agent wrote (never a secret: those take the arm above).
                    FieldValue::Unavailable => {
                        let written = match action {
                            Action::SetValue(text) | Action::TypeText(text) => text.clone(),
                            _ => String::new(),
                        };
                        fields.insert("typed".into(), json!(written));
                        fields.insert("source".into(), json!("keys"));
                    }
                },
            }
            "type"
        }
        Action::Select(wanted) => {
            let value = settled_choice(&observer, &element, wanted);
            fields.insert("value".into(), json!(value));
            "select"
        }
        Action::PressKey(combo) => {
            fields.insert("keys".into(), json!(keys_of(combo)));
            "press"
        }
        Action::Scroll(direction, amount) => {
            let direction = match direction {
                Direction::Up => "up",
                Direction::Down => "down",
                Direction::Left => "left",
                Direction::Right => "right",
            };
            fields.insert("direction".into(), json!(direction));
            fields.insert("amount".into(), json!(amount));
            "scroll"
        }
        _ => return None,
    };
    fields.insert("by".into(), json!("agent"));
    if let Some((x, y)) = spot
        && kind == "click"
    {
        fields.insert("point".into(), json!({ "x": x, "y": y }));
    }
    let line = json!({
        "kind": kind,
        "at_ms": now_ms(),
        "spot": spot.map(|(x, y)| json!([x, y])),
        "fields": fields,
    });
    serde_json::to_string(&line).ok()
}

/// The option a `select` left chosen. A list can show its new value a moment
/// after the call returns (Chrome's print preview shows the previous
/// destination for a while), and a value read at once recorded the option
/// that was there before. The value is read until it shows the option asked
/// for, up to a second; the option asked for is kept when it never does, since
/// the select itself succeeded.
fn settled_choice(observer: &Observer, element: &UIAElement, wanted: &str) -> String {
    let shows_wanted = |value: &str| {
        let (value, wanted) = (value.trim().to_lowercase(), wanted.trim().to_lowercase());
        !wanted.is_empty() && value.contains(&wanted)
    };
    let end = std::time::Instant::now() + std::time::Duration::from_millis(1000);
    loop {
        if let Some(value) = observer
            .choice_value(element)
            .filter(|value| shows_wanted(value))
        {
            return value;
        }
        if std::time::Instant::now() >= end {
            return wanted.to_owned();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn click(fields: &mut Map<String, Value>, button: &str, count: u32) -> &'static str {
    fields.insert("button".into(), json!(button));
    fields.insert("count".into(), json!(count));
    "click"
}

/// One line, written in one call to a file opened for appending, so the lines
/// of two agents acting at once do not mix.
fn append(inbox: &Path, line: &str) {
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(inbox)
        .and_then(|mut file| file.write_all(format!("{line}\n").as_bytes()));
    if let Err(error) = written {
        tracing::debug!(%error, inbox = %inbox.display(), "record: the agent action was not noted");
    }
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}
