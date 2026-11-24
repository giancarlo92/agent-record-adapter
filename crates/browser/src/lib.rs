// Part of Agent Record Adapter (see NOTICE).
//! Web pages through the official, unmodified agent-browser, in the
//! background.
//!
//! `agent-record browser <agent-browser args...>` makes sure a Chrome with
//! DevTools and our DevTools proxy run, then starts agent-browser with
//! `--cdp <proxy port>` and the person's arguments, passing its output and
//! exit code through. The proxy keeps the browser behind the person's windows
//! (it answers `Page.bringToFront` and `Target.activateTarget` itself and
//! opens new targets in the background) and notes agent actions in running
//! recordings ([`proxy`]).
//!
//! Two browsers:
//! - the **dedicated** Chrome (default): a profile of its own under
//!   `%LOCALAPPDATA%\AgentRecordAdapter\chrome-profile`, started with no
//!   window and given one behind the person's ([`chrome`]);
//! - the person's **daily** Chrome (`--daily [port]`), only when they turned
//!   its DevTools on; otherwise the desktop commands drive it through window
//!   messages, and this says so.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod cdp;
pub mod chrome;
pub mod inbox;
pub mod locate;
pub mod proxy;
mod window;

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

pub use chrome::{DailyEndpoint, NO_DEVTOOLS_MESSAGE};
pub use inbox::InboxSource;
pub use proxy::{ProxyConfig, RunningProxy};

/// The internal argument that runs the long-lived proxy process.
pub const SERVE_PROXY_FLAG: &str = "--serve-proxy";

/// How the front calls this crate.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// The arguments that make the running executable reach [`run`] again
    /// (`["browser"]` for `agent-record browser ...`), used to start the
    /// proxy process.
    pub self_args: Vec<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            self_args: vec!["browser".into()],
        }
    }
}

/// Which browser a command drives.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// The dedicated Chrome.
    Dedicated,
    /// The person's daily Chrome, on this DevTools port if they named one.
    Daily(Option<u16>),
}

/// `agent-record browser <args...>`: [`run_with`] with the default options.
pub fn run(args: Vec<String>) -> ExitCode {
    run_with(args, &RunOptions::default())
}

