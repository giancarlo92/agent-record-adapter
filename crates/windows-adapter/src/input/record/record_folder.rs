// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! The folder a File Explorer window shows, for a recording: its title only
//! names the last folder ("inbox"), and a replay has to open that same folder.
//! Read from the Shell's list of its open windows (`IShellWindows`): each one's
//! window handle and the address it shows (`file:///C:/...`).

use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{IShellWindows, IWebBrowserApp, ShellWindows};
use windows::core::Interface;

/// The path of the folder the Explorer window `hwnd` shows, when it shows a
/// folder of the file system (not "Home" or "This PC").
pub(super) fn folder_of(hwnd: isize) -> Option<String> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
        let count = windows.Count().ok()?;
        for i in 0..count {
            let Ok(item) = windows.Item(&VARIANT::from(i)) else {
                continue;
            };
            let Ok(browser) = item.cast::<IWebBrowserApp>() else {
                continue;
            };
            if browser.HWND().ok().map(|h| h.0) != Some(hwnd) {
                continue;
            }
            let url = browser.LocationURL().ok()?.to_string();
            return path_of(&url);
        }
        None
    }
}

/// `file:///C:/a%20b` → `C:\a b`; anything else (a shell location) → none.
fn path_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file:///")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && i + 2 < raw.len()
            && let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16)
        {
            bytes.push(v);
            i += 3;
            continue;
        }
        bytes.push(raw[i]);
        i += 1;
    }
    Some(String::from_utf8_lossy(&bytes).replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_file_url_becomes_a_windows_path() {
        assert_eq!(
            super::path_of("file:///C:/Users/a%20b/inbox").as_deref(),
            Some(r"C:\Users\a b\inbox")
        );
        assert_eq!(super::path_of("::{20D04FE0}"), None);
    }
}
