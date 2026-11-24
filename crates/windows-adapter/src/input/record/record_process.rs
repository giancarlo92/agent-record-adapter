// Part of Agent Record Adapter (see NOTICE).
//! The executable behind a recorded window.
//!
//! It is read when the action happens, while the process still runs: a
//! demonstration often closes the app at the end, and a replay must be able
//! to start that app again from its path.

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

/// Full path of the executable of process `pid`, or `None` when it cannot be
/// read (the process ended, or it belongs to a more privileged session).
pub(super) fn image_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0_u16; 32_768];
    let mut size = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
    unsafe { CloseHandle(handle) };
    (ok != 0 && size != 0).then(|| String::from_utf16_lossy(&buffer[..size as usize]))
}

#[cfg(test)]
#[path = "record_process_tests.rs"]
mod tests;
