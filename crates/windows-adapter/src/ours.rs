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

    /// With a DevTools endpoint: through it (`--in-app` as the app's own
    /// links, and a full load the app sends home followed by one in-app try).
    /// Without one (the person's own browser): the address bar, as window
    /// messages.
    fn navigate_window(
        &self,
        window: &WindowInfo,
        url: &str,
        in_app: bool,
        lease: &InteractionLease,
    ) -> Result<serde_json::Value, AdapterError> {
        if let Some(result) =
            crate::actions::devtools_navigate::navigate(window, url, in_app, lease.deadline())
        {
            return result;
        }
        // Without DevTools a path of the site (/users) is resolved against the
        // address the tab shows and loaded through the address bar: a whole
        // load, not the app's own route change (that needs DevTools).
        if url.starts_with('/') {
            let Some(absolute) = resolve_on_tab(window, url) else {
                return Err(AdapterError::new(
                    agent_desktop_core::ErrorCode::ActionNotSupported,
                    "The tab's address could not be read to resolve a path of its site",
                )
                .with_suggestion("Give the whole address (http://host:port/path)")
                .with_disposition(agent_desktop_core::DeliverySemantics::not_delivered()));
            };
            let mut answer =
                crate::actions::chromium_input::navigate(window, &absolute, lease.deadline())?;
            answer["asked"] = serde_json::json!(url);
            answer["loaded_whole_page"] = serde_json::json!(in_app);
            return Ok(answer);
        }
        if in_app {
            return Err(AdapterError::new(
                agent_desktop_core::ErrorCode::ActionNotSupported,
                "--in-app needs a browser with a DevTools endpoint (launch --cdp)",
            )
            .with_suggestion(
                "Give the whole address (it opens in the same tab, keeping the session), or click the app's own link",
            )
            .with_disposition(agent_desktop_core::DeliverySemantics::not_delivered()));
        }
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

    fn close_window(
        &self,
        window: &WindowInfo,
        _lease: &InteractionLease,
    ) -> Result<bool, AdapterError> {
        // An app may answer with a "save?" dialog that comes forward: the
        // guard gives the person their window back.
        let input = crate::actions::focus_guard::during_input(
            Some(window.pid.get()),
            crate::actions::focus_guard::PersonFront::now(),
            800,
        );
        let result = crate::actions::close_window::close(window);
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

    fn browser_address(&self, window_id: &str) -> Option<String> {
        crate::input::record::browser_address(window_id)
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

/// A path of a site (`/users`) as a whole address on the site the window's
/// tab shows (`http://127.0.0.1:4200/users`); `None` when the tab's address
/// cannot be read or is not a web address.
fn resolve_on_tab(window: &WindowInfo, path: &str) -> Option<String> {
    let top = crate::actions::chromium_input::is_chromium_top(window)?;
    let client = uiautomation::UIAutomation::new().ok()?;
    // A browser just started behind other windows takes a few seconds to
    // describe its page.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let page = loop {
        if let Some(page) = crate::actions::chromium_input::page_address(top, &client) {
            break page;
        }
        if std::time::Instant::now() >= end {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    };
    resolve_path(&page, path)
}

/// `path` on the origin of `page` (scheme, host and port; a page shown
/// without its scheme is taken as http, as the address bar does).
fn resolve_path(page: &str, path: &str) -> Option<String> {
    let page = page.trim();
    let (scheme, rest) = match page.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("http".to_string(), page),
    };
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|host| !host.is_empty())?;
    Some(format!("{scheme}://{host}{path}"))
}

#[cfg(test)]
mod tests {
    use super::resolve_path;

    #[test]
    fn a_path_is_resolved_on_the_tab_origin() {
        assert_eq!(
            resolve_path("http://127.0.0.1:4200/dashboard?x=1", "/dashboard/admision").as_deref(),
            Some("http://127.0.0.1:4200/dashboard/admision")
        );
        assert_eq!(
            resolve_path("127.0.0.1:4200/dashboard", "/a").as_deref(),
            Some("http://127.0.0.1:4200/a")
        );
        assert_eq!(resolve_path("file:///C:/x.html", "/a"), None);
    }
}
