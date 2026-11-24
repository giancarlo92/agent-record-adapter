// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! The work desktop: background mode's way for apps that only take input in
//! an active window.
//!
//! Some apps ignore every key and click sent to a window in the background:
//! WordCraft and DeckCraft (egui) take keys only while their window is active,
//! and GridCraft handles a key right after another one only when it paints a
//! frame, which a window behind the person's does not always do. Activating
//! them takes the person's foreground. A Windows desktop of its own does not:
//! a program started there never shows on the person's screen, and its window
//! is the active one of that desktop, so it takes keys as it would from a
//! person.
//!
//! With `AGENT_DESKTOP_WORK_DESKTOP=<name>` (letters, digits, `-`, `_`) every
//! command runs on that desktop: this process starts the same command again
//! there, with the same standard handles, and returns its exit code. What it
//! launches opens there, and snapshots and actions see only that desktop's
//! windows, never the person's. When the desktop cannot be made, the command
//! runs on the person's desktop as the original does. Background mode off
//! (`AGENT_DESKTOP_BACKGROUND=0`) ignores it.
//!
//! The real pointer and keyboard only reach the desktop on screen: on the work
//! desktop input goes as window messages (an app that only takes the real
//! pointer, as Windows 11 Paint's canvas, cannot be drawn on there).

/// The variable that names the work desktop.
pub const ENV: &str = "AGENT_DESKTOP_WORK_DESKTOP";

/// The work desktop's name, when one is set and valid.
pub fn name() -> Option<String> {
    let value = std::env::var(ENV).ok()?;
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| value.to_owned())
}

/// Whether this process runs on the work desktop.
pub(crate) fn active() -> bool {
    use std::sync::OnceLock;
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| {
        name()
            .is_some_and(|name| imp::current().is_some_and(|here| here.eq_ignore_ascii_case(&name)))
    })
}

/// Runs this command again on the work desktop when one is set and this
/// process is not on it yet, and returns that run's exit code. `None`: no work
/// desktop, background mode off, already there, or the desktop or the process
/// could not be made (the command then runs here).
pub fn run_there() -> Option<u32> {
    if !agent_desktop_core::background::enabled() {
        return None;
    }
    let name = name()?;
    if imp::current().is_some_and(|here| here.eq_ignore_ascii_case(&name)) {
        return None;
    }
    imp::run_on(&name)
}

#[cfg(target_os = "windows")]
mod imp {
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, CreateDesktopW, GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetCurrentThreadId, GetExitCodeProcess, INFINITE, PROCESS_INFORMATION,
        STARTF_USESTDHANDLES, STARTUPINFOW, WaitForSingleObject,
    };

    const GENERIC_ALL: u32 = 0x1000_0000;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The name of this thread's desktop ("Default" on the person's).
    pub(super) fn current() -> Option<String> {
        unsafe {
            let desk = GetThreadDesktop(GetCurrentThreadId());
            if desk.is_null() {
                return None;
            }
            let mut buf = [0u16; 256];
            let mut needed = 0u32;
            let ok = GetUserObjectInformationW(
                desk as HANDLE,
                UOI_NAME,
                buf.as_mut_ptr().cast(),
                (buf.len() * 2) as u32,
                &mut needed,
            );
            if ok == 0 {
                return None;
            }
            let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }

    pub(super) fn run_on(name: &str) -> Option<u32> {
        let exe = std::env::current_exe().ok()?;
        unsafe {
            // Made the first time, opened after: it lasts while a program runs on it.
            let desk = CreateDesktopW(
                wide(name).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                GENERIC_ALL,
                std::ptr::null(),
            );
            if desk.is_null() {
                tracing::warn!("work desktop {name}: it could not be made; running here");
                return None;
            }
            let std_handles = [
                GetStdHandle(STD_INPUT_HANDLE),
                GetStdHandle(STD_OUTPUT_HANDLE),
                GetStdHandle(STD_ERROR_HANDLE),
            ];
            for handle in std_handles {
                if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
                }
            }
            let mut desktop = wide(&format!("WinSta0\\{name}"));
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            startup.lpDesktop = desktop.as_mut_ptr();
            startup.dwFlags = STARTF_USESTDHANDLES;
            startup.hStdInput = std_handles[0];
            startup.hStdOutput = std_handles[1];
            startup.hStdError = std_handles[2];
            let mut command: Vec<u16> =
                wide(&windows_command_line(&exe, std::env::args_os().skip(1)));
            let mut process: PROCESS_INFORMATION = std::mem::zeroed();
            let started = CreateProcessW(
                wide(&exe.to_string_lossy()).as_ptr(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut process,
            );
            if started == 0 {
                CloseDesktop(desk);
                tracing::warn!(
                    "work desktop {name}: the command could not start there; running here"
                );
                return None;
            }
            WaitForSingleObject(process.hProcess, INFINITE);
            let mut code = 1u32;
            GetExitCodeProcess(process.hProcess, &mut code);
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
            CloseDesktop(desk);
            Some(code)
        }
    }

    /// A command line Windows parses back into the same arguments.
    fn windows_command_line(
        exe: &std::path::Path,
        args: impl Iterator<Item = std::ffi::OsString>,
    ) -> String {
        let mut out = quote(&exe.to_string_lossy());
        for arg in args {
            out.push(' ');
            out.push_str(&quote(&arg.to_string_lossy()));
        }
        out
    }

    fn quote(arg: &str) -> String {
        if !arg.is_empty() && !arg.contains([' ', '\t', '"', '\n']) {
            return arg.to_owned();
        }
        let mut out = String::from("\"");
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                _ => {
                    out.push_str(&"\\".repeat(backslashes));
                    out.push(c);
                    backslashes = 0;
                }
            }
        }
        out.push_str(&"\\".repeat(backslashes * 2));
        out.push('"');
        out
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn arguments_survive_the_command_line() {
            assert_eq!(super::quote("press"), "press");
            assert_eq!(super::quote("a b"), "\"a b\"");
            assert_eq!(super::quote(r#"say "hi""#), r#""say \"hi\"""#);
            assert_eq!(super::quote(r"C:\dir\ "), r#""C:\dir\ ""#);
            assert_eq!(super::quote(r"C:\dir with\"), r#""C:\dir with\\""#);
            assert_eq!(super::quote(""), "\"\"");
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub(super) fn current() -> Option<String> {
        None
    }
    pub(super) fn run_on(_name: &str) -> Option<u32> {
        None
    }
}
