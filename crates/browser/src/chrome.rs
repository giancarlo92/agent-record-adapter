// Part of Agent Record Adapter (see NOTICE).
//! The dedicated Chrome: a profile of its own, a DevTools port, started with
//! no window and given one behind the person's windows, never activated.
//!
//! The person's own profiles are never touched or copied. Their daily Chrome
//! is reached only when they turned its DevTools on themselves (see
//! [`daily_endpoint`]).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Chromium switches that keep a page working while other windows cover its
/// window (the same the desktop layer gives every Chromium it launches).
pub const ANTI_OCCLUSION_FLAGS: &[&str] = &[
    "--disable-features=CalculateNativeWinOcclusion",
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
];

/// Default DevTools port of the dedicated Chrome (`AGENT_RECORD_CHROME_PORT`).
pub const DEFAULT_CHROME_PORT: u16 = 9333;

/// The folder of our own state: `%LOCALAPPDATA%\AgentRecordAdapter`
/// (`AGENT_RECORD_STATE_DIR` overrides it).
pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("AGENT_RECORD_STATE_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("AgentRecordAdapter")
}

/// The dedicated Chrome's profile folder.
pub fn profile_dir() -> PathBuf {
    state_dir().join("chrome-profile")
}

/// The dedicated Chrome's DevTools port.
pub fn chrome_port() -> u16 {
    env_port("AGENT_RECORD_CHROME_PORT").unwrap_or(DEFAULT_CHROME_PORT)
}

pub(crate) fn env_port(name: &str) -> Option<u16> {
    std::env::var(name).ok()?.trim().parse().ok()
}

/// A plain HTTP GET on 127.0.0.1:`port`; the body when the answer is 200.
pub fn http_get(port: u16, path: &str, timeout: Duration) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let raw = read_http_message(&mut stream)?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    head.lines()
        .next()
        .is_some_and(|line| line.contains(" 200"))
        .then(|| body.to_owned())
}

/// Reads one HTTP answer: its head, then `Content-Length` bytes of body (or
/// up to the end when it has none). Chrome's DevTools server keeps the
/// connection open after answering, so reading to the end would wait.
pub fn read_http_message(stream: &mut impl Read) -> Option<Vec<u8>> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let body_start = end + 4;
            match content_length(&raw[..end]) {
                Some(length) => {
                    while raw.len() < body_start + length {
                        let read = stream.read(&mut chunk).ok()?;
                        if read == 0 {
                            break;
                        }
                        raw.extend_from_slice(&chunk[..read]);
                    }
                    raw.truncate(body_start + length);
                    return Some(raw);
                }
                None => {
                    // No length: the server closes when done.
                    let _ = stream.read_to_end(&mut raw);
                    return Some(raw);
                }
            }
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return (!raw.is_empty()).then_some(raw);
        }
        raw.extend_from_slice(&chunk[..read]);
    }
}

/// The `Content-Length` of an HTTP head.
pub fn content_length(head: &[u8]) -> Option<usize> {
    String::from_utf8_lossy(head).lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

/// The browser's `/json/version` on `port`, when a DevTools endpoint answers.
pub fn version(port: u16) -> Option<Value> {
    let body = http_get(port, "/json/version", Duration::from_millis(800))?;
    serde_json::from_str(&body).ok()
}

/// The Chrome (or another Chromium browser) to start: `AGENT_RECORD_CHROME`,
/// else Chrome, else Edge, at their usual places.
pub fn find_chrome() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AGENT_RECORD_CHROME") {
        return Some(PathBuf::from(path));
    }
    let mut candidates = Vec::new();
    for var in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            let base = PathBuf::from(base);
            candidates.push(base.join(r"Google\Chrome\Application\chrome.exe"));
            candidates.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// The command line of the dedicated Chrome.
pub fn launch_args(port: u16, profile: &Path, headless: bool) -> Vec<String> {
    let mut args = vec![
        format!("--remote-debugging-port={port}"),
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
    ];
    args.extend(ANTI_OCCLUSION_FLAGS.iter().map(|flag| (*flag).to_owned()));
    if headless {
        args.push("--headless=new".into());
    } else {
        // No window at start: the first one is made behind (open_window).
        args.push("--no-startup-window".into());
    }
    args
}

