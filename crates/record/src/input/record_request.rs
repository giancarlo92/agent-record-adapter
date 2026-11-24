// Part of Agent Record Adapter (see NOTICE).
use std::path::PathBuf;

/// What a `record` session watches and when it stops.
///
/// `app`, `window_id` and `display` narrow which actions are kept; all absent
/// keeps every window except the recorder's own process. `display` is a
/// display id from `list-displays`: a pointer action is kept when it lands on
/// that display, and a key action when the focused window's center is on it.
/// A session ends at the first of: the caller's interrupt, `stop_file`
/// appearing on disk, or `max_duration_ms` elapsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRequest {
    pub app: Option<String>,
    pub window_id: Option<String>,
    pub display: Option<String>,
    pub max_duration_ms: u64,
    pub stop_file: Option<PathBuf>,
    pub include_injected: bool,
    /// Where to write an MP4 video of the recorded area (the window, the
    /// display, or the display an app is on), when one is asked for.
    pub video: Option<PathBuf>,
    /// Show a small always-on-top indicator while recording, with a Stop
    /// button that ends the session (`stop_reason: "indicator"`).
    pub indicator: bool,
}
