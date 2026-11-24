// Part of Agent Record Adapter (see NOTICE).
//! `secret`: passwords a person stores once and scripts and agents use by
//! name, without ever reading them.
//!
//! `set` saves a value the person typed (the CLI asks for it without showing
//! it); `list` shows names and bindings; `delete` removes one; `fill` writes a
//! secret into a field after checking it may go there (see
//! `secret_policy`), and removes the value from anything it returns. No
//! command returns a value. Through MCP only `list` and `fill` are tools.

use serde_json::{Value, json};

use crate::AppError;
use crate::adapter::PlatformAdapter;
use crate::commands::helpers::load_ref_entry;
use crate::commands::secret_policy::{FieldFacts, StoredSecret, redact, refusal};
use crate::commands::secret_store::{self, SecretInfo};
use crate::context::CommandContext;

const MAX_NAME: usize = 64;
const MAX_SECRET: usize = 1_024;
const ADDRESS_BAR: &str = "Address and search bar";

pub struct SetArgs {
    pub name: String,
    pub value: String,
    pub app: Option<String>,
    pub site: Option<String>,
    pub any_field: bool,
}

pub struct FillArgs {
    pub ref_id: String,
    pub name: String,
    pub timeout_ms: Option<u64>,
}

pub fn valid_name(name: &str) -> Result<(), AppError> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(AppError::invalid_input(format!(
            "A secret name has 1 to {MAX_NAME} letters, digits, '-', '_' or '.'"
        )))
    }
}

pub fn set(args: SetArgs) -> Result<Value, AppError> {
    valid_name(&args.name)?;
    if args.value.is_empty() || args.value.len() > MAX_SECRET {
        return Err(AppError::invalid_input(format!(
            "A secret has 1 to {MAX_SECRET} bytes"
        )));
    }
    let secret = StoredSecret {
        secret: args.value,
        app: args.app.clone(),
        site: args.site.clone(),
        any_field: args.any_field,
    };
    let info = remember(&args.name, &secret)?;
    Ok(json!({ "saved": info }))
}

pub fn list() -> Result<Value, AppError> {
    let index = secret_store::default_index()?;
    Ok(json!({ "secrets": secret_store::read_index(&index) }))
}

pub fn delete(name: &str) -> Result<Value, AppError> {
    valid_name(name)?;
    let removed = secret_store::remove(name)?;
    let index = secret_store::default_index()?;
    let entries: Vec<SecretInfo> = secret_store::read_index(&index)
        .into_iter()
        .filter(|entry| entry.name != name)
        .collect();
    secret_store::write_index(&index, &entries)?;
    Ok(json!({ "name": name, "removed": removed }))
}

pub fn fill(
    args: FillArgs,
    adapter: &dyn PlatformAdapter,
    context: &CommandContext,
) -> Result<Value, AppError> {
    valid_name(&args.name)?;
    let entry = load_ref_entry(&args.ref_id, context)?;
    let stored = secret_store::load(&args.name)?;
    let needs_address = stored.as_ref().is_none_or(|secret| secret.site.is_some());
    let address = if needs_address {
        browser_address(&entry, adapter, context)
    } else {
        None
    };
    let (secret, asked) = match stored {
        Some(secret) => (secret, false),
        None => (
            ask_first_use(&args.name, &entry, address.as_deref(), adapter)?,
            true,
        ),
    };
    let facts = FieldFacts {
        role: &entry.identity.role,
        states: &entry.capabilities.states,
        app: entry.source.source_app.as_deref(),
        address: address.as_deref(),
    };
    if let Some(reason) = refusal(&secret, &facts) {
        return Err(AppError::invalid_input(format!(
            "The secret '{}' was not written: {reason}",
            args.name
        )));
    }
    // The write goes through set-value itself, so it keeps its input policy,
    // verification and errors. On Windows it never brings the window to the
    // front: a web field gets the text as window messages (which also fire
    // the events a React or Vue form listens for), a classic text box as
    // WM_SETTEXT. A recording running meanwhile notes the secret's name only.
    let _recording = crate::commands::record_background::secret_write(&args.name);
    let outcome = agent_desktop_core::commands::set_value::execute(
        agent_desktop_core::commands::set_value::SetValueArgs {
            ref_id: args.ref_id,
            value: secret.secret.clone(),
            timeout_ms: args.timeout_ms,
        },
        adapter,
        context,
    );
    // A password field with no name (web login forms often leave it unnamed)
    // is refused by upstream as not unique: it is found again by its place.
    let outcome = match outcome {
        Err(AppError::Adapter(error))
            if error.code == crate::ErrorCode::AmbiguousTarget
                && error
                    .details
                    .as_ref()
                    .is_some_and(|details| details["kind"] == "identity_not_unique") =>
        {
            let refused = error.message.clone();
            let request = agent_desktop_core::actions::action_request::ActionRequest::headless(
                crate::Action::SetValue(secret.secret.clone()),
            );
            adapter
                .act_by_place(&entry, request)
                .map_err(AppError::from)
                .and_then(|result| {
                    let mut value = serde_json::to_value(&result)
                        .map_err(|error| AppError::Internal(error.to_string()))?;
                    value["resolved_by"] = json!("place");
                    value["refusal_replaced"] = json!(refused);
                    Ok(value)
                })
        }
        other => other,
    };
    match outcome {
        Ok(mut value) => {
            redact(&mut value, &secret.secret);
            if let Some(fields) = value.as_object_mut() {
                for revealing in ["keystrokes", "text", "typed", "value"] {
                    fields.remove(revealing);
                }
            }
            value["secret"] = json!(args.name);
            if asked {
                value["saved_now"] = json!(true);
            }
            Ok(value)
        }
        Err(error) => Err(redact_error(error, &secret.secret)),
    }
}

