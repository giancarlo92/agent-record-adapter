// Part of Agent Record Adapter (see NOTICE).
//! The names a shortcut's keys have in any language.
//!
//! A menu or a button shows its keys in the language of the app ("Ctrl+S",
//! "Strg+S", "Mayús+Ctrl+S", "Maj+Ctrl+S"...). No list of translations is kept
//! here: Windows itself names every key for each keyboard layout installed
//! (`GetKeyNameTextW`), and those names, with the English ones, map to one
//! canonical name each.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The keys a shortcut shows by name, with the canonical name used here.
const NAMED_KEYS: &[(u16, &str)] = &[
    (0x11, "ctrl"),      // VK_CONTROL
    (0x10, "shift"),     // VK_SHIFT
    (0x12, "alt"),       // VK_MENU
    (0x0D, "enter"),     // VK_RETURN
    (0x1B, "escape"),    // VK_ESCAPE
    (0x2E, "delete"),    // VK_DELETE
    (0x2D, "insert"),    // VK_INSERT
    (0x21, "pageup"),    // VK_PRIOR
    (0x22, "pagedown"),  // VK_NEXT
    (0x24, "home"),      // VK_HOME
    (0x23, "end"),       // VK_END
    (0x09, "tab"),       // VK_TAB
    (0x20, "space"),     // VK_SPACE
    (0x08, "backspace"), // VK_BACK
    (0x25, "left"),      // VK_LEFT
    (0x26, "up"),        // VK_UP
    (0x27, "right"),     // VK_RIGHT
    (0x28, "down"),      // VK_DOWN
];

/// The keys that sit on the numeric keypad's block or beside it share their scan
/// code with a keypad key; the extended bit names the one meant.
const EXTENDED: &[u16] = &[0x2E, 0x2D, 0x21, 0x22, 0x24, 0x23, 0x25, 0x26, 0x27, 0x28];

/// The canonical name of a key as a shortcut shows it (`"Strg"` gives
/// `"ctrl"`), or the text itself, lowercased, when it names no known key.
pub(crate) fn canonical(part: &str) -> String {
    let part = part.trim().to_lowercase();
    aliases().get(&part).map_or(part, |name| (*name).to_owned())
}

fn aliases() -> &'static HashMap<String, &'static str> {
    static ALIASES: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    ALIASES.get_or_init(|| {
        let mut map: HashMap<String, &'static str> = HashMap::new();
        // The English names, and the usual short forms of them.
        for (alias, name) in [
            ("control", "ctrl"),
            ("ctl", "ctrl"),
            ("return", "enter"),
            ("esc", "escape"),
            ("del", "delete"),
            ("ins", "insert"),
            ("pgup", "pageup"),
            ("page up", "pageup"),
            ("prior", "pageup"),
            ("pgdn", "pagedown"),
            ("page down", "pagedown"),
            ("next", "pagedown"),
            ("windows", "win"),
            ("meta", "win"),
            ("super", "win"),
        ] {
            map.insert(alias.to_owned(), name);
        }
        for (name, canonical) in system_names() {
            map.entry(name).or_insert(canonical);
        }
        map
    })
}

/// What Windows calls each named key in every keyboard layout installed.
#[cfg(target_os = "windows")]
fn system_names() -> Vec<(String, &'static str)> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        ActivateKeyboardLayout, GetKeyNameTextW, GetKeyboardLayout, GetKeyboardLayoutList,
        MAPVK_VK_TO_VSC, MapVirtualKeyW,
    };
    let mut names = Vec::new();
    // SAFETY: the calls read the keyboard layouts and key names into buffers
    // of the sizes given; switching the layout affects only this thread and
    // is undone below.
    unsafe {
        let original = GetKeyboardLayout(0);
        let count = GetKeyboardLayoutList(0, std::ptr::null_mut());
        let mut layouts = vec![std::ptr::null_mut(); count.max(0) as usize];
        let count = GetKeyboardLayoutList(count, layouts.as_mut_ptr());
        layouts.truncate(count.max(0) as usize);
        for layout in layouts {
            ActivateKeyboardLayout(layout, 0);
            for (vk, canonical) in NAMED_KEYS {
                let scan = MapVirtualKeyW(u32::from(*vk), MAPVK_VK_TO_VSC);
                if scan == 0 {
                    continue;
                }
                let extended = if EXTENDED.contains(vk) { 1 << 24 } else { 0 };
                let mut buffer = [0u16; 64];
                let length = GetKeyNameTextW(
                    ((scan as i32) << 16) | extended,
                    buffer.as_mut_ptr(),
                    buffer.len() as i32,
                );
                if length > 0 {
                    let name = String::from_utf16_lossy(&buffer[..length as usize]);
                    names.push((name.trim().to_lowercase(), *canonical));
                }
            }
        }
        ActivateKeyboardLayout(original, 0);
    }
    names
}

#[cfg(not(target_os = "windows"))]
fn system_names() -> Vec<(String, &'static str)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_names_and_this_computer_s_own_key_names_are_canonical() {
        assert_eq!(canonical("Control"), "ctrl");
        assert_eq!(canonical(" Return "), "enter");
        assert_eq!(canonical("S"), "s");
        // Whatever this computer's keyboard layouts call Ctrl, it is ctrl.
        for (name, key) in system_names() {
            assert_eq!(canonical(&name), key, "{name}");
        }
    }
}
