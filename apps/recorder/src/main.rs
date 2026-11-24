// Part of Agent Record Adapter (see NOTICE).
//! Desktop Recorder: records a task (with or without video) and turns it into a script that
//! repeats it, without AI, and plays it in the background.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod look;
mod repair;
mod script;
mod steps;
mod texts;
mod tool;
mod update;
mod view;

fn main() -> eframe::Result<()> {
    // desktop-recorder --make-script <recording.jsonl> <folder>: the script, without the window
    // (every value repeats as recorded).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--make-script") && args.len() == 4 {
        let recording = std::fs::read_to_string(&args[2]).unwrap_or_default();
        // The steps marked to ask before they run sit next to the recording.
        let marks = std::path::Path::new(&args[2])
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let steps = steps::parse_dir(marks, &recording);
        let folder = std::path::Path::new(&args[3]);
        let _ = std::fs::create_dir_all(folder);
        let name = std::path::Path::new(&args[2])
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = script::render(&name, &args[2], &steps, &script::candidates(&steps));
        let ok = std::fs::write(folder.join("script.mjs"), text).is_ok()
            && std::fs::write(folder.join(script::LIBRARY_NAME), script::LIBRARY).is_ok();
        std::process::exit(if ok && steps.len() > 1 { 0 } else { 1 });
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([look::PANEL_WIDTH, 96.0])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_title(texts::texts().title),
        ..Default::default()
    };
    eframe::run_native(
        texts::texts().title,
        options,
        Box::new(|cc| {
            no_activate();
            Ok(Box::new(app::App::new(&cc.egui_ctx)))
        }),
    )
}

/// The bar floats over the app being recorded and never takes its focus: a
/// click on Record or Stop reaches the button at once (an activating window
/// spent the first click on activation, so Stop needed two), and the person
/// keeps typing where they were.
#[cfg(windows)]
fn no_activate() {
    // The window is created right after this runs: a short-lived thread waits
    // for this process's top-level windows and marks them.
    std::thread::spawn(|| {
        // eframe makes helper windows first and the bar a moment later, so
        // every visible window of the process is marked for a few seconds.
        for _ in 0..50 {
            mark_own_windows();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

#[cfg(windows)]
fn mark_own_windows() -> usize {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GWLP_WNDPROC, GetWindowLongPtrW, GetWindowThreadProcessId,
        IsWindowVisible, SetWindowLongPtrW, WS_EX_NOACTIVATE,
    };
    unsafe extern "system" fn each(window: HWND, marked: LPARAM) -> i32 {
        let mut pid = 0u32;
        // SAFETY: reads the owner and style of a top-level window and adds one
        // extended style bit to this process's own visible windows.
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
            if pid == std::process::id() && IsWindowVisible(window) != 0 {
                let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
                SetWindowLongPtrW(window, GWL_EXSTYLE, style | WS_EX_NOACTIVATE as isize);
                let procedure = GetWindowLongPtrW(window, GWLP_WNDPROC);
                if procedure != steady_press as usize as isize
                    && let Ok(mut originals) = ORIGINALS.lock()
                {
                    originals.push((window as usize, procedure));
                    SetWindowLongPtrW(window, GWLP_WNDPROC, steady_press as usize as isize);
                }
                *(marked as *mut usize) += 1;
            }
        }
        1
    }
    let mut marked = 0usize;
    // SAFETY: the callback gets a pointer to `marked`, alive for the call.
    unsafe { EnumWindows(Some(each), &mut marked as *mut usize as LPARAM) };
    marked
}

/// The window procedures `steady_press` stands in front of, by window.
#[cfg(windows)]
static ORIGINALS: std::sync::Mutex<Vec<(usize, isize)>> = std::sync::Mutex::new(Vec::new());

/// A pointer that jumps onto the bar and presses at once (an accessibility or
/// automation tool, touch, a pen), or comes back to the very pixel it left,
/// reaches egui as a press whose position it was never told, and egui drops
/// it. Every press is preceded by moves that end on its own point.
#[cfg(windows)]
unsafe extern "system" fn steady_press(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE,
        WM_RBUTTONDOWN, WNDPROC,
    };
    const BUTTONS: usize = 0x0001 | 0x0002 | 0x0010; // MK_LBUTTON, MK_RBUTTON, MK_MBUTTON
    let original = ORIGINALS.lock().ok().and_then(|originals| {
        originals
            .iter()
            .rev()
            .find(|(hwnd, _)| *hwnd == window as usize)
            .map(|(_, procedure)| *procedure)
    });
    // SAFETY: `original` is the window procedure this window had before, read
    // from the window itself; it is called as Windows would call it.
    unsafe {
        let Some(original) = original else {
            return DefWindowProcW(window, message, wparam, lparam);
        };
        let original: WNDPROC = std::mem::transmute::<isize, WNDPROC>(original);
        if matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
            // winit reports a move only when the point changed, and egui forgets
            // the point when the pointer leaves: back on the same pixel, a move
            // one pixel off first makes the move to the point itself count.
            let x = (lparam & 0xFFFF) as u16 as i16;
            let beside = (lparam & !0xFFFF) | (x.wrapping_add(1) as u16 as isize);
            CallWindowProcW(original, window, WM_MOUSEMOVE, wparam & !BUTTONS, beside);
            CallWindowProcW(original, window, WM_MOUSEMOVE, wparam & !BUTTONS, lparam);
        }
        CallWindowProcW(original, window, message, wparam, lparam)
    }
}

#[cfg(not(windows))]
fn no_activate() {}
