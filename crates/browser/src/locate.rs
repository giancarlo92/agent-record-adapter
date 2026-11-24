// Part of Agent Record Adapter (see NOTICE).
//! Finding the official agent-browser binary.

use std::path::{Path, PathBuf};

/// The agent-browser executable, looked up in this order:
/// 1. `AGENT_RECORD_BROWSER_BIN`, then `AGENT_BROWSER_BIN` (the installer
///    gives the MCP server this one);
/// 2. next to the running executable (`agent-browser.exe`, or the npm
///    package's `agent-browser-win32-x64.exe`);
/// 3. our installer's private npm prefix,
///    `~/.agent-record-adapter/browser/node_modules`: the native executable
///    inside the package (its `bin/` or its platform package), then the
///    `.bin/agent-browser(.cmd)` shim;
/// 4. the npm global install (`%APPDATA%\npm\node_modules`), the same way;
/// 5. `PATH` (`agent-browser.exe`, then the npm shim `agent-browser.cmd`).
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
                .join(".agent-record-adapter")
                .join("browser")
        });
    let npm = std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("npm"));
    candidates(next_to_exe.as_deref(), private.as_deref(), npm.as_deref())
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| on_path(&["agent-browser.exe", "agent-browser.cmd", "agent-browser"]))
}

/// The fixed places to look, in order. `private_prefix` is our installer's
/// npm prefix; `npm_dir` the global npm folder (`%APPDATA%\npm`).
pub fn candidates(
    exe_dir: Option<&Path>,
    private_prefix: Option<&Path>,
    npm_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(dir) = exe_dir {
        found.push(dir.join("agent-browser.exe"));
        found.push(dir.join("agent-browser-win32-x64.exe"));
    }
    if let Some(prefix) = private_prefix {
        let modules = prefix.join("node_modules");
        found.extend(native_in(&modules));
        found.push(modules.join(".bin").join("agent-browser.exe"));
        found.push(modules.join(".bin").join("agent-browser.cmd"));
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
pub fn command_for(binary: &Path) -> std::process::Command {
    let is_script = binary
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_script {
        let mut command = std::process::Command::new("cmd");
        command.arg("/C").arg(binary);
        command
    } else {
        std::process::Command::new(binary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_dir_then_private_prefix_then_npm() {
        let app = Path::new("app");
        let private = Path::new("home")
            .join(".agent-record-adapter")
            .join("browser");
        let npm = Path::new("npm");
        let found = candidates(Some(app), Some(&private), Some(npm));
        assert_eq!(found[0], app.join("agent-browser.exe"));
        assert_eq!(
            found[2],
            private
                .join("node_modules")
                .join("agent-browser")
                .join("bin")
                .join("agent-browser-win32-x64.exe")
        );
        assert_eq!(
            found[6],
            private
                .join("node_modules")
                .join(".bin")
                .join("agent-browser.cmd")
        );
        assert!(found[7].starts_with(npm.join("node_modules")));
        assert_eq!(found.len(), 10);
    }

    #[test]
    fn shims_run_through_cmd() {
        let command = command_for(Path::new(r"C:\npm\agent-browser.cmd"));
        assert_eq!(command.get_program(), "cmd");
        let command = command_for(Path::new(r"C:\npm\agent-browser.exe"));
        assert_eq!(command.get_program(), r"C:\npm\agent-browser.exe");
    }
}
