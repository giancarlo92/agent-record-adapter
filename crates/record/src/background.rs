// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! Background mode: the fork's ways of acting without the person's mouse,
//! keyboard or foreground, tried before the original agent-desktop's own.
//!
//! On (the default), every action first takes the fork's background route
//! where there is one: an accessibility or DevTools action on a browser page,
//! a click or a key as messages to the window, a dropdown opened through its
//! container, a classic text box written with `WM_SETTEXT`, a launch that keeps
//! the person's window in front. When that route does not apply, or has
//! nothing to deliver, the original agent-desktop's code runs as it always
//! did, as the backup. Off (`AGENT_DESKTOP_BACKGROUND=0`), only the original
//! code runs, exactly as upstream ships it.

use std::sync::OnceLock;

/// The variable that turns background mode off (`0`, `false`, `off`, `no`).
pub const ENV: &str = "AGENT_DESKTOP_BACKGROUND";

/// Whether background mode is on for this process.
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match std::env::var(ENV) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => true,
    })
}

/// How long the windows noted before an action stay the reference for a
/// `wait --event window-opened` that follows it.
const ACTION_WINDOWS_FRESH_MS: u64 = 20_000;
const ACTION_WINDOWS_FILE: &str = "last-action-windows.json";

/// Notes the top-level windows open just before a click agent-desktop sends
/// in the background. A dialog the click opens (a page's file picker) often
/// appears while the click is still finishing, so a `wait --event
/// window-opened` started after it took the dialog for one already open and
/// waited in vain; with this note, that wait counts it as opened.
pub fn note_windows_before_action(window_ids: Vec<String>) {
    let Ok(root) = crate::storage::state_root::resolve_configured_state_root() else {
        return;
    };
    let note = serde_json::json!({ "at_ms": now_ms(), "ids": window_ids });
    let _ = std::fs::write(root.join(ACTION_WINDOWS_FILE), note.to_string());
}

/// Leaves out of a `window-opened` wait's starting list the windows that were
/// not open before the last background click, when that click was moments
/// ago: the wait then reports them as opened.
pub fn forget_windows_opened_by_last_action(windows: &mut Vec<crate::WindowInfo>) {
    if !enabled() {
        return;
    }
    let Ok(root) = crate::storage::state_root::resolve_configured_state_root() else {
        return;
    };
    let Ok(raw) = std::fs::read(root.join(ACTION_WINDOWS_FILE)) else {
        return;
    };
    let Ok(note) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return;
    };
    forget_windows_not_in(windows, &note, now_ms());
}

fn forget_windows_not_in(windows: &mut Vec<crate::WindowInfo>, note: &serde_json::Value, now: u64) {
    let fresh = note["at_ms"]
        .as_u64()
        .is_some_and(|at| now.saturating_sub(at) <= ACTION_WINDOWS_FRESH_MS);
    let Some(before) = note["ids"].as_array().filter(|_| fresh) else {
        return;
    };
    let before: std::collections::HashSet<&str> = before
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    windows.retain(|window| before.contains(window.id.as_str()));
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
