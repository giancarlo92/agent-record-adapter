// Part of Agent Record Adapter (see NOTICE).
//! Puts the windows a program just opened behind every other window.
//!
//! A window shown without being activated still lands on top of the stack,
//! right under the window in use: it covers the person's other windows, and
//! it is the one Windows activates next when the person closes or minimizes
//! what they have in front. Sent to the bottom of the stack, it is neither.
//! The person brings it forward whenever they want from the taskbar.
//!
//! A Chromium window is left where it is unless the caller says it keeps
//! working while covered (a browser started for background work does): one
//! that other windows cover completely stops updating its page and its
//! accessibility tree, and at the bottom of the stack it is always covered.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetWindowThreadProcessId, HWND_BOTTOM,
    IsWindowVisible, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPos,
};

const SWEEP_EVERY: Duration = Duration::from_millis(30);

pub(crate) struct Behind {
    sent: Vec<isize>,
    last: Option<Instant>,
    chromium_too: bool,
}

impl Behind {
    pub(crate) fn new(chromium_too: bool) -> Self {
        Self {
            sent: Vec::new(),
            last: None,
            chromium_too,
        }
    }

    /// Sends each visible top-level window of the target that has not been
    /// sent yet to the bottom of the stack, without activating anything.
    pub(crate) fn sweep(&mut self, mut is_target: impl FnMut(u32) -> bool) {
        if self.last.is_some_and(|last| last.elapsed() < SWEEP_EVERY) {
            return;
        }
        self.last = Some(Instant::now());
        let mut windows: Vec<isize> = Vec::new();
        unsafe { EnumWindows(Some(collect), &mut windows as *mut Vec<isize> as LPARAM) };
        for window in windows {
            if self.sent.contains(&window) {
                continue;
            }
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(window as HWND, &mut pid) };
            if !is_target(pid) || (!self.chromium_too && is_chromium(window as HWND)) {
                continue;
            }
            // An always-on-top window (a recorder's bar, a floating palette)
            // is meant to float: sending it to the bottom would also take its
            // topmost style away.
            if is_topmost(window as HWND) {
                self.sent.push(window);
                continue;
            }
            unsafe {
                SetWindowPos(
                    window as HWND,
                    HWND_BOTTOM,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            self.sent.push(window);
        }
    }
}

/// The visible top-level windows now, as the `w-<handle>` ids `list-windows`
/// reports, noted before a background click (see `note_windows_before_action`
/// in the core): a dialog the click opens is then counted as opened by a
/// `wait --event window-opened` that starts after it appeared.
pub(crate) fn note_open_windows() {
    let mut windows: Vec<isize> = Vec::new();
    unsafe { EnumWindows(Some(collect), &mut windows as *mut Vec<isize> as LPARAM) };
    agent_desktop_core::background::note_windows_before_action(
        windows
            .into_iter()
            .map(|w| format!("w-{}", w as usize))
            .collect(),
    );
}

fn is_chromium(window: HWND) -> bool {
    let mut class = [0u16; 64];
    let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    length > 0
        && String::from_utf16_lossy(&class[..length as usize]).starts_with("Chrome_WidgetWin")
}

unsafe extern "system" fn collect(window: HWND, list: LPARAM) -> i32 {
    if unsafe { IsWindowVisible(window) } != 0 {
        let windows = unsafe { &mut *(list as *mut Vec<isize>) };
        windows.push(window as isize);
    }
    1
}

/// Puts `person` back in front. A process that is not in front may not do
/// that on its own, so this thread joins the input queue of the window that
/// took the foreground for the call.
pub(crate) fn give_back(person: HWND, taken_by: HWND) {
    let own = unsafe { GetCurrentThreadId() };
    let theirs = unsafe { GetWindowThreadProcessId(taken_by, std::ptr::null_mut()) };
    let attached =
        theirs != 0 && theirs != own && unsafe { AttachThreadInput(own, theirs, 1) } != 0;
    unsafe {
        BringWindowToTop(person);
        SetForegroundWindow(person);
    }
    if attached {
        unsafe { AttachThreadInput(own, theirs, 0) };
    }
}

/// Whether a window is the taskbar or the window switcher: a click there is
/// the person choosing a window.
pub(crate) fn is_taskbar(window: HWND) -> bool {
    let mut class = [0u16; 64];
    let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    if length <= 0 {
        return false;
    }
    let class = String::from_utf16_lossy(&class[..length as usize]);
    matches!(
        class.as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "XamlExplorerHostIslandWindow"
    )
}

/// Minimize or restore without activating the window (background mode); other
/// window operations are upstream's. The report is the one upstream builds
/// for a window operation.
pub(crate) fn window_op_without_activation(
    win: &agent_desktop_core::WindowInfo,
    op: &agent_desktop_core::WindowOp,
) -> Option<Result<agent_desktop_core::OperationReport, agent_desktop_core::AdapterError>> {
    use agent_desktop_core::WindowOp;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, ShowWindowAsync,
    };
    let show = match op {
        WindowOp::Minimize => SW_SHOWMINNOACTIVE,
        WindowOp::Restore => SW_SHOWNOACTIVATE,
        _ => return None,
    };
    let hwnd = match crate::system::window::window_ops::parse_window_id(&win.id) {
        Ok(hwnd) => hwnd,
        Err(error) => return Some(Err(error)),
    };
    // SAFETY: ShowWindowAsync takes any window handle; a stale one fails.
    let sent = unsafe { ShowWindowAsync(hwnd as HWND, show) } != 0
        || crate::tree::automation::window_exists(hwnd);
    let dispatched = if sent {
        Ok(())
    } else {
        Err(agent_desktop_core::AdapterError::new(
            agent_desktop_core::ErrorCode::WindowNotFound,
            "The window is gone",
        ))
    };
    Some(agent_desktop_core::OperationReport::from_window_dispatch(
        win, op, None, dispatched,
    ))
}

/// Whether a window is always on top (`WS_EX_TOPMOST`).
pub(crate) fn is_topmost(window: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_TOPMOST,
    };
    // SAFETY: GetWindowLongPtrW reads a style of any window handle; a stale
    // one answers 0.
    let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;
    style & WS_EX_TOPMOST != 0
}
