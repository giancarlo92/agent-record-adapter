// Part of Agent Record Adapter (see NOTICE).
//! Keeping the app up to date, like Claude Code and Codex: it asks npm for the latest version of
//! agent-record-adapter now and then and, when the person chooses to update, runs
//! `npx agent-record-adapter@<version> update` and waits for it. That installs the new version for
//! the agents already set up (a program in use is set aside, so this app may keep running); then
//! the app opens the new one and closes. When it fails, the app stays and says why.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const PACKAGE: &str = "agent-record-adapter";

/// This app's version: what the installer wrote next to it, else the one it was built as.
pub fn current() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read_to_string(exe.with_file_name("version.txt")).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
}

/// The latest published version, from npm (`DESKTOP_RECORDER_LATEST` in tests). `None` offline.
pub fn latest() -> Option<String> {
    if let Ok(version) = std::env::var("DESKTOP_RECORDER_LATEST") {
        return Some(version);
    }
    let mut command = Command::new("curl.exe");
    command
        .args(["-s", "-f", "-m", "15"])
        .arg(format!("https://registry.npmjs.org/{PACKAGE}/latest"))
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(crate::tool::CREATE_NO_WINDOW);
    let output = command.output().ok()?;
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    answer["version"].as_str().map(str::to_owned)
}

/// Whether `candidate` is newer than `installed` (numbers compared part by part: 0.10.0 > 0.9.2).
pub fn newer(candidate: &str, installed: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .take(3)
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parts(candidate) > parts(installed)
}

/// Runs the update and waits for it (call it off the window's thread). Returns the error's last
/// line when it fails. `DESKTOP_RECORDER_UPDATE_FROM` (a package file) replaces npm in tests, and
/// `DESKTOP_RECORDER_HOME` becomes the installer's `--home`.
pub fn run(version: &str) -> Result<(), String> {
    let (folder, spec) = match std::env::var_os("DESKTOP_RECORDER_UPDATE_FROM") {
        // npx takes a package file only as ./name from its own folder.
        Some(file) => {
            let file = PathBuf::from(file);
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (
                file.parent().map(Path::to_path_buf).unwrap_or_default(),
                format!("./{name}"),
            )
        }
        None => (crate::tool::home(), format!("{PACKAGE}@{version}")),
    };
    let mut command = Command::new("cmd.exe");
    command
        .args(["/d", "/c", "npx", "--yes", &spec, "update"])
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(home) = std::env::var_os("DESKTOP_RECORDER_HOME") {
        command.arg("--home").arg(home);
    }
    #[cfg(windows)]
    command.creation_flags(crate::tool::CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| format!("npx could not start: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let errors = String::from_utf8_lossy(&output.stderr);
    Err(errors
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("npx failed")
        .trim_start_matches("agent-record-adapter: ")
        .to_owned())
}

/// Opens the installed app (the new one) without taking the focus.
pub fn reopen() -> Result<(), String> {
    let app = crate::tool::app_home()
        .join(crate::tool::PRODUCT_DIR)
        .join("bin")
        .join(if cfg!(windows) {
            "desktop-recorder.exe"
        } else {
            "desktop-recorder"
        });
    crate::tool::run(&["launch".to_owned(), app.display().to_string()]).map(|_| ())
}

/// How the last update went, when it failed in the last ten minutes (read once).
pub fn failure() -> Option<String> {
    let file = crate::tool::app_home()
        .join(crate::tool::PRODUCT_DIR)
        .join("update-result.json");
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    let result: serde_json::Value = serde_json::from_str(&text).ok()?;
    let at = result["at"].as_u64().unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if result["ok"].as_bool() == Some(true) || now.saturating_sub(at) > 10 * 60 * 1000 {
        return None;
    }
    result["error"].as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn versions_compare_part_by_part() {
        assert!(newer("0.3.0", "0.2.0"));
        assert!(newer("0.10.0", "0.9.2"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.1.5", "0.2.0"));
    }
}
