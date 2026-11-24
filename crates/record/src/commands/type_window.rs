// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! `type w-<id> TEXT`: types text into a window rather than into a ref, for a
//! document or a canvas that accessibility shows read-only (so it has no ref):
//! WordCraft's page, a slide. The text goes to the window's focused field, else
//! to its document, as the messages real keys send; the person's keyboard and
//! foreground are not used (see the Windows adapter's `type_in_window`).

use crate::{AppError, adapter::PlatformAdapter};
use serde_json::{Value, json};

/// Whether `type`'s target names a window (`w-<id>`) instead of a ref.
pub fn is_window_target(target: &str) -> bool {
    target
        .strip_prefix("w-")
        .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
}

pub fn execute(
    window_id: &str,
    text: &str,
    adapter: &dyn PlatformAdapter,
) -> Result<Value, AppError> {
    let window = super::window_target::resolve_window_for_app(None, Some(window_id), adapter)?;
    let lease = adapter.acquire_interaction_lease(crate::Deadline::standard()?)?;
    adapter.type_in_window(&window, text, &lease)?;
    Ok(json!({ "window": window.id, "typed": text.chars().count(), "sent": true }))
}

#[cfg(test)]
#[path = "type_window_tests.rs"]
mod tests;
