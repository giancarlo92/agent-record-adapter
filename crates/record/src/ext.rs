// Part of Agent Record Adapter (see NOTICE).
//! Operations upstream's `PlatformAdapter` traits do not have. In the old fork
//! they were new default methods of those traits; here they are a trait of
//! our own that our Windows adapter implements, so upstream stays unchanged.

use agent_desktop_core::{
    AdapterError, InteractionLease, OperationReport, PlatformAdapter, ProcessId, WindowInfo,
};

use crate::{RecordRequest, RecordSummary, RecordedEvent, StrokeParams};

/// A click at a point of a window: in pixels of the window's screenshot, or,
/// with `screen`, in screen pixels (as element bounds are).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointClick {
    pub x: i32,
    pub y: i32,
    pub double: bool,
    pub screen: bool,
}

/// A menu item looked for anywhere in a menu bar: by the key it shows
/// ("Ctrl+Enter"), or by its name in any submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuSearch {
    Shortcut(String),
    Name(String),
}

/// Receives each event a `record` session observes, in order. An error stops
/// the session and is returned from `record_input`.
pub type RecordSink<'a> = dyn FnMut(RecordedEvent) -> Result<(), AdapterError> + 'a;

/// Our operations. Every method has a not-supported default, so a platform
/// without them still builds.
pub trait RecordOps {
    /// Runs the command of an item of `window`'s menu bar, found by its path
    /// of item names, without opening the menu or taking the focus. Returns
    /// the item names as the menu shows them.
    fn invoke_menu(
        &self,
        _window: &WindowInfo,
        _path: &[String],
        _lease: &InteractionLease,
    ) -> Result<Vec<String>, AdapterError> {
        Err(AdapterError::not_supported("invoke_menu"))
    }

    /// The path of the first item of `window`'s menu bar that matches
    /// `search`, from the menu bar down, as the menu shows the names.
    fn find_menu_item(
        &self,
        _window: &WindowInfo,
        _search: &MenuSearch,
        _lease: &InteractionLease,
    ) -> Result<Vec<String>, AdapterError> {
        Err(AdapterError::not_supported("find_menu_item"))
    }

    /// Opens `url` in the tab a browser window shows, keeping its session
    /// (`in_app`: the way a single-page app's own links change the address).
    fn navigate_window(
        &self,
        _window: &WindowInfo,
        _url: &str,
        _in_app: bool,
        _lease: &InteractionLease,
    ) -> Result<serde_json::Value, AdapterError> {
        Err(AdapterError::not_supported("navigate"))
    }

    /// Clicks (or double-clicks) a point of `window` without moving the cursor
    /// or taking the focus.
    fn click_at(
        &self,
        _window: &WindowInfo,
        _point: &PointClick,
        _lease: &InteractionLease,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::not_supported("click_at"))
    }

    /// Types text into a window (its focused field, else its document)
    /// without the person's keyboard or foreground (`type w-<id> TEXT`).
    fn type_in_window(
        &self,
        _window: &WindowInfo,
        _text: &str,
        _lease: &InteractionLease,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::not_supported("type_in_window"))
    }

    /// Holds the left button down at the stroke's first point, moves the
    /// pointer through its points at their times and releases it at the last.
    fn stroke(
        &self,
        _params: StrokeParams,
        _lease: &InteractionLease,
    ) -> Result<OperationReport, AdapterError> {
        Err(AdapterError::not_supported("stroke"))
    }

    /// Observes the user's own clicks, typing, key presses and scrolls until
    /// the request's stop condition, passing each one to `sink` as it
    /// happens. It never sends input and never takes the interaction lease.
    fn record_input(
        &self,
        _request: &RecordRequest,
        _sink: &mut RecordSink<'_>,
    ) -> Result<RecordSummary, AdapterError> {
        Err(AdapterError::not_supported("record_input"))
    }

    /// The address of the page a browser window shows, scheme included, read
    /// in a way that does not depend on the browser's language. `None` when
    /// the window shows no page or the platform cannot read it.
    fn browser_address(&self, _window_id: &str) -> Option<String> {
        None
    }

    /// Asks the person at the screen for a secret in a window of the
    /// platform's own, its characters hidden. `Ok(None)` when they cancel.
    fn ask_secret(&self, _title: &str, _message: &str) -> Result<Option<String>, AdapterError> {
        Err(AdapterError::not_supported("ask_secret"))
    }

    /// Runs `request` on the element a saved ref describes, found again by
    /// its place (role, name and bounds in its window): for twins upstream
    /// refuses as not unique (an unnamed password field, row icons).
    fn act_by_place(
        &self,
        _entry: &crate::RefEntry,
        _request: agent_desktop_core::actions::action_request::ActionRequest,
    ) -> Result<agent_desktop_core::ActionResult, AdapterError> {
        Err(AdapterError::not_supported("act_by_place"))
    }

    /// Runs `work`, which makes a running process show a new window, and
    /// keeps that window behind the person's until it has settled.
    fn while_new_windows_stay_behind(&self, _pid: ProcessId, work: &mut dyn FnMut()) {
        work();
    }
}

/// An upstream adapter with our operations too.
pub trait Adapter: PlatformAdapter + RecordOps {}

impl<T: PlatformAdapter + RecordOps> Adapter for T {}
