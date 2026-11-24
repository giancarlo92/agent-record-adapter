// Part of Agent Record Adapter (see NOTICE).
//! Classifies the keys a `record` session sees and names them in the
//! vocabulary `press` accepts (`return`, `ctrl+s`, `shift+tab`).

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_CAPITAL: u32 = 0x14;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_DELETE: u32 = 0x2E;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;
const VK_F1: u32 = 0x70;
const VK_F12: u32 = 0x7B;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModifierKey {
    Shift,
    Ctrl,
    Alt,
    Win,
    CapsLock,
}

/// What a key does to a recording.
///
/// `Commit` keys end the text being typed and are recorded as a `press`.
/// `Navigation` keys edit inside a text field but are a `press` anywhere
/// else. `Editing` and `Text` keys only change the text being typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyClass {
    Modifier(ModifierKey),
    Commit,
    Navigation,
    Editing,
    Text,
}

pub(super) fn classify(vk: u32) -> KeyClass {
    match vk {
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => KeyClass::Modifier(ModifierKey::Shift),
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => KeyClass::Modifier(ModifierKey::Ctrl),
        VK_MENU | VK_LMENU | VK_RMENU => KeyClass::Modifier(ModifierKey::Alt),
        VK_LWIN | VK_RWIN => KeyClass::Modifier(ModifierKey::Win),
        VK_CAPITAL => KeyClass::Modifier(ModifierKey::CapsLock),
        VK_RETURN | VK_TAB | VK_ESCAPE | VK_PRIOR | VK_NEXT => KeyClass::Commit,
        VK_F1..=VK_F12 => KeyClass::Commit,
        VK_LEFT | VK_UP | VK_RIGHT | VK_DOWN | VK_HOME | VK_END => KeyClass::Navigation,
        VK_BACK | VK_DELETE => KeyClass::Editing,
        _ => KeyClass::Text,
    }
}

/// The `press` name of a key, or `None` for a key `press` cannot name
/// (punctuation and other layout-dependent keys).
pub(super) fn key_name(vk: u32) -> Option<String> {
    let named = match vk {
        VK_RETURN => "return",
        VK_TAB => "tab",
        VK_ESCAPE => "escape",
        VK_SPACE => "space",
        VK_BACK => "backspace",
        VK_DELETE => "forwarddelete",
        VK_PRIOR => "pageup",
        VK_NEXT => "pagedown",
        VK_END => "end",
        VK_HOME => "home",
        VK_LEFT => "left",
        VK_UP => "up",
        VK_RIGHT => "right",
        VK_DOWN => "down",
        VK_F1..=VK_F12 => return Some(format!("f{}", vk - VK_F1 + 1)),
        0x30..=0x39 | 0x41..=0x5A => {
            return char::from_u32(vk).map(|c| c.to_ascii_lowercase().to_string());
        }
        _ => return None,
    };
    Some(named.to_string())
}

/// The modifiers held while the session watches the keyboard, tracked from
/// the hook's own transitions so they never depend on another thread's
/// keyboard state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ModifierState {
    pub(super) shift: bool,
    pub(super) ctrl: bool,
    pub(super) alt: bool,
    pub(super) win: bool,
    pub(super) caps_lock: bool,
}

impl ModifierState {
    pub(super) fn apply(&mut self, key: ModifierKey, down: bool) {
        match key {
            ModifierKey::Shift => self.shift = down,
            ModifierKey::Ctrl => self.ctrl = down,
            ModifierKey::Alt => self.alt = down,
            ModifierKey::Win => self.win = down,
            ModifierKey::CapsLock if down => self.caps_lock = !self.caps_lock,
            ModifierKey::CapsLock => {}
        }
    }

    /// Whether the next key is a shortcut rather than text. Ctrl and Alt
    /// held together are AltGr on layouts that type characters with it, so
    /// that pair alone does not make a shortcut.
    pub(super) fn is_shortcut(&self) -> bool {
        self.win || (self.ctrl != self.alt)
    }

    /// The `press` combo for `key` under the held modifiers, in the order
    /// `press` documents: `ctrl`, `alt`, `shift`, `cmd`.
    pub(super) fn combo(&self, key: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        if self.win {
            parts.push("cmd");
        }
        parts.push(key);
        parts.join("+")
    }
}

#[cfg(test)]
#[path = "record_keys_tests.rs"]
mod tests;
