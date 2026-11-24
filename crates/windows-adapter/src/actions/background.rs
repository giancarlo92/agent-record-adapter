// Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
//! Background mode for the Windows adapter, in one place.
//!
//! The original agent-desktop's code is called unchanged, through the two
//! functions here: with background mode off (`AGENT_DESKTOP_BACKGROUND=0`),
//! or when the caller allows taking the focus, they only call it. On, they
//! first try the fork's ways that need neither the mouse, the keyboard nor
//! the foreground, and fall back to the original code when those do not
//! apply or deliver nothing. Keeping the hooks here leaves the original files
//! with one line each, so upstream changes merge cleanly.

use agent_desktop_core::{
    ActionResult, AdapterError, InteractionLease, NativeHandle,
    actions::action_request::ActionRequest,
};

/// Upstream's own `execute_action`, the fallback of every route here.
pub(crate) type OriginalAction<'a> = &'a dyn Fn(
    &NativeHandle,
    ActionRequest,
    &InteractionLease,
) -> Result<ActionResult, AdapterError>;

/// Every element action. Background mode: a page of a browser with a DevTools
/// endpoint is acted on through it, and anything else runs the original code
/// while the person's window is kept in front (see `focus_guard`).
pub(crate) fn execute_action(
    handle: &NativeHandle,
    request: ActionRequest,
    lease: &InteractionLease,
    original: OriginalAction<'_>,
) -> Result<ActionResult, AdapterError> {
    // A background recording running now records the action too: its hooks
    // only hear the person's own mouse and keyboard. The element is described
    // before the action, which may close its window (a dialog's Open button).
    #[cfg(target_os = "windows")]
    let action = request.action.clone();
    #[cfg(target_os = "windows")]
    let noted = if handle.is_null() {
        None
    } else {
        crate::input::record::record_agent::before(handle)
    };
    let result = if request.policy.allow_focus_steal
        || handle.is_null()
        || !agent_desktop_core::background::enabled()
    {
        original(handle, request, lease)
    } else {
        imp::execute_action(handle, request, lease, original)
    };
    #[cfg(target_os = "windows")]
    if result.is_ok()
        && let Some(noted) = noted
    {
        noted.after(&action);
    }
    result
}

#[cfg(target_os = "windows")]
mod imp {
    use super::OriginalAction;
    use agent_desktop_core::{
        Action, ActionResult, AdapterError, Deadline, DeliverySemantics, ErrorCode,
        InteractionLease, InteractionPolicy, NativeHandle, actions::action_request::ActionRequest,
    };

    use crate::tree::element::{UIAElement, uia_element};
    use crate::tree::live_read::corroborate_verified_process;

