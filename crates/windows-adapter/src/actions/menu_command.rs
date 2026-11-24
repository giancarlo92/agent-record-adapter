// Part of Agent Record Adapter (see NOTICE).
//! A menu-bar command, run without opening the menu.
//!
//! A classic menu bar (the window's `HMENU`) only opens in the window in
//! front, so a background action cannot open it. Choosing an item ends in one
//! message to the window, `WM_COMMAND` with the item's id; that message is
//! sent here directly. Apps that fill a menu when it opens (Eclipse and
//! DBeaver do) are told it opens first (`WM_INITMENUPOPUP`), as they would be
//! when a person opens it, and that it closed after the command.

use agent_desktop_core::{AdapterError, Deadline, ErrorCode, MenuSearch, WindowInfo};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMenu, GetMenuItemCount, GetMenuItemInfoW, HMENU, MENUITEMINFOW, MFS_DISABLED, MFT_SEPARATOR,
    MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, PostMessageW, SMTO_ABORTIFHUNG,
    SMTO_BLOCK, SendMessageTimeoutW, WM_COMMAND, WM_INITMENU, WM_INITMENUPOPUP, WM_UNINITMENUPOPUP,
};

const SEND_TIMEOUT_MS: u32 = 2_000;

/// A key combo with Ctrl, Alt or Shift that an item of `window`'s classic
/// menu bar shows, run as that item's command. `None` when the combo has no
/// modifier, the window has no menu bar, or no item shows those keys: the
/// caller then presses the keys.
pub(crate) fn run_shortcut(
    window: &WindowInfo,
    combo: &agent_desktop_core::KeyCombo,
    deadline: Deadline,
) -> Option<agent_desktop_core::ActionResult> {
    use agent_desktop_core::{Action, ActionResult, ActionStep, Modifier, StepMechanism};
    if combo.modifiers.is_empty() {
        return None;
    }
    let hwnd = crate::system::window::window_ops::parse_window_id(&window.id).ok()?;
    if unsafe { GetMenu(hwnd as _) }.is_null() {
        return None;
    }
    let mut keys: Vec<String> = combo
        .modifiers
        .iter()
        .map(|modifier| {
            match modifier {
                Modifier::Ctrl => "ctrl",
                Modifier::Alt => "alt",
                Modifier::Shift => "shift",
                Modifier::Meta => "win",
            }
            .to_owned()
        })
        .collect();
    keys.push(combo.key.clone());
    let path = find(window, &MenuSearch::Shortcut(keys.join("+")), deadline).ok()?;
    invoke(window, &path, deadline).ok()?;
    let step = ActionStep::succeeded("MenuCommand.Shortcut")
        .with_mechanism(StepMechanism::SemanticApi)
        .with_verified(false);
    Some(ActionResult::from_execution(
        &Action::PressKey(combo.clone()),
        vec![step],
    ))
}

struct Item {
    text: String,
    /// The keys the menu shows next to the name ("Ctrl+Enter"), if any.
    keys: Option<String>,
    id: u32,
    submenu: HMENU,
    disabled: bool,
    position: u32,
}

