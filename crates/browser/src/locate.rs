// Part of Agent Record Adapter (see NOTICE).
//! Starting agent-browser: the one agent-record.exe carries inside (see
//! [`set_embedded`]), or a separate executable for development and tests.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

static EMBEDDED: OnceLock<fn() -> Option<Command>> = OnceLock::new();

/// The program this runs in carries agent-browser: `make` gives a command
/// that starts it (agent-record.exe registers this at start).
pub fn set_embedded(make: fn() -> Option<Command>) {
    let _ = EMBEDDED.set(make);
}

/// A command that starts agent-browser: `AGENT_RECORD_BROWSER_BIN` or
/// `AGENT_BROWSER_BIN` when it names an executable that exists, else the one
/// this program carries, else one found as [`find_agent_browser`] says.
pub fn browser_command() -> Option<Command> {
    for var in ["AGENT_RECORD_BROWSER_BIN", "AGENT_BROWSER_BIN"] {
        if let Some(path) = std::env::var_os(var).filter(|p| !p.is_empty()) {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(command_for(&path));
            }
        }
    }
    if let Some(make) = EMBEDDED.get() {
        return make();
    }
    find_agent_browser().map(|path| command_for(&path))
}

/// The agent-browser executable, looked up in this order:
/// 1. `AGENT_RECORD_BROWSER_BIN`, then `AGENT_BROWSER_BIN` (the installer
///    gives the MCP server this one);
/// 2. next to the running executable (`agent-browser.exe`, or the npm
///    package's `agent-browser-win32-x64.exe`);
/// 3. the installer's program folder, `~/.desktop-recorder-mcp/bin`, which
///    carries agent-browser built from its unmodified sources;
/// 4. only for a repository checkout with no install: the npm global install
///    (`%APPDATA%\npm\node_modules`), then `PATH` (`agent-browser.exe`, then
///    the npm shim `agent-browser.cmd`).
pub fn find_agent_browser() -> Option<PathBuf> {
    for var in ["AGENT_RECORD_BROWSER_BIN", "AGENT_BROWSER_BIN"] {
        if let Some(path) = std::env::var_os(var).filter(|p| !p.is_empty()) {
            return Some(PathBuf::from(path));
        }
    }
    let next_to_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let private = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| {
            PathBuf::from(home)
                .join(".desktop-recorder-mcp")
                .join("bin")
        });
    let npm = std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("npm"));
    candidates(next_to_exe.as_deref(), private.as_deref(), npm.as_deref())
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| on_path(&["agent-browser.exe", "agent-browser.cmd", "agent-browser"]))
}

/// The fixed places to look, in order. `install_bin` is the installer's
/// program folder; `npm_dir` the global npm folder (`%APPDATA%\npm`).
pub fn candidates(
    exe_dir: Option<&Path>,
    install_bin: Option<&Path>,
    npm_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(dir) = exe_dir {
        found.push(dir.join("agent-browser.exe"));
        found.push(dir.join("agent-browser-win32-x64.exe"));
    }
    if let Some(bin) = install_bin {
        found.push(bin.join("agent-browser.exe"));
    }
    if let Some(npm) = npm_dir {
        found.extend(native_in(&npm.join("node_modules")));
    }
    found
}

/// The native executables an installed agent-browser package can hold, under
/// a `node_modules` folder.
fn native_in(modules: &Path) -> Vec<PathBuf> {
    vec![
        modules
            .join("agent-browser")
            .join("bin")
            .join("agent-browser-win32-x64.exe"),
        modules
            .join("@agent-browser")
            .join("cli-win32-x64-msvc")
            .join("bin")
            .join("agent-browser.exe"),
        modules
            .join("agent-browser")
            .join("node_modules")
            .join("@agent-browser")
            .join("cli-win32-x64-msvc")
            .join("bin")
            .join("agent-browser.exe"),
    ]
}

fn on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    names.iter().find_map(|name| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// How to start `binary`: a `.cmd`/`.bat` shim goes through `cmd /C`.
pub fn command_for(binary: &Path) -> Command {
    let is_script = binary
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_script {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(binary);
        command
    } else {
        Command::new(binary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_dir_then_install_folder_then_npm() {
        let app = Path::new("app");
        let bin = Path::new("home").join(".desktop-recorder-mcp").join("bin");
        let npm = Path::new("npm");
        let found = candidates(Some(app), Some(&bin), Some(npm));
        assert_eq!(found[0], app.join("agent-browser.exe"));
        assert_eq!(found[2], bin.join("agent-browser.exe"));
        assert!(
            found[3..]
                .iter()
                .all(|path| path.starts_with(npm.join("node_modules")))
        );
        assert_eq!(found.len(), 6);
    }

    #[test]
    fn shims_run_through_cmd() {
        let command = command_for(Path::new(r"C:\npm\agent-browser.cmd"));
        assert_eq!(command.get_program(), "cmd");
        let command = command_for(Path::new(r"C:\npm\agent-browser.exe"));
        assert_eq!(command.get_program(), r"C:\npm\agent-browser.exe");
    }
}
