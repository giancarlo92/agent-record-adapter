// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! A click at a point of a window, as window messages.
//!
//! Some content is not in the accessibility tree at all: a results grid, a
//! canvas, a chart an app draws itself. A person clicks it by where it is on
//! screen. Here the point is given in pixels of the window's screenshot, the
//! innermost child window under it is found, and the press and release a real
//! click sends are posted to that window. The cursor does not move, the
//! person's input is not used and posted messages never activate a window.

use agent_desktop_core::{AdapterError, ErrorCode, PointClick, WindowInfo};
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, ChildWindowFromPointEx,
    GetWindowRect, PostMessageW, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

const MK_LBUTTON: usize = 0x0001;
const MAX_DEPTH: usize = 32;

/// Clicks (or double-clicks) `window` at a point: pixels from the top-left
/// corner of its screenshot, or screen pixels with `point.screen`.
pub(crate) fn click_at(window: &WindowInfo, point: &PointClick) -> Result<(), AdapterError> {
    let double = point.double;
    let top = crate::system::window::window_ops::parse_window_id(&window.id)?;
    let frame = frame(top).ok_or_else(|| {
        AdapterError::new(
            ErrorCode::WindowNotFound,
            "The window's position could not be read",
        )
    })?;
    let (x, y) = if point.screen {
        (point.x - frame.left, point.y - frame.top)
    } else {
        (point.x, point.y)
    };
    if x < 0 || y < 0 || x >= frame.right - frame.left || y >= frame.bottom - frame.top {
        return Err(AdapterError::new(
            ErrorCode::InvalidArgs,
            format!(
                "The point is outside the window ({}x{} pixels, as in its screenshot)",
                frame.right - frame.left,
                frame.bottom - frame.top
            ),
        ));
    }
    let screen = POINT {
        x: frame.left + x,
        y: frame.top + y,
    };
    crate::actions::behind::note_open_windows();
    // A background recording running now records the click too.
    let noted =
        crate::input::record::record_agent::before_point(top, (screen.x, screen.y), (x, y), double);
    let (target, point) = innermost(top, screen);
    let at = (((point.y as u16) as usize) << 16 | (point.x as u16) as usize) as isize;
    let post =
        |message: u32, keys: usize| unsafe { PostMessageW(target as _, message, keys, at) != 0 };
    let mut sent =
        post(WM_MOUSEMOVE, 0) && post(WM_LBUTTONDOWN, MK_LBUTTON) && post(WM_LBUTTONUP, 0);
    if double {
        sent = sent && post(WM_LBUTTONDBLCLK, MK_LBUTTON) && post(WM_LBUTTONUP, 0);
    }
    if !sent {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The window did not accept the click (WM_LBUTTONDOWN)",
        ));
    }
    if let Some(noted) = noted {
        noted.after();
    }
    Ok(())
}

/// The rectangle a window's screenshot covers: its frame as drawn, else its
/// window rectangle.
fn frame(hwnd: isize) -> Option<RECT> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let framed = unsafe {
        DwmGetWindowAttribute(
            hwnd as _,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut rect as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    } == 0;
    if !framed && unsafe { GetWindowRect(hwnd as _, &mut rect) } == 0 {
        return None;
    }
    Some(rect)
}

/// The innermost visible, enabled child window under `screen`, with the point
/// in its client coordinates.
fn innermost(top: isize, screen: POINT) -> (isize, POINT) {
    let mut current = top;
    for _ in 0..MAX_DEPTH {
        let mut local = screen;
        unsafe { ScreenToClient(current as _, &mut local) };
        let child = unsafe {
            ChildWindowFromPointEx(
                current as _,
                local,
                CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT,
            )
        } as isize;
        if child == 0 || child == current {
            return (current, local);
        }
        current = child;
    }
    let mut local = screen;
    unsafe { ScreenToClient(current as _, &mut local) };
    (current, local)
}
