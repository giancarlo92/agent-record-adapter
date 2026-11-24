// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! `navigate`: open an address in the tab a browser window shows, keeping its
//! session (cookies, storage), through the browser's DevTools endpoint: the
//! window never comes forward and the person's keyboard is not used.

use crate::{AppError, adapter::PlatformAdapter};
use serde_json::Value;

pub struct NavigateArgs {
    pub app: Option<String>,
    pub window_id: Option<String>,
    pub url: String,
    pub in_app: bool,
}

pub fn execute(args: NavigateArgs, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let url = args.url.trim();
    let scheme = url
        .split_once("://")
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    let relative = url.starts_with('/');
    if !relative && !matches!(scheme.as_deref(), Some("http" | "https" | "file")) {
        return Err(AppError::invalid_input(
            "navigate takes an http, https or file address, or a path of the same site (/users)",
        ));
    }
    if relative && !args.in_app {
        return Err(AppError::invalid_input(
            "A path alone (/users) needs --in-app; give the whole address to load it",
        ));
    }
    let window = super::window_target::resolve_window_for_app(
        args.app.as_deref(),
        args.window_id.as_deref(),
        adapter,
    )?;
    let lease = adapter.acquire_interaction_lease(crate::Deadline::standard()?)?;
    Ok(adapter.navigate_window(&window, url, args.in_app, &lease)?)
}
