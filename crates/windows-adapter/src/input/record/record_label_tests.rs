// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn a_label_is_a_short_text() {
    assert!(usable("Password"));
    assert!(usable("  First name  "));
    assert!(!usable("   "));
    assert!(!usable(&"word ".repeat(20)));
}

#[test]
fn only_text_roles_name_a_field() {
    assert!(is_text_role("statictext"));
    assert!(!is_text_role("button"));
    assert!(!is_text_role("textfield"));
}

#[test]
fn only_form_fields_take_a_label_beside_them() {
    assert!(is_field_role("textfield"));
    assert!(!is_field_role("unknown"), "a drawn grid keeps its point");
    assert!(!is_field_role("group"));
}
