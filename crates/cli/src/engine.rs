// Part of Agent Record Adapter (see NOTICE).
//! The engine: upstream agent-desktop's own CLI, built with our adapter
//! (crates/engine). Commands we do not run ourselves go to it unchanged.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const ENGINE_ENV: &str = "AGENT_RECORD_ENGINE";

/// The engine's executable: `AGENT_RECORD_ENGINE` when set (a separate
/// build, for development and tests), else agent-record itself, which
/// carries the engine inside (see `parts.rs`).
pub(crate) fn path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(ENGINE_ENV) {
        return Ok(PathBuf::from(path));
    }
    std::env::current_exe().map_err(|error| format!("Cannot locate agent-record: {error}"))
}

/// A command for the engine with these arguments.
pub(crate) fn command(args: &[String]) -> Result<Command, String> {
    let mut command = match std::env::var_os(ENGINE_ENV) {
        Some(path) => Command::new(path),
        None => crate::parts::command(crate::parts::Part::Engine)?,
    };
    command.args(args);
    Ok(command)
}

/// Runs the engine with the same arguments, standard streams and exit code.
/// A ref action the engine refuses as not unique (twins) is tried again by
/// the element's place (see the Windows crate's `twins.rs`).
pub(crate) fn run(args: &[String]) -> ExitCode {
    if crate::run::ref_action(args).is_some() {
        return run_ref_action(args, crate::run::by_place);
    }
    // A read of a ref with a twin is read by its place too.
    if crate::first_command(args) == Some("get") && args.iter().any(|arg| arg.starts_with('@')) {
        return run_ref_action(args, crate::run::read_by_place);
    }
    if crate::first_command(args) == Some("find") {
        return crate::run::find(args);
    }
    if crate::first_command(args) == Some("wait")
        && args
            .iter()
            .any(|arg| arg == "--text" || arg.starts_with("--text="))
        && args
            .iter()
            .any(|arg| arg == "--window-id" || arg.starts_with("--window-id="))
    {
        return crate::run::wait_text_in_window(args);
    }
    run_plain(args)
}

/// The engine's JSON envelope for these arguments.
pub(crate) fn envelope(args: &[String]) -> Result<serde_json::Value, String> {
    let output = command(args)?
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|error| format!("Cannot run the engine: {error}"))?;
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("The engine gave no JSON envelope: {error}"))
}

fn run_ref_action(args: &[String], by_place: fn(&[String]) -> Option<ExitCode>) -> ExitCode {
    let output = match command(args).and_then(|mut command| {
        command
            .stdin(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|error| format!("Cannot run the engine: {error}"))
    }) {
        Ok(output) => output,
        Err(message) => {
            return crate::run::emit(
                "engine",
                Err(agent_desktop_core::AppError::Internal(message)),
            );
        }
    };
    let envelope: Option<serde_json::Value> = serde_json::from_slice(&output.stdout).ok();
    let twin = envelope.as_ref().is_some_and(|envelope| {
        envelope["error"]["code"] == "AMBIGUOUS_TARGET"
            && envelope["error"]["details"]["kind"] == "identity_not_unique"
    });
    if twin && let Some(code) = by_place(args) {
        return code;
    }
    use std::io::Write;
    let _ = std::io::stdout().write_all(&output.stdout);
    ExitCode::from(
        output
            .status
            .code()
            .map_or(1, |code| u8::try_from(code).unwrap_or(1)),
    )
}

fn run_plain(args: &[String]) -> ExitCode {
    let status = command(args).and_then(|mut command| {
        // A window-opened wait counts the windows a background click opened
        // while it was finishing (see the Windows crate's hooks.rs).
        let opened = args
            .windows(2)
            .any(|pair| pair[0] == "--event" && pair[1] == "window-opened")
            || args.iter().any(|arg| arg == "--event=window-opened");
        if crate::first_command(args) == Some("wait") && opened {
            command.env("AGENT_RECORD_WAIT_OPENED", "1");
        }
        command
            .status()
            .map_err(|error| format!("Cannot run the engine: {error}"))
    });
    match status {
        Ok(status) => ExitCode::from(
            status
                .code()
                .map_or(1, |code| u8::try_from(code).unwrap_or(1)),
        ),
        Err(message) => {
            let error = agent_desktop_core::AppError::Internal(message);
            crate::run::emit("engine", Err(error))
        }
    }
}
