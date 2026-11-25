// Part of Agent Record Adapter (see NOTICE).
//! An option chosen in a web page's own list (`<select>`) in a Chromium
//! browser, in the background.
//!
//! Chromium shows a closed `<select>` as a combo box with no options: they
//! exist only while its list is open, and that list is a window of its own
//! drawn over the person's windows. A value written through accessibility is
//! ignored (the page keeps its choice, and its `change` handler never runs:
//! a region list filled from the country stays empty). A person who does not
//! open the list focuses it and types the option's text; Chromium picks the
//! option that begins with it and tells the page (`change`). That is done
//! here, as window messages behind the person's windows; the option's text
//! is the value asked for, so any language works. The combo box then shows
//! the option chosen, which proves it.

use std::time::{Duration, Instant};

use agent_desktop_core::{ActionStep, AdapterError, Deadline, ErrorCode, StepMechanism};
use uiautomation::patterns::UIValuePattern;
use uiautomation::types::ControlType;

use crate::tree::element::UIAElement;

fn shown(element: &uiautomation::UIElement) -> String {
    element
        .get_pattern::<UIValuePattern>()
        .and_then(|pattern| pattern.get_value())
        .unwrap_or_default()
}

fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Chooses `value` in a Chromium page's `<select>`; `None` when `element` is
/// not one (the caller goes on to its other ways).
pub(crate) fn select(
    element: &UIAElement,
    value: &str,
    deadline: Deadline,
) -> Result<Option<Vec<ActionStep>>, AdapterError> {
    use uiautomation::patterns::UIExpandCollapsePattern;
    let combo = &element.0;
    if combo.get_framework_id().ok().as_deref()
        != Some(crate::actions::chromium_input::CHROMIUM_FRAMEWORK)
        || combo.get_control_type().ok() != Some(ControlType::ComboBox)
        || combo.get_pattern::<UIExpandCollapsePattern>().is_err()
        || value.trim().is_empty()
    {
        return Ok(None);
    }
    // A closed <select> has no parts of its own in the tree. A combo box with
    // parts is something else: an editable one (a text field with
    // suggestions), or a component library's (PrimeNG: a span with its text,
    // opened through its container, see `dropdown`).
    let Ok(client) = uiautomation::UIAutomation::new() else {
        return Ok(None);
    };
    let has_parts = client
        .create_true_condition()
        .ok()
        .and_then(|all| {
            combo
                .find_first(uiautomation::types::TreeScope::Children, &all)
                .ok()
        })
        .is_some();
    if has_parts {
        return Ok(None);
    }
    let step = |verified: bool| {
        ActionStep::succeeded("WebSelect.TypeAhead")
            .with_mechanism(StepMechanism::SemanticApi)
            .with_verified(verified)
    };
    if same(&shown(combo), value) {
        return Ok(Some(vec![step(true)]));
    }
    let Some(top) = crate::actions::chromium_input::top_window(combo, &client) else {
        return Ok(None);
    };
    // The list takes the keyboard focus without opening (a click would open it).
    if combo.set_focus().is_err() {
        return Ok(None);
    }
    std::thread::sleep(Duration::from_millis(80));
    let mut keys = Vec::new();
    for unit in value.trim().encode_utf16() {
        // SAFETY: VkKeyScanW only maps a character to a key of this keyboard.
        let scan = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::VkKeyScanW(unit) };
        let vk = if scan == -1 { 0 } else { (scan & 0xFF) as u16 };
        if vk == 0 {
            keys.push((
                windows_sys::Win32::UI::WindowsAndMessaging::WM_CHAR,
                unit as usize,
                1,
            ));
        } else {
            keys.extend(crate::actions::posted_keys::key_messages(
                vk,
                Some(unit),
                false,
            ));
        }
    }
    if !crate::actions::chromium_input::send_keys(top, &keys) {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The browser did not take the option's text",
        ));
    }
    let end = Instant::now() + Duration::from_millis(1500);
    let mut now = shown(combo);
    while !same(&now, value) && Instant::now() < end && !deadline.is_expired() {
        std::thread::sleep(Duration::from_millis(100));
        now = shown(combo);
    }
    if same(&now, value) {
        return Ok(Some(vec![step(true)]));
    }
    // An option whose text holds the value without beginning with it ("+34"
    // in "ES +34"): the options are gone through with the arrow keys, as a
    // person does in a closed list, from the first one.
    if let Some(found) = walk_to(top, combo, value, deadline) {
        return Ok(Some(vec![step(true)]).filter(|_| found));
    }
    Err(AdapterError::new(
        ErrorCode::ActionFailed,
        format!("The list shows '{now}' after choosing '{}'", value.trim()),
    )
    .with_suggestion("Check the option's text with snapshot"))
}

/// The longest list gone through with the arrow keys.
const MAX_OPTIONS: usize = 400;

/// Goes through a closed list with Home and Down until an option's text holds
/// `value` (as a word part, any case). `Some(true)` when one does; `None`
/// when none does, after the list was put back on the option it showed.
fn walk_to(
    top: windows_sys::Win32::Foundation::HWND,
    combo: &uiautomation::UIElement,
    value: &str,
    deadline: Deadline,
) -> Option<bool> {
    use crate::actions::posted_keys::key_messages;
    const VK_HOME: u16 = 0x24;
    const VK_DOWN: u16 = 0x28;
    let wanted = value.trim().to_lowercase();
    let first = shown(combo);
    let press =
        |vk: u16| crate::actions::chromium_input::send_keys(top, &key_messages(vk, None, true));
    // The text a key leads to, once the list shows it.
    let after = |before: &str| {
        let end = Instant::now() + Duration::from_millis(400);
        let mut now = shown(combo);
        while now == before && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(30));
            now = shown(combo);
        }
        now
    };
    if !press(VK_HOME) {
        return None;
    }
    let mut now = after(&first);
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_OPTIONS {
        if now.to_lowercase().contains(&wanted) {
            return Some(true);
        }
        if seen.contains(&now) || deadline.is_expired() {
            break;
        }
        seen.push(now.clone());
        if !press(VK_DOWN) {
            break;
        }
        now = after(&now);
    }
    // None holds it: back to the option shown before.
    if press(VK_HOME) {
        let mut now = after("");
        for _ in 0..MAX_OPTIONS {
            if now == first || !press(VK_DOWN) {
                break;
            }
            let next = after(&now);
            if next == now {
                break;
            }
            now = next;
        }
    }
    None
}
