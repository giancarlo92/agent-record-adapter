// Part of Agent Record Adapter (see NOTICE).
//! Where the proxy notes the agent's page actions while a recording runs.
//!
//! A recording session is `<recordings>/<id>.json` with its `pid` and its
//! `stop_file` (`<id>.stop`). While its process lives and the stop file does
//! not exist, it reads its agent inbox `<id>.agent.jsonl` and records each
//! line as the agent's step (the same files the recorder uses for the
//! desktop's agent actions).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Where the inboxes to write come from.
#[derive(Debug, Clone)]
pub enum InboxSource {
    /// No recording: nothing is noted.
    None,
    /// These inbox files, always.
    Files(Vec<PathBuf>),
    /// The inboxes of the recordings running in this folder, looked up at
    /// each action.
    RecordingsDir(PathBuf),
}

impl InboxSource {
    /// The recordings folder the recorder uses: `AGENT_RECORD_RECORDINGS_DIR`,
    /// else `<AGENT_DESKTOP_HOME or ~/.agent-desktop>/recordings`.
    pub fn default_recordings() -> InboxSource {
        if let Some(dir) = std::env::var_os("AGENT_RECORD_RECORDINGS_DIR") {
            return InboxSource::RecordingsDir(PathBuf::from(dir));
        }
        let root = std::env::var_os("AGENT_DESKTOP_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(".agent-desktop")));
        match root {
            Some(root) => InboxSource::RecordingsDir(root.join("recordings")),
            None => InboxSource::None,
        }
    }

    /// The inboxes to write now.
    pub fn inboxes(&self) -> Vec<PathBuf> {
        match self {
            InboxSource::None => Vec::new(),
            InboxSource::Files(files) => files.clone(),
            InboxSource::RecordingsDir(dir) => running_inboxes(dir),
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The agent inboxes of the recordings running in `dir`.
pub fn running_inboxes(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let session: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            let pid = session.get("pid")?.as_u64()?;
            let stop = PathBuf::from(session.get("stop_file")?.as_str()?);
            let pid = u32::try_from(pid).ok()?;
            (pid != 0 && process_alive(pid) && !stop.exists())
                .then(|| stop.with_extension("agent.jsonl"))
        })
        .collect();
    found.sort();
    found
}

/// Appends `line` to `inbox` in one write, so lines of two writers do not mix.
pub fn append(inbox: &Path, line: &Value) {
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(inbox)
        .and_then(|mut file| file.write_all(format!("{line}\n").as_bytes()));
    if let Err(error) = written {
        eprintln!(
            "agent-record browser: the agent action was not noted in {}: {error}",
            inbox.display()
        );
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// Whether process `pid` is still running.
#[cfg(target_os = "windows")]
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain Win32 calls on a handle this function owns and closes.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0u32;
        let read = GetExitCodeProcess(process, &mut code) != 0;
        CloseHandle(process);
        read && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(target_os = "windows"))]
pub fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent-record-browser-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_running_session_without_stop_file_has_an_inbox() {
        let dir = scratch("running");
        let stop = dir.join("rec1.stop");
        std::fs::write(
            dir.join("rec1.json"),
            json!({"id":"rec1","pid":std::process::id(),"stop_file":stop}).to_string(),
        )
        .unwrap();
        assert_eq!(running_inboxes(&dir), vec![dir.join("rec1.agent.jsonl")]);
        std::fs::write(&stop, "").unwrap();
        assert!(running_inboxes(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_session_has_none() {
        let dir = scratch("dead");
        std::fs::write(
            dir.join("rec2.json"),
            json!({"id":"rec2","pid":0,"stop_file":dir.join("rec2.stop")}).to_string(),
        )
        .unwrap();
        assert!(running_inboxes(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_writes_one_line_each() {
        let dir = scratch("append");
        let inbox = dir.join("x.agent.jsonl");
        append(&inbox, &json!({"kind":"click"}));
        append(&inbox, &json!({"kind":"type"}));
        let text = std::fs::read_to_string(&inbox).unwrap();
        assert_eq!(text.lines().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
