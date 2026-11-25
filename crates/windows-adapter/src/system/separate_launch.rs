// Part of Agent Record Adapter (see NOTICE).
//! A launch upstream refuses or would attach: a program started with a
//! profile directory of its own (`--user-data-dir=...` for a Chromium
//! browser, `-data <folder>` for an Eclipse-based app) is a separate instance
//! whatever else of it is running, and on the work desktop an instance on the
//! person's desktop is not one to attach to. Started here, behind the
//! person's windows (`SW_SHOWNOACTIVATE`); a Chromium browser also gets the
//! switches that keep its pages working while other windows cover it.

use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

use agent_desktop_core::system::launch_options::LaunchOptions;
use agent_desktop_core::system::launch_result::LaunchResult;
use agent_desktop_core::{
    AdapterError, Deadline, ErrorCode, ObservationOps, ProcessId, WindowFilter,
};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
    STARTUPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE;

/// Whether the arguments give the program a profile directory of its own.
pub(crate) fn has_own_profile(args: &[String]) -> bool {
    args.iter().enumerate().any(|(index, arg)| {
        arg.starts_with("--user-data-dir")
            || (arg == "-data" && args.get(index + 1).is_some_and(|dir| !dir.starts_with('-')))
    })
}

/// Whether the program is a Chromium browser (not an Electron app).
pub(crate) fn is_chromium_browser(id: &str) -> bool {
    let name = std::path::Path::new(id.trim_matches('"'))
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        name.as_str(),
        "chrome" | "msedge" | "brave" | "chromium" | "vivaldi"
    )
}

/// A Chromium window other windows cover completely is treated as hidden: its
/// page and accessibility tree stop updating. These switches turn that off.
pub(crate) fn keep_working_while_covered(args: &mut Vec<String>) {
    const FEATURE: &str = "CalculateNativeWinOcclusion";
    match args
        .iter_mut()
        .find(|arg| arg.starts_with("--disable-features="))
    {
        Some(existing) if !existing.contains(FEATURE) => {
            existing.push(',');
            existing.push_str(FEATURE);
        }
        Some(_) => {}
        None => args.push(format!("--disable-features={FEATURE}")),
    }
    for switch in [
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
    ] {
        if !args.iter().any(|arg| arg == switch) {
            args.push(switch.to_owned());
        }
    }
}

fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    format!("\"{}\"", arg.replace('"', "\\\""))
}

fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

/// Starts `exe` (a full path) with `args`, behind the person's windows
/// (`SW_SHOWNOACTIVATE`), and returns its process id.
fn spawn(exe: &str, args: &[String], options: &LaunchOptions) -> Result<u32, AdapterError> {
    let mut line = quote(exe);
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    let mut line = wide(&line);
    let environment: Option<Vec<u16>> = (!options.env.is_empty()).then(|| {
        let mut block: Vec<u16> = Vec::new();
        let mut vars: std::collections::BTreeMap<String, String> = std::env::vars().collect();
        vars.extend(options.env.clone());
        for (key, value) in vars {
            block.extend(std::ffi::OsStr::new(&format!("{key}={value}")).encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    });
    let cwd = options.cwd.as_ref().map(|dir| wide(&dir.to_string_lossy()));
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_SHOWNOACTIVATE as u16,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: every pointer is to a live, NUL-terminated buffer owned here;
    // the command line buffer is mutable as CreateProcessW requires.
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            if environment.is_some() {
                CREATE_UNICODE_ENVIRONMENT
            } else {
                0
            },
            environment
                .as_ref()
                .map_or(std::ptr::null(), |block| block.as_ptr().cast()),
            cwd.as_ref().map_or(std::ptr::null(), |dir| dir.as_ptr()),
            &startup,
            &mut info,
        )
    };
    if created == 0 {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            format!("Could not start {exe}: {}", std::io::Error::last_os_error()),
        ));
    }
    // SAFETY: both handles were returned by CreateProcessW and are closed once.
    unsafe {
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    Ok(info.dwProcessId)
}

