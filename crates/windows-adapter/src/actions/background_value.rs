// Part of Agent Record Adapter (see NOTICE).
//! Writing a value and pressing a button without bringing the target's window
//! to the front.
//!
//! UI Automation sends no input, but its providers for the classic Win32
//! controls (also behind WinForms) activate the window first: `SetValue` on an
//! edit control and `Invoke` on a button both bring it to the front, and
//! whatever the person is typing at that moment would land in it. A headless
//! action on such a control goes as a window message instead, which never
//! activates anything:
//!
//! - an edit control gets its text as `WM_SETTEXT`;
//! - a push button's owner gets the `BN_CLICKED` command a real click ends in.
//!
//! Every other provider keeps its UI Automation pattern, under `focus_guard`.
//! Chromium (Chrome, Edge, Electron) is one: it activates its window as the
//! value or the click arrives, and the guard gives the foreground back within
//! milliseconds. Typing into Chromium by window messages is not used: while
//! the window believes it is active, keys the person is pressing reach it.

use agent_desktop_core::{ActionStep, AdapterError, Deadline, ErrorCode, StepMechanism};
use windows::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, GetClassNameW, GetDlgCtrlID, GetParent, PostMessageW, SMTO_ABORTIFHUNG, SMTO_BLOCK,
    SendMessageTimeoutW, WM_COMMAND, WM_SETTEXT,
};

use crate::actions::value_write::gated_pattern_value_equals;
use crate::tree::element::UIAElement;

const SET_TEXT_LABEL: &str = "WindowMessage.WM_SETTEXT";
const SET_TEXT_TIMEOUT_MS: u32 = 2_000;
const CLICK_LABEL: &str = "WindowMessage.BN_CLICKED";

/// Writes `value` into `element` without activating its window, or `None`
/// when the element is not a Win32 edit control.
pub(crate) fn background_set_value(
    element: &UIAElement,
    value: &str,
    deadline: Deadline,
) -> Result<Option<Vec<ActionStep>>, AdapterError> {
    match win32_edit(element) {
        Some(edit) => set_text(element, edit, value, deadline).map(|step| Some(vec![step])),
        None => Ok(None),
    }
}
/// The element's own window handle and its window class, in lower case.
fn native_window(element: &UIAElement) -> Option<(isize, String)> {
    let handle = element.0.get_native_window_handle().ok()?;
    let hwnd: HWND = handle.into();
    let hwnd = hwnd.0 as isize;
    if hwnd == 0 {
        return None;
    }
    let mut class = [0u16; 128];
    let length = unsafe { GetClassNameW(hwnd as _, class.as_mut_ptr(), class.len() as i32) };
    if length <= 0 {
        return None;
    }
    Some((
        hwnd,
        String::from_utf16_lossy(&class[..length as usize]).to_ascii_lowercase(),
    ))
}

/// The element's window handle when it is a Win32 edit control
/// (`Edit`, `RichEdit*`, `WindowsForms10.EDIT.*`).
fn win32_edit(element: &UIAElement) -> Option<isize> {
    let (hwnd, class) = native_window(element)?;
    let edit = class == "edit"
        || class.starts_with("richedit")
        || class.starts_with("windowsforms10.edit");
    edit.then_some(hwnd)
}

/// Presses a Win32 push button (`Button`, `WindowsForms10.BUTTON.*`) by
/// posting to its owner the `BN_CLICKED` command a real click ends in, or
/// returns `None` for anything else. Posted, not sent: a handler that opens a
/// dialog must not keep this call waiting.
pub(crate) fn background_click(element: &UIAElement) -> Result<Option<ActionStep>, AdapterError> {
    let Some((hwnd, class)) = native_window(element) else {
        return Ok(None);
    };
    let button = class == "button" || class.starts_with("windowsforms10.button");
    let push_button = element
        .0
        .get_control_type()
        .is_ok_and(|kind| kind == uiautomation::types::ControlType::Button);
    if !button || !push_button || unsafe { IsWindowEnabled(hwnd as _) } == 0 {
        return Ok(None);
    }
    let owner = unsafe { GetParent(hwnd as _) };
    if owner.is_null() {
        return Ok(None);
    }
    let id = unsafe { GetDlgCtrlID(hwnd as _) } as u32 as usize & 0xFFFF;
    let command = id | ((BN_CLICKED as usize) << 16);
    if unsafe { PostMessageW(owner, WM_COMMAND, command, hwnd) } == 0 {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The button's window did not accept the click (WM_COMMAND)",
        ));
    }
    Ok(Some(
        ActionStep::succeeded(CLICK_LABEL)
            .with_mechanism(StepMechanism::SemanticApi)
            .with_verified(false),
    ))
}

fn set_text(
    element: &UIAElement,
    hwnd: isize,
    value: &str,
    deadline: Deadline,
) -> Result<ActionStep, AdapterError> {
    deadline.ensure_remaining()?;
    let mut text: Vec<u16> = value.encode_utf16().collect();
    text.push(0);
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd as _,
            WM_SETTEXT,
            0,
            text.as_ptr() as isize,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            SET_TEXT_TIMEOUT_MS,
            &mut result,
        )
    };
    text.iter_mut().for_each(|unit| *unit = 0);
    if sent == 0 || result == 0 {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The text box did not accept the text (WM_SETTEXT)",
        ));
    }
    let verified = gated_pattern_value_equals(element, value)?;
    Ok(ActionStep::succeeded(SET_TEXT_LABEL)
        .with_mechanism(StepMechanism::SemanticApi)
        .with_verified(verified.unwrap_or(false)))
}
