// Part of Agent Record Adapter (see NOTICE).
//! The trait methods our background layer answers first. Each tries our route
//! and falls back to the unmodified upstream adapter (`self.inner`).
//!
//! With background mode off (`AGENT_DESKTOP_BACKGROUND=0`) every method here
//! only calls upstream.
use agent_desktop_core as core;
use agent_desktop_core::actions::action_request::ActionRequest;
use agent_desktop_core::actions::capability;
use agent_desktop_core::*;

use crate::WindowsAdapter;

/// Set by the front for a `wait --event window-opened` (see
/// `ours_capture_signal_baseline`).
pub const WAIT_OPENED_ENV: &str = "AGENT_RECORD_WAIT_OPENED";
/// When the last action of this process was delivered.
static LAST_ACTION: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
/// How long a read-back may run after the command's own time ran out.
const READ_AFTER_DEADLINE_MS: u64 = 1_500;

const LIVE_RETRIES: u32 = 40;
const LIVE_RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(400);

impl WindowsAdapter {
    pub(crate) fn ours_execute_action(
        &self,
        handle: &NativeHandle,
        request: ActionRequest,
        lease: &InteractionLease,
    ) -> Result<ActionResult, AdapterError> {
        let original = |handle: &NativeHandle, request: ActionRequest, lease: &InteractionLease| {
            self.inner.execute_action(handle, request, lease)
        };
        let result = crate::actions::background::execute_action(handle, request, lease, &original);
        if result.is_ok()
            && let Ok(mut last) = LAST_ACTION.lock()
        {
            *last = Some(std::time::Instant::now());
        }
        result
    }

    /// The first baseline of a `window-opened` wait (the front sets
    /// `AGENT_RECORD_WAIT_OPENED`) leaves out the windows a background click
    /// opened moments before, while it was still finishing (a page's file
    /// picker): the wait then reports them as opened.
    pub(crate) fn ours_capture_signal_baseline(
        &self,
        filter: &SignalFilter,
        deadline: Deadline,
    ) -> Result<SignalBaseline, AdapterError> {
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        let mut baseline = self.inner.capture_signal_baseline(filter, deadline)?;
        if FIRST.swap(false, std::sync::atomic::Ordering::SeqCst)
            && background::enabled()
            && std::env::var_os(WAIT_OPENED_ENV).is_some()
        {
            background::forget_windows_opened_by_last_action(&mut baseline.windows);
        }
        Ok(baseline)
    }

    /// A key in one window, in the background: a shortcut its classic menu or
    /// one of its buttons shows runs as that command; any other key is posted
    /// to the window that holds its focus. Upstream's press is the fallback.
    pub(crate) fn ours_press_key_for_window(
        &self,
        window: &WindowInfo,
        combo: &KeyCombo,
        policy: InteractionPolicy,
        lease: &InteractionLease,
    ) -> Result<ActionResult, AdapterError> {
        let result = (|| {
            if !policy.is_headed() && background::enabled() {
                if let Some(result) =
                    crate::actions::menu_command::run_shortcut(window, combo, lease.deadline())
                {
                    return Ok(result);
                }
                if let Some(result) =
                    crate::actions::menu_command::run_button_shortcut(window, combo)
                {
                    return Ok(result);
                }
                match crate::actions::posted_keys::press(window, combo, lease.deadline()) {
                    Ok(result) => return Ok(result),
                    Err(error)
                        if error.disposition != DeliverySemantics::delivered_unverified() => {}
                    Err(error) => return Err(error),
                }
            }
            self.inner
                .press_key_for_window(window, combo, policy, lease)
        })();
        if result.is_ok() {
            crate::input::record::record_agent::note_window_key(window, combo);
        }
        result
    }

