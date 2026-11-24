// Part of Agent Record Adapter (see NOTICE).
//! Pointer input for `stroke`: a small copy of upstream's private
//! `input::mouse` helpers (vercel-labs/agent-desktop, Apache-2.0).

use std::time::Duration;

use agent_desktop_core::{AdapterError, Deadline};

/// Sleeps up to `duration`, never past the deadline.
pub(crate) fn sleep_bounded(deadline: Deadline, duration: Duration) -> Result<(), AdapterError> {
    let capped = deadline.remaining_slice(duration)?;
    std::thread::sleep(capped);
    deadline.ensure_remaining()
}

pub(crate) mod mouse_coord {
    use agent_desktop_core::Point;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    /// A point in `SendInput`'s normalized absolute space (0..=65535).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct NormalizedPoint {
        pub(crate) x: i32,
        pub(crate) y: i32,
        pub(crate) virtual_desktop: bool,
    }

    const NORMALIZED_MAX: f64 = 65535.0;

    fn axis(coordinate: f64, origin: i32, extent: i32) -> i32 {
        if extent <= 1 {
            return 0;
        }
        let scaled = ((coordinate - f64::from(origin)) * NORMALIZED_MAX) / f64::from(extent - 1);
        scaled.round().clamp(0.0, NORMALIZED_MAX) as i32
    }

    /// Normalizes against the primary monitor, or the whole virtual screen
    /// when the point is outside it.
    pub(crate) fn normalize_point(point: &Point) -> NormalizedPoint {
        // SAFETY: GetSystemMetrics reads process-wide metrics and has no
        // preconditions.
        let (width, height) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        let inside = point.x >= 0.0
            && point.y >= 0.0
            && point.x < f64::from(width)
            && point.y < f64::from(height);
        if inside {
            return NormalizedPoint {
                x: axis(point.x, 0, width),
                y: axis(point.y, 0, height),
                virtual_desktop: false,
            };
        }
        // SAFETY: as above.
        let (left, top, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        NormalizedPoint {
            x: axis(point.x, left, vw),
            y: axis(point.y, top, vh),
            virtual_desktop: true,
        }
    }
}

pub(crate) mod mouse_send {
    use super::mouse_coord::NormalizedPoint;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
    };
    pub(crate) use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct MouseInputEvent {
        pub(crate) dx: i32,
        pub(crate) dy: i32,
        pub(crate) flags: u32,
    }

    pub(crate) fn move_input(point: NormalizedPoint) -> MouseInputEvent {
        let mut flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE;
        if point.virtual_desktop {
            flags |= MOUSEEVENTF_VIRTUALDESK;
        }
        MouseInputEvent {
            dx: point.x,
            dy: point.y,
            flags,
        }
    }

    pub(crate) fn button_input(flag: u32) -> MouseInputEvent {
        MouseInputEvent {
            dx: 0,
            dy: 0,
            flags: flag,
        }
    }

    pub(crate) fn post_mouse_inputs(events: &[MouseInputEvent]) {
        if events.is_empty() {
            return;
        }
        let raw: Vec<INPUT> = events
            .iter()
            .map(|event| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: event.dx,
                        dy: event.dy,
                        mouseData: 0,
                        dwFlags: event.flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            })
            .collect();
        // SAFETY: `raw` is a live, correctly sized array of INPUT structs.
        unsafe {
            SendInput(
                raw.len() as u32,
                raw.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    }
}

pub(crate) mod drag_state {
    use agent_desktop_core::AdapterError;

    use super::mouse_coord::NormalizedPoint;
    use super::mouse_send::{MOUSEEVENTF_LEFTUP, button_input, move_input, post_mouse_inputs};

    /// Releases the left button at the origin when a stroke aborts with the
    /// button down.
    pub(crate) struct DragReleaseGuard {
        origin: NormalizedPoint,
        armed: bool,
        delivered: bool,
        released: bool,
    }

    impl DragReleaseGuard {
        pub(crate) fn new(origin: NormalizedPoint) -> Self {
            Self {
                origin,
                armed: false,
                delivered: false,
                released: false,
            }
        }

        pub(crate) fn arm(&mut self) {
            self.armed = true;
        }

        pub(crate) fn mark_delivered(&mut self) {
            self.delivered = true;
        }

        pub(crate) fn disarm(&mut self) {
            self.armed = false;
        }

        pub(crate) fn enrich_error(mut self, error: AdapterError) -> AdapterError {
            let released = self.release();
            let error = if self.delivered {
                error
                    .with_disposition(agent_desktop_core::DeliverySemantics::delivered_unverified())
            } else {
                error
            };
            error.with_details(serde_json::json!({ "emergency_release_posted": released }))
        }

        fn release(&mut self) -> bool {
            if !self.armed || self.released {
                return false;
            }
            post_mouse_inputs(&[move_input(self.origin), button_input(MOUSEEVENTF_LEFTUP)]);
            self.released = true;
            true
        }
    }

    impl Drop for DragReleaseGuard {
        fn drop(&mut self) {
            self.release();
        }
    }
}
