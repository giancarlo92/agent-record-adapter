// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! `click-at`: click a point of a window, given in pixels of its screenshot,
//! for content that is not in the accessibility tree (a results grid, a
//! canvas). It goes to the window as messages: the cursor does not move and
//! the window does not come forward.

use crate::{AppError, adapter::PlatformAdapter};
use serde_json::{Value, json};

pub struct ClickAtArgs {
    pub app: Option<String>,
    pub window_id: Option<String>,
    pub x: i32,
    pub y: i32,
    pub double: bool,
    pub screen: bool,
}

pub fn execute(args: ClickAtArgs, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let window = super::window_target::resolve_window_for_app(
        args.app.as_deref(),
        args.window_id.as_deref(),
        adapter,
    )?;
    let lease = adapter.acquire_interaction_lease(crate::Deadline::standard()?)?;
    let point = crate::PointClick {
        x: args.x,
        y: args.y,
        double: args.double,
        screen: args.screen,
    };
    adapter.click_at(&window, &point, &lease)?;
    Ok(
        json!({ "window": window.id, "x": args.x, "y": args.y, "double": args.double, "sent": true }),
    )
}
