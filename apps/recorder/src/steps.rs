// Part of Agent Record Adapter (see NOTICE).
//! From a recording (agent-record's JSON Lines) to the steps a script repeats, without AI.
//!
//! The rules are those of the recording-to-script skill: keep what changes the result, leave out
//! what a replay must not repeat (a click that only focused a field, Ctrl+A before typing, a
//! click on a container, scrolling), merge what a person did in several moves (the three parts of
//! a date field, an option typed and confirmed with Enter, text typed around Enter or an
//! autocompletion), and mark what has no background command instead of guessing it.

use serde_json::Value;

/// How a step finds its element again: by id first, else by role and name. An element with
/// neither (a code editor) is found inside the document the window shows (`in_document`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locator {
    pub native_id: Option<String>,
    pub role: Option<String>,
    pub name: Option<String>,
    pub in_document: bool,
}

/// The window a step acts in, as the recording saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRef {
    pub id: String,
    pub app: String,
    pub title: String,
    pub path: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenBrowser {
        url: String,
    },
    OpenApp {
        path: String,
    },
    SetValue {
        at: Locator,
        field: String,
        value: String,
    },
    Select {
        at: Locator,
        field: String,
        value: String,
    },
    Check {
        at: Locator,
        field: String,
        on: bool,
    },
    Click {
        at: Locator,
        field: String,
        irreversible: bool,
    },
    /// A menu item, clicked in a menu the person opened: found again by its name.
    MenuItem {
        name: String,
    },
    /// A click on something with no element (a toolbar drawn by the app, a canvas): the same point
    /// of the window, in screen pixels from its top-left corner.
    ClickAt {
        dx: i32,
        dy: i32,
        double: bool,
    },
    /// Keys that run a command (Ctrl+Enter): done through the menu item that shows them.
    Key {
        keys: String,
    },
    /// Ctrl+V in a field: the clipboard's text added to it.
    Paste {
        at: Locator,
        field: String,
    },
    Secret {
        at: Locator,
        field: String,
        name: String,
    },
    /// Something the person did that has no background command (a right-click menu, a drag on
    /// a canvas): kept in the list so nobody thinks it was replayed.
    Skipped {
        what: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub window: WindowRef,
    pub action: Action,
}

/// Roles a click on is only a click in a container or a focus change, not an action.
const NO_ACTION_ROLES: &[&str] = &[
    "group",
    "pane",
    "window",
    "unknown",
    "statictext",
    "image",
    "document",
    "toolbar",
    "tablist",
    "list",
    "outline",
    "table",
];
/// Roles whose click only puts the caret in: the typing that follows is the action.
const EDITABLE_ROLES: &[&str] = &[
    "textfield",
    "combobox",
    "document",
    "spinbutton",
    "incrementor",
];
/// Roles a text or an icon is part of: a click on the text is a click on them.
const ACTIONABLE_ROLES: &[&str] = &[
    "button",
    "menuitem",
    "link",
    "hyperlink",
    "tab",
    "tabitem",
    "treeitem",
    "listitem",
    "option",
    "checkbox",
    "radiobutton",
];
/// Keys that only move the caret or select text before typing: the typed value already holds the result.
const EDIT_KEYS: &[&str] = &[
    "ctrl+a",
    "home",
    "end",
    "left",
    "right",
    "up",
    "down",
    "tab",
    "shift+tab",
    "backspace",
    "delete",
    "ctrl+z",
    "escape",
];
/// Ids that name a kind of element rather than one element.
const GENERIC_IDS: &[&str] = &[
    "RootWebArea",
    "TitleBar",
    "SystemMenuBar",
    "MenuBar",
    "Close",
    "Minimize-Restore",
    "Maximize-Restore",
    "TextBlock",
    "ContentPresenter",
    "Light Dismiss",
];
/// Apps whose windows are not part of the task: the taskbar and the Start menu (the script
/// opens the app itself), the desktop's compositor, and this recorder.
const NOT_THE_TASK: &[&str] = &["explorer.exe", "dwm.exe", "desktop-recorder.exe"];

