// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn chromium_browsers_are_recognised_by_their_executable() {
    assert!(is_browser("chrome.exe"));
    assert!(is_browser("MSEdge.exe"));
    assert!(!is_browser("gridcraft.exe"));
}

#[test]
fn only_the_unnamed_chromium_widget_is_a_page_without_its_tree() {
    assert!(is_bare_chromium(Some("Chrome_WidgetWin_1"), false));
    assert!(!is_bare_chromium(Some("Chrome_WidgetWin_1"), true));
    assert!(!is_bare_chromium(Some("Edit"), false));
    assert!(!is_bare_chromium(None, false));
}
