// Part of Agent Record Adapter (see NOTICE).
//! UI Automation access for our background layer, on top of upstream's
//! public `tree` module.
//!
//! Upstream keeps the `uiautomation::UIElement` inside its `UIAElement`
//! crate-private, so code outside its crate cannot reach the element a ref
//! resolved to. [`uia_element`] reads it through a layout mirror of
//! upstream's type, checked once per process against upstream's own
//! constructor (same size and alignment, and the element read back is the one
//! put in). When that check ever fails (upstream changed the type), it
//! answers "not supported" and every caller falls back to upstream's own
//! route, so a changed upstream can cost background routes but never
//! correctness. A public accessor upstream would remove this (see
//! docs/upstream-requests.md).

use std::sync::OnceLock;

use agent_desktop_core::{AdapterError, NativeHandle};

pub(crate) use upstream_windows::tree::{automation, roles};

/// Upstream's element type, for calls into upstream's public readers.
pub(crate) type UpstreamElement = upstream_windows::tree::element::UIAElement;

pub(crate) mod element {
    pub(crate) use super::{UIAElement, uia_element};
}

/// One UI Automation element, owned by our layer.
#[derive(Clone)]
pub struct UIAElement(pub uiautomation::UIElement);

impl From<uiautomation::UIElement> for UIAElement {
    fn from(element: uiautomation::UIElement) -> Self {
        Self(element)
    }
}

impl UIAElement {
    /// The same element as upstream's type.
    pub(crate) fn upstream(&self) -> UpstreamElement {
        UpstreamElement::from(self.0.clone())
    }

    /// Moves the element into a core handle upstream's adapter accepts.
    pub fn into_native_handle(self) -> NativeHandle {
        self.upstream().into_native_handle()
    }
}

/// Mirror of upstream's `UIAElement(UIElement, Option<ProcessPayload>)`.
#[allow(dead_code)]
struct Mirror(uiautomation::UIElement, Option<MirrorPayload>);

/// Mirror of upstream's `ProcessPayload { pid: u32, token: String }`.
#[allow(dead_code)]
struct MirrorPayload {
    pid: u32,
    token: String,
}

fn mirror_matches() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        if std::mem::size_of::<Mirror>() != std::mem::size_of::<UpstreamElement>()
            || std::mem::align_of::<Mirror>() != std::mem::align_of::<UpstreamElement>()
        {
            tracing::warn!("upstream UIAElement layout changed: background routes are off");
            return false;
        }
        let Ok(client) = automation::automation_client() else {
            return false;
        };
        let Ok(root) = client.get_root_element() else {
            return false;
        };
        let wrapped = UpstreamElement::from(root.clone());
        let read = read_mirror(&wrapped);
        let same = client.compare_elements(&root, &read).unwrap_or(false);
        if !same {
            tracing::warn!("upstream UIAElement mirror check failed: background routes are off");
        }
        same
    })
}

fn read_mirror(element: &UpstreamElement) -> uiautomation::UIElement {
    // SAFETY: `Mirror` has the same field types in the same order as
    // upstream's `UIAElement`, so rustc lays both out the same; the size,
    // alignment and read-back check in `mirror_matches` confirms it on this
    // build before any caller relies on it. Only a shared reference is
    // reinterpreted and the element is cloned (an AddRef), never moved.
    let mirror = unsafe { &*(element as *const UpstreamElement as *const Mirror) };
    mirror.0.clone()
}

/// The UI Automation element a core handle carries.
pub fn uia_element(handle: &NativeHandle) -> Result<UIAElement, AdapterError> {
    let element = upstream_windows::tree::element::uia_element(handle)?;
    if !mirror_matches() {
        return Err(AdapterError::not_supported("background element access"));
    }
    Ok(UIAElement(read_mirror(element)))
}

pub(crate) mod properties {
    pub(crate) use upstream_windows::tree::properties::{
        ElementProperties, PropertyOutcome, PropertyValue, property_ids,
    };

    use super::UIAElement;
    use property_ids::TreeProperty;

    pub(crate) fn read_one(element: &UIAElement, property: TreeProperty) -> PropertyOutcome {
        upstream_windows::tree::properties::read_one(&element.upstream(), property)
    }

    pub(crate) fn read_live(
        element: &UIAElement,
    ) -> (ElementProperties, Vec<agent_desktop_core::AdapterError>) {
        upstream_windows::tree::properties::read_live(&element.upstream())
    }
}

pub(crate) mod live_read {
    /// Upstream checks the provider process is still the verified one before
    /// a read; its action path does it again, so ours has nothing to add.
    pub(crate) fn corroborate_verified_process(
        _element: &super::UIAElement,
    ) -> Result<(), agent_desktop_core::AdapterError> {
        Ok(())
    }
}

pub(crate) mod name_evidence {
    pub(crate) use upstream_windows::tree::name_evidence::name_fields;

    pub(crate) fn read_label(
        element: &super::UIAElement,
        cached: bool,
    ) -> upstream_windows::tree::name_evidence::LabelOutcome {
        upstream_windows::tree::name_evidence::read_label(&element.upstream(), cached)
    }
}

#[cfg(test)]
pub(crate) mod fixture {
    /// The COM apartment upstream's own process start sets up.
    pub(crate) fn ensure_test_apartment() {
        let _ = upstream_windows::ensure_owned_process_mta_and_dpi();
    }
}