    pub(super) fn execute_action(
        handle: &NativeHandle,
        request: ActionRequest,
        lease: &InteractionLease,
        original: OriginalAction<'_>,
    ) -> Result<ActionResult, AdapterError> {
        let written = match &request.action {
            Action::SetValue(value) if !value.is_empty() => Some(value.clone()),
            _ => None,
        };
        let chosen = match &request.action {
            Action::Select(value) => Some(value.clone()),
            _ => None,
        };
        let retry = request.clone();
        // A click may open a dialog (a page's file picker) while it is still
        // finishing; the windows open now let the wait that follows find it,
        // whichever route the click takes (DevTools included).
        if matches!(
            request.action,
            Action::Click | Action::DoubleClick | Action::TripleClick | Action::RightClick
        ) {
            crate::actions::behind::note_open_windows();
        }
        let target_pid = uia_element(handle)
            .ok()
            .and_then(|element| element.0.get_process_id().ok());
        // The whole action is guarded, the DevTools route too, against any
        // process of the target's program: a page's file picker opened by a
        // click runs in another process of the browser and was activated,
        // taking the person's foreground.
        let result = crate::actions::focus_guard::guarded_program(target_pid, || {
            if let Ok(element) = uia_element(handle) {
                corroborate_verified_process(&element)?;
                // A file's name in File Explorer written through accessibility renames
                // it; the cell then holds another name, so it is not read back.
                if let Action::SetValue(value) = &request.action
                    && explorer_name(&element)
                    && let Ok(pattern) = element
                        .0
                        .get_pattern::<uiautomation::patterns::UIValuePattern>()
                    && pattern.set_value(value).is_ok()
                {
                    let step = agent_desktop_core::ActionStep::succeeded("ValuePattern.Rename")
                        .with_mechanism(agent_desktop_core::StepMechanism::SemanticApi)
                        .with_verified(false);
                    return Ok(ActionResult::from_execution(&request.action, vec![step]));
                }
                // An item of a long list chosen by name: found through the list even
                // out of view, made real and selected (see list_item.rs).
                if let Some(value) = &chosen
                    && crate::actions::list_item::select_by_name(&element.0, value).is_some()
                {
                    let step = agent_desktop_core::ActionStep::succeeded("ItemContainer.Select")
                        .with_mechanism(agent_desktop_core::StepMechanism::SemanticApi)
                        .with_verified(false);
                    return Ok(ActionResult::from_execution(&request.action, vec![step]));
                }
                // A page with DevTools is driven by agent-browser
                // (`agent-record browser ...`); a page reached here is acted on
                // as a desktop window (see `click` below).
                // The routes the fork hooked inside upstream's dispatch, tried
                // here before upstream's own action (see docs/architecture.md).
                match &request.action {
                    Action::Click => {
                        let again = request.clone();
                        return click(&element, request.policy, lease.deadline(), &|| {
                            original(handle, again.clone(), lease)
                        });
                    }
                    Action::SetValue(value) => {
                        if let Some(steps) = crate::actions::background_value::background_set_value(
                            &element,
                            value,
                            lease.deadline(),
                        )? {
                            return Ok(ActionResult::from_execution(&request.action, steps));
                        }
                    }
                    Action::Select(value) => {
                        if let Some(steps) =
                            crate::actions::dropdown::select(&element, value, lease.deadline())?
                        {
                            return Ok(ActionResult::from_execution(&request.action, steps));
                        }
                    }
                    Action::Expand | Action::Collapse => {
                        let want_open = matches!(request.action, Action::Expand);
                        return match original(handle, request.clone(), lease) {
                            Err(error) if error.code == ErrorCode::ActionFailed => {
                                match crate::actions::dropdown::disclosure_fallback(
                                    &element,
                                    want_open,
                                    lease.deadline(),
                                )? {
                                    Some(step) => Ok(ActionResult::from_execution(
                                        &request.action,
                                        vec![step],
                                    )),
                                    None => Err(error),
                                }
                            }
                            other => other,
                        };
                    }
                    _ => {}
                }
                // Text typed into a document or a canvas (no value to write) goes as
                // the messages real keys send, to the window that holds it.
                if let Action::TypeText(text) = &request.action
                    && let Some(result) = crate::actions::posted_keys::type_text(&element, text)
                {
                    return Ok(result);
                }
            }
            let outcome = original(handle, request, lease);
            // A combo box or field with no option of that name to choose (an
            // editable combo, a field with no list): the value is written.
            match (outcome, &chosen) {
                (Err(error), Some(value))
                    if matches!(
                        error.code,
                        ErrorCode::ElementNotFound | ErrorCode::ActionNotSupported
                    ) =>
                {
                    match uia_element(handle) {
                        Ok(element) if crate::actions::value_write::value_writable(&element) => {
                            // Upstream's own value write, asked for as set-value.
                            let mut write = retry;
                            write.action = Action::SetValue(value.clone());
                            let written = original(handle, write, lease)?;
                            Ok(ActionResult::from_execution(
                                &Action::Select(value.clone()),
                                written.steps,
                            ))
                        }
                        _ => Err(error),
                    }
                }
                (outcome, _) => outcome,
            }
        });
        // A web field that only takes typed input (PrimeNG's calendar parses
        // its text only after a key went down) gets its last character typed
        // again, in the background; the text stays the same. Through DevTools
        // when the browser has it (nothing is activated); otherwise as window
        // messages, the browser told it is active for them, under a guard that
        // gives the person their window back (see `focus_guard::during_input`).
        if result.is_ok()
            && let Some(value) = &written
            && let Ok(element) = uia_element(handle)
        {
            let person = crate::actions::focus_guard::PersonFront::now();
            let input = crate::actions::focus_guard::during_input(target_pid, person, 1200);
            if crate::actions::chromium_input::retype_last_char(&element, value)
                && let Some(guard) = input.input_sent()
            {
                guard.finish();
            }
        }
        result
    }

    /// Whether an element is a file's name in a File Explorer window (its item
    /// or the item's Name cell).
    fn explorer_name(element: &UIAElement) -> bool {
        let class = element.0.get_classname().unwrap_or_default();
        if class != "UIRenameTextElement"
            && class != "UIItem"
            && !element.0.get_name().is_ok_and(|n| n == "Name")
        {
            return false;
        }
        element
            .0
            .get_process_id()
            .ok()
            .and_then(|pid| {
                crate::system::process_identity::process_image_name(
                    agent_desktop_core::ProcessId::from(pid),
                )
            })
            .is_some_and(|image| image.eq_ignore_ascii_case("explorer.exe"))
    }

