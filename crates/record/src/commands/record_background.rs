// Part of Agent Record Adapter (see NOTICE).
//! `record start` and `record stop`: a recording that runs in the
//! background between two calls, for an agent or a tool that cannot hold a
//! process open.
//!
//! `start` runs `agent-desktop record` as a detached process with a stop
//! file, waits until it reports `recording_ready`, and returns its id and
//! paths. `stop` creates the stop file, waits for the `recording_stopped`
//! line (which the session writes after it has finished its video), and
//! returns the summary. Each session is described by
//! `<state root>/recordings/<id>.json`; the events and the video go where the
//! caller asked, by default beside that file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppError;
use crate::adapter::PlatformAdapter;
use crate::commands::record::{RECORDING_READY_KIND, default_video_path};
use crate::commands::record_display;

const READY_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(100);
/// How long `stop` waits by default: a video needs a moment to be finished.
pub const DEFAULT_STOP_TIMEOUT_MS: u64 = 60_000;

/// What `record start` records; the same scope and options as `record`.
pub struct StartArgs {
    pub out: Option<PathBuf>,
    pub app: Option<String>,
    pub window_id: Option<String>,
    pub display: Option<String>,
    pub max_duration_ms: Option<u64>,
    pub include_injected: bool,
    pub video: bool,
    pub video_out: Option<PathBuf>,
    /// Show the on-screen recording indicator, with its Stop button.
    pub indicator: bool,
}

/// A recording running in the background.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub pid: u32,
    pub out: PathBuf,
    pub video: Option<PathBuf>,
    pub stop_file: PathBuf,
    /// When it started, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub started_ms: u64,
    /// What it records: `window <id>`, `app <name>`, `display <id>` or `everything`.
    #[serde(default)]
    pub scope: String,
}

pub fn start(mut args: StartArgs, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let display = match &args.display {
        Some(selector) => {
            let displays = adapter.list_displays(crate::Deadline::standard()?)?;
            let chosen = record_display::resolve(selector, &displays)?;
            args.display = Some(chosen.id.clone());
            Some(record_display::describe(&displays, chosen))
        }
        None => None,
    };
    let dir = sessions_dir()?;
    let id = new_id(&dir);
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| dir.join(format!("{id}.jsonl")));
    let video = match (&args.video_out, args.video) {
        (Some(path), _) => Some(path.clone()),
        (None, true) => Some(default_video_path(&out)),
        (None, false) => None,
    };
    let session = Session {
        id: id.clone(),
        pid: 0,
        stop_file: dir.join(format!("{id}.stop")),
        out,
        video,
        started_ms: now_ms(),
        scope: scope_of(&args),
    };
    let log = dir.join(format!("{id}.log"));
    let mut child = match spawn(&args, &session, &log, true) {
        Ok(child) => child,
        Err(_) => spawn(&args, &session, &log, false)?,
    };
    let session = Session {
        pid: child.id(),
        ..session
    };
    write_state(&dir, &session)?;
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if has_line(&session.out, RECORDING_READY_KIND) {
            break;
        }
        if let Ok(Some(status)) = child.try_wait() {
            let reason = tail(&log);
            let _ = std::fs::remove_file(state_path(&dir, &session.id));
            let _ = std::fs::remove_file(&log);
            return Err(AppError::invalid_input(format!(
                "The recording ended before it was ready ({status}): {reason}"
            )));
        }
        if Instant::now() >= deadline {
            let _ = std::fs::write(&session.stop_file, b"");
            return Err(AppError::Internal(
                "The recording did not get ready within 20 s".into(),
            ));
        }
        std::thread::sleep(POLL);
    }
    let mut result = json!({
        "id": session.id,
        "state": "recording",
        "pid": session.pid,
        "scope": session.scope,
        "out": session.out.to_string_lossy(),
        "video": session.video.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "indicator": args.indicator,
    });
    if let Some(display) = display {
        result["display"] = display;
    }
    Ok(result)
}