/// Runs the item at `path` of `window`'s menu bar. Returns the item names as
/// the menu shows them.
pub(crate) fn invoke(
    window: &WindowInfo,
    path: &[String],
    deadline: Deadline,
) -> Result<Vec<String>, AdapterError> {
    let hwnd = crate::system::window::window_ops::parse_window_id(&window.id)?;
    let bar = unsafe { GetMenu(hwnd as _) };
    if bar.is_null() {
        return Err(AdapterError::new(
            ErrorCode::ActionNotSupported,
            "This window has no classic menu bar",
        )
        .with_suggestion(
            "Its menus are part of the window: find the menu item with snapshot and click it",
        ));
    }
    send(hwnd, WM_INITMENU, bar as usize, 0);
    let mut menu = bar;
    let mut opened = Vec::new();
    let mut chosen = Vec::new();
    for (depth, wanted) in path.iter().enumerate() {
        deadline.ensure_remaining()?;
        let items = items(menu);
        let Some(item) = items.iter().find(|item| same(&item.text, wanted)) else {
            close(hwnd, &opened);
            let names: Vec<&str> = items.iter().map(|item| item.text.as_str()).collect();
            let keys: serde_json::Map<String, serde_json::Value> = items
                .iter()
                .filter_map(|item| Some((item.text.clone(), item.keys.clone()?.into())))
                .collect();
            return Err(AdapterError::new(
                ErrorCode::ElementNotFound,
                format!("The menu has no item '{wanted}'"),
            )
            .with_details(serde_json::json!({
                "kind": "no_such_menu_item",
                "items": names,
                "shortcuts": keys,
            }))
            .with_suggestion("Choose one of details.items"));
        };
        chosen.push(item.text.clone());
        let last = depth + 1 == path.len();
        if item.disabled {
            close(hwnd, &opened);
            return Err(AdapterError::new(
                ErrorCode::ActionFailed,
                format!("The menu item '{}' is disabled now", item.text),
            ));
        }
        if !last {
            if item.submenu.is_null() {
                close(hwnd, &opened);
                return Err(AdapterError::new(
                    ErrorCode::ElementNotFound,
                    format!("The menu item '{}' has no submenu", item.text),
                ));
            }
            // Lets an app that fills its menus as they open (SWT, Eclipse) do it now.
            send(
                hwnd,
                WM_INITMENUPOPUP,
                item.submenu as usize,
                item.position as isize,
            );
            opened.push(item.submenu);
            menu = item.submenu;
            continue;
        }
        if !item.submenu.is_null() {
            close(hwnd, &opened);
            return Err(AdapterError::new(
                ErrorCode::InvalidArgs,
                format!("'{}' opens a submenu: name one of its items too", item.text),
            ));
        }
        // Posted, not sent: a command that opens a dialog must not keep this call waiting.
        if unsafe { PostMessageW(hwnd as _, WM_COMMAND, item.id as usize & 0xFFFF, 0) } == 0 {
            close(hwnd, &opened);
            return Err(AdapterError::new(
                ErrorCode::ActionFailed,
                "The window did not accept the menu command (WM_COMMAND)",
            ));
        }
        close(hwnd, &opened);
        return Ok(chosen);
    }
    Ok(chosen)
}

/// The path of the first item that shows `search`'s keys, or has its name, in
/// any submenu of `window`'s menu bar. Each submenu is told it opens before
/// it is read (apps that fill menus as they open), and that it closed after.
pub(crate) fn find(
    window: &WindowInfo,
    search: &MenuSearch,
    deadline: Deadline,
) -> Result<Vec<String>, AdapterError> {
    let hwnd = crate::system::window::window_ops::parse_window_id(&window.id)?;
    let bar = unsafe { GetMenu(hwnd as _) };
    if bar.is_null() {
        return Err(AdapterError::new(
            ErrorCode::ActionNotSupported,
            "This window has no classic menu bar",
        )
        .with_suggestion(
            "Its menus are part of the window: find the menu item with snapshot and click it",
        ));
    }
    send(hwnd, WM_INITMENU, bar as usize, 0);
    let mut path = Vec::new();
    let found = walk(hwnd, bar, search, &mut path, 0, deadline)?;
    if found {
        return Ok(path);
    }
    let what = match search {
        MenuSearch::Shortcut(keys) => format!("No menu item runs with {keys}"),
        MenuSearch::Name(name) => format!("The menu has no item '{name}'"),
    };
    Err(AdapterError::new(ErrorCode::ElementNotFound, what)
        .with_details(serde_json::json!({ "kind": "no_such_menu_item" }))
        .with_suggestion("List the items with menu --window-id W <a top menu name>"))
}

const MAX_MENU_DEPTH: usize = 5;

fn walk(
    hwnd: isize,
    menu: HMENU,
    search: &MenuSearch,
    path: &mut Vec<String>,
    depth: usize,
    deadline: Deadline,
) -> Result<bool, AdapterError> {
    deadline.ensure_remaining()?;
    for item in items(menu) {
        let hit = match search {
            MenuSearch::Shortcut(keys) => item
                .keys
                .as_deref()
                .is_some_and(|shown| same_keys(shown, keys)),
            MenuSearch::Name(name) => same(&item.text, name),
        };
        if hit && item.submenu.is_null() {
            path.push(item.text);
            return Ok(true);
        }
        if !item.submenu.is_null() && depth < MAX_MENU_DEPTH {
            send(
                hwnd,
                WM_INITMENUPOPUP,
                item.submenu as usize,
                item.position as isize,
            );
            path.push(item.text.clone());
            let found = walk(hwnd, item.submenu, search, path, depth + 1, deadline);
            unsafe { PostMessageW(hwnd as _, WM_UNINITMENUPOPUP, item.submenu as usize, 0) };
            if found? {
                return Ok(true);
            }
            path.pop();
        }
    }
    Ok(false)
}

