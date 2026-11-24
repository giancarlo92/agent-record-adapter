// Part of Agent Record Adapter (see NOTICE).
//! Fields named by the text before them.
//!
//! Many forms put a field's label in plain text before it, unlinked, so the
//! field has no accessible name ("Apellido Paterno" ... an unnamed text box).
//! The recorder names such a field by that text, and scripts look it up by
//! that name; upstream's `find` never sees it. When `find --name` in one
//! window matches nothing, the window's snapshot is read in document order
//! and each unnamed field gets the last text seen since the previous field.
//! Acting on the ref goes through twins-by-place when upstream refuses it as
//! not unique (`twins.rs`).

use serde_json::{Value, json};

/// Roles that take input and can be named by the text before them.
const FIELDS: &[&str] = &[
    "textfield",
    "combobox",
    "spinbutton",
    "searchfield",
    "listbox",
    "slider",
    "checkbox",
];
/// Roles whose name is text a person reads as a label.
const TEXTS: &[&str] = &["statictext", "label", "heading"];

/// Unnamed fields with the label the text before them gives, in document
/// order: (label, node).
pub(crate) fn labelled_fields(tree: &Value) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    let mut pending: Option<String> = None;
    walk(tree, &mut pending, &mut found);
    found
}

fn walk(node: &Value, pending: &mut Option<String>, found: &mut Vec<(String, Value)>) {
    let role = node["role"].as_str().unwrap_or_default();
    let name = node["name"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if FIELDS.contains(&role) {
        match name {
            None => {
                if let Some(label) = pending.take() {
                    let mut field = node.clone();
                    if let Some(object) = field.as_object_mut() {
                        object.remove("children");
                    }
                    found.push((label, field));
                }
            }
            Some(_) => *pending = None,
        }
    } else if TEXTS.contains(&role)
        && let Some(text) = name
        && text.chars().count() <= 80
    {
        *pending = Some(text.trim_end_matches([':', '*', ' ']).to_string());
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            walk(child, pending, found);
        }
    }
}

/// Whether a field's label answers `--name` (exact or contained, ignoring case).
pub(crate) fn name_matches(label: &str, wanted: &str, exact: bool) -> bool {
    let (label, wanted) = (label.to_lowercase(), wanted.to_lowercase());
    if exact {
        label == wanted
    } else {
        label.contains(&wanted)
    }
}

/// `find`'s answer from the labelled fields of a snapshot: `match` for
/// `--first`, else `matches`.
pub(crate) fn find_answer(
    snapshot: &Value,
    role: Option<&str>,
    name: &str,
    exact: bool,
    first: bool,
) -> Option<Value> {
    let matches: Vec<Value> = labelled_fields(&snapshot["data"]["tree"])
        .into_iter()
        .filter(|(label, field)| {
            name_matches(label, name, exact) && role.is_none_or(|role| field["role"] == role)
        })
        .filter(|(_, field)| !field["ref_id"].is_null())
        .map(|(label, mut field)| {
            field["name"] = json!(label);
            field["named_by"] = json!("text_before");
            field
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    let snapshot_id = snapshot["data"]["snapshot_id"].clone();
    Some(if first {
        json!({ "match": matches[0], "snapshot_id": snapshot_id })
    } else {
        json!({ "matches": matches, "snapshot_id": snapshot_id })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Value {
        json!({ "role": "window", "children": [
            { "role": "statictext", "name": "Oftalmología" },
            { "role": "statictext", "name": "Apellido Paterno" },
            { "role": "textfield", "ref_id": "@s1:e91" },
            { "role": "group", "children": [
                { "role": "statictext", "name": "Apellido Materno:" },
                { "role": "textfield", "ref_id": "@s1:e92" }
            ]},
            { "role": "textfield", "ref_id": "@s1:e93" },
            { "role": "statictext", "name": "Email" },
            { "role": "textfield", "name": "Correo", "ref_id": "@s1:e94" }
        ]})
    }

    #[test]
    fn an_unnamed_field_takes_the_text_just_before_it() {
        let found = labelled_fields(&tree());
        let labels: Vec<_> = found
            .iter()
            .map(|(label, f)| (label.as_str(), f["ref_id"].clone()))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("Apellido Paterno", json!("@s1:e91")),
                ("Apellido Materno", json!("@s1:e92"))
            ]
        );
    }

    #[test]
    fn a_field_with_no_text_since_the_previous_one_gets_no_label() {
        assert!(
            !labelled_fields(&tree())
                .iter()
                .any(|(_, f)| f["ref_id"] == "@s1:e93")
        );
    }

    #[test]
    fn find_answers_in_find_shape() {
        let snapshot = json!({ "data": { "tree": tree(), "snapshot_id": "s1" } });
        let answer =
            find_answer(&snapshot, Some("textfield"), "Apellido Materno", true, true).unwrap();
        assert_eq!(answer["match"]["ref_id"], "@s1:e92");
        assert_eq!(answer["match"]["name"], "Apellido Materno");
        assert!(find_answer(&snapshot, None, "Teléfono", false, true).is_none());
    }
}