/// The recordings running in the background and the displays there are to
/// record, for a person or an agent deciding what to do.
pub fn status(adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let dir = sessions_dir()?;
    let now = now_ms();
    let recordings: Vec<Value> = sessions(&dir)
        .into_iter()
        .map(|session| {
            let running = alive(session.pid);
            // A recording that ended keeps the length it had when it stopped.
            let elapsed = if running {
                now.saturating_sub(session.started_ms)
            } else {
                ended_length(&session).unwrap_or_else(|| now.saturating_sub(session.started_ms))
            };
            json!({
                "id": session.id,
                "state": if running { "recording" } else { "ended" },
                "scope": session.scope,
                "elapsed_ms": elapsed,
                "out": session.out.to_string_lossy(),
                "video": session.video.as_ref().map(|p| p.to_string_lossy().into_owned()),
            })
        })
        .collect();
    let displays = adapter
        .list_displays(crate::Deadline::standard()?)
        .map(|displays| record_display::numbered(&displays))
        .unwrap_or(Value::Array(Vec::new()));
    Ok(json!({ "recording": recordings, "displays": displays }))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

pub(crate) fn scope_of(args: &StartArgs) -> String {
    if let Some(id) = &args.window_id {
        format!("window {id}")
    } else if let Some(app) = &args.app {
        format!("app {app}")
    } else if let Some(display) = &args.display {
        format!("display {display}")
    } else {
        "everything".into()
    }
}

pub fn stop(id: Option<&str>, timeout_ms: Option<u64>) -> Result<Value, AppError> {
    let dir = sessions_dir()?;
    let all = sessions(&dir);
    // Without an id, a recording whose process already ended (closed with
    // its app, say) is not one of "the running ones" to choose among; with
    // none alive, the ended ones are reported (and cleaned up) below.
    let live: Vec<Session> = all.iter().filter(|s| alive(s.pid)).cloned().collect();
    if id.is_none() && live.is_empty() && all.len() > 1 {
        let cleared: Vec<String> = all.iter().map(|s| s.id.clone()).collect();
        for session in &all {
            let _ = std::fs::remove_file(&session.stop_file);
            let _ = std::fs::remove_file(state_path(&dir, &session.id));
            let _ = std::fs::remove_file(dir.join(format!("{}.log", session.id)));
            let _ = std::fs::remove_file(agent_inbox(&session.stop_file));
        }
        return Err(AppError::invalid_input_with_suggestion(
            format!(
                "No recording is running; {} had ended without being stopped (their files are kept) and were cleared: {}",
                cleared.len(),
                cleared.join(", ")
            ),
            "Start one with record start",
        ));
    }
    let running = if id.is_none() && !live.is_empty() {
        live
    } else {
        all
    };
    let session = choose(&running, id)?.clone();
    if !session.stop_file.exists() {
        std::fs::write(&session.stop_file, b"").map_err(AppError::Io)?;
    }
    let wait = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_STOP_TIMEOUT_MS));
    let deadline = Instant::now() + wait;
    let trailer = loop {
        if let Some(line) = last_line_of_kind(&session.out, "recording_stopped") {
            break line;
        }
        if !alive(session.pid) {
            let actions = count_actions(&session.out);
            let _ = std::fs::remove_file(&session.stop_file);
            let _ = std::fs::remove_file(state_path(&dir, &session.id));
            let _ = std::fs::remove_file(dir.join(format!("{}.log", session.id)));
            let _ = std::fs::remove_file(agent_inbox(&session.stop_file));
            return Err(AppError::invalid_input_with_suggestion(
                format!(
                    "Recording {} ended before it was stopped (its process was closed); {actions} actions were kept in {}{}",
                    session.id,
                    session.out.display(),
                    session
                        .video
                        .as_ref()
                        .map(|v| format!(", and the video up to that moment in {}", v.display()))
                        .unwrap_or_default()
                ),
                "Start it again with record start",
            ));
        }
        if Instant::now() >= deadline {
            return Err(AppError::Internal(format!(
                "Recording {} did not stop within {} ms; run record stop again to keep waiting",
                session.id,
                wait.as_millis()
            )));
        }
        std::thread::sleep(POLL);
    };
    let _ = std::fs::remove_file(&session.stop_file);
    let _ = std::fs::remove_file(state_path(&dir, &session.id));
    let _ = std::fs::remove_file(dir.join(format!("{}.log", session.id)));
    let _ = std::fs::remove_file(agent_inbox(&session.stop_file));
    let mut result = json!({
        "id": session.id,
        "state": "stopped",
        "out": session.out.to_string_lossy(),
        "events": trailer.get("events"),
        "duration_ms": trailer.get("t_ms"),
        "stop_reason": trailer.get("stop_reason"),
    });
    if let Some(video) = trailer.get("video") {
        result["video"] = video.clone();
    } else if session.video.is_some() {
        result["video_error"] =
            json!("The video was asked for but not written; see the recording's log");
    }
    Ok(result)
}

