// Part of Agent Record Adapter (see NOTICE).
//! Keeping the dedicated Chrome's windows behind the person's.

/// The person's foreground window now (0 when there is none or off Windows).
#[cfg(target_os = "windows")]
pub fn foreground() -> isize {
    // SAFETY: GetForegroundWindow takes no arguments and only reads.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as isize }
}

#[cfg(not(target_os = "windows"))]
pub fn foreground() -> isize {
    0
}

/// Waits (up to 3 s) for the visible windows of process `pid`, sends them to
/// the bottom of the stack without activating them, and gives the foreground
/// back to `before` when one of them took it.
#[cfg(target_os = "windows")]
pub fn send_behind(pid: u32, before: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetForegroundWindow, SetWindowPos,
    };
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut windows = Vec::new();
    while windows.is_empty() && std::time::Instant::now() < end {
        windows = visible_windows_of(pid);
        if windows.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    for hwnd in &windows {
        // SAFETY: hwnd came from EnumWindows; SetWindowPos fails harmlessly on
        // a window that closed meanwhile.
        unsafe {
            SetWindowPos(
                *hwnd as _,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    // SAFETY: plain reads of the foreground window and its owner.
    unsafe {
        let now = GetForegroundWindow();
        let mut owner = 0u32;
        GetWindowThreadProcessId(now, &mut owner);
        if owner == pid && before != 0 && before != now as isize {
            SetForegroundWindow(before as _);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn send_behind(_pid: u32, _before: isize) {}

#[cfg(target_os = "windows")]
fn visible_windows_of(pid: u32) -> Vec<isize> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };
    struct Search {
        pid: u32,
        found: Vec<isize>,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
        // SAFETY: `data` is the &mut Search passed to EnumWindows below, alive
        // for the whole enumeration.
        let search = unsafe { &mut *(data as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: hwnd is a live top-level window handed by EnumWindows.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner == search.pid && IsWindowVisible(hwnd) != 0 {
                search.found.push(hwnd as isize);
            }
        }
        1
    }
    let mut search = Search {
        pid,
        found: Vec::new(),
    };
    // SAFETY: the callback only touches `search`, which outlives the call.
    unsafe {
        EnumWindows(Some(each), &mut search as *mut Search as LPARAM);
    }
    search.found
}
