// Part of Agent Record Adapter (see NOTICE).
//! The agent-desktop Windows adapter, unmodified, with our background layer in
//! front of it.
//!
//! Everything the upstream crate exports is re-exported as is, except
//! [`WindowsAdapter`]: ours wraps upstream's and tries the background route
//! first, falling back to the original. The engine binary (upstream's own
//! `src/main.rs`) depends on this crate under upstream's crate name, so it
//! builds our adapter without a line of upstream changing.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![cfg(target_os = "windows")]

pub use upstream_windows::*;

mod actions;
// Written by scripts/gen_forwarding.py, which `--check` compares byte for byte.
#[rustfmt::skip]
mod forward;
mod hooks;
mod input;
mod ours;
mod system;
pub mod tree;
pub mod twins;

pub use system::work_desktop::run_there as run_on_work_desktop;

/// Upstream's adapter with the background layer in front.
pub struct WindowsAdapter {
    inner: upstream_windows::WindowsAdapter,
}

impl WindowsAdapter {
    pub fn new() -> Self {
        Self {
            inner: upstream_windows::WindowsAdapter::new(),
        }
    }

    /// The unmodified upstream adapter, for the fallback route.
    pub fn upstream(&self) -> &upstream_windows::WindowsAdapter {
        &self.inner
    }
}

impl Default for WindowsAdapter {
    fn default() -> Self {
        Self::new()
    }
}