/// Picks the session to stop: the one named, or the only one running.
pub(crate) fn choose<'a>(
    running: &'a [Session],
    id: Option<&str>,
) -> Result<&'a Session, AppError> {
    match id {
        Some(id) => running.iter().find(|s| s.id == id).ok_or_else(|| {
            AppError::invalid_input(format!("No recording '{id}' is running ({})", ids(running)))
        }),
        None => match running {
            [only] => Ok(only),
            [] => Err(AppError::invalid_input("No recording is running")),
            _ => Err(AppError::invalid_input_with_suggestion(
                format!(
                    "{} recordings are running ({})",
                    running.len(),
                    ids(running)
                ),
                "Pass --id to choose one",
            )),
        },
    }
}

fn ids(running: &[Session]) -> String {
    let names: Vec<&str> = running.iter().map(|s| s.id.as_str()).collect();
    if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    }
}

fn spawn(
    args: &StartArgs,
    session: &Session,
    log: &Path,
    breakaway: bool,
) -> Result<std::process::Child, AppError> {
    let exe = std::env::current_exe().map_err(AppError::Io)?;
    let argv = child_args(args, session);
    let log_file = std::fs::File::create(log).map_err(AppError::Io)?;
    let mut command = std::process::Command::new(exe);
    command
        .args(argv)
        .stdin(std::process::Stdio::null())
        .stdout(log_file.try_clone().map_err(AppError::Io)?)
        .stderr(log_file);
    detach(&mut command, breakaway);
    command.spawn().map_err(AppError::Io)
}

/// The arguments of the foreground `record` the session runs as.
pub(crate) fn child_args(args: &StartArgs, session: &Session) -> Vec<std::ffi::OsString> {
    let mut argv: Vec<std::ffi::OsString> = vec![
        "record".into(),
        "--out".into(),
        session.out.clone().into(),
        "--stop-file".into(),
        session.stop_file.clone().into(),
    ];
    for (flag, value) in [
        ("--app", &args.app),
        ("--window-id", &args.window_id),
        ("--display", &args.display),
    ] {
        if let Some(value) = value {
            argv.push(flag.into());
            argv.push(value.into());
        }
    }
    if let Some(ms) = args.max_duration_ms {
        argv.push("--max-duration-ms".into());
        argv.push(ms.to_string().into());
    }
    if args.include_injected {
        argv.push("--include-injected".into());
    }
    if let Some(video) = &session.video {
        argv.push("--video-out".into());
        argv.push(video.clone().into());
    }
    // A foreground `record` shows the indicator unless told not to: an app that shows its own
    // Stop (Desktop Recorder) asks for none.
    if !args.indicator {
        argv.push("--no-indicator".into());
    }
    argv
}

/// Detaches the session from the caller. Windows lets a child inherit every
/// inheritable handle of its parent, and the caller's pipes (a shell reading
/// this command's output, an MCP client) are inheritable: a child holding
/// them would keep the caller waiting until the recording ends. They are
/// made non-inheritable first; the child gets only its log and no input.
/// A caller that runs its tools in a job closed with it (an agent CLI that
/// ends after each turn) would kill the session too, so the child breaks
/// away from the job; `start` retries without that when the job forbids it.
#[cfg(target_os = "windows")]
fn detach(command: &mut std::process::Command, breakaway: bool) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    const STD_HANDLES: [u32; 3] = [-10i32 as u32, -11i32 as u32, -12i32 as u32];
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
        fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    for which in STD_HANDLES {
        let handle = unsafe { GetStdHandle(which) };
        if !handle.is_null() && handle as isize != -1 {
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let job = if breakaway {
        CREATE_BREAKAWAY_FROM_JOB
    } else {
        0
    };
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | job);
}

#[cfg(not(target_os = "windows"))]
fn detach(_command: &mut std::process::Command, _breakaway: bool) {}