    /// Upstream's own click of the element.
    type OriginalClick<'a> = &'a dyn Fn() -> Result<ActionResult, AdapterError>;

    /// A click. Background mode tries, in order: a classic push button pressed
    /// with `BN_CLICKED`; a tab its app draws itself, as window messages; the
    /// trigger of a web dropdown that only its container opens; a pointer click
    /// on a page of the person's own Chromium browser. Then the original click
    /// chain; when that is refused before anything happened, the click goes as
    /// window messages at the element.
    pub(crate) fn click(
        element: &UIAElement,
        policy: InteractionPolicy,
        deadline: Deadline,
        original: OriginalClick<'_>,
    ) -> Result<ActionResult, AdapterError> {
        if policy.allow_focus_steal || !agent_desktop_core::background::enabled() {
            return original();
        }
        let done = |step| Ok(ActionResult::from_execution(&Action::Click, vec![step]));
        if let Some(step) = crate::actions::background_value::background_click(element)? {
            return done(step);
        }
        if crate::actions::posted_click::is_drawn_tab(element)
            && let Some(step) = crate::actions::posted_click::click(element)?
        {
            return done(step);
        }
        if let Some(step) = crate::actions::dropdown::trigger_click(element, deadline)? {
            return done(step);
        }
        if let Some(step) = crate::actions::chromium_input::pointer_click(element, deadline)? {
            return done(step);
        }
        let control = ControlState::of(element);
        let result = match original() {
            Err(error) if error.disposition == DeliverySemantics::not_delivered() => {
                match crate::actions::posted_click::click(element)? {
                    Some(step) => done(step),
                    None => Err(error),
                }
            }
            other => other,
        };
        // A checkbox or radio button of a component library (PrimeNG, Material)
        // whose real input is hidden: its state changes only on the click of
        // the box around it. When the click above left it as it was, the box
        // gets a pointer click, and the new state is checked.
        if let Some(before) = control
            && !before.changed(element, std::time::Duration::from_millis(600))
            && let Some(step) =
                crate::actions::chromium_input::pointer_click_control(element, deadline)?
        {
            let verified = before.changed(element, std::time::Duration::from_millis(1500));
            if verified || result.is_err() {
                return done(step.with_verified(verified));
            }
        }
        result
    }

    /// The state of a checkbox (toggle state) or radio button (selected)
    /// before a click, to tell whether the click changed it.
    #[derive(Clone, Copy)]
    pub(crate) enum ControlState {
        Radio,
        Check(Option<i32>),
    }

    impl ControlState {
        pub(crate) fn of(element: &UIAElement) -> Option<Self> {
            use crate::tree::properties::{property_ids::TreeProperty, read_one};
            use uiautomation::types::ControlType;
            match element.0.get_control_type().ok()? {
                ControlType::RadioButton => {
                    (read_one(element, TreeProperty::SelectionItemIsSelected).flag() != Some(true))
                        .then_some(Self::Radio)
                }
                ControlType::CheckBox => Some(Self::Check(
                    read_one(element, TreeProperty::ToggleState).number(),
                )),
                _ => None,
            }
        }

        /// Whether the control shows the click's effect within `wait`, and
        /// keeps it: a radio button selected, a checkbox in another state. A
        /// component library's hidden input flips for a moment and is put
        /// back from the library's own model (PrimeNG), which is no change.
        pub(crate) fn changed(self, element: &UIAElement, wait: std::time::Duration) -> bool {
            use crate::tree::properties::{property_ids::TreeProperty, read_one};
            let read = || match self {
                Self::Radio => {
                    read_one(element, TreeProperty::SelectionItemIsSelected).flag() == Some(true)
                }
                Self::Check(before) => {
                    let state = read_one(element, TreeProperty::ToggleState).number();
                    state.is_some() && state != before
                }
            };
            let end = std::time::Instant::now() + wait;
            loop {
                if read() {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    if read() {
                        return true;
                    }
                }
                if std::time::Instant::now() >= end {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::OriginalAction;
    use agent_desktop_core::{
        ActionResult, AdapterError, InteractionLease, NativeHandle,
        actions::action_request::ActionRequest,
    };

    pub(super) fn execute_action(
        handle: &NativeHandle,
        request: ActionRequest,
        lease: &InteractionLease,
        original: OriginalAction<'_>,
    ) -> Result<ActionResult, AdapterError> {
        original(handle, request, lease)
    }
}

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
pub(crate) use imp::ControlState;
