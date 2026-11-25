// Part of Agent Record Adapter (see NOTICE).
//! agent-record.exe carries the two upstream programs inside: the engine
//! (agent-desktop, with our adapter) and agent-browser, both from their
//! unmodified sources. Each runs in a process of its own, as before, but the
//! process is this same executable started with `AGENT_RECORD_PART`.
//!
//! The variable reads `<part>@<pid>`: the part, and the process that started
//! it. A process takes a part only when that pid is its parent, so a program
//! the engine launches (which inherits the variable) and then runs
//! agent-record gets agent-record, not the part. A part re-marks the variable
//! with its own pid, so the processes it starts of itself (the engine's MCP
//! calls, agent-browser's daemon) keep the part.
//!
//! `AGENT_RECORD_ENGINE` / `AGENT_RECORD_BROWSER_BIN` still point to separate
//! executables (development, tests).

use std::process::{Command, ExitCode};

const VAR: &str = "AGENT_RECORD_PART";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Engine,
    Browser,
}

impl Part {
    fn name(self) -> &'static str {
        match self {
            Part::Engine => "engine",
            Part::Browser => "browser",
        }
    }
}

/// The part this process was started as, if any; it then marks the variable
/// for the processes it starts.
pub(crate) fn requested() -> Option<Part> {
    let value = std::env::var(VAR).ok()?;
    let (part, starter) = parse(&value)?;
    if Some(starter) != parent_pid() {
        return None;
    }
    // SAFETY: called first thing in `main`, before any other thread exists.
    unsafe { std::env::set_var(VAR, mark(part)) };
    Some(part)
}

/// Runs the part in this process.
pub(crate) fn run(part: Part) -> ExitCode {
    match part {
        Part::Engine => agent_record_engine::main(),
        Part::Browser => {
            agent_record_web_engine::main();
            ExitCode::SUCCESS
        }
    }
}

/// A command that starts `part` (this executable, marked).
pub(crate) fn command(part: Part) -> Result<Command, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("Cannot locate agent-record: {error}"))?;
    let mut command = Command::new(exe);
    command.env(VAR, mark(part));
    Ok(command)
}

fn mark(part: Part) -> String {
    format!("{}@{}", part.name(), std::process::id())
}

fn parse(value: &str) -> Option<(Part, u32)> {
    let (name, pid) = value.split_once('@')?;
    let part = match name {
        "engine" => Part::Engine,
        "browser" => Part::Browser,
        _ => return None,
    };
    Some((part, pid.parse().ok()?))
}

/// The pid of the process that started this one.
#[cfg(windows)]
fn parent_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let own = std::process::id();
    // SAFETY: plain Win32 calls on a snapshot handle this function owns and
    // closes; the entry is sized as the API asks.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0);
        let mut found = None;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            if entry.th32ProcessID == own {
                found = Some(entry.th32ParentProcessID);
                break;
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(not(windows))]
fn parent_pid() -> Option<u32> {
    Some(std::os::unix::process::parent_id())
}

#[cfg(test)]
mod tests {
    use super::{Part, parse};

    #[test]
    fn a_part_is_named_with_the_process_that_started_it() {
        assert_eq!(parse("engine@42"), Some((Part::Engine, 42)));
        assert_eq!(parse("browser@7"), Some((Part::Browser, 7)));
        assert_eq!(parse("engine"), None);
        assert_eq!(parse("other@1"), None);
        assert_eq!(parse("engine@x"), None);
    }
}
