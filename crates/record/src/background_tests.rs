// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
use super::*;

fn window(id: &str, title: &str) -> crate::WindowInfo {
    serde_json::from_value(serde_json::json!({
        "id": id, "title": title, "app_name": "chrome.exe", "pid": 7,
        "bounds": null, "is_focused": false
    }))
    .expect("a window")
}

fn ids(windows: &[crate::WindowInfo]) -> Vec<&str> {
    windows.iter().map(|w| w.id.as_str()).collect()
}

#[test]
fn a_dialog_opened_by_the_last_click_is_left_out_of_the_starting_list() {
    let mut windows = vec![
        window("w-1", "Studio - Google Chrome"),
        window("w-9", "Open"),
    ];
    let note = serde_json::json!({ "at_ms": 10_000, "ids": ["w-1"] });
    forget_windows_not_in(&mut windows, &note, 12_000);
    assert_eq!(ids(&windows), ["w-1"]);
}

#[test]
fn an_old_note_changes_nothing() {
    let mut windows = vec![
        window("w-1", "Studio - Google Chrome"),
        window("w-9", "Open"),
    ];
    let note = serde_json::json!({ "at_ms": 10_000, "ids": ["w-1"] });
    forget_windows_not_in(&mut windows, &note, 10_000 + ACTION_WINDOWS_FRESH_MS + 1);
    assert_eq!(ids(&windows), ["w-1", "w-9"]);
}

#[test]
fn a_broken_note_changes_nothing() {
    let mut windows = vec![window("w-1", "Studio - Google Chrome")];
    forget_windows_not_in(&mut windows, &serde_json::json!({ "ids": "w-1" }), 1);
    assert_eq!(ids(&windows), ["w-1"]);
}