pub fn parse(recording: &str) -> Vec<Step> {
    let events: Vec<Value> = recording
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let mut steps: Vec<Step> = Vec::new();
    let mut last_option: Option<String> = None;
    let mut last_clicked: Option<(Locator, String)> = None;
    let mut date: Option<DateParts> = None;
    // A key pressed in a field waits for what follows: more typing in the same field already
    // holds what it did (a new line, an autocompletion), anything else makes it a step.
    let mut pending: Option<(Locator, Step)> = None;
    let mut right_click: Option<Step> = None;
    // The popup an autocompletion was picked in, so a second click in it is not another pick.
    let mut completed_in: Option<String> = None;
    let mut opened_apps: Vec<String> = Vec::new();
    // The last main window of each app, where a menu item clicked in a popup is chosen.
    let mut main_window: Vec<WindowRef> = Vec::new();
    for event in &events {
        let kind = event["kind"].as_str().unwrap_or_default();
        if !matches!(kind, "click" | "type" | "select" | "press" | "drag") {
            continue;
        }
        let Some(window) = window_of(event) else {
            continue;
        };
        if NOT_THE_TASK
            .iter()
            .any(|app| window.app.eq_ignore_ascii_case(app))
        {
            continue;
        }
        let element = actionable(event);
        let role = element["role"].as_str().unwrap_or_default().to_owned();
        let name = element["name"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let at = locator(&element);

        // What a key pressed earlier did, now that the next event is known.
        if let Some((field, key_step)) = pending.take() {
            let more_typing = kind == "type" && at == field;
            if !more_typing {
                steps.push(key_step);
            }
        }
        // A right-click opens a menu: the item chosen in it is the step. A recording that could
        // not read the item has a click on nothing instead, which must not be replayed there.
        if let Some(skipped) = right_click.take() {
            let chosen = kind == "click" && role == "menuitem" && !name.is_empty();
            let unread = kind == "click" && role.is_empty() && name.is_empty();
            if !chosen {
                steps.push(skipped);
            }
            if unread {
                continue;
            }
        }
        if let Some(parts) = date.as_mut()
            && !(kind == "type" && role == "incrementor" && name.ends_with(&parts.field))
        {
            let finished = date.take().expect("date parts");
            steps.extend(finished.into_step());
        }
        let app_key = window.app.to_lowercase();
        let popup = window.title.is_empty() || window.title == "PopupHost";
        if !opened_apps.contains(&app_key) && !popup {
            opened_apps.push(app_key.clone());
            steps.push(open_step(&window));
        }
        if !popup {
            main_window.retain(|w| !w.app.eq_ignore_ascii_case(&window.app));
            main_window.push(window.clone());
        }
        if completed_in.as_deref() != Some(window.id.as_str()) {
            completed_in = None;
        }
        match kind {
            "click" => {
                let button = event["button"].as_str().unwrap_or("left");
                let count = event["count"].as_u64().unwrap_or(1);
                if button == "right" {
                    let place = if name.is_empty() {
                        short_title(&window)
                    } else {
                        name
                    };
                    right_click = Some(step(
                        &window,
                        Action::Skipped {
                            what: format!(
                                "the right-click menu on \"{place}\" and what was chosen in it"
                            ),
                        },
                    ));
                    continue;
                }
                if is_light_dismiss(&element) {
                    // The click opened a menu, and the menu's overlay was under the pointer by
                    // the time it was read: the item chosen next is found by its name.
                    continue;
                }
                if popup && completed_in.as_deref() == Some(window.id.as_str()) {
                    continue;
                }
                if popup && let Some(completion) = completion_of(&name, &role) {
                    // A pick in an autocompletion list completes what was being typed.
                    if let Some(Step {
                        action: Action::SetValue { value, .. },
                        window: typed_in,
                    }) = steps.last_mut()
                        && typed_in.app.eq_ignore_ascii_case(&window.app)
                        && let Some(done) = complete(value, &completion)
                    {
                        *value = done;
                        completed_in = Some(window.id.clone());
                        continue;
                    }
                }
                if role == "menuitem" && !name.is_empty() {
                    let owner = main_window
                        .iter()
                        .find(|w| w.app.eq_ignore_ascii_case(&window.app))
                        .cloned()
                        .unwrap_or_else(|| window.clone());
                    steps.push(step(&owner, Action::MenuItem { name }));
                } else if let Some(checked) = event["state_after"]["checked"].as_bool() {
                    steps.push(step(
                        &window,
                        Action::Check {
                            at,
                            field: name,
                            on: checked,
                        },
                    ));
                } else if EDITABLE_ROLES.contains(&role.as_str()) {
                    last_clicked = Some((at, name));
                } else if !NO_ACTION_ROLES.contains(&role.as_str())
                    && (at.native_id.is_some() || !name.is_empty())
                {
                    steps.push(step(
                        &window,
                        Action::Click {
                            at,
                            field: name,
                            irreversible: false,
                        },
                    ));
                } else if role.is_empty()
                    && name.is_empty()
                    && !popup
                    && let Some((dx, dy)) = offset(event)
                {
                    steps.push(step(
                        &window,
                        Action::ClickAt {
                            dx,
                            dy,
                            double: count == 2,
                        },
                    ));
                }
            }
            "type" if role == "option" => last_option = Some(name),
            "type" if role == "incrementor" => {
                let Some((field_at, field)) = last_clicked.clone() else {
                    continue;
                };
                let parts =
                    date.get_or_insert_with(|| DateParts::new(window.clone(), field_at, field));
                parts.add(&name, event["value"].as_str().unwrap_or_default());
            }
            "type" if event["secure"].as_bool() == Some(true) => {
                // An agent's `secret fill` names the secret it used.
                let secret = event["secret"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| secret_name(&window));
                steps.push(step(
                    &window,
                    Action::Secret {
                        at,
                        field: field_name(&name, &element),
                        name: secret,
                    },
                ));
            }
            "type" => {
                let value = event["value"].as_str().unwrap_or_default().to_owned();
                let field = field_name(&name, &element);
                if !set_last_value(&mut steps, &at, &value) {
                    steps.push(step(&window, Action::SetValue { at, field, value }));
                }
            }
            "select" => {
                let value = event["value"].as_str().unwrap_or_default().to_owned();
                steps.push(step(
                    &window,
                    Action::Select {
                        at,
                        field: name,
                        value,
                    },
                ));
                last_option = None;
            }
            "press" => {
                let keys = event["keys"].as_str().unwrap_or_default().to_lowercase();
                // What the field held when the key went down: an autocompletion or a paste
                // changed it without typing.
                if let Some(before) = event["value_before"].as_str() {
                    set_last_value(&mut steps, &at, before);
                }
                let editable = EDITABLE_ROLES.contains(&role.as_str());
                if role == "combobox" && keys == "return" {
                    if let Some(option) = last_option.take()
                        && !steps
                            .iter()
                            .rev()
                            .take(1)
                            .any(|s| matches!(&s.action, Action::Select { at: a, .. } if *a == at))
                    {
                        steps.push(step(
                            &window,
                            Action::Select {
                                at,
                                field: name,
                                value: option,
                            },
                        ));
                    }
                    continue;
                }
                // Space on a radio button or checkbox chooses it, Enter on a button or link presses
                // it: replayed as that click, which works in the background where keys may not.
                let named = at.native_id.is_some() || !name.is_empty();
                if named && keys == "space" && role == "radiobutton" {
                    steps.push(step(
                        &window,
                        Action::Check {
                            at,
                            field: name,
                            on: true,
                        },
                    ));
                    continue;
                }
                if named
                    && ((keys == "space" && role == "checkbox")
                        || (keys == "return" && matches!(role.as_str(), "button" | "link")))
                {
                    steps.push(step(
                        &window,
                        Action::Click {
                            at,
                            field: name,
                            irreversible: false,
                        },
                    ));
                    continue;
                }
                if EDIT_KEYS.contains(&keys.as_str())
                    || (editable && moves_caret(&keys))
                    || (keys == "return" && role == "document")
                {
                    continue;
                }
                let key_step = if keys == "ctrl+v" && editable && window.url.is_none() {
                    step(
                        &window,
                        Action::Paste {
                            at: at.clone(),
                            field: field_name(&name, &element),
                        },
                    )
                } else if window.url.is_none() && is_command_key(&keys) {
                    step(&window, Action::Key { keys })
                } else {
                    let place = if name.is_empty() {
                        short_title(&window)
                    } else {
                        name
                    };
                    step(
                        &window,
                        Action::Skipped {
                            what: format!("key {keys} in \"{place}\""),
                        },
                    )
                };
                if editable {
                    pending = Some((at, key_step));
                } else {
                    steps.push(key_step);
                }
            }
            "drag" => steps.push(step(
                &window,
                Action::Skipped {
                    what: format!("drag in \"{}\"", short_title(&window)),
                },
            )),
            _ => {}
        }
    }
    if let Some((_, key_step)) = pending.take() {
        steps.push(key_step);
    }
    steps.extend(right_click.take());
    if let Some(parts) = date.take() {
        steps.extend(parts.into_step());
    }
    steps
}

/// Several moves in one field end in its last value: sets the value of the field's last step,
/// when no other step came after it. Returns whether there was one.
fn set_last_value(steps: &mut [Step], at: &Locator, value: &str) -> bool {
    if let Some(Step {
        action:
            Action::SetValue {
                at: previous,
                value: kept,
                ..
            },
        ..
    }) = steps.last_mut()
        && previous == at
    {
        *kept = value.to_owned();
        return true;
    }
    false
}

fn step(window: &WindowRef, action: Action) -> Step {
    Step {
        window: window.clone(),
        action,
    }
}

fn open_step(window: &WindowRef) -> Step {
    let action = match (&window.url, &window.path) {
        (Some(url), _) => Action::OpenBrowser { url: full_url(url) },
        (None, Some(path)) => Action::OpenApp { path: path.clone() },
        (None, None) => Action::OpenApp {
            path: window.app.clone(),
        },
    };
    step(window, action)
}

/// A page address as a browser takes it. Recordings keep the scheme; an older one has the address
/// bar's text without it: a file path gets `file:///`, a server on this computer or the local
/// network `http://` (a development server rarely has a certificate), any other host `https://`.
fn full_url(url: &str) -> String {
    let bytes = url.as_bytes();
    if bytes.len() > 2 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\') {
        return format!("file:///{}", url.replace('\\', "/"));
    }
    if url.contains("://") {
        url.to_owned()
    } else if is_local_host(url) {
        format!("http://{url}")
    } else {
        format!("https://{url}")
    }
}