    /// Launch: a folder opens in File Explorer; a window of the launched app
    /// that takes the person's foreground gives it back.
    pub(crate) fn ours_launch_app(
        &self,
        id: &str,
        options: &core::system::launch_options::LaunchOptions,
        lease: &InteractionLease,
    ) -> Result<core::system::launch_result::LaunchResult, AdapterError> {
        if !background::enabled() || options.activate {
            return self.inner.launch_app(id, options, lease);
        }
        // A folder opens in File Explorer, behind the person's windows.
        let folder = std::path::Path::new(id.trim_matches('"'));
        if folder.is_absolute() && folder.is_dir() {
            let person = crate::actions::focus_guard::person_front();
            let result =
                crate::system::separate_launch::launch_folder(folder, options, &self.inner)?;
            if let Some(window) = &result.window {
                crate::actions::focus_guard::give_back_from(person, &window.id);
            }
            return Ok(result);
        }
        // On the work desktop, an instance running on the person's desktop is
        // not one to attach to: a new one starts here.
        let mut options = options.clone();
        if crate::system::work_desktop::active() {
            options.attach_if_running = false;
        }
        let options = &options;
        // A program with a profile of its own (or any program, on the work
        // desktop) starts as a separate instance; upstream would attach or
        // refuse. Only for a full path: a bare name is upstream's to resolve.
        let exe = std::path::Path::new(id.trim_matches('"'));
        let separate = crate::system::separate_launch::has_own_profile(&options.args)
            || crate::system::work_desktop::active();
        if separate && exe.is_absolute() && exe.is_file() {
            let person = crate::actions::focus_guard::person_front();
            let result = crate::system::separate_launch::launch(id, options, &self.inner)?;
            crate::actions::focus_guard::after_launch(person, u32::from(result.pid), exe);
            return Ok(result);
        }
        // Every Chromium browser started here keeps its pages working while
        // other windows cover it: it is driven behind the person's windows.
        let mut options = options.clone();
        if crate::system::separate_launch::is_chromium_browser(id) {
            crate::system::separate_launch::keep_working_while_covered(&mut options.args);
        }
        let options = &options;
        self.launch_behind(id, options, lease)
    }

    fn launch_behind(
        &self,
        id: &str,
        options: &core::system::launch_options::LaunchOptions,
        lease: &InteractionLease,
    ) -> Result<core::system::launch_result::LaunchResult, AdapterError> {
        let person = crate::actions::focus_guard::person_front();
        let result = self.inner.launch_app(id, options, lease)?;
        crate::actions::focus_guard::after_launch(
            person,
            u32::from(result.pid),
            std::path::Path::new(id),
        );
        Ok(result)
    }

    /// Minimize and restore never activate a window in background mode.
    pub(crate) fn ours_window_op(
        &self,
        win: &WindowInfo,
        op: WindowOp,
        lease: &InteractionLease,
    ) -> Result<OperationReport, AdapterError> {
        if background::enabled()
            && let Some(report) = crate::actions::behind::window_op_without_activation(win, &op)
        {
            return report;
        }
        self.inner.window_op(win, op, lease)
    }

    /// A minimized window's elements are offscreen and no action reaches
    /// them: it is shown behind the person's windows (never activated) first.
    pub(crate) fn ours_observe_tree(
        &self,
        root: ObservationRoot<'_>,
        request: &ObservationRequest,
    ) -> Result<ObservedTree, AdapterError> {
        if background::enabled()
            && let ObservationRoot::Window(window) = &root
            && let Ok(hwnd) = crate::system::window::window_ops::parse_window_id(&window.id)
        {
            crate::actions::chromium_input::show_behind_if_minimized(hwnd as _);
        }
        self.inner.observe_tree(root, request)
    }

    /// A live read an app left incomplete (busy right after it opened or a
    /// dialog appeared) is read again within the deadline. Background mode
    /// then adds what the action gates should know (see `background_evidence`).
    pub(crate) fn ours_get_live_element(
        &self,
        handle: &NativeHandle,
        deadline: Deadline,
    ) -> Result<LiveElement, AdapterError> {
        // An action delivered moments ago whose time ran out before upstream
        // could read it back (a file dialog's name field: the path was written
        // and the dialog then took long to answer) is read with a short time of
        // its own, instead of turning a delivered write into a TIMEOUT.
        let deadline = match LAST_ACTION.lock().ok().and_then(|last| *last) {
            Some(at)
                if background::enabled()
                    && deadline.remaining() < std::time::Duration::from_millis(50)
                    && at.elapsed() < std::time::Duration::from_secs(10) =>
            {
                Deadline::detached_after(READ_AFTER_DEADLINE_MS).unwrap_or(deadline)
            }
            _ => deadline,
        };
        let mut tries = 0;
        let mut live = loop {
            match self.inner.get_live_element(handle, deadline) {
                Err(err)
                    if err.code == ErrorCode::AppUnresponsive
                        && background::enabled()
                        && tries < LIVE_RETRIES
                        && deadline.remaining() > std::time::Duration::from_millis(600) =>
                {
                    tries += 1;
                    std::thread::sleep(LIVE_RETRY_PAUSE);
                }
                other => break other?,
            }
        };
        if background::enabled() {
            background_evidence(handle, &mut live);
        }
        Ok(live)
    }

