// Part of Agent Record Adapter (see NOTICE).
//! A click delivered as window messages, for elements whose app refuses the
//! accessibility click.
//!
//! Some providers refuse every accessibility action on an element that is
//! plainly there: SWT (DBeaver, Eclipse) answers `UIA_E_ELEMENTNOTENABLED` to
//! both `DoDefaultAction` and `SelectionItem.Select` on its tabs, and apps that
//! draw their own controls often expose no action at all. The window that
//! holds the element still handles the messages a real click sends it: a
//! button press and release at the element, posted to that window. Nothing
//! moves the person's cursor or goes through their input, and posted messages
//! never activate a window, so this stays a background action.
//!
//! Tabs an app draws itself (no window of their own) are clicked this way
//! first; anything else only when its accessibility click was not delivered.

use agent_desktop_core::{ActionStep, AdapterError, ErrorCode, StepMechanism};
use uiautomation::patterns::UISelectionItemPattern;
use uiautomation::types::ControlType;
use windows::Win32::Foundation::HWND;
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ScreenToClient,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, IsWindowVisible, PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

use crate::tree::automation::automation_client;
use crate::tree::element::UIAElement;

const LABEL: &str = "WindowMessage.WM_LBUTTON";
const MK_LBUTTON: usize = 0x0001;
const MAX_ANCESTORS: usize = 16;
const USER_DEFAULT_SCREEN_DPI: u32 = 96;
const SETTLE: std::time::Duration = std::time::Duration::from_millis(50);
const CONFIRM_FOR: std::time::Duration = std::time::Duration::from_millis(1_500);

/// A tab drawn by its app inside another window (it has no window of its own).
pub(crate) fn is_drawn_tab(element: &UIAElement) -> bool {
    element.0.get_control_type().ok() == Some(ControlType::TabItem) && own_window(element).is_none()
}

/// Clicks `element` with posted mouse messages at its center, or returns
/// `None` when no window of its app holds that point.
pub(crate) fn click(element: &UIAElement) -> Result<Option<ActionStep>, AdapterError> {
    let Ok(rect) = element.0.get_bounding_rectangle() else {
        return Ok(None);
    };
    let (width, height) = (
        rect.get_right() - rect.get_left(),
        rect.get_bottom() - rect.get_top(),
    );
    if width <= 0 || height <= 0 {
        return Ok(None);
    }
    let center = POINT {
        x: rect.get_left() + width / 2,
        y: rect.get_top() + height / 2,
    };
    let Some((hwnd, point)) = holding_window(element, center) else {
        tracing::debug!("posted click: no window of the app holds the element");
        return Ok(None);
    };
    let at = ((point.y as u16 as usize) << 16) | point.x as u16 as usize;
    let posted = unsafe {
        PostMessageW(hwnd as _, WM_MOUSEMOVE, 0, at as isize) != 0
            && PostMessageW(hwnd as _, WM_LBUTTONDOWN, MK_LBUTTON, at as isize) != 0
            && PostMessageW(hwnd as _, WM_LBUTTONUP, 0, at as isize) != 0
    };
    if !posted {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The element's window did not accept the click (WM_LBUTTONDOWN)",
        ));
    }
    Ok(Some(
        ActionStep::succeeded(LABEL)
            .with_mechanism(StepMechanism::SemanticApi)
            .with_verified(selected_soon(element)),
    ))
}

fn own_window(element: &UIAElement) -> Option<isize> {
    let handle = element.0.get_native_window_handle().ok()?;
    let hwnd: HWND = handle.into();
    let hwnd = hwnd.0 as isize;
    (hwnd != 0).then_some(hwnd)
}

/// The window nearest to the element (its own, else its nearest ancestor with
/// one), shown and enabled, when its client area holds the element's center,
/// with that point in its client coordinates.
///
/// Some providers (SWT's tabs) report bounds without the display scaling,
/// counted from the corner of the display the window is on (at 125 % a tab
/// 500 px into that display reads as 400 px in), while their windows are
/// scaled. The point scaled from that corner is tried first, then the center
/// as reported; it must fall in the nearest window, never one further out.
fn holding_window(element: &UIAElement, center: POINT) -> Option<(isize, POINT)> {
    let walker = automation_client().ok()?.get_control_view_walker().ok()?;
    let mut windows: Vec<isize> = own_window(element).into_iter().collect();
    let mut current = element.0.clone();
    for _ in 0..MAX_ANCESTORS {
        let Ok(parent) = walker.get_parent(&current) else {
            break;
        };
        current = parent;
        let Ok(handle) = current.get_native_window_handle() else {
            continue;
        };
        let hwnd: HWND = handle.into();
        let hwnd = hwnd.0 as isize;
        if hwnd != 0 {
            windows.push(hwnd);
        }
    }
    let nearest = *windows.first()?;
    let scale = unsafe { GetDpiForWindow(nearest as _) };
    let mut candidates = Vec::with_capacity(2);
    if scale > USER_DEFAULT_SCREEN_DPI {
        let origin = display_origin(nearest);
        let grow = |value: i32, from: i32| {
            from + ((value - from) as i64 * scale as i64 / USER_DEFAULT_SCREEN_DPI as i64) as i32
        };
        candidates.push(POINT {
            x: grow(center.x, origin.x),
            y: grow(center.y, origin.y),
        });
    }
    candidates.push(center);
    let point = candidates
        .into_iter()
        .find_map(|wanted| client_point(nearest, wanted))?;
    let usable =
        unsafe { IsWindowVisible(nearest as _) != 0 && IsWindowEnabled(nearest as _) != 0 };
    usable.then_some((nearest, point))
}

/// The top-left corner of the display `hwnd` is on, in screen pixels.
fn display_origin(hwnd: isize) -> POINT {
    let monitor = unsafe { MonitorFromWindow(hwnd as _, MONITOR_DEFAULTTONEAREST) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if monitor.is_null() || unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return POINT { x: 0, y: 0 };
    }
    POINT {
        x: info.rcMonitor.left,
        y: info.rcMonitor.top,
    }
}

/// `screen` in `hwnd`'s client coordinates, when it falls inside its client area.
fn client_point(hwnd: isize, screen: POINT) -> Option<POINT> {
    let mut point = screen;
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let inside = unsafe {
        ScreenToClient(hwnd as _, &mut point) != 0
            && GetClientRect(hwnd as _, &mut client) != 0
            && point.x >= client.left
            && point.x < client.right
            && point.y >= client.top
            && point.y < client.bottom
    };
    inside.then_some(point)
}

/// For an element that can be selected (a tab, a list item), whether it reads
/// as selected within a moment: the messages are handled by the app's own
/// thread, after this call returns. Anything else is reported unverified.
fn selected_soon(element: &UIAElement) -> bool {
    let Ok(pattern) = element.0.get_pattern::<UISelectionItemPattern>() else {
        return false;
    };
    let until = std::time::Instant::now() + CONFIRM_FOR;
    loop {
        let selected = pattern.is_selected().unwrap_or(false);
        if selected || std::time::Instant::now() >= until {
            return selected;
        }
        std::thread::sleep(SETTLE);
    }
}