/// Whether the session's process still runs.
#[cfg(target_os = "windows")]
pub(crate) fn alive(pid: u32) -> bool {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn GetExitCodeProcess(process: *mut std::ffi::c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let mut code = 0;
    let read = unsafe { GetExitCodeProcess(process, &mut code) } != 0;
    unsafe { CloseHandle(process) };
    read && code == STILL_ACTIVE
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn sessions_dir() -> Result<PathBuf, AppError> {
    let dir = crate::storage::state_root::resolve_configured_state_root()?.join("recordings");
    std::fs::create_dir_all(&dir).map_err(AppError::Io)?;
    Ok(dir)
}

fn state_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

fn write_state(dir: &Path, session: &Session) -> Result<(), AppError> {
    let raw = serde_json::to_vec_pretty(session).map_err(|e| AppError::Internal(e.to_string()))?;
    std::fs::write(state_path(dir, &session.id), raw).map_err(AppError::Io)
}

/// The file where agent-desktop's own actions (an agent's `click`,
/// `set-value`...) are written for the recording whose stop file is
/// `stop_file`. The keyboard and mouse hooks that record a person never see
/// those actions, which go through accessibility; the recording reads this
/// file and records them as the person's would be (`by: "agent"`).
pub fn agent_inbox(stop_file: &Path) -> PathBuf {
    stop_file.with_extension("agent.jsonl")
}

/// The agent inboxes of the recordings running in the background now.
pub fn running_agent_inboxes() -> Vec<PathBuf> {
    let Ok(dir) = crate::storage::state_root::resolve_configured_state_root()
        .map(|root| root.join("recordings"))
    else {
        return Vec::new();
    };
    sessions(&dir)
        .into_iter()
        .filter(|session| session.pid != 0 && alive(session.pid) && !session.stop_file.exists())
        .map(|session| agent_inbox(&session.stop_file))
        .collect()
}

/// The name of the secret `secret fill` is writing right now, if it is: a
/// recording then notes a secure field filled with that secret, never the
/// value.
static SECRET_WRITE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Marks, until it is dropped, that the value being written is the secret
/// `name` (see `secret_being_written`).
pub struct SecretWrite(());

impl Drop for SecretWrite {
    fn drop(&mut self) {
        if let Ok(mut current) = SECRET_WRITE.lock() {
            *current = None;
        }
    }
}

pub fn secret_write(name: &str) -> SecretWrite {
    if let Ok(mut current) = SECRET_WRITE.lock() {
        *current = Some(name.to_owned());
    }
    SecretWrite(())
}

/// The secret being written now, if any.
pub fn secret_being_written() -> Option<String> {
    SECRET_WRITE.lock().ok().and_then(|current| current.clone())
}

/// The recordings started in the background that have not been stopped.
pub fn sessions(dir: &Path) -> Vec<Session> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Session> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| serde_json::from_slice(&std::fs::read(path).ok()?).ok())
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

pub(crate) fn new_id(dir: &Path) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let mut id = format!("rec-{stamp}");
    let mut n = 1;
    while state_path(dir, &id).exists() {
        n += 1;
        id = format!("rec-{stamp}-{n}");
    }
    id
}

fn count_actions(path: &Path) -> usize {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| {
            !matches!(
                line.get("kind").and_then(Value::as_str),
                Some("recording_started" | "recording_ready" | "recording_stopped") | None
            )
        })
        .count()
}

fn has_line(path: &Path, kind: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|text| text.contains(&format!("\"kind\":\"{kind}\"")))
        .unwrap_or(false)
}

/// How long an ended recording ran: its `recording_stopped` line's time, or,
/// when its process ended before writing it, the last write to its file.
fn ended_length(session: &Session) -> Option<u64> {
    if let Some(t_ms) = last_line_of_kind(&session.out, "recording_stopped")
        .and_then(|line| line.get("t_ms").and_then(Value::as_u64))
    {
        return Some(t_ms);
    }
    let written = std::fs::metadata(&session.out).ok()?.modified().ok()?;
    let written_ms = written
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some(written_ms.saturating_sub(session.started_ms))
}

fn last_line_of_kind(path: &Path, kind: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|line| line.get("kind").and_then(Value::as_str) == Some(kind))
}

fn tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let trimmed = text.trim();
    let start = trimmed.len().saturating_sub(600);
    trimmed.get(start..).unwrap_or(trimmed).to_string()
}

#[cfg(test)]
#[path = "record_background_tests.rs"]
mod tests;
