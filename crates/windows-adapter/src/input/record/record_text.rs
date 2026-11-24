// Part of Agent Record Adapter (see NOTICE).
//! The characters a key press types, read from the foreground window's
//! keyboard layout without touching the layout's dead-key state.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, ToUnicodeEx};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::record_keys::ModifierState;

const DO_NOT_CHANGE_KEYBOARD_STATE: u32 = 0x4;
const KEY_DOWN: u8 = 0x80;
const KEY_TOGGLED: u8 = 0x01;
const VK_SHIFT: usize = 0x10;
const VK_CONTROL: usize = 0x11;
const VK_MENU: usize = 0x12;
const VK_CAPITAL: usize = 0x14;
const VK_LSHIFT: usize = 0xA0;
const VK_LCONTROL: usize = 0xA2;
const VK_RMENU: usize = 0xA5;

/// The text `vk` types under `modifiers`, or `None` for a key that types no
/// printable character. A dead key types nothing on its own here, so an
/// accent composed with a dead key is reported without its accent; the
/// recorded field value, when the control exposes one, is the exact text.
pub(super) fn typed_text(vk: u32, scan: u32, modifiers: &ModifierState) -> Option<String> {
    let mut state = [0u8; 256];
    if modifiers.shift {
        state[VK_SHIFT] = KEY_DOWN;
        state[VK_LSHIFT] = KEY_DOWN;
    }
    if modifiers.ctrl {
        state[VK_CONTROL] = KEY_DOWN;
        state[VK_LCONTROL] = KEY_DOWN;
    }
    if modifiers.alt {
        state[VK_MENU] = KEY_DOWN;
        state[VK_RMENU] = KEY_DOWN;
    }
    if modifiers.caps_lock {
        state[VK_CAPITAL] = KEY_TOGGLED;
    }
    let layout = unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        GetKeyboardLayout(thread)
    };
    let mut buffer = [0u16; 8];
    let written = unsafe {
        ToUnicodeEx(
            vk,
            scan,
            state.as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            DO_NOT_CHANGE_KEYBOARD_STATE,
            layout,
        )
    };
    if written <= 0 {
        return None;
    }
    let text = String::from_utf16(&buffer[..written as usize]).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text)
}
