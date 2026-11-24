// Part of Agent Record Adapter (see NOTICE).
//! Small versions of `agent-desktop-core` command helpers that are private
//! there (`pub(crate)`), for our commands. Adapted from vercel-labs/agent-desktop
//! (Apache-2.0): window_target, helpers::load_ref_entry, operation_response,
//! point_resolve.

pub mod window_target {
    use crate::adapter::PlatformAdapter;
    use crate::{AdapterError, AppError, Deadline, ErrorCode, WindowFilter, WindowInfo};

    /// The window `--window-id` names, else the main window of `--app`, else
    /// the focused window.
    pub fn resolve_window_for_app(
        app: Option<&str>,
        window_id: Option<&str>,
        adapter: &dyn PlatformAdapter,
    ) -> Result<WindowInfo, AppError> {
        let deadline = Deadline::standard()?;
        let filter = WindowFilter {
            focused_only: app.is_none() && window_id.is_none(),
            app: app.map(str::to_string),
        };
        let mut candidates: Vec<WindowInfo> = adapter
            .list_windows(&filter, deadline)?
            .into_iter()
            .filter(|window| window_id.is_none_or(|id| window.id == id))
            .collect();
        if candidates.is_empty() {
            let what = match (window_id, app) {
                (Some(id), _) => format!("Window '{id}' was not found"),
                (None, Some(app)) => format!("No window of '{app}' was found"),
                (None, None) => "No focused window".to_string(),
            };
            return Err(AdapterError::new(ErrorCode::WindowNotFound, what)
                .with_suggestion("Run 'list-windows' to refresh window IDs, then retry.")
                .with_disposition(crate::DeliverySemantics::not_delivered())
                .into());
        }
        if candidates
            .iter()
            .any(|window| window.state.visible == Some(true))
        {
            candidates.retain(|window| window.state.visible == Some(true));
        }
        if let Some(index) = candidates.iter().position(|window| window.state.is_focused) {
            return Ok(candidates.swap_remove(index));
        }
        Ok(candidates.swap_remove(0))
    }
}

pub mod helpers {
    use crate::context::CommandContext;
    use crate::{AppError, RefEntry, RefStore};

    /// The saved entry of a snapshot-qualified ref (`@<snapshot>:eN`).
    pub fn load_ref_entry(ref_id: &str, context: &CommandContext) -> Result<RefEntry, AppError> {
        let invalid = || {
            AppError::invalid_input_with_suggestion(
                format!("Invalid ref_id '{ref_id}': expected @<snapshot_id>:e<N>"),
                "Use the snapshot-qualified ref that snapshot prints, for example @s8f3k2p9:e1.",
            )
        };
        let (snapshot_id, number) = ref_id
            .strip_prefix('@')
            .and_then(|rest| rest.rsplit_once(":e"))
            .ok_or_else(invalid)?;
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            return Err(invalid());
        }
        let store = RefStore::for_session(context.session_id())?;
        let refmap = store.load_snapshot(snapshot_id)?;
        refmap
            .get(&format!("@e{number}"))
            .cloned()
            .ok_or_else(|| AppError::stale_ref(ref_id))
    }
}

pub mod operation_response {
    use crate::{AdapterError, AppError, OperationReport};
    use serde_json::Value;

    /// The report as JSON, with the request's fields added where absent.
    pub fn operation_response(report: &OperationReport, request: Value) -> Result<Value, AppError> {
        let mut response = serde_json::to_value(report).map_err(|error| {
            AppError::Adapter(
                AdapterError::internal(format!("operation report could not be encoded: {error}"))
                    .with_disposition(report.disposition()),
            )
        })?;
        if let (Some(body), Value::Object(fields)) = (response.as_object_mut(), request) {
            for (key, value) in fields {
                body.entry(key).or_insert(value);
            }
        }
        Ok(response)
    }
}

pub mod point_resolve {
    use crate::adapter::PlatformAdapter;
    use crate::context::CommandContext;
    use crate::{AdapterError, AppError};

    /// Commands that move the real cursor need --headed.
    pub fn require_cursor_policy(context: &CommandContext, command: &str) -> Result<(), AppError> {
        let policy = context.physical_input_policy();
        if policy.allow_cursor_move {
            return Ok(());
        }
        Err(AdapterError::policy_denied_for_policy(
            format!("{command} moves the cursor and is disabled in headless mode"),
            policy,
        )
        .into())
    }

    pub fn require_pointer_capability(adapter: &dyn PlatformAdapter) -> Result<(), AppError> {
        match adapter.physical_pointer_refusal() {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }
}