/// Whether the keys a menu shows ("Ctrl+Enter", "Strg+Eingabe") are the keys
/// asked for ("ctrl+return"): same modifiers in any order, each key known by
/// any of its names (`key_names`: the English ones and what Windows calls it
/// in each keyboard layout installed).
pub(crate) fn same_keys(shown: &str, wanted: &str) -> bool {
    fn normal(text: &str) -> Vec<String> {
        let mut parts: Vec<String> = text
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(crate::actions::key_names::canonical)
            .collect();
        let rank = |part: &String| match part.as_str() {
            "ctrl" => 0,
            "alt" => 1,
            "shift" => 2,
            "win" => 3,
            _ => 4,
        };
        parts.sort_by_key(rank);
        parts
    }
    let shown = normal(shown);
    !shown.is_empty() && shown == normal(wanted)
}

/// Tells the window the submenus "opened" here closed again, after the command
/// already queued (posted, so they arrive in order).
fn close(hwnd: isize, opened: &[HMENU]) {
    for menu in opened.iter().rev() {
        unsafe { PostMessageW(hwnd as _, WM_UNINITMENUPOPUP, *menu as usize, 0) };
    }
}

fn send(hwnd: isize, message: u32, wparam: usize, lparam: isize) {
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(
            hwnd as _,
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            SEND_TIMEOUT_MS,
            &mut result,
        )
    };
}

fn items(menu: HMENU) -> Vec<Item> {
    let count = unsafe { GetMenuItemCount(menu) }.max(0) as u32;
    (0..count)
        .filter_map(|position| item(menu, position))
        .collect()
}

fn item(menu: HMENU, position: u32) -> Option<Item> {
    let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MENUITEMINFOW>() as u32;
    info.fMask = MIIM_STRING | MIIM_ID | MIIM_SUBMENU | MIIM_STATE | MIIM_FTYPE;
    if unsafe { GetMenuItemInfoW(menu, position, 1, &mut info) } == 0
        || info.fType & MFT_SEPARATOR != 0
    {
        return None;
    }
    let mut text = vec![0u16; info.cch as usize + 1];
    info.dwTypeData = text.as_mut_ptr();
    info.cch += 1;
    if unsafe { GetMenuItemInfoW(menu, position, 1, &mut info) } == 0 {
        return None;
    }
    let length = text
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(text.len());
    let raw = String::from_utf16_lossy(&text[..length]);
    Some(Item {
        text: display_name(&raw),
        keys: raw
            .split_once('\t')
            .map(|(_, keys)| keys.trim().to_owned())
            .filter(|keys| !keys.is_empty()),
        id: info.wID,
        submenu: info.hSubMenu,
        disabled: info.fState & MFS_DISABLED != 0,
        position,
    })
}

