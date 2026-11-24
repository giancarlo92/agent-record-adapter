// Part of Agent Record Adapter (see NOTICE).
//! Item checks from upstream's private `actions::select_search`.

use agent_desktop_core::LocatorField;
use upstream_windows::tree::properties::ElementProperties;

use crate::tree::UIAElement;
use crate::tree::properties::{property_ids::TreeProperty, read_one};

pub(crate) fn selection_item_available(element: &UIAElement) -> bool {
    read_one(element, TreeProperty::SelectionItemAvailable).flag() == Some(true)
}

/// Whether the element's accessible name is `value` (case-insensitive), as
/// upstream computes the name (with its label).
pub(crate) fn name_matches(element: &UIAElement, value: &str) -> bool {
    let properties = ElementProperties::from_reads(vec![
        (TreeProperty::Name, read_one(element, TreeProperty::Name)),
        (
            TreeProperty::FullDescription,
            read_one(element, TreeProperty::FullDescription),
        ),
        (
            TreeProperty::HelpText,
            read_one(element, TreeProperty::HelpText),
        ),
        (
            TreeProperty::IsPassword,
            read_one(element, TreeProperty::IsPassword),
        ),
    ]);
    let label = crate::tree::name_evidence::read_label(element, false);
    let (name, _) = crate::tree::name_evidence::name_fields(&properties, &label);
    matches!(name, LocatorField::Known(text) if text.eq_ignore_ascii_case(value))
}
