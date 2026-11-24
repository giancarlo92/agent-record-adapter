// Part of Agent Record Adapter (see NOTICE).
//! The on-screen recording indicator.
//!
//! A small always-on-top window at the bottom of the recorded display shows
//! a red dot, the elapsed time and a Stop button, so a person always sees
//! that a session records and can end it themselves, whoever started it.
//! It never takes the focus, it is left out of the session's video
//! (`WDA_EXCLUDEFROMCAPTURE`), and its clicks are not recorded: it belongs
//! to the recorder's own process, which the session never records.

use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
    Ellipse, EndPaint, FillRect, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromPoint, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    KillTimer, MA_NOACTIVATE, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer,
    SetWindowDisplayAffinity, SetWindowTextW, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_DESTROY, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_CHILD,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use super::record_stop::request_stop;
use super::record_video_area::Area;

const STOP_BUTTON: usize = 1;
const TICK: usize = 1;
const WIDTH: i32 = 236;
const HEIGHT: i32 = 44;
const MARGIN: i32 = 16;

static STARTED: Mutex<Option<Instant>> = Mutex::new(None);
static BUTTON: AtomicIsize = AtomicIsize::new(0);

/// The indicator, shown until it is dropped.
pub(super) struct Indicator {
    window: std::sync::Arc<AtomicIsize>,
    thread: Option<JoinHandle<()>>,
}

impl Indicator {
    /// Shows the indicator at the bottom of the display `area` is on;
    /// `started` is when the session started.
    pub(super) fn show(area: Area, started: Instant) -> Self {
        if let Ok(mut slot) = STARTED.lock() {
            *slot = Some(started);
        }
        let window = std::sync::Arc::new(AtomicIsize::new(0));
        let shared = std::sync::Arc::clone(&window);
        let thread = std::thread::Builder::new()
            .name("record-indicator".into())
            .spawn(move || run(area, &shared))
            .ok();
        Self { window, thread }
    }
}

impl Drop for Indicator {
    fn drop(&mut self) {
        for _ in 0..50 {
            let hwnd = self.window.load(Ordering::SeqCst);
            if hwnd != 0 {
                unsafe { PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0) };
                break;
            }
            if self.thread.as_ref().is_none_or(|t| t.is_finished()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn scaled(value: i32) -> i32 {
    let dpi = unsafe { GetDpiForSystem() }.max(96) as i32;
    value * dpi / 96
}

fn run(area: Area, window: &AtomicIsize) {
    let class = wide("AgentDesktopRecordIndicator");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let mut definition: WNDCLASSEXW = unsafe { std::mem::zeroed() };
    definition.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
    definition.lpfnWndProc = Some(procedure);
    definition.hInstance = instance;
    definition.lpszClassName = class.as_ptr();
    unsafe { RegisterClassExW(&definition) };

    let (width, height) = (scaled(WIDTH), scaled(HEIGHT));
    let center = POINT {
        x: area.x + area.width as i32 / 2,
        y: area.y + area.height as i32 / 2,
    };
    let monitor = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTOPRIMARY) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    unsafe { GetMonitorInfoW(monitor, &mut info) };
    let work = info.rcWork;
    let x = work.left + (work.right - work.left - width) / 2;
    let y = work.bottom - height - scaled(MARGIN);

    let title = wide("Recording");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return;
    }
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) };
    window.store(hwnd as isize, Ordering::SeqCst);
    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    window.store(0, Ordering::SeqCst);
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let class = wide("BUTTON");
            let label = wide("Stop");
            let button = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    label.as_ptr(),
                    WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
                    scaled(WIDTH - 84),
                    scaled(8),
                    scaled(74),
                    scaled(HEIGHT - 16),
                    hwnd,
                    STOP_BUTTON as _,
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                )
            };
            BUTTON.store(button as isize, Ordering::SeqCst);
            unsafe { SetTimer(hwnd, TICK, 500, None) };
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_TIMER => {
            unsafe { InvalidateRect(hwnd, std::ptr::null(), 1) };
            0
        }
        WM_COMMAND if (wparam & 0xFFFF) == STOP_BUTTON => {
            request_stop();
            let button = BUTTON.load(Ordering::SeqCst) as HWND;
            let label = wide("Stopping…");
            unsafe {
                SetWindowTextW(button, label.as_ptr());
                EnableWindow(button, 0);
            }
            0
        }
        WM_PAINT => {
            unsafe { paint(hwnd) };
            0
        }
        WM_DESTROY => {
            unsafe {
                KillTimer(hwnd, TICK);
                PostQuitMessage(0);
            }
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    let dc = unsafe { BeginPaint(hwnd, &mut paint) };
    let background = unsafe { CreateSolidBrush(0x0028_2828) };
    let whole = RECT {
        left: 0,
        top: 0,
        right: scaled(WIDTH),
        bottom: scaled(HEIGHT),
    };
    unsafe { FillRect(dc, &whole, background) };
    let red = unsafe { CreateSolidBrush(0x0030_30E0) };
    let previous = unsafe { SelectObject(dc, red) };
    let dot = scaled(14);
    let top = (scaled(HEIGHT) - dot) / 2;
    unsafe { Ellipse(dc, scaled(14), top, scaled(14) + dot, top + dot) };
    unsafe { SelectObject(dc, previous) };
    let font = unsafe {
        CreateFontW(
            -scaled(15),
            0,
            0,
            0,
            600,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            wide("Segoe UI").as_ptr(),
        )
    };
    let old_font = unsafe { SelectObject(dc, font) };
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00FF_FFFF);
    }
    let elapsed = STARTED
        .lock()
        .ok()
        .and_then(|slot| *slot)
        .map(|started| started.elapsed().as_secs())
        .unwrap_or(0);
    let text = wide(&format!("REC  {:02}:{:02}", elapsed / 60, elapsed % 60));
    let mut area = RECT {
        left: scaled(38),
        top: 0,
        right: scaled(WIDTH - 90),
        bottom: scaled(HEIGHT),
    };
    unsafe {
        DrawTextW(dc, text.as_ptr(), -1, &mut area, DT_SINGLELINE | DT_VCENTER);
        SelectObject(dc, old_font);
        DeleteObject(font);
        DeleteObject(red);
        DeleteObject(background);
        EndPaint(hwnd, &paint);
    }
}
