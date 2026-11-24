// Part of Agent Record Adapter (see NOTICE).
//! The platform-neutral part of Agent Record Adapter.
//!
//! It sits on the unmodified `agent-desktop-core`: every public item of core
//! is re-exported here, so the commands ported from the old fork keep their
//! `crate::` paths. The few private core helpers they used are small copies in
//! [`compat`], and the operations upstream's adapter traits do not have
//! (record, menu, click at a point, navigate, secrets...) are [`RecordOps`],
//! which our Windows adapter implements.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub use agent_desktop_core::*;

pub mod background;
pub mod commands;
mod compat;
mod ext;
mod input;

pub use compat::{refs, storage, tree};
pub use ext::{Adapter, MenuSearch, PointClick, RecordOps, RecordSink};
pub use input::record_request::RecordRequest;
pub use input::record_summary::{RecordSummary, RecordedVideo};
pub use input::recorded_event::RecordedEvent;
pub use input::stroke_params::{MAX_STROKE_MS, MAX_STROKE_POINTS, StrokeParams, StrokePoint};

/// `crate::adapter::PlatformAdapter` in the ported commands means an adapter
/// with our extra operations too.
pub mod adapter {
    pub use crate::ext::Adapter as PlatformAdapter;
    pub use agent_desktop_core::adapter::*;
}