/// The first time a secret is needed and none is saved under its name: the
/// person types it in a window of its own (never through the agent), and it
/// is saved bound to this field's app and site, then used.
fn ask_first_use(
    name: &str,
    entry: &crate::RefEntry,
    address: Option<&str>,
    adapter: &dyn PlatformAdapter,
) -> Result<StoredSecret, AppError> {
    let site = address
        .map(crate::commands::secret_policy::host)
        .filter(|host| !host.is_empty());
    let mut secret = StoredSecret {
        secret: String::new(),
        app: entry.source.source_app.clone(),
        site,
        any_field: false,
    };
    let facts = FieldFacts {
        role: &entry.identity.role,
        states: &entry.capabilities.states,
        app: entry.source.source_app.as_deref(),
        address,
    };
    if let Some(reason) = refusal(&secret, &facts) {
        return Err(AppError::invalid_input(format!(
            "No secret is saved as '{name}', and none is asked for here: {reason}"
        )));
    }
    let place = match (&secret.app, &secret.site) {
        (Some(app), Some(site)) => format!("{app} on {site}"),
        (Some(app), None) => app.clone(),
        (None, Some(site)) => site.clone(),
        (None, None) => "this field".into(),
    };
    let message = format!(
        "A script or an agent needs the password saved as '{name}', for {place}.\n\n\
         Type it once: it is kept encrypted by your system and used only there, \
         and the agent never sees it."
    );
    let Some(value) = crate::commands::secret_ask::ask(adapter, &message)? else {
        return Err(AppError::invalid_input(format!(
            "The secret '{name}' was not given (the window was cancelled or closed); nothing was written"
        )));
    };
    if value.len() > MAX_SECRET {
        return Err(AppError::invalid_input(format!(
            "A secret has 1 to {MAX_SECRET} bytes"
        )));
    }
    secret.secret = value;
    remember(name, &secret)?;
    Ok(secret)
}

/// Saves a secret and its line in the index.
fn remember(name: &str, secret: &StoredSecret) -> Result<SecretInfo, AppError> {
    secret_store::save(name, secret)?;
    let info = SecretInfo {
        name: name.to_string(),
        app: secret.app.clone(),
        site: secret.site.clone(),
        any_field: secret.any_field,
        saved_ms: now_ms(),
    };
    let index = secret_store::default_index()?;
    let entries = secret_store::with_entry(secret_store::read_index(&index), info.clone());
    secret_store::write_index(&index, &entries)?;
    Ok(info)
}

/// The address bar's text in the field's browser window, if it has one.
fn browser_address(
    entry: &crate::RefEntry,
    adapter: &dyn PlatformAdapter,
    context: &CommandContext,
) -> Option<String> {
    use agent_desktop_core::commands::find::{self, FindArgs, FindFilterArgs, FindSelectionArgs};
    let found = find::execute(
        FindArgs {
            app: entry.source.source_app.clone(),
            window_id: entry.source.source_window_id.clone(),
            root: None,
            surface: crate::SnapshotSurface::Window,
            filter: FindFilterArgs {
                role: Some("textfield".into()),
                name: Some(ADDRESS_BAR.into()),
                description: None,
                native_id: None,
                value: None,
                text: None,
                exact: true,
            },
            states: Vec::new(),
            selection: FindSelectionArgs {
                count: false,
                first: true,
                last: false,
                nth: None,
                limit: None,
            },
            timeout_ms: Some(5_000),
        },
        adapter,
        context,
    )
    .ok()?;
    found["match"]["value"]
        .as_str()
        .filter(|text| !text.is_empty())
        .map(String::from)
}

fn redact_error(error: AppError, secret: &str) -> AppError {
    match error {
        AppError::Adapter(mut adapter) => {
            adapter.message = adapter.message.replace(secret, "[secret]");
            if let Some(details) = adapter.details.as_mut() {
                redact(details, secret);
            }
            AppError::Adapter(adapter)
        }
        AppError::Internal(text) => AppError::Internal(text.replace(secret, "[secret]")),
        other => other,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "secret_tests.rs"]
mod tests;
