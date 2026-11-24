// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! A web dropdown whose own parts do not open it.
//!
//! Component libraries (PrimeNG's `p-select`, many custom selects) draw a
//! dropdown as a container that holds a combobox and a "dropdown trigger"
//! button, and open it from the container's click handler. In Chrome the
//! combobox's `ExpandCollapse` does nothing, and the trigger's default action
//! is delivered without reaching that handler, so `expand` exhausted its chain
//! and `click` on the trigger came back unverified with the list closed. The
//! container advertises `Invoke`, which runs its click handler: invoking it
//! opens (or closes) the list. The combobox's own expanded state, which the
//! page keeps current, proves it.

use std::time::{Duration, Instant};

use agent_desktop_core::{ActionStep, AdapterError, Deadline};
use uiautomation::types::ControlType;

use crate::actions::chain::{
    DeliveryOutcome, after_delivery, build_step, capped_verification_end, invoke_available,
    invoke_pattern_delivered,
};
use crate::actions::disclosure::ExpandKind;
use crate::tree::automation::automation_client;
use crate::tree::element::UIAElement;
use crate::tree::properties::property_ids::TreeProperty;
use crate::tree::properties::read_one;

pub(crate) const CONTAINER_LABEL: &str = "Dropdown.ContainerInvoke";

/// How far up the container may be: the trigger and the combobox are its
/// children, or their children.
const MAX_UP: usize = 2;
/// A page opens its list on the next frame or two; a background window can
/// be slower.
const OPEN_TIMEOUT: Duration = Duration::from_millis(2_000);
const POLL: Duration = Duration::from_millis(50);

pub(crate) struct Dropdown {
    container: UIAElement,
    combo: UIAElement,
}

impl Dropdown {
    /// The element that holds the combobox, the trigger and, while open, the
    /// list of options.
    pub(crate) fn container(&self) -> &UIAElement {
        &self.container
    }
}

/// The dropdown `element` belongs to: the nearest ancestor that advertises
/// `Invoke` and holds a combobox with an expanded state. `None` for anything
/// else, so ordinary buttons and comboboxes keep their own actions.
pub(crate) fn find(element: &UIAElement) -> Option<Dropdown> {
    let walker = automation_client().ok()?.get_control_view_walker().ok()?;
    let mut current = element.0.clone();
    for _ in 0..MAX_UP {
        let parent = walker.get_parent(&current).ok()?;
        let container = UIAElement(parent.clone());
        if invoke_available(&container)
            && let Some(combo) = combobox_in(&walker, &parent, element)
        {
            return Some(Dropdown { container, combo });
        }
        current = parent;
    }
    None
}

fn combobox_in(
    walker: &uiautomation::UITreeWalker,
    parent: &uiautomation::UIElement,
    _element: &UIAElement,
) -> Option<UIAElement> {
    let mut child = walker.get_first_child(parent).ok();
    while let Some(current) = child {
        let candidate = UIAElement(current.clone());
        if current.get_control_type().ok() == Some(ControlType::ComboBox)
            && state(&candidate).is_some()
        {
            return Some(candidate);
        }
        child = walker.get_next_sibling(&current).ok();
    }
    None
}

fn state(element: &UIAElement) -> Option<ExpandKind> {
    if read_one(element, TreeProperty::ExpandCollapseAvailable).flag() != Some(true) {
        return None;
    }
    read_one(element, TreeProperty::ExpandCollapseState)
        .number()
        .and_then(ExpandKind::from_i32)
}

fn is_open(kind: ExpandKind) -> bool {
    matches!(kind, ExpandKind::Expanded | ExpandKind::PartiallyExpanded)
}

/// Opens (`want_open`) or closes the list by invoking the container, then
/// waits for the combobox to say so. Never invokes when the combobox already
/// is where it should be, or when its state cannot be read; a combobox that
/// reports no expanded state yet (a leaf) counts as closed, and the opening is
/// still proven by the state it then reports.
pub(crate) fn set_open(
    dropdown: &Dropdown,
    want_open: bool,
    deadline: Deadline,
) -> Result<DeliveryOutcome, AdapterError> {
    match state(&dropdown.combo) {
        Some(kind) if is_open(kind) == want_open => {
            return Ok(DeliveryOutcome::SatisfiedNoDelivery);
        }
        // Before its first opening a page often leaves the combobox without
        // an expanded state, which Chrome reports as a leaf: it is closed.
        None => return Ok(DeliveryOutcome::NotDelivered),
        Some(_) => {}
    }
    if !invoke_pattern_delivered(&dropdown.container)? {
        return Ok(DeliveryOutcome::NotDelivered);
    }
    let verified = wait_for(dropdown, want_open, deadline).map_err(after_delivery)?;
    Ok(DeliveryOutcome::from_delivery(true, verified))
}

fn wait_for(
    dropdown: &Dropdown,
    want_open: bool,
    deadline: Deadline,
) -> Result<bool, AdapterError> {
    let end = capped_verification_end(deadline, OPEN_TIMEOUT)?;
    loop {
        if state(&dropdown.combo).is_some_and(|kind| is_open(kind) == want_open) {
            return Ok(true);
        }
        if Instant::now() >= end {
            return Ok(false);
        }
        std::thread::sleep(deadline.remaining_slice(POLL)?);
    }
}