/// The name as a person reads it: no `&` shortcut marks, no `\t` key text.
pub(crate) fn display_name(raw: &str) -> String {
    let label = raw.split('\t').next().unwrap_or_default();
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                out.push('&');
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

/// Whether a menu name matches what was asked: ignoring case, shortcut marks
/// and a trailing `...`.
pub(crate) fn same(shown: &str, wanted: &str) -> bool {
    let plain = |text: &str| {
        display_name(text)
            .trim_end_matches('…')
            .trim_end_matches("...")
            .trim()
            .to_lowercase()
    };
    plain(shown) == plain(wanted)
}

/// A shortcut run as the button of the window that does it, through UI
/// Automation (Invoke), when the app has no classic menu for it: a button whose
/// accelerator key, or whose name ("Save (Ctrl+S)", "Guardar (Ctrl+G)"), shows
/// those keys, in whatever language the app names them (`same_keys`). The
/// button is never looked for by the words of its name. An app that takes keys
/// only while its editor has the keyboard still runs such buttons. `None` when
/// the window has no such button: the caller presses the keys.
pub(crate) fn run_button_shortcut(
    window: &WindowInfo,
    combo: &agent_desktop_core::KeyCombo,
) -> Option<agent_desktop_core::ActionResult> {
    use agent_desktop_core::{Action, ActionResult, ActionStep, Modifier, StepMechanism};
    use uiautomation::patterns::UIInvokePattern;
    use uiautomation::types::{Handle, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    if combo.modifiers.is_empty() {
        return None;
    }
    let mut keys: Vec<&str> = combo
        .modifiers
        .iter()
        .map(|modifier| match modifier {
            Modifier::Ctrl => "ctrl",
            Modifier::Alt => "alt",
            Modifier::Shift => "shift",
            Modifier::Meta => "win",
        })
        .collect();
    keys.push(&combo.key);
    let wanted = keys.join("+");
    let hwnd = crate::system::window::window_ops::parse_window_id(&window.id).ok()?;
    let client = crate::tree::automation::automation_client().ok()?;
    let root = client.element_from_handle(Handle::from(hwnd)).ok()?;
    let condition = client
        .create_property_condition(
            UIProperty::IsInvokePatternAvailable,
            Variant::from(true),
            None,
        )
        .ok()?;
    let buttons = root.find_all(TreeScope::Descendants, &condition).ok()?;
    let button = buttons.into_iter().find(|b| {
        b.get_accelerator_key()
            .is_ok_and(|keys| same_keys(&keys, &wanted))
            || b.get_name().is_ok_and(|name| {
                shown_keys(&name)
                    .iter()
                    .any(|keys| same_keys(keys, &wanted))
            })
    })?;
    button
        .get_pattern::<UIInvokePattern>()
        .ok()?
        .invoke()
        .ok()?;
    let step = ActionStep::succeeded("Invoke.ShortcutButton")
        .with_mechanism(StepMechanism::SemanticApi)
        .with_verified(false);
    Some(ActionResult::from_execution(
        &Action::PressKey(combo.clone()),
        vec![step],
    ))
}

/// The shortcuts a name shows at its end, in parentheses: "Save (Ctrl+S)" gives
/// ["Ctrl+S"], "Save (Ctrl+S, Alt+2)" gives ["Ctrl+S", "Alt+2"]. Empty when the
/// name ends with no such keys.
fn shown_keys(name: &str) -> Vec<&str> {
    let Some(inner) = name.trim_end().strip_suffix(')') else {
        return Vec::new();
    };
    let Some(open) = inner.rfind('(') else {
        return Vec::new();
    };
    inner[open + 1..]
        .split([',', ';'])
        .map(str::trim)
        .filter(|keys| keys.contains('+'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{display_name, same, same_keys, shown_keys};

    #[test]
    fn a_button_s_keys_are_read_from_its_name_in_any_language() {
        assert_eq!(shown_keys("Save (Ctrl+S)"), ["Ctrl+S"]);
        assert_eq!(shown_keys("Guardar (Ctrl+G)"), ["Ctrl+G"]);
        assert_eq!(shown_keys("保存 (Ctrl+S)"), ["Ctrl+S"]);
        assert_eq!(shown_keys("Save (Ctrl+S, Alt+2)"), ["Ctrl+S", "Alt+2"]);
        assert!(shown_keys("Bold").is_empty());
        assert!(shown_keys("Notes (draft)").is_empty());
        assert!(same_keys(shown_keys("Guardar (Ctrl+G)")[0], "ctrl+g"));
    }

    #[test]
    fn menu_keys_match_the_keys_a_recording_names() {
        assert!(same_keys("Ctrl+Enter", "ctrl+return"));
        assert!(same_keys("Shift+Ctrl+S", "ctrl+shift+s"));
        assert!(same_keys("Ctrl+]", "ctrl+]"));
        assert!(!same_keys("Ctrl+Shift+Enter", "ctrl+return"));
        assert!(!same_keys("", "ctrl+return"));
    }

    #[test]
    fn menu_names_read_as_a_person_reads_them() {
        assert_eq!(display_name("&File"), "File");
        assert_eq!(display_name("New SQL script\tCtrl+]"), "New SQL script");
        assert_eq!(display_name("Save && Close"), "Save & Close");
        assert!(same("Save &As...\tCtrl+Shift+S", "save as"));
        assert!(same("Export…", "Export..."));
        assert!(!same("Save", "Save As"));
    }
}
