// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
use super::is_window_target;

#[test]
fn a_window_target_is_w_and_digits() {
    assert!(is_window_target("w-8716556"));
    assert!(!is_window_target("@s1:e2"));
    assert!(!is_window_target("w-"));
    assert!(!is_window_target("w-12a"));
}