/// Whether a bare address names this computer or a private network address.
fn is_local_host(url: &str) -> bool {
    let authority = url.split(['/', '?', '#']).next().unwrap_or_default();
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    }
    .to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host == "::1" {
        return true;
    }
    let Ok(ip) = host.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn window_of(event: &Value) -> Option<WindowRef> {
    let w = &event["window"];
    Some(WindowRef {
        id: w["id"].as_str()?.to_owned(),
        app: w["app_name"].as_str().unwrap_or_default().to_owned(),
        title: w["title"].as_str().unwrap_or_default().to_owned(),
        path: w["path"].as_str().map(str::to_owned),
        url: w["url"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(str::to_owned),
    })
}

/// The element a click or key acted on: a text or an icon inside a button, a menu item or a
/// list item stands for that item.
fn actionable(event: &Value) -> Value {
    let element = &event["element"];
    let role = element["role"].as_str().unwrap_or_default();
    if matches!(role, "statictext" | "image" | "text")
        && let Some(parent) = event["ancestors"].as_array().and_then(|a| a.first())
        && ACTIONABLE_ROLES.contains(&parent["role"].as_str().unwrap_or_default())
        && parent["name"]
            .as_str()
            .is_some_and(|n| !n.trim().is_empty())
    {
        return parent.clone();
    }
    element.clone()
}

/// The full-window overlay a flyout menu puts up to close when clicked outside it.
fn is_light_dismiss(element: &Value) -> bool {
    element["native_id"].as_str() == Some("Light Dismiss")
        || element["class_name"].as_str() == Some("PopupRoot")
}

/// Where a click on nothing that has a name was, from the window's top-left corner.
fn offset(event: &Value) -> Option<(i32, i32)> {
    let point = &event["point"];
    let bounds = &event["window"]["bounds"];
    // Points are whole pixels; bounds may be written as 614.0.
    let x = point["x"].as_f64()? - bounds["x"].as_f64()?;
    let y = point["y"].as_f64()? - bounds["y"].as_f64()?;
    Some((x.round() as i32, y.round() as i32))
}

/// The text an autocompletion item puts in: "alumno a - Table" puts "alumno a".
fn completion_of(name: &str, role: &str) -> Option<String> {
    if !matches!(role, "option" | "listitem" | "statictext" | "treeitem") || name.is_empty() {
        return None;
    }
    let text = name.split(" - ").next().unwrap_or(name).trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// `typed` with its last word completed to `completion`, when the completion starts with it
/// ("FROM ALUMN" and "alumno a" give "FROM alumno a").
fn complete(typed: &str, completion: &str) -> Option<String> {
    let start = typed
        .rfind(|c: char| c.is_whitespace() || "(,;.=".contains(c))
        .map_or(0, |at| at + 1);
    let word = &typed[start..];
    if word.is_empty() || !completion.to_lowercase().starts_with(&word.to_lowercase()) {
        return None;
    }
    Some(format!("{}{completion}", &typed[..start]))
}

/// Keys a menu item can run: with Ctrl or Alt, or a function key.
/// Keys that only move the caret or select in a field (Ctrl+End, Shift+Home): what follows in
/// the field already holds their effect, and a replay that sets the field's text needs none.
fn moves_caret(keys: &str) -> bool {
    let mut parts: Vec<&str> = keys.split('+').collect();
    let Some(key) = parts.pop() else {
        return false;
    };
    matches!(
        key,
        "home" | "end" | "left" | "right" | "up" | "down" | "pageup" | "pagedown"
    ) && parts.iter().all(|part| matches!(*part, "ctrl" | "shift"))
}

fn is_command_key(keys: &str) -> bool {
    keys.contains("ctrl+")
        || keys.contains("alt+")
        || keys.rsplit('+').next().is_some_and(|key| {
            key.len() > 1 && key.starts_with('f') && key[1..].parse::<u8>().is_ok()
        })
}

fn locator(element: &Value) -> Locator {
    let native_id = element["native_id"]
        .as_str()
        .filter(|id| !id.is_empty() && !GENERIC_IDS.contains(id) && !is_handle(id))
        .map(str::to_owned);
    let name = element["name"]
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .or_else(|| element["label"].as_str().filter(|l| !l.is_empty()))
        .or_else(|| {
            element["help_text"]
                .as_str()
                .filter(|h| !h.is_empty() && h.len() < 60)
        })
        .map(str::to_owned);
    let role = element["role"].as_str().map(str::to_owned);
    let in_document = native_id.is_none()
        && name.is_none()
        && role.as_deref().is_some_and(|r| EDITABLE_ROLES.contains(&r));
    Locator {
        native_id,
        role,
        name,
        in_document,
    }
}

/// A number that is a window handle, not an id the app gave: it changes every time the app runs.
fn is_handle(id: &str) -> bool {
    id.parse::<u64>().is_ok_and(|n| n >= 65_536)
}

/// What a person calls the field: its name, else its label or placeholder.
fn field_name(name: &str, element: &Value) -> String {
    if !name.is_empty() {
        return name.to_owned();
    }
    element["label"]
        .as_str()
        .or_else(|| element["help_text"].as_str())
        .unwrap_or("text")
        .to_owned()
}

/// The window's title without the document part that changes between runs, for messages.
fn short_title(window: &WindowRef) -> String {
    window
        .title
        .rsplit(" - ")
        .next()
        .unwrap_or(&window.title)
        .to_owned()
}

/// A name for the password of this app or site: `opensource-demo-orangehrmlive-com-login`, `wordcraft-login`.
fn secret_name(window: &WindowRef) -> String {
    let local_file = |url: &str| url.starts_with("file:") || url.as_bytes().get(1) == Some(&b':');
    let base = match &window.url {
        Some(url) if !local_file(url) => url
            .split("://")
            .last()
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap_or("site")
            .to_owned(),
        _ => window.app.trim_end_matches(".exe").to_owned(),
    };
    let slug: String = base
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{}-login", slug.trim_matches('-'))
}

/// The day, month and year a person typed into the three parts of a date field.
struct DateParts {
    window: WindowRef,
    at: Locator,
    field: String,
    day: Option<String>,
    month: Option<String>,
    year: Option<String>,
}

impl DateParts {
    fn new(window: WindowRef, at: Locator, field: String) -> Self {
        Self {
            window,
            at,
            field,
            day: None,
            month: None,
            year: None,
        }
    }

    fn add(&mut self, part: &str, value: &str) {
        let slot = match part
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "day" | "día" | "dia" => &mut self.day,
            "month" | "mes" => &mut self.month,
            "year" | "año" | "ano" => &mut self.year,
            _ => return,
        };
        *slot = Some(value.to_owned());
    }

    fn into_step(self) -> Option<Step> {
        let (Some(day), Some(month), Some(year)) = (self.day, self.month, self.year) else {
            return None;
        };
        let value = format!("{year:0>4}-{month:0>2}-{day:0>2}");
        Some(Step {
            window: self.window,
            action: Action::SetValue {
                at: self.at,
                field: self.field,
                value,
            },
        })
    }
}

#[cfg(test)]
#[path = "steps_tests.rs"]
mod tests;

/// The file next to a recording that lists the steps that ask before they run
/// (their places in the step list), as the person marked them in the app.
pub const ASK_BEFORE_FILE: &str = "ask-before.json";

/// Marks which clicks ask before they run: the steps the person marked, else
/// the last click on a button, the one that usually ends a task (it sends,
/// saves, pays). By the step's place and role, never by the words on the
/// button, so it is the same in every language.
pub fn mark_ask_before(steps: &mut [Step], marked: Option<&[usize]>) {
    let last_button = steps.iter().rposition(|step| {
        matches!(&step.action, Action::Click { at, .. } if at.role.as_deref() == Some("button"))
    });
    for (index, step) in steps.iter_mut().enumerate() {
        if let Action::Click { irreversible, .. } = &mut step.action {
            *irreversible = match marked {
                Some(list) => list.contains(&index),
                None => Some(index) == last_button,
            };
        }
    }
}

/// The steps the person marked in a recording's folder, if they marked any.
pub fn read_ask_before(dir: &std::path::Path) -> Option<Vec<usize>> {
    let text = std::fs::read_to_string(dir.join(ASK_BEFORE_FILE)).ok()?;
    serde_json::from_str::<Value>(&text).ok()?["steps"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
                .collect()
        })
}

/// Saves the steps that ask before they run, for the app and the script.
pub fn write_ask_before(dir: &std::path::Path, marked: &[usize]) -> std::io::Result<()> {
    std::fs::write(
        dir.join(ASK_BEFORE_FILE),
        serde_json::json!({ "steps": marked }).to_string(),
    )
}

/// The steps of the recording in `dir`, with the clicks that ask before they run.
pub fn parse_dir(dir: &std::path::Path, recording: &str) -> Vec<Step> {
    let mut steps = parse(recording);
    mark_ask_before(&mut steps, read_ask_before(dir).as_deref());
    steps
}