/// Whether the dedicated Chrome runs headless (`AGENT_RECORD_CHROME_HEADLESS=1`).
pub fn headless_requested() -> bool {
    std::env::var("AGENT_RECORD_CHROME_HEADLESS")
        .is_ok_and(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
}

/// Makes sure the dedicated Chrome answers on its DevTools port, starting it
/// when it does not. Returns the port.
pub fn ensure_dedicated() -> Result<u16, String> {
    let port = chrome_port();
    if version(port).is_some() {
        ensure_window(port);
        return Ok(port);
    }
    let chrome = find_chrome().ok_or_else(|| {
        "No Chrome or Edge was found; set AGENT_RECORD_CHROME to a Chromium browser".to_owned()
    })?;
    let profile = profile_dir();
    std::fs::create_dir_all(&profile)
        .map_err(|e| format!("cannot create {}: {e}", profile.display()))?;
    let headless = headless_requested();
    let mut command = Command::new(&chrome);
    command
        .args(launch_args(port, &profile, headless))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    let before = crate::window::foreground();
    let child = command
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", chrome.display()))?;
    let end = Instant::now() + Duration::from_secs(20);
    while version(port).is_none() {
        if Instant::now() > end {
            return Err(format!(
                "{} started (pid {}) but its DevTools port {port} never answered",
                chrome.display(),
                child.id()
            ));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let _ = std::fs::write(
        state_dir().join("chrome.json"),
        json!({ "pid": child.id(), "port": port, "headless": headless }).to_string(),
    );
    if !headless {
        open_window(port);
        crate::window::send_behind(child.id(), before);
    }
    Ok(port)
}

/// Gives a headed dedicated Chrome a window when it has none (it was started
/// with `--no-startup-window`, or the person closed its last window).
fn ensure_window(port: u16) {
    let has_page = http_get(port, "/json/list", Duration::from_millis(800))
        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        .and_then(|list| list.as_array().cloned())
        .is_some_and(|list| list.iter().any(|t| t["type"] == "page"));
    if !has_page {
        let before = crate::window::foreground();
        open_window(port);
        if let Some(pid) = dedicated_pid() {
            crate::window::send_behind(pid, before);
        }
    }
}

fn dedicated_pid() -> Option<u32> {
    let raw = std::fs::read(state_dir().join("chrome.json")).ok()?;
    let state: Value = serde_json::from_slice(&raw).ok()?;
    u32::try_from(state.get("pid")?.as_u64()?).ok()
}

/// Opens a window with a blank page behind the person's windows, through the
/// browser's DevTools (`Target.createTarget` with `newWindow` and
/// `background`).
pub fn open_window(port: u16) -> bool {
    let Some(ws) =
        version(port).and_then(|v| v.get("webSocketDebuggerUrl")?.as_str().map(str::to_owned))
    else {
        return false;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return false,
    };
    runtime.block_on(async move {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let Ok(Ok((mut socket, _))) =
            tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(ws))
                .await
        else {
            return false;
        };
        let command = json!({ "id": 1, "method": "Target.createTarget",
            "params": { "url": "about:blank", "newWindow": true, "background": true } });
        if socket
            .send(Message::text(command.to_string()))
            .await
            .is_err()
        {
            return false;
        }
        let answered = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(Ok(message)) = socket.next().await {
                if let Message::Text(text) = message
                    && let Ok(value) = serde_json::from_str::<Value>(text.as_str())
                    && value["id"] == 1
                {
                    return value["result"]["targetId"].is_string();
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        let _ = socket.close(None).await;
        answered
    })
}

/// The DevTools endpoint of the person's daily Chrome, when they turned it on
/// themselves: `--remote-debugging-port` (a port that answers `/json/version`)
/// or Chrome 144+'s `chrome://inspect/#remote-debugging` switch, which writes
/// the port and the browser's WebSocket path in `DevToolsActivePort` of the
/// default profile folder (only that file is read).
///
/// `port`: the port the person named, if any.
pub fn daily_endpoint(port: Option<u16>) -> Result<DailyEndpoint, String> {
    if let Some(port) = port {
        if version(port).is_some() {
            return Ok(DailyEndpoint {
                port,
                browser_ws_path: None,
            });
        }
    }
    if let Some((active_port, path)) = devtools_active_port()
        && port.is_none_or(|p| p == active_port)
        && TcpStream::connect_timeout(
            &([127, 0, 0, 1], active_port).into(),
            Duration::from_millis(500),
        )
        .is_ok()
    {
        return Ok(DailyEndpoint {
            port: active_port,
            browser_ws_path: Some(path),
        });
    }
    Err(NO_DEVTOOLS_MESSAGE.to_owned())
}

/// Why the daily Chrome cannot be driven through agent-browser.
pub const NO_DEVTOOLS_MESSAGE: &str = "Your daily Chrome has no DevTools port open, so agent-browser cannot drive it. \
The desktop commands drive it instead, in the background, through window messages (chromium_input): \
use agent-record snapshot/click/type/navigate on its window. \
To use agent-browser on it, turn on chrome://inspect/#remote-debugging in that Chrome (Chrome 144+) and approve the prompt, \
or start it with --remote-debugging-port and a profile of its own.";

/// The daily Chrome's DevTools endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyEndpoint {
    pub port: u16,
    /// `/devtools/browser/<id>` when only the WebSocket is served (Chrome's
    /// own remote-debugging switch serves no HTTP discovery).
    pub browser_ws_path: Option<String>,
}

fn devtools_active_port() -> Option<(u16, String)> {
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    [r"Google\Chrome\User Data", r"Microsoft\Edge\User Data"]
        .iter()
        .find_map(|dir| {
            parse_devtools_active_port(
                &std::fs::read_to_string(base.join(dir).join("DevToolsActivePort")).ok()?,
            )
        })
}

/// The port and WebSocket path a `DevToolsActivePort` file names.
pub fn parse_devtools_active_port(text: &str) -> Option<(u16, String)> {
    let mut lines = text.lines();
    let port = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    path.starts_with("/devtools/browser")
        .then(|| (port, path.to_owned()))
}

#[cfg(target_os = "windows")]
pub(crate) fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn detach(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_args_keep_pages_alive_and_own_profile() {
        let args = launch_args(9333, Path::new(r"C:\state\chrome-profile"), false);
        assert!(args.contains(&"--remote-debugging-port=9333".to_owned()));
        assert!(args.contains(&r"--user-data-dir=C:\state\chrome-profile".to_owned()));
        assert!(args.contains(&"--no-startup-window".to_owned()));
        for flag in ANTI_OCCLUSION_FLAGS {
            assert!(args.contains(&(*flag).to_owned()));
        }
        let headless = launch_args(9333, Path::new("p"), true);
        assert!(headless.contains(&"--headless=new".to_owned()));
        assert!(!headless.contains(&"--no-startup-window".to_owned()));
    }

    #[test]
    fn devtools_active_port_file() {
        assert_eq!(
            parse_devtools_active_port("9555\n/devtools/browser/abc-1\n"),
            Some((9555, "/devtools/browser/abc-1".to_owned()))
        );
        assert_eq!(parse_devtools_active_port("x\n/devtools/browser/a"), None);
        assert_eq!(parse_devtools_active_port("9555\n"), None);
    }

    #[test]
    fn a_closed_port_answers_nothing() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(version(port).is_none());
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;

    #[test]
    fn an_answer_ends_at_its_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbodyEXTRA".to_vec();
        let read = read_http_message(&mut std::io::Cursor::new(raw)).unwrap();
        assert!(read.ends_with(b"\r\n\r\nbody"));
        assert_eq!(
            content_length(b"HTTP/1.1 200 OK\r\ncontent-length: 12"),
            Some(12)
        );
    }
}
