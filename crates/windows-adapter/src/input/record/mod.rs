// Part of Agent Record Adapter (see NOTICE).
//! Observes the user's own input for `record`.
//!
//! Low-level mouse and keyboard hooks run on a dedicated thread that only
//! forwards each raw transition over a channel, because Windows removes a
//! low-level hook whose callback is slow. The calling thread reads UI
//! Automation for every action it receives and streams one event per action.
//! Input that Windows marks as injected is dropped unless the request asks for
//! it. `agent-desktop`'s own actions reach a background recording another
//! way, through its agent inbox (see `record_agent`).

use agent_desktop_core::{AdapterError, RecordRequest, RecordSink, RecordSummary};

#[cfg(target_os = "windows")]
pub(crate) mod record_agent;
#[cfg(target_os = "windows")]
mod record_browser;
mod record_copy;
#[cfg(target_os = "windows")]
mod record_element;
#[cfg(target_os = "windows")]
mod record_folder;
mod record_followup;
#[cfg(target_os = "windows")]
mod record_gesture;
#[cfg(target_os = "windows")]
mod record_hook;
mod record_indicator;
#[cfg(target_os = "windows")]
mod record_keys;
mod record_label;
#[cfg(target_os = "windows")]
mod record_process;
mod record_promote;
#[cfg(target_os = "windows")]
mod record_session;
#[cfg(target_os = "windows")]
mod record_stop;
#[cfg(target_os = "windows")]
mod record_text;
mod record_video;
mod record_video_area;
mod record_video_frame;
#[cfg(target_os = "windows")]
mod record_window;

#[cfg(target_os = "windows")]
pub(crate) fn record_input(
    request: &RecordRequest,
    sink: &mut RecordSink<'_>,
) -> Result<RecordSummary, AdapterError> {
    record_session::run(request, sink)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn record_input(
    _request: &RecordRequest,
    _sink: &mut RecordSink<'_>,
) -> Result<RecordSummary, AdapterError> {
    Err(AdapterError::not_supported("record_input"))
}
