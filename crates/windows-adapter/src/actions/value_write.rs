// Part of Agent Record Adapter (see NOTICE).
//! Value reads our routes need, from upstream's private `actions::value_write`.

use agent_desktop_core::AdapterError;
use uiautomation::patterns::UIValuePattern;

use crate::tree::UIAElement;
use crate::tree::properties::{property_ids::TreeProperty, read_one};

/// Whether the element has a value pattern that is not read-only.
pub(crate) fn value_writable(element: &UIAElement) -> bool {
    read_one(element, TreeProperty::ValueAvailable).flag() == Some(true)
        && read_one(element, TreeProperty::ValueIsReadOnly).flag() == Some(false)
}

/// Whether the element's value now equals `expected`; `None` for a password
/// field (never read) or a value that cannot be read.
pub(crate) fn gated_pattern_value_equals(
    element: &UIAElement,
    expected: &str,
) -> Result<Option<bool>, AdapterError> {
    if read_one(element, TreeProperty::IsPassword).flag() != Some(false) {
        return Ok(None);
    }
    Ok(element
        .0
        .get_pattern::<UIValuePattern>()
        .and_then(|pattern| pattern.get_value())
        .ok()
        .map(|value| value == expected))
}