    /// A work desktop with no window yet is an empty list (Windows reports
    /// enumerating an empty desktop as a failure).
    pub(crate) fn ours_list_windows(
        &self,
        filter: &WindowFilter,
        deadline: Deadline,
    ) -> Result<Vec<WindowInfo>, AdapterError> {
        match self.inner.list_windows(filter, deadline) {
            Err(_) if background::enabled() && crate::system::work_desktop::active() => {
                Ok(Vec::new())
            }
            other => other,
        }
    }

    pub(crate) fn ours_get_live_actions(
        &self,
        handle: &NativeHandle,
        deadline: Deadline,
    ) -> Result<Option<Vec<String>>, AdapterError> {
        self.inner.get_live_actions(handle, deadline)
    }
}

/// What background mode can do that upstream's action gates do not know
/// about, given to them as evidence (the old fork changed the gates instead):
///
/// - an element with real bounds and no press action (a grid cell, a canvas)
///   is clicked as window messages at its centre: it offers `Click`;
/// - a list, tree or grid chooses one of its items by name, and an editable
///   field takes a value no option has: they offer `Select`;
/// - a tab, radio button or checkbox reported offscreen while it has real
///   bounds (SWT tabs, PrimeNG's clipped inputs) is acted on through
///   accessibility, which needs no screen position: it is not offscreen;
/// - a radio button's checked state is its `SelectionItem.IsSelected`
///   (Chromium and WinForms radios have no toggle state; upstream PR #679).
fn background_evidence(handle: &NativeHandle, live: &mut LiveElement) {
    let has_bounds = live.bounds.is_some_and(|b| b.width > 0.0 && b.height > 0.0);
    let add = |actions: &mut Vec<String>, action: &str| {
        if !actions.iter().any(|a| a == action) {
            actions.push(action.to_string());
        }
    };
    if has_bounds {
        add(&mut live.available_actions, capability::CLICK);
    }
    let role = live.state.role.clone();
    if matches!(
        role.as_str(),
        "list" | "listbox" | "tree" | "grid" | "table" | "datagrid"
    ) || live
        .available_actions
        .iter()
        .any(|a| a == capability::SET_VALUE)
    {
        add(&mut live.available_actions, capability::SELECT);
    }
    if has_bounds
        && matches!(role.as_str(), "tab" | "radiobutton" | "checkbox")
        && live.state.offscreen == Some(true)
    {
        live.state.offscreen = Some(false);
        live.state
            .states
            .retain(|state| state != tree::state::OFFSCREEN);
    }
    // A radio button's state is its SelectionItem.IsSelected: with it read,
    // its states are complete (upstream wants a toggle state radios lack).
    if role == "radiobutton"
        && let Ok(element) = crate::tree::uia_element(handle)
        && let Some(selected) = crate::tree::properties::read_one(
            &element,
            crate::tree::properties::property_ids::TreeProperty::SelectionItemIsSelected,
        )
        .flag()
    {
        live.states_complete = true;
        if live.state.hidden.is_none() {
            live.state.hidden = Some(false);
        }
        if live.state.offscreen.is_none() {
            live.state.offscreen = Some(false);
        }
        let checked = live.state.states.iter().any(|s| s == tree::state::CHECKED);
        if selected && !checked {
            live.state.states.push(tree::state::CHECKED.to_string());
        }
    }
}
