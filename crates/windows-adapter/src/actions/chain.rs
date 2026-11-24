// Part of Agent Record Adapter (see NOTICE).
//! A copy of the parts of upstream's private `actions::chain` our routes use
//! (vercel-labs/agent-desktop, Apache-2.0).

use std::time::{Duration, Instant};

use agent_desktop_core::{ActionStep, AdapterError, Deadline, DeliverySemantics, StepMechanism};
use uiautomation::patterns::UIInvokePattern;

use crate::tree::UIAElement;
use crate::tree::properties::{property_ids::TreeProperty, read_one};

/// The end of a verification window: `cap` from now, never past the deadline.
pub(crate) fn capped_verification_end(
    deadline: Deadline,
    cap: Duration,
) -> Result<Instant, AdapterError> {
    let remaining = deadline.remaining();
    if remaining.is_zero() {
        return Err(deadline.timeout_error());
    }
    let local = Instant::now() + cap;
    Ok(Instant::now()
        .checked_add(remaining)
        .map_or(local, |end| end.min(local)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeliveryOutcome {
    NotDelivered,
    SatisfiedNoDelivery,
    DeliveredUnverified,
    DeliveredVerified,
}

impl DeliveryOutcome {
    pub(crate) fn from_delivery(delivered: bool, verified: bool) -> Self {
        match (delivered, verified) {
            (false, _) => Self::NotDelivered,
            (true, false) => Self::DeliveredUnverified,
            (true, true) => Self::DeliveredVerified,
        }
    }
}

pub(crate) fn after_delivery(error: AdapterError) -> AdapterError {
    error.with_disposition(DeliverySemantics::delivered_unverified())
}

pub(crate) fn build_step(label: &'static str, outcome: DeliveryOutcome) -> ActionStep {
    let built = match outcome {
        DeliveryOutcome::NotDelivered => ActionStep::skipped(label),
        DeliveryOutcome::SatisfiedNoDelivery => ActionStep::skipped(label).with_verified(true),
        DeliveryOutcome::DeliveredUnverified => ActionStep::succeeded(label).with_verified(false),
        DeliveryOutcome::DeliveredVerified => ActionStep::succeeded(label).with_verified(true),
    };
    built.with_mechanism(StepMechanism::SemanticApi)
}

pub(crate) fn invoke_available(element: &UIAElement) -> bool {
    read_one(element, TreeProperty::InvokeAvailable).flag() == Some(true)
}

/// Whether `Invoke` was accepted. Upstream classifies COM failures further;
/// here a failure is "not delivered", so the caller tries its next route.
pub(crate) fn invoke_pattern_delivered(element: &UIAElement) -> Result<bool, AdapterError> {
    Ok(element
        .0
        .get_pattern::<UIInvokePattern>()
        .and_then(|pattern| pattern.invoke())
        .is_ok())
}
