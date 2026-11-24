// Part of Agent Record Adapter (see NOTICE).
//! `InputOps::stroke`: a left-button drag along a path, through `SendInput`.
//!
//! A drag goes straight from one point to another; a stroke follows every
//! point of a path at its time, which is how a line drawn on a canvas is
//! replayed. Like `drag`, it uses bare coordinates, `--headed` is the guard
//! (enforced upstream), and the release guard posts the button up at the
//! origin when the sequence aborts with the button down.

use std::time::{Duration, Instant};

use agent_desktop_core::{AdapterError, Deadline, StrokeParams};

use crate::input::drag_state::DragReleaseGuard;
use crate::input::mouse::mouse_coord;
use crate::input::mouse::mouse_send::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, button_input, move_input, post_mouse_inputs,
};
use crate::input::mouse::sleep_bounded;

/// The pause after the cursor arrives and after the button goes down, so the
/// target sees each before the next.
const PICKUP_DELAY_MS: u64 = 60;

pub(crate) fn synthesize_stroke(
    params: StrokeParams,
    deadline: Deadline,
) -> Result<(), AdapterError> {
    params.validate()?;
    let points: Vec<_> = params
        .points
        .iter()
        .map(|p| (mouse_coord::normalize_point(&p.point), p.at_ms))
        .collect();
    let (origin, _) = points[0];

    deadline.ensure_remaining()?;
    post_mouse_inputs(&[move_input(origin)]);
    sleep_bounded(deadline, Duration::from_millis(PICKUP_DELAY_MS))?;

    let mut guard = DragReleaseGuard::new(origin);
    guard.arm();
    post_mouse_inputs(&[button_input(MOUSEEVENTF_LEFTDOWN)]);
    guard.mark_delivered();
    let pressed = Instant::now();

    let outcome = (|| -> Result<(), AdapterError> {
        sleep_bounded(deadline, Duration::from_millis(PICKUP_DELAY_MS))?;
        for &(point, at_ms) in &points[1..] {
            let due = pressed + Duration::from_millis(at_ms + PICKUP_DELAY_MS);
            let wait = due.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                sleep_bounded(deadline, wait)?;
            }
            deadline.ensure_remaining()?;
            post_mouse_inputs(&[move_input(point)]);
            guard.mark_delivered();
        }
        post_mouse_inputs(&[button_input(MOUSEEVENTF_LEFTUP)]);
        guard.mark_delivered();
        guard.disarm();
        Ok(())
    })();

    outcome.map_err(|error| guard.enrich_error(error))
}
