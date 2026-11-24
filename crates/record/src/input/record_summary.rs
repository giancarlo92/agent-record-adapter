// Part of Agent Record Adapter (see NOTICE).
use serde::Serialize;

/// How a `record` session ended.
///
/// `stop_reason` is `interrupted`, `stop_file`, `max_duration` or
/// `indicator` (the person pressed Stop on the on-screen indicator).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordSummary {
    pub duration_ms: u64,
    pub stop_reason: &'static str,
    /// The video of the session, when one was asked for and written.
    pub video: Option<RecordedVideo>,
}

/// A session's video. `offset_ms` is when its first frame was taken, on the
/// clock of the recording's `t_ms`: an action at `t_ms` is at
/// `t_ms - offset_ms` in the video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordedVideo {
    pub path: String,
    pub offset_ms: u64,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
}
