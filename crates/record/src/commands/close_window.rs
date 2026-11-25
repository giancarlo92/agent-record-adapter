// Part of Agent Record Adapter (see NOTICE).
//! `close-window w-<id>`: closes one window, as its own Close button does,
//! without the person's mouse or foreground. agent-desktop has `close-app`
//! (every window of an app) but no way to close one window; its guide asks
//! to click the window's Close button, whose name depends on the language.
//! An app may ask to save first: that dialog is then the window to answer,
//! and `closed` is false.

use crate::{AppError, adapter::PlatformAdapter};
use serde_json::{Value, json};

pub fn execute(window_id: &str, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let window = super::window_target::resolve_window_for_app(None, Some(window_id), adapter)?;
    let lease = adapter.acquire_interaction_lease(crate::Deadline::standard()?)?;
    let closed = adapter.close_window(&window, &lease)?;
    Ok(json!({ "window": window.id, "sent": true, "closed": closed }))
}
