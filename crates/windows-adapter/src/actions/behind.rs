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
    let locked = foreground_lock::pause();
    unsafe {
        BringWindowToTop(person);
        SetForegroundWindow(person);
    }
    if locked {
        foreground_lock::resume();
    }
    if attached {
        unsafe { AttachThreadInput(own, theirs, 0) };
    }
}

/// Keeps other programs from taking the foreground while an action runs.
///
/// Windows lets a program take the foreground only while the person has not
/// used the keyboard or mouse for a while (the foreground lock timeout, 200 s
/// by default) or when it was just started from the window in front. Then a
/// browser told it is active (so it takes a click or keys behind the person's
/// windows), or restored behind them, brings itself forward. In those same
/// cases this process may lock the foreground (`LockSetForegroundWindow`),
/// and then nobody else can take it; when it may not, nobody else may either,
/// and the lock is not needed. The person's own clicks and Alt+Tab still
/// switch windows (the system lifts the lock for them), and the lock ends
/// with the action, or with this process.
pub(crate) mod foreground_lock {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LSFW_LOCK, LSFW_UNLOCK, LockSetForegroundWindow,
    };

    /// How many holders want the lock now, in this process.
    static HOLDERS: AtomicUsize = AtomicUsize::new(0);

    /// Held while an action runs; released when the last holder drops.
    pub(crate) struct Held(());

    pub(crate) fn hold() -> Held {
        if HOLDERS.fetch_add(1, Ordering::SeqCst) == 0 {
            // SAFETY: takes no pointers; fails harmlessly when not allowed.
            let locked = unsafe { LockSetForegroundWindow(LSFW_LOCK) } != 0;
            if std::env::var_os("AGENT_DESKTOP_GUARD_DEBUG").is_some() {
                let error = std::io::Error::last_os_error();
                eprintln!("foreground lock: taken={locked} ({error})");
            }
        }
        Held(())
    }

    impl Drop for Held {
        fn drop(&mut self) {
            if HOLDERS.fetch_sub(1, Ordering::SeqCst) == 1 {
                // SAFETY: as above.
                unsafe { LockSetForegroundWindow(LSFW_UNLOCK) };
            }
        }
    }

    /// Lifts the lock for a moment (to give the person their window back);
    /// true when it was held, so `resume` puts it back.
    pub(crate) fn pause() -> bool {
        let held = HOLDERS.load(Ordering::SeqCst) > 0;
        if held {
            // SAFETY: as above.
            unsafe { LockSetForegroundWindow(LSFW_UNLOCK) };
        }
        held
    }

    pub(crate) fn resume() {
        if HOLDERS.load(Ordering::SeqCst) > 0 {
            // SAFETY: as above.
            unsafe { LockSetForegroundWindow(LSFW_LOCK) };
        }
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
    // The window handles the change on its own thread a moment later: nobody
    // takes the foreground meanwhile (see `foreground_lock`).
    let _lock = foreground_lock::hold();
    // SAFETY: ShowWindowAsync takes any window handle; a stale one fails.
    let sent = unsafe { ShowWindowAsync(hwnd as HWND, show) } != 0
        || crate::tree::automation::window_exists(hwnd);
    if sent {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
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