/// Runs agent-browser with `args` against the dedicated Chrome (or the daily
/// one with a leading `--daily [port]`), through the proxy. Arguments that
/// already choose a browser (`--cdp`, `--auto-connect`, `--provider`/`-p`,
/// `--engine`) and commands that need none (`--help`, `--version`,
/// `install`, `upgrade`, `skills`, `doctor`) go to agent-browser unchanged.
pub fn run_with(args: Vec<String>, options: &RunOptions) -> ExitCode {
    std_handles_not_inherited();
    if args.first().map(String::as_str) == Some(SERVE_PROXY_FLAG) {
        return serve_proxy_process(&args[1..]);
    }
    let Some(binary) = locate::find_agent_browser() else {
        eprintln!(
            "agent-record browser: agent-browser was not found. Install it with `npm install -g agent-browser` \
or set AGENT_RECORD_BROWSER_BIN to its executable."
        );
        return ExitCode::from(127);
    };
    let (target, args) = split_target(args);
    if passes_through(&args) {
        return exec(&binary, &args);
    }
    match prepare(&target, options) {
        Ok(proxy_port) => {
            let mut full = vec!["--cdp".to_owned(), proxy_port.to_string()];
            full.extend(args);
            exec(&binary, &full)
        }
        Err(message) => {
            eprintln!("agent-record browser: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `agent-browser mcp` (its stdio MCP server), ready to spawn, with its
/// browser the dedicated Chrome behind our proxy: Chrome and the proxy are
/// started when needed. `agent-browser mcp` takes only `--tools` and
/// `--config`; each tool call runs agent-browser's CLI, which reads its CDP
/// target from `AGENT_BROWSER_CDP` (a session default, `cli/src/flags.rs`),
/// so the proxy's port goes there.
pub fn mcp_command() -> Result<Command, String> {
    mcp_command_with(&Target::Dedicated, &RunOptions::default())
}

/// [`mcp_command`] for any target and front.
/// Windows gives every child all the inheritable handles of its parent, with
/// null standard streams too: a detached Chrome or proxy started from here
/// would hold the caller's pipe open, and a caller reading our output would
/// wait for them to end. Our own standard handles are made non-inheritable
/// (a child that should share them gets a duplicate from `Stdio::inherit`).
fn std_handles_not_inherited() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: GetStdHandle returns this process's handle (or null);
            // clearing its inherit flag changes nothing else about it.
            unsafe {
                let handle = GetStdHandle(which);
                if !handle.is_null() {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }
}

pub fn mcp_command_with(target: &Target, options: &RunOptions) -> Result<Command, String> {
    std_handles_not_inherited();
    let binary = locate::find_agent_browser().ok_or_else(|| {
        "agent-browser was not found; install it or set AGENT_RECORD_BROWSER_BIN".to_owned()
    })?;
    let proxy_port = prepare(target, options)?;
    let mut command = locate::command_for(&binary);
    command
        .arg("mcp")
        .env("AGENT_BROWSER_CDP", proxy_port.to_string());
    Ok(command)
}

/// Makes sure the browser of `target` and its proxy run; returns the proxy's
/// port, which agent-browser gets as `--cdp`.
pub fn prepare(target: &Target, options: &RunOptions) -> Result<u16, String> {
    let (chrome_port, ws_path) = match target {
        Target::Dedicated => (chrome::ensure_dedicated()?, None),
        Target::Daily(port) => {
            let endpoint = chrome::daily_endpoint(*port)?;
            (endpoint.port, endpoint.browser_ws_path)
        }
    };
    ensure_proxy(chrome_port, ws_path.as_deref(), options)
}

/// Takes a leading `--daily [port]` (or `--daily=<port>`) off `args`.
pub fn split_target(mut args: Vec<String>) -> (Target, Vec<String>) {
    let Some(first) = args.first().cloned() else {
        return (Target::Dedicated, args);
    };
    if let Some(port) = first.strip_prefix("--daily=") {
        args.remove(0);
        return (Target::Daily(port.parse().ok()), args);
    }
    if first == "--daily" {
        args.remove(0);
        let port = args.first().and_then(|a| a.parse::<u16>().ok());
        if port.is_some() {
            args.remove(0);
        }
        return (Target::Daily(port), args);
    }
    (Target::Dedicated, args)
}

/// Whether `args` already choose a browser or need none.
pub fn passes_through(args: &[String]) -> bool {
    const CHOOSES_BROWSER: &[&str] = &["--cdp", "--auto-connect", "--provider", "-p", "--engine"];
    const NO_BROWSER: &[&str] = &[
        "--help",
        "-h",
        "help",
        "--version",
        "-V",
        "install",
        "upgrade",
        "skills",
        "doctor",
    ];
    if args.is_empty() {
        return true;
    }
    if args.iter().any(|a| {
        CHOOSES_BROWSER
            .iter()
            .any(|flag| a == flag || a.starts_with(&format!("{flag}=")))
    }) {
        return true;
    }
    args.iter().find(|a| !a.starts_with('-')).map_or_else(
        || args.iter().any(|a| NO_BROWSER.contains(&a.as_str())),
        |command| NO_BROWSER.contains(&command.as_str()),
    )
}

/// Runs agent-browser with pipes of its own, relayed to ours. Its first run
/// starts a long-lived daemon that inherits whatever output handles it was
/// given; with ours, a caller reading our output (a pipe) would wait for that
/// daemon to end. So this returns when agent-browser itself exits, whatever
/// still holds its pipes.
fn exec(binary: &std::path::Path, args: &[String]) -> ExitCode {
    let spawned = locate::command_for(binary)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let status = spawned.and_then(|mut child| {
        let relay = |mut from: Box<dyn std::io::Read + Send>, to_err: bool| {
            std::thread::spawn(move || {
                use std::io::Write;
                let mut buffer = [0u8; 8192];
                while let Ok(read) = from.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    let _ = if to_err {
                        std::io::stderr().write_all(&buffer[..read])
                    } else {
                        let mut out = std::io::stdout();
                        out.write_all(&buffer[..read]).and_then(|()| out.flush())
                    };
                }
            })
        };
        let out = child.stdout.take().map(|pipe| relay(Box::new(pipe), false));
        let err = child.stderr.take().map(|pipe| relay(Box::new(pipe), true));
        let status = child.wait();
        // What agent-browser wrote before it exited is in the pipes: give the
        // relays a moment to pass it on, without waiting for a daemon.
        let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
        for relay in [out, err].into_iter().flatten() {
            while !relay.is_finished() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        status
    });
    match status {
        Ok(status) => ExitCode::from(
            status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1),
        ),
        Err(error) => {
            eprintln!(
                "agent-record browser: cannot run {}: {error}",
                binary.display()
            );
            ExitCode::FAILURE
        }
    }
}

fn proxy_state(chrome_port: u16) -> PathBuf {
    chrome::state_dir()
        .join("proxies")
        .join(format!("{chrome_port}.json"))
}

/// Makes sure a proxy for Chrome's `chrome_port` runs (a detached process of
/// the running executable) and returns its port.
pub fn ensure_proxy(
    chrome_port: u16,
    ws_path: Option<&str>,
    options: &RunOptions,
) -> Result<u16, String> {
    let state = proxy_state(chrome_port);
    if let Some(port) = std::fs::read(&state)
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .and_then(|v| u16::try_from(v.get("port")?.as_u64()?).ok())
        && proxy::probe_running(port) == Some(chrome_port)
    {
        return Ok(port);
    }
    let port = chrome::env_port("AGENT_RECORD_PROXY_PORT")
        .filter(|_| chrome_port == chrome::chrome_port())
        .map_or_else(free_port, Ok)
        .map_err(|e| format!("no free port for the DevTools proxy: {e}"))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command
        .args(&options.self_args)
        .arg(SERVE_PROXY_FLAG)
        .args([
            "--chrome-port",
            &chrome_port.to_string(),
            "--port",
            &port.to_string(),
        ]);
    if let Some(path) = ws_path {
        command.args(["--ws-path", path]);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    chrome::detach(&mut command);
    let child = command
        .spawn()
        .map_err(|e| format!("cannot start the DevTools proxy: {e}"))?;
    let end = Instant::now() + Duration::from_secs(5);
    while proxy::probe_running(port) != Some(chrome_port) {
        if Instant::now() > end {
            return Err(format!(
                "the DevTools proxy (pid {}) never answered on port {port}",
                child.id()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some(parent) = state.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &state,
        json!({ "port": port, "pid": child.id(), "chrome_port": chrome_port }).to_string(),
    );
    Ok(port)
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}

/// The proxy process: `--serve-proxy --chrome-port N --port M [--ws-path P]`.
/// It serves until Chrome goes away.
fn serve_proxy_process(args: &[String]) -> ExitCode {
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let (Some(chrome_port), Some(port)) = (
        value("--chrome-port").and_then(|p| p.parse::<u16>().ok()),
        value("--port").and_then(|p| p.parse::<u16>().ok()),
    ) else {
        eprintln!(
            "usage: {SERVE_PROXY_FLAG} --chrome-port <port> --port <port> [--ws-path <path>]"
        );
        return ExitCode::from(2);
    };
    let config = ProxyConfig {
        chrome_port,
        inbox: InboxSource::default_recordings(),
        exit_with_chrome: true,
        browser_ws_path: value("--ws-path"),
    };
    match proxy::serve_blocking(port, config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-record browser proxy: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn daily_target_is_taken_off_the_front() {
        assert_eq!(
            split_target(strings(&["--daily", "9555", "snapshot"])),
            (Target::Daily(Some(9555)), strings(&["snapshot"]))
        );
        assert_eq!(
            split_target(strings(&["--daily", "snapshot"])),
            (Target::Daily(None), strings(&["snapshot"]))
        );
        assert_eq!(
            split_target(strings(&["--daily=9222", "open", "x"])),
            (Target::Daily(Some(9222)), strings(&["open", "x"]))
        );
        assert_eq!(
            split_target(strings(&["open", "x"])),
            (Target::Dedicated, strings(&["open", "x"]))
        );
    }

    #[test]
    fn own_browser_choices_and_help_pass_through() {
        assert!(passes_through(&strings(&["--cdp", "9222", "snapshot"])));
        assert!(passes_through(&strings(&["--auto-connect", "open", "x"])));
        assert!(passes_through(&strings(&["--help"])));
        assert!(passes_through(&strings(&["skills", "get", "core"])));
        assert!(passes_through(&strings(&[])));
        assert!(!passes_through(&strings(&["open", "file:///C:/a.html"])));
        assert!(!passes_through(&strings(&["snapshot", "-i"])));
    }
}