/// The first window that `matches`, waited for up to `wait`.
fn wait_for_window(
    upstream: &upstream_windows::WindowsAdapter,
    wait: Duration,
    matches: &dyn Fn(&agent_desktop_core::WindowInfo) -> bool,
) -> Option<agent_desktop_core::WindowInfo> {
    let end = Instant::now() + wait;
    let filter = WindowFilter {
        focused_only: false,
        app: None,
    };
    // The program's own window, not a helper one of its toolkit: winit keeps
    // a small untitled event window on its thread, shown before the main one.
    // A titled window is waited for a moment; failing one, any of its own.
    let titled_wait = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(deadline) = Deadline::after(2_000)
            && let Ok(windows) = upstream.list_windows(&filter, deadline)
        {
            let mine: Vec<_> = windows
                .into_iter()
                .filter(|w| matches(w) && !toolkit_helper(w))
                .collect();
            let area = |w: &&agent_desktop_core::WindowInfo| {
                w.bounds.map_or(0.0, |b| b.width * b.height) as i64
            };
            let titled = mine
                .iter()
                .filter(|w| w.state.visible == Some(true) && !w.title.trim().is_empty())
                .max_by_key(area);
            if let Some(window) = titled {
                return Some(window.clone());
            }
            if Instant::now() >= titled_wait
                && let Some(first) = mine
                    .iter()
                    .find(|w| w.state.visible == Some(true))
                    .or(mine.first())
            {
                return Some(first.clone());
            }
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// A window a UI toolkit makes for itself (winit's event target), never one
/// of the program's own.
fn toolkit_helper(window: &agent_desktop_core::WindowInfo) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let Ok(hwnd) = crate::system::window::window_ops::parse_window_id(&window.id) else {
        return false;
    };
    let mut name = [0u16; 64];
    // SAFETY: a buffer of the length given; a stale handle answers 0.
    let len = unsafe { GetClassNameW(hwnd as _, name.as_mut_ptr(), name.len() as i32) };
    len > 0
        && String::from_utf16_lossy(&name[..len as usize])
            == crate::actions::posted_keys::WINIT_EVENT_TARGET
}

/// Starts `exe` (a full path) with `options`, behind the person's windows,
/// and reports it as upstream's launch does.
pub(crate) fn launch(
    exe: &str,
    options: &LaunchOptions,
    upstream: &upstream_windows::WindowsAdapter,
) -> Result<LaunchResult, AdapterError> {
    let exe = exe.trim_matches('"');
    let mut args = options.args.clone();
    if is_chromium_browser(exe) {
        keep_working_while_covered(&mut args);
    }
    let pid = spawn(exe, &args, options)?;
    let wait = Duration::from_millis(options.timeout_ms.clamp(1_000, 60_000));
    let window = wait_for_window(upstream, wait, &|w| u32::from(w.pid) == pid);
    Ok(LaunchResult {
        app: exe.to_owned(),
        pid: ProcessId::from(pid),
        process_instance: window.as_ref().and_then(|w| w.process_instance.clone()),
        window,
        cdp: None,
        renderer: None,
        suggestion: None,
    })
}

/// A folder in File Explorer. Explorer hands the folder to the shell's own
/// process (or, on the work desktop, opens a process of its own), so the
/// window is found by its title, the folder's name, among windows that were
/// not open before.
pub(crate) fn launch_folder(
    folder: &std::path::Path,
    options: &LaunchOptions,
    upstream: &upstream_windows::WindowsAdapter,
) -> Result<LaunchResult, AdapterError> {
    let windows = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let explorer = format!("{windows}\\explorer.exe");
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| folder.to_string_lossy().to_string());
    let filter = WindowFilter {
        focused_only: false,
        app: None,
    };
    let before: Vec<String> = Deadline::after(2_000)
        .ok()
        .and_then(|deadline| upstream.list_windows(&filter, deadline).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|w| w.id)
        .collect();
    // On the work desktop Explorer must open a process of its own there
    // (`/separate`); otherwise it hands the folder to the person's shell.
    let argument = if crate::system::work_desktop::active() {
        format!("/separate,{}", folder.to_string_lossy())
    } else {
        folder.to_string_lossy().to_string()
    };
    let pid = spawn(&explorer, &[argument], options)?;
    let titled = |title: &str| title == name || title.starts_with(&format!("{name} - "));
    let window = wait_for_window(upstream, Duration::from_millis(8_000), &|w| {
        titled(&w.title) && !before.contains(&w.id)
    });
    Ok(LaunchResult {
        app: explorer,
        pid: window.as_ref().map_or(ProcessId::from(pid), |w| w.pid),
        process_instance: window.as_ref().and_then(|w| w.process_instance.clone()),
        window,
        cdp: None,
        renderer: None,
        suggestion: None,
    })
}
