// Part of Agent Record Adapter (see NOTICE).
//! The label beside an unnamed field.
//!
//! Many web forms draw a field's label as text next to it without tying the
//! two together, so the field has no name, no id and no placeholder (a
//! password box often has none of them). The text just before it is how a
//! person, and a replay, tells it apart: the recorder looks back through the
//! previous siblings of the field and of its nearest parents for a short
//! static text.

use uiautomation::{UIElement, UITreeWalker};

use agent_desktop_core::LocatorField;

use crate::tree::element::UIAElement;
use crate::tree::properties::property_ids::TreeProperty;
use crate::tree::properties::read_live;
use crate::tree::roles::resolve_role;

const LEVELS: usize = 3;
const SIBLINGS: usize = 3;
const DEPTH: usize = 3;
const MAX_LABEL: usize = 60;

/// The nearest short text before `element`, if any.
pub(super) fn nearby_label(walker: &UITreeWalker, element: &UIElement) -> Option<String> {
    let mut current = element.clone();
    for _ in 0..LEVELS {
        let mut sibling = walker.get_previous_sibling(&current).ok();
        for _ in 0..SIBLINGS {
            let Some(found) = sibling else {
                break;
            };
            if let Some(text) = text_in(walker, &found, DEPTH) {
                return Some(text);
            }
            sibling = walker.get_previous_sibling(&found).ok();
        }
        current = walker.get_parent(&current).ok()?;
    }
    None
}

/// The first static text in `element` or below it, `depth` levels at most.
fn text_in(walker: &UITreeWalker, element: &UIElement, depth: usize) -> Option<String> {
    let (properties, _) = read_live(&UIAElement::from(element.clone()));
    let is_text =
        matches!(resolve_role(&properties), LocatorField::Known(role) if is_text_role(&role));
    if is_text {
        let name = properties.get(TreeProperty::Name).non_blank_text()?;
        return usable(&name).then(|| name.trim().to_string());
    }
    if depth == 0 {
        return None;
    }
    let mut child = walker.get_first_child(element).ok();
    while let Some(found) = child {
        if let Some(text) = text_in(walker, &found, depth - 1) {
            return Some(text);
        }
        child = walker.get_next_sibling(&found).ok();
    }
    None
}

/// The fields a label beside them names; a click on a drawn surface (a
/// canvas, a grid with no cells) keeps its point instead.
pub(super) fn is_field_role(role: &str) -> bool {
    matches!(
        role,
        "textfield" | "combobox" | "checkbox" | "radiobutton" | "switch" | "slider" | "listbox"
    )
}

pub(super) fn is_text_role(role: &str) -> bool {
    matches!(role, "statictext" | "text" | "label")
}

/// A label is short: a long text is a paragraph, not a field's name.
pub(super) fn usable(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && text.chars().count() <= MAX_LABEL
}

#[cfg(test)]
#[path = "record_label_tests.rs"]
mod tests;
