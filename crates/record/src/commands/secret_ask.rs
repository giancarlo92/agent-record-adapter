// Part of Agent Record Adapter (see NOTICE).
//! Asking a person for a secret the first time it is needed: the platform's
//! own window where the adapter has one (Windows), otherwise the system's
//! dialog tools (`osascript` on macOS, `zenity` or `kdialog` on Linux).

use crate::adapter::PlatformAdapter;
use crate::{AppError, ErrorCode};

pub const TITLE: &str = "Agent Record Adapter: password needed";

/// The secret the person typed, or `None` when they cancelled.
pub fn ask(adapter: &dyn PlatformAdapter, message: &str) -> Result<Option<String>, AppError> {
    match adapter.ask_secret(TITLE, message) {
        Err(error) if error.code == ErrorCode::PlatformNotSupported => fallback(message),
        other => Ok(other?),
    }
}

#[cfg(target_os = "macos")]
fn fallback(message: &str) -> Result<Option<String>, AppError> {
    let quoted = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "text returned of (display dialog \"{}\" default answer \"\" with hidden answer \
         with title \"{}\" buttons {{\"Cancel\", \"Save and continue\"}} default button 2)",
        quoted(message),
        quoted(TITLE)
    );
    run(std::process::Command::new("osascript").args(["-e", &script]))
}

#[cfg(target_os = "linux")]
fn fallback(message: &str) -> Result<Option<String>, AppError> {
    let zenity = std::process::Command::new("zenity")
        .arg("--version")
        .output();
    if zenity.is_ok_and(|out| out.status.success()) {
        return run(std::process::Command::new("zenity").args(["--password", "--title", TITLE]));
    }
    run(std::process::Command::new("kdialog").args(["--title", TITLE, "--password", message]))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn fallback(_message: &str) -> Result<Option<String>, AppError> {
    Err(AppError::invalid_input(
        "This platform cannot ask for a secret; save it first with: agent-desktop secret set <name>",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run(command: &mut std::process::Command) -> Result<Option<String>, AppError> {
    let output = command.output().map_err(|error| {
        AppError::invalid_input(format!(
            "No dialog tool to ask for the secret ({error}); save it first with: agent-desktop secret set <name>"
        ))
    })?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value = text.trim_end_matches(['\r', '\n']).to_string();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(test)]
#[path = "secret_ask_tests.rs"]
mod tests;