/// A click on a dropdown's trigger (a button inside it with an expanded
/// state and no `Invoke` of its own): it toggles the list through the
/// container. `None` when `element` is no such trigger or nothing was
/// delivered, so the ordinary click chain runs.
pub(crate) fn trigger_click(
    element: &UIAElement,
    deadline: Deadline,
) -> Result<Option<ActionStep>, AdapterError> {
    if invoke_available(element) || state(element).is_none() {
        return Ok(None);
    }
    if matches!(
        element.0.get_control_type().ok(),
        Some(ControlType::ComboBox | ControlType::Edit) | None
    ) {
        return Ok(None);
    }
    let Some(dropdown) = find(element) else {
        return Ok(None);
    };
    let Some(current) = state(&dropdown.combo) else {
        return Ok(None);
    };
    let outcome = set_open(&dropdown, !is_open(current), deadline)?;
    Ok((outcome != DeliveryOutcome::NotDelivered).then(|| build_step(CONTAINER_LABEL, outcome)))
}

/// `expand` or `collapse` after the element's own chain gave up: the same
/// through the dropdown it belongs to, if it belongs to one.
pub(crate) fn disclosure_fallback(
    element: &UIAElement,
    want_open: bool,
    deadline: Deadline,
) -> Result<Option<ActionStep>, AdapterError> {
    let Some(dropdown) = find(element) else {
        return Ok(None);
    };
    let outcome = set_open(&dropdown, want_open, deadline)?;
    Ok((outcome != DeliveryOutcome::NotDelivered).then(|| build_step(CONTAINER_LABEL, outcome)))
}

/// UIA_ListItemControlTypeId and UIA_DocumentControlTypeId.
const LIST_ITEM_CONTROL_TYPE: i32 = 50007;
const DOCUMENT_CONTROL_TYPE: i32 = 50030;
const MAX_TO_DOCUMENT: usize = 96;

/// The option named `value` of an open dropdown whose list the page put
/// elsewhere: PrimeNG appends it to the end of the page when the dropdown is
/// in a dialog (`appendTo="body"`), outside the dropdown's container. The one
/// shown option with that name in the element's web document; `None` when
/// there is none or more than one (never a guess).
pub(crate) fn option_in_page(element: &UIAElement, value: &str) -> Option<UIAElement> {
    use uiautomation::types::{TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    let client = automation_client().ok()?;
    let walker = client.get_control_view_walker().ok()?;
    let mut document = element.0.clone();
    let mut found_document = false;
    for _ in 0..MAX_TO_DOCUMENT {
        if document.get_control_type().ok().map(|kind| kind as i32) == Some(DOCUMENT_CONTROL_TYPE) {
            found_document = true;
            break;
        }
        document = walker.get_parent(&document).ok()?;
    }
    if !found_document {
        return None;
    }
    let condition = client
        .create_property_condition(
            UIProperty::ControlType,
            Variant::from(LIST_ITEM_CONTROL_TYPE),
            None,
        )
        .ok()?;
    let mut shown = document
        .find_all(TreeScope::Descendants, &condition)
        .ok()?
        .into_iter()
        .map(UIAElement::from)
        .filter(|item| {
            crate::actions::select_search::selection_item_available(item)
                && crate::actions::select_search::name_matches(item, value)
                && item.0.is_offscreen().ok() == Some(false)
        });
    match (shown.next(), shown.next()) {
        (Some(one), None) => Some(one),
        _ => None,
    }
}

/// `select` on a web dropdown that only its container opens (PrimeNG): the
/// list is opened through the container, the option found in it (or, when the
/// page shows its list elsewhere, in the page) and selected. `None` when the
/// element is not such a dropdown or the option is not there, so upstream's
/// own select runs. The old fork did this inside upstream's select.
pub(crate) fn select(
    element: &UIAElement,
    value: &str,
    deadline: Deadline,
) -> Result<Option<Vec<ActionStep>>, AdapterError> {
    use uiautomation::patterns::UISelectionItemPattern;
    use uiautomation::types::TreeScope;
    let Some(dropdown) = find(element) else {
        return Ok(None);
    };
    if set_open(&dropdown, true, deadline)? == DeliveryOutcome::NotDelivered {
        return Ok(None);
    }
    let end = Instant::now() + Duration::from_millis(1_000);
    let option = loop {
        let in_container = automation_client()
            .ok()
            .and_then(|client| client.create_true_condition().ok())
            .and_then(|all| {
                dropdown
                    .container()
                    .0
                    .find_all(TreeScope::Descendants, &all)
                    .ok()
            })
            .unwrap_or_default()
            .into_iter()
            .map(UIAElement::from)
            .find(|item| {
                crate::actions::select_search::selection_item_available(item)
                    && crate::actions::select_search::name_matches(item, value)
            });
        if let Some(found) = in_container.or_else(|| option_in_page(element, value)) {
            break Some(found);
        }
        if Instant::now() >= end || deadline.remaining().is_zero() {
            break None;
        }
        std::thread::sleep(POLL);
    };
    let Some(option) = option else {
        return Ok(None);
    };
    let selected = option
        .0
        .get_pattern::<UISelectionItemPattern>()
        .and_then(|pattern| pattern.select())
        .is_ok();
    if !selected {
        return Ok(None);
    }
    Ok(Some(vec![build_step(
        "Dropdown.SelectOption",
        DeliveryOutcome::from_delivery(true, false),
    )]))
}
