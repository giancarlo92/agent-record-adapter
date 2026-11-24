// Part of Agent Record Adapter (see NOTICE).
use serde_json::json;

use super::*;

fn args(text: &str) -> Vec<String> {
    text.split(' ').map(str::to_string).collect()
}

#[test]
fn first_actionable_skips_a_first_match_with_no_ref() {
    let matches = vec![
        json!({ "name": "File name", "role": "group", "ref_id": null }),
        json!({ "name": "Other", "role": "button", "ref_id": "@s1:e2" }),
        json!({ "name": "File name", "role": "textfield", "ref_id": "@s1:e3" }),
    ];
    assert_eq!(first_actionable(&matches).unwrap()["ref_id"], "@s1:e3");
}

#[test]
fn first_actionable_takes_the_text_box_of_an_editable_combo_box() {
    let matches = vec![
        json!({ "name": "File name", "role": "group", "ref_id": null }),
        json!({ "name": "File name", "role": "combobox", "ref_id": "@s1:e2" }),
        json!({ "name": "File name", "role": "textfield", "ref_id": "@s1:e3" }),
    ];
    assert_eq!(first_actionable(&matches).unwrap()["ref_id"], "@s1:e3");
}

#[test]
fn first_actionable_never_takes_another_name() {
    let matches = vec![
        json!({ "name": "Save", "role": "group", "ref_id": null }),
        json!({ "name": "Cancel", "role": "button", "ref_id": "@s1:e2" }),
    ];
    assert!(first_actionable(&matches).is_none());
}

#[test]
fn find_text_looks_in_names_values_and_children() {
    let tree = json!({ "name": "root", "children": [
        { "role": "statictext", "name": "estado: inicio" },
        { "role": "textfield", "value": "Ana Quispe", "ref_id": "@s1:e4" }
    ]});
    assert_eq!(find_text(&tree, "Quispe").unwrap()["ref_id"], "@s1:e4");
    assert_eq!(find_text(&tree, "estado:").unwrap()["role"], "statictext");
    assert!(find_text(&tree, "missing").is_none());
}

#[test]
fn ref_actions_are_recognized_with_their_value() {
    let parsed = ref_action(&args("--session s1 set-value @s1:e4 hello")).unwrap();
    assert_eq!(parsed.command, "set-value");
    assert_eq!(parsed.ref_id, "@s1:e4");
    assert_eq!(parsed.value.as_deref(), Some("hello"));
    assert!(ref_action(&args("snapshot --app x")).is_none());
}

#[test]
fn window_type_is_told_from_a_ref_type() {
    assert!(is_window_type(&args("type w-123 hello")));
    assert!(!is_window_type(&args("type @s1:e4 hello")));
}
