// Part of Agent Record Adapter (see NOTICE).
//! Closing one window as its Close button does: `WM_CLOSE` is posted to it,
//! which never activates it and needs no words in any language. The app
//! handles it as a click on its Close button (it may ask to save first).

use std::time::{Duration, Instant};

use agent_desktop_core::{AdapterError, ErrorCode, WindowInfo};
use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW, WM_CLOSE};

/// How long a window gets to close before it is reported as still open.
const CLOSE_WAIT: Duration = Duration::from_secs(3);

/// Asks the window to close; true when it closed within `CLOSE_WAIT`.
pub(crate) fn close(window: &WindowInfo) -> Result<bool, AdapterError> {
    let hwnd = crate::system::window::window_ops::parse_window_id(&window.id)?;
    // SAFETY: `hwnd` came from a window id; a stale handle makes these calls fail, never crash.
    if unsafe { IsWindow(hwnd as _) } == 0 {
        return Err(AdapterError::new(
            ErrorCode::WindowNotFound,
            "The window is no longer open",
        ));
    }
    // SAFETY: posting a message with no pointers in it.
    if unsafe { PostMessageW(hwnd as _, WM_CLOSE, 0, 0) } == 0 {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The window did not take the close request",
        ));
    }
    let start = Instant::now();
    while start.elapsed() < CLOSE_WAIT {
        // SAFETY: as above.
        if unsafe { IsWindow(hwnd as _) } == 0 {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(false)
}
