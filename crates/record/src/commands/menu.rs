// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! `menu`: run a command of a window's menu bar by its path, in the
//! background. A menu bar only opens in the window in front, so instead of
//! opening it the command is sent to the window the way choosing the item
//! does.

use crate::{AppError, MenuSearch, adapter::PlatformAdapter};
use serde_json::{Value, json};

pub struct MenuArgs {
    pub app: Option<String>,
    pub window_id: Option<String>,
    pub path: Vec<String>,
    /// Instead of a path: the item showing this key ("ctrl+enter"), anywhere in the menu bar.
    pub shortcut: Option<String>,
    /// Instead of a path: the item with this name, in any submenu.
    pub find: Option<String>,
}

pub fn execute(args: MenuArgs, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let path: Vec<String> = args
        .path
        .iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect();
    let search = match (args.shortcut, args.find) {
        (Some(keys), _) if !keys.trim().is_empty() => Some(MenuSearch::Shortcut(keys)),
        (_, Some(name)) if !name.trim().is_empty() => Some(MenuSearch::Name(name)),
        _ => None,
    };
    if path.is_empty() && search.is_none() {
        return Err(AppError::invalid_input_with_suggestion(
            "The menu path is empty",
            "Name the items from the menu bar down (menu File Export), or use --shortcut or --find",
        ));
    }
    let window = super::window_target::resolve_window_for_app(
        args.app.as_deref(),
        args.window_id.as_deref(),
        adapter,
    )?;
    let lease = adapter.acquire_interaction_lease(crate::Deadline::standard()?)?;
    let path = match &search {
        Some(search) => adapter.find_menu_item(&window, search, &lease)?,
        None => path,
    };
    let chosen = adapter.invoke_menu(&window, &path, &lease)?;
    Ok(json!({ "window": window.id, "path": chosen, "sent": true }))
}
