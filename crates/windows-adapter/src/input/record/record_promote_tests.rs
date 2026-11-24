// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn an_unnamed_icon_or_caption_is_a_decoration() {
    assert!(is_decoration(Some("image"), false));
    assert!(is_decoration(Some("statictext"), false));
    assert!(is_decoration(None, false));
}

#[test]
fn a_named_leaf_or_a_control_is_not_a_decoration() {
    assert!(
        !is_decoration(Some("image"), true),
        "a named picture stands on its own"
    );
    assert!(!is_decoration(Some("button"), false));
    assert!(
        !is_decoration(Some("group"), false),
        "a canvas or panel is its own target"
    );
}

#[test]
fn buttons_and_toggles_take_their_decorations_presses() {
    assert!(is_actionable(Some("switch")));
    assert!(is_actionable(Some("button")));
    assert!(!is_actionable(Some("group")));
    assert!(!is_actionable(None));
}

#[test]
fn an_unnamed_block_of_a_page_stands_for_its_control() {
    assert!(is_web_block(Some("group"), false, true));
    assert!(
        !is_web_block(Some("group"), false, false),
        "a desktop canvas stays its own target"
    );
    assert!(
        !is_web_block(Some("group"), true, true),
        "a named group stands on its own"
    );
    assert!(is_actionable(Some("option")));
}
