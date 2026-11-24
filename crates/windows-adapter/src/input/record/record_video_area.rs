// Part of Agent Record Adapter (see NOTICE).
//! The part of the screen a session's video shows.
//!
//! It follows the session's scope: the window (`--window-id`), the display
//! (`--display`), the display an app's first window is on (`--app`), or the
//! primary display. It is fixed when the session starts, in the physical
//! pixels the hooks report; an H.264 frame needs an even size.

use agent_desktop_core::{AdapterError, ErrorCode, RecordRequest, app_name_matches};
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
};

use super::record_window::display_bounds;

const MIN_SIDE: u32 = 16;
const MAX_WIDTH: u32 = 4096;
const MAX_HEIGHT: u32 = 2304;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Area {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Area {
    /// The area of a rectangle, its size made even and kept within what an
    /// H.264 encoder takes.
    pub(super) fn of(x: i32, y: i32, width: i64, height: i64) -> Option<Self> {
        let even = |side: i64, max: u32| (side.clamp(0, i64::from(max)) as u32) & !1;
        let (width, height) = (even(width, MAX_WIDTH), even(height, MAX_HEIGHT));
        (width >= MIN_SIDE && height >= MIN_SIDE).then_some(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn of_rect(rect: RECT) -> Option<Self> {
        Self::of(
            rect.left,
            rect.top,
            i64::from(rect.right) - i64::from(rect.left),
            i64::from(rect.bottom) - i64::from(rect.top),
        )
    }
}

/// The area the session's video shows.
pub(super) fn area_for(request: &RecordRequest) -> Result<Area, AdapterError> {
    let area = if let Some(id) = &request.window_id {
        window_handle(id).and_then(window_area)
    } else if let Some(id) = &request.display {
        let bounds = display_bounds(id)?;
        Area::of(
            bounds.x as i32,
            bounds.y as i32,
            bounds.width as i64,
            bounds.height as i64,
        )
    } else if let Some(app) = &request.app {
        app_window(app)
            .map(|hwnd| unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) })
            .and_then(monitor_area)
            .or_else(primary_area)
    } else {
        primary_area()
    };
    area.ok_or_else(|| {
        AdapterError::new(
            ErrorCode::InvalidArgs,
            "Video: the recorded area has no size on screen",
        )
        .with_suggestion("Restore the window, or record a display")
    })
}

fn window_handle(id: &str) -> Option<HWND> {
    let raw: usize = id.strip_prefix("w-")?.parse().ok()?;
    (raw != 0).then_some(raw as HWND)
}

fn window_area(hwnd: HWND) -> Option<Area> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let framed = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut rect as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    } == 0;
    if !framed && unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    Area::of_rect(rect)
}

fn primary_area() -> Option<Area> {
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    monitor_area(monitor)
}

fn monitor_area(monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR) -> Option<Area> {
    if monitor.is_null() {
        return None;
    }
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    Area::of_rect(info.rcMonitor)
}

struct Search<'a> {
    app: &'a str,
    found: Option<HWND>,
}

/// The first visible top-level window of an app.
fn app_window(app: &str) -> Option<HWND> {
    let mut search = Search { app, found: None };
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    search.found
}

unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> i32 {
    let search = unsafe { &mut *(data as *mut Search) };
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let matches = super::record_process::image_path(pid)
        .as_deref()
        .and_then(|path| {
            std::path::Path::new(path)
                .file_name()?
                .to_str()
                .map(String::from)
        })
        .is_some_and(|name| app_name_matches(&name, search.app));
    if matches {
        search.found = Some(hwnd);
        return 0;
    }
    1
}

#[cfg(test)]
#[path = "record_video_area_tests.rs"]
mod tests;
