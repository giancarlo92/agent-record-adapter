// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn every_modifier_variant_classifies_as_its_modifier() {
    for (vk, key) in [
        (0x10, ModifierKey::Shift),
        (0xA1, ModifierKey::Shift),
        (0x11, ModifierKey::Ctrl),
        (0xA3, ModifierKey::Ctrl),
        (0x12, ModifierKey::Alt),
        (0xA5, ModifierKey::Alt),
        (0x5B, ModifierKey::Win),
        (0x14, ModifierKey::CapsLock),
    ] {
        assert_eq!(classify(vk), KeyClass::Modifier(key), "vk {vk:#x}");
    }
}

#[test]
fn keys_that_end_typing_are_commits_and_editing_keys_are_not() {
    assert_eq!(classify(0x0D), KeyClass::Commit);
    assert_eq!(classify(0x09), KeyClass::Commit);
    assert_eq!(classify(0x1B), KeyClass::Commit);
    assert_eq!(classify(0x74), KeyClass::Commit);
    assert_eq!(classify(0x25), KeyClass::Navigation);
    assert_eq!(classify(0x24), KeyClass::Navigation);
    assert_eq!(classify(0x08), KeyClass::Editing);
    assert_eq!(classify(0x2E), KeyClass::Editing);
    assert_eq!(classify(0x41), KeyClass::Text);
    assert_eq!(classify(0x20), KeyClass::Text);
    assert_eq!(classify(0xBC), KeyClass::Text);
}

#[test]
fn names_round_trip_through_the_press_vocabulary() {
    assert_eq!(key_name(0x0D).as_deref(), Some("return"));
    assert_eq!(key_name(0x08).as_deref(), Some("backspace"));
    assert_eq!(key_name(0x2E).as_deref(), Some("forwarddelete"));
    assert_eq!(key_name(0x70).as_deref(), Some("f1"));
    assert_eq!(key_name(0x7B).as_deref(), Some("f12"));
    assert_eq!(key_name(0x53).as_deref(), Some("s"));
    assert_eq!(key_name(0x35).as_deref(), Some("5"));
    assert_eq!(key_name(0xBC), None);
    for vk in [
        0x0D, 0x09, 0x1B, 0x20, 0x08, 0x2E, 0x21, 0x22, 0x25, 0x70, 0x41, 0x30,
    ] {
        let name = key_name(vk).expect("named key");
        // Upstream's key map is private; the name must at least be a key
        // name `press` documents (lowercase, no spaces).
        assert!(
            !name.is_empty() && !name.contains(' ') && name == name.to_lowercase(),
            "press cannot parse {name}"
        );
    }
}

#[test]
fn a_shortcut_needs_ctrl_alt_or_win_but_not_altgr() {
    let mut state = ModifierState::default();
    assert!(!state.is_shortcut());
    state.apply(ModifierKey::Shift, true);
    assert!(!state.is_shortcut());
    state.apply(ModifierKey::Ctrl, true);
    assert!(state.is_shortcut());
    state.apply(ModifierKey::Alt, true);
    assert!(!state.is_shortcut(), "ctrl+alt is AltGr");
    state.apply(ModifierKey::Ctrl, false);
    assert!(state.is_shortcut());
    state.apply(ModifierKey::Alt, false);
    state.apply(ModifierKey::Win, true);
    assert!(state.is_shortcut());
}

#[test]
fn caps_lock_toggles_on_each_press_only() {
    let mut state = ModifierState::default();
    state.apply(ModifierKey::CapsLock, true);
    state.apply(ModifierKey::CapsLock, false);
    assert!(state.caps_lock);
    state.apply(ModifierKey::CapsLock, true);
    assert!(!state.caps_lock);
}

#[test]
fn combos_list_modifiers_in_press_order() {
    let state = ModifierState {
        shift: true,
        ctrl: true,
        alt: false,
        win: false,
        caps_lock: false,
    };
    assert_eq!(state.combo("s"), "ctrl+shift+s");
    assert_eq!(ModifierState::default().combo("return"), "return");
}
