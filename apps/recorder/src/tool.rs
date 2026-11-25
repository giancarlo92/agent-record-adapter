// Part of Agent Record Adapter (see NOTICE).
//! agent-record (Agent Record Adapter), run as a separate program: listing what can be recorded, starting and
//! stopping a recording, and running a script with node. Every call runs without a console
//! window, and with the same private state folder the installed MCP server uses.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use serde_json::Value;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Something that can be recorded: a window, or a whole display.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub label: String,
    pub args: Vec<String>,
}

pub fn home() -> PathBuf {
    PathBuf::from(
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .unwrap_or_default(),
    )
}

/// Where the app keeps its settings and, by default, its recordings: the person's home, or
/// `DESKTOP_RECORDER_HOME` (for tests, which must not touch the person's own settings).
pub fn app_home() -> PathBuf {
    std::env::var_os("DESKTOP_RECORDER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(home)
}

/// The folder Agent Record Adapter installs into and keeps its state in, under a home folder.
pub const PRODUCT_DIR: &str = ".desktop-recorder-mcp";

/// agent-record: next to this app, else where Agent Record Adapter installed it.
pub fn agent_desktop() -> Option<PathBuf> {
    if let Some(bin) = std::env::var_os("AGENT_RECORD_BIN") {
        return Some(PathBuf::from(bin));
    }
    let name = if cfg!(windows) {
        "agent-record.exe"
    } else {
        "agent-record"
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)));
    let installed = home().join(PRODUCT_DIR).join("bin").join(name);
    [beside, Some(installed)]
        .into_iter()
        .flatten()
        .find(|path| path.exists())
}

/// agent-record, checked to be the one this app was packaged with: a release
/// package builds the app with `AGENT_RECORD_SHA256` (the hash of the
/// agent-record.exe it ships), so a changed agent-record.exe is refused
/// instead of run. `AGENT_RECORD_BIN` (development, tests) is not checked.
pub fn checked_agent_record() -> Result<PathBuf, String> {
    let program =
        agent_desktop().ok_or("agent-record was not found: run npx desktop-recorder-mcp")?;
    let Some(expected) = option_env!("AGENT_RECORD_SHA256") else {
        return Ok(program);
    };
    if std::env::var_os("AGENT_RECORD_BIN").is_some() {
        return Ok(program);
    }
    if same_hash(&program, expected) {
        Ok(program)
    } else {
        Err(format!(
            "{} is not the agent-record this Desktop Recorder was installed with, so it was not run: install again with npx desktop-recorder-mcp",
            program.display()
        ))
    }
}

/// Whether the file's SHA-256 is `expected` (hex); remembered while the file
/// keeps its size and time.
fn same_hash(file: &Path, expected: &str) -> bool {
    use sha2::{Digest, Sha256};
    type Checked = Option<(PathBuf, u64, std::time::SystemTime, bool)>;
    static CHECKED: std::sync::Mutex<Checked> = std::sync::Mutex::new(None);
    let Ok(meta) = std::fs::metadata(file) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let mut checked = CHECKED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((path, len, time, ok)) = checked.as_ref()
        && path == file
        && *len == meta.len()
        && *time == modified
    {
        return *ok;
    }
    let ok = std::fs::read(file)
        .map(|bytes| {
            let digest = Sha256::digest(&bytes);
            let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
            hex.eq_ignore_ascii_case(expected.trim())
        })
        .unwrap_or(false);
    *checked = Some((file.to_path_buf(), meta.len(), modified, ok));
    ok
}

fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    let state = home().join(PRODUCT_DIR).join("state");
    if std::env::var_os("AGENT_DESKTOP_HOME").is_none() && state.exists() {
        command.env("AGENT_DESKTOP_HOME", state);
    }
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// One agent-record command: its `data`, or the error's message.
pub fn run(args: &[String]) -> Result<Value, String> {
    let program = checked_agent_record()?;
    let output = command(&program)
        .args(args)
        // A recording started from here leaves the bar's own clicks out.
        .env("AGENT_RECORD_IGNORE_PID", std::process::id().to_string())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let answer: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| String::from_utf8_lossy(&output.stderr).trim().to_owned())?;
    if answer["ok"].as_bool() == Some(true) {
        Ok(answer["data"].clone())
    } else {
        Err(answer["error"]["message"]
            .as_str()
            .unwrap_or("agent-record failed")
            .to_owned())
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// The displays, then the windows of other apps, as a person names them.
pub fn targets(display_word: &str) -> Vec<Target> {
    let mut out = Vec::new();
    if let Ok(displays) = run(&args(&["list-displays"])) {
        for display in displays.as_array().into_iter().flatten() {
            let number = display["number"].as_u64().unwrap_or(0);
            let primary = if display["is_primary"].as_bool() == Some(true) {
                " *"
            } else {
                ""
            };
            out.push(Target {
                label: format!("{display_word} {number}{primary}"),
                args: vec!["--display".into(), number.to_string()],
            });
        }
    }
    let own = std::process::id();
    if let Ok(windows) = run(&args(&["list-windows"])) {
        for window in windows.as_array().into_iter().flatten() {
            let app = window["app_name"].as_str().unwrap_or_default();
            let title = window["title"].as_str().unwrap_or_default();
            let skip = window["pid"].as_u64() == Some(own as u64)
                || title.is_empty()
                || app.eq_ignore_ascii_case("explorer.exe")
                || window["minimized"].as_bool() == Some(true);
            if !skip {
                let id = window["id"].as_str().unwrap_or_default().to_owned();
                out.push(Target {
                    label: format!("{title}  ({app})"),
                    args: vec!["--window-id".into(), id],
                });
            }
        }
    }
    out
}

/// Starts a recording into `dir` (recording.jsonl, and video.mp4 when asked). Returns its id.
pub fn start(target: &Target, video: bool, dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut list = vec![
        "record".to_owned(),
        "start".to_owned(),
        "--out".to_owned(),
        dir.join("recording.jsonl").display().to_string(),
        // The bar shows the time and the Stop button, so agent-record's own indicator stays off.
        "--no-indicator".to_owned(),
    ];
    list.extend(target.args.iter().cloned());
    // Input that Windows marks as synthesized (an on-screen keyboard, a voice
    // or accessibility tool, an automated test) is left out unless asked for.
    if std::env::var("AGENT_RECORD_INCLUDE_INJECTED").is_ok_and(|value| value == "1") {
        list.push("--include-injected".to_owned());
    }
    if video {
        list.extend([
            "--video".to_owned(),
            "--video-out".to_owned(),
            dir.join("video.mp4").display().to_string(),
        ]);
    }
    let data = run(&list)?;
    Ok(data["id"].as_str().unwrap_or_default().to_owned())
}

pub fn stop(id: &str) -> Result<Value, String> {
    run(&args(&["record", "stop", "--id", id]))
}

/// Whether the recording is still running (the person may press Stop on the red indicator).
pub fn running(id: &str) -> bool {
    run(&args(&["record", "status"])).is_ok_and(|data| {
        data["recording"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| r["id"].as_str() == Some(id) && r["state"].as_str() != Some("ended"))
    })
}

/// Runs `node <script> <values>` in `dir`, sending each line it prints and writing them all to
/// `last-run.log` there (what an agent repairing the script reads). Ends with the exit code.
pub fn run_script(dir: &Path, script: &str, values: Vec<String>, lines: Sender<String>) {
    let (dir, script) = (dir.to_path_buf(), script.to_owned());
    let log = std::sync::Arc::new(std::sync::Mutex::new(
        std::fs::File::create(dir.join(crate::repair::LOG)).ok(),
    ));
    let write = move |line: &str| {
        use std::io::Write;
        if let Ok(mut file) = log.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = writeln!(file, "{line}");
        }
    };
    let lines = Logged {
        send: lines,
        write: std::sync::Arc::new(write),
    };
    std::thread::spawn(move || {
        let node = if cfg!(windows) { "node.exe" } else { "node" };
        let mut command = command(Path::new(node));
        // The script uses this same agent-record, not whatever else is on PATH.
        match checked_agent_record() {
            Ok(program) => {
                command.env("AGENT_RECORD_BIN", program);
            }
            Err(message) => {
                let _ = lines.send(message);
                let _ = lines.send("\u{0}1".to_owned());
                return;
            }
        }
        command
            .current_dir(&dir)
            .arg(&script)
            .args(values)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = lines.send(format!("node could not start: {error}"));
                let _ = lines.send("\u{0}1".to_owned());
                return;
            }
        };
        let stderr = child.stderr.take();
        let errors = lines.clone();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stderr.expect("stderr"))
                .lines()
                .map_while(Result::ok)
            {
                let _ = errors.send(line);
            }
        });
        for line in BufReader::new(child.stdout.take().expect("stdout"))
            .lines()
            .map_while(Result::ok)
        {
            let _ = lines.send(line);
        }
        let _ = reader.join();
        let code = child
            .wait()
            .ok()
            .and_then(|status| status.code())
            .unwrap_or(1);
        lines.write_only(&format!("exit code {code}"));
        let _ = lines.send(format!("\u{0}{code}"));
    });
}

/// The lines of a play: sent to the bar and kept in the log.
#[derive(Clone)]
struct Logged {
    send: Sender<String>,
    write: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
}

impl Logged {
    fn send(&self, line: String) -> Result<(), std::sync::mpsc::SendError<String>> {
        if !line.starts_with('\u{0}') {
            (self.write)(&line);
        }
        self.send.send(line)
    }

    fn write_only(&self, line: &str) {
        (self.write)(line);
    }
}

/// What the app remembers between uses: the folder and whether to record video.
pub fn load_settings() -> (Option<String>, bool) {
    let path = app_home().join(PRODUCT_DIR).join("recorder.json");
    let saved: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    (
        saved["folder"].as_str().map(str::to_owned),
        saved["video"].as_bool().unwrap_or(false),
    )
}

pub fn save_settings(folder: &str, video: bool) {
    let dir = app_home().join(PRODUCT_DIR);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join("recorder.json"),
        serde_json::json!({ "folder": folder, "video": video }).to_string(),
    );
}

/// The folder dialog of the system, starting at `current`.
pub fn pick_folder(current: &str) -> Option<String> {
    rfd::FileDialog::new()
        .set_directory(current)
        .pick_folder()
        .map(|p| p.display().to_string())
}

/// Opens a folder in the file explorer.
pub fn open_folder(dir: &Path) {
    let _ = Command::new("explorer").arg(dir).spawn();
}
