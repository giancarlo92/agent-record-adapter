// Part of Agent Record Adapter (see NOTICE).
//! Our operations (`RecordOps`) on the Windows adapter: menus, a click at a
//! point, navigation, typing into a window, strokes, recording, secrets.
use agent_desktop_core::{
    AdapterError, InteractionLease, MenuSearch, OperationReport, PointClick, ProcessId, RecordOps,
    RecordRequest, RecordSink, RecordSummary, StrokeParams, WindowInfo,
};

use crate::WindowsAdapter;

impl RecordOps for WindowsAdapter {
    fn invoke_menu(
        &self,
        window: &WindowInfo,
        path: &[String],
        lease: &InteractionLease,
    ) -> Result<Vec<String>, AdapterError> {
        crate::actions::menu_command::invoke(window, path, lease.deadline())
    }

    fn find_menu_item(
        &self,
        window: &WindowInfo,
        search: &MenuSearch,
        lease: &InteractionLease,
    ) -> Result<Vec<String>, AdapterError> {
        crate::actions::menu_command::find(window, search, lease.deadline())
    }

    /// Without DevTools: the browser's address bar, as window messages.
    /// A page with DevTools is driven by agent-browser (`agent-record browser`).
    fn navigate_window(
        &self,
        window: &WindowInfo,
        url: &str,
        _in_app: bool,
        lease: &InteractionLease,
    ) -> Result<serde_json::Value, AdapterError> {
        crate::actions::chromium_input::navigate(window, url, lease.deadline())
    }

    fn click_at(
        &self,
        window: &WindowInfo,
        point: &PointClick,
        _lease: &InteractionLease,
    ) -> Result<(), AdapterError> {
        // A browser clicked as window messages may take the foreground: a guard
        // gives the person their window back (see `focus_guard::during_input`).
        let input = crate::actions::focus_guard::during_input(
            Some(window.pid.get()),
            crate::actions::focus_guard::PersonFront::now(),
            800,
        );
        let result = crate::actions::point_click::click_at(window, point);
        if let Some(guard) = input.input_sent() {
            guard.finish();
        }
        result
    }

    fn type_in_window(
        &self,
        window: &WindowInfo,
        text: &str,
        _lease: &InteractionLease,
    ) -> Result<(), AdapterError> {
        crate::actions::posted_keys::type_in_window(window, text)
    }

    fn stroke(
        &self,
        params: StrokeParams,
        lease: &InteractionLease,
    ) -> Result<OperationReport, AdapterError> {
        let report = stroke_report(&params);
        crate::input::stroke::synthesize_stroke(params, lease.deadline())?;
        Ok(report)
    }

    fn record_input(
        &self,
        request: &RecordRequest,
        sink: &mut RecordSink<'_>,
    ) -> Result<RecordSummary, AdapterError> {
        crate::input::record::record_input(request, sink)
    }

    fn ask_secret(&self, title: &str, message: &str) -> Result<Option<String>, AdapterError> {
        crate::input::secret_prompt::ask(title, message)
    }

    fn act_by_place(
        &self,
        entry: &agent_desktop_core::RefEntry,
        request: agent_desktop_core::actions::action_request::ActionRequest,
    ) -> Result<agent_desktop_core::ActionResult, AdapterError> {
        crate::twins::act_by_place(self, entry, request)
    }

    fn while_new_windows_stay_behind(&self, pid: ProcessId, work: &mut dyn FnMut()) {
        let guard = crate::actions::focus_guard::Guard::for_new_window(u32::from(pid));
        work();
        if let Some(guard) = guard {
            guard.finish();
        }
    }
}

/// What a stroke reports: a physical drag from its first point to its last.
fn stroke_report(params: &StrokeParams) -> OperationReport {
    let origin = agent_desktop_core::Point { x: 0.0, y: 0.0 };
    let first = params
        .points
        .first()
        .map_or_else(|| origin.clone(), |p| p.point.clone());
    let last = params.points.last().map_or(origin, |p| p.point.clone());
    OperationReport::new(
        agent_desktop_core::ActionResult::delivered_unverified("stroke").with_steps(vec![
            agent_desktop_core::ActionStep::succeeded("pointer_stroke")
                .with_mechanism(agent_desktop_core::StepMechanism::PhysicalSynthetic),
        ]),
        agent_desktop_core::OperationTarget::Drag {
            from: first,
            to: last,
        },
    )
}
