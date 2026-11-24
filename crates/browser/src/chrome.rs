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
/// window (the same the desktop layer gives every Chromium it launches), with
/// `CalculateNativeWinOcclusion` in [`disabled_features`].
pub const ANTI_OCCLUSION_FLAGS: &[&str] = &[
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
];

/// The one `--disable-features` switch of the dedicated browser (Chromium
/// reads only the last one, so every feature goes in it): occlusion off, and
/// for Edge its implicit sign-in, which would sign a new profile into the
/// person's Windows Microsoft account and sync their extensions into a browser
/// that agents drive.
pub fn disabled_features(browser: &Path) -> String {
    let mut features = vec!["CalculateNativeWinOcclusion"];
    if profile_name(browser) == "edge-profile" {
        features.push("msImplicitSignin");
    }
    format!("--disable-features={}", features.join(","))
}

/// Default DevTools port of the dedicated Chrome (`AGENT_RECORD_CHROME_PORT`).
pub const DEFAULT_CHROME_PORT: u16 = 9333;

/// The folder of our own state: `%LOCALAPPDATA%\DesktopRecorder`
/// (`AGENT_RECORD_STATE_DIR` overrides it).
pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("AGENT_RECORD_STATE_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    state_dir_in(&base)
}

/// `DesktopRecorder` under `base`. The folder an earlier build used
/// (`AgentRecordAdapter`) is renamed to it the first time, so the dedicated
/// Chrome keeps its profile; while that browser still runs the rename fails and
/// the earlier folder stays in use.
pub fn state_dir_in(base: &Path) -> PathBuf {
    let current = base.join("DesktopRecorder");
    let earlier = base.join("AgentRecordAdapter");
    if current.exists() || !earlier.is_dir() || std::fs::rename(&earlier, &current).is_ok() {
        current
    } else {
        earlier
    }
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

/// The Chromium browsers found by themselves, in the order they are tried:
/// a short name, the program under Program Files (or the person's
/// `%LOCALAPPDATA%` for a per-user install) and its own `User Data` folder.
pub const KNOWN_BROWSERS: &[(&str, &str, &str)] = &[
    (
        "chrome",
        r"Google\Chrome\Application\chrome.exe",
        r"Google\Chrome\User Data",
    ),
    (
        "edge",
        r"Microsoft\Edge\Application\msedge.exe",
        r"Microsoft\Edge\User Data",
    ),
    (
        "brave",
        r"BraveSoftware\Brave-Browser\Application\brave.exe",
        r"BraveSoftware\Brave-Browser\User Data",
    ),
];

/// The Chromium browser to start as the dedicated one: `AGENT_RECORD_CHROME`
/// (any Chromium program), else the one `AGENT_RECORD_BROWSER` names
/// (`chrome`, `edge` or `brave`), else Chrome, Edge or Brave, the first one
/// installed.
pub fn find_chrome() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AGENT_RECORD_CHROME") {
        return Some(PathBuf::from(path));
    }
    let wanted = std::env::var("AGENT_RECORD_BROWSER")
        .ok()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty());
    let bases: Vec<PathBuf> = ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|var| std::env::var_os(var).map(PathBuf::from))
        .collect();
    browser_candidates(&bases, wanted.as_deref())
        .into_iter()
        .find(|path| path.is_file())
}

/// Where each known browser (only `wanted`, when named) can be installed.
pub fn browser_candidates(bases: &[PathBuf], wanted: Option<&str>) -> Vec<PathBuf> {
    KNOWN_BROWSERS
        .iter()
        .filter(|(name, _, _)| wanted.is_none_or(|wanted| wanted == *name))
        .flat_map(|(_, program, _)| bases.iter().map(move |base| base.join(program)))
        .collect()
}

/// The dedicated profile of a browser program: one per browser, since
/// Chrome, Edge and Brave cannot share a profile folder. Chrome's keeps the
/// name it always had (`chrome-profile`).
pub fn profile_for(browser: &Path) -> PathBuf {
    state_dir().join(profile_name(browser))
}

fn profile_name(browser: &Path) -> String {
    let stem = browser
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("chrome")
        .to_ascii_lowercase();
    let name = if stem == "msedge" {
        "edge"
    } else {
        stem.as_str()
    };
    format!("{name}-profile")
}

/// The command line of the dedicated Chrome.
pub fn launch_args(port: u16, profile: &Path, headless: bool) -> Vec<String> {
    launch_args_for(Path::new("chrome.exe"), port, profile, headless)
}

/// The command line of the dedicated browser `browser` (Chrome, Edge, Brave).
pub fn launch_args_for(browser: &Path, port: u16, profile: &Path, headless: bool) -> Vec<String> {
    let mut args = vec![
        format!("--remote-debugging-port={port}"),
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        // A browser ended by a shutdown would otherwise greet the next agent
        // with a "restore pages" prompt over the page.
        "--hide-crash-restore-bubble".into(),
        disabled_features(browser),
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
    // The dedicated Chrome may already run on another port (started with
    // AGENT_RECORD_CHROME_PORT): a second start with the same profile would only
    // hand over to it and never answer, so it is used as it is.
    if let Some(running) = saved_port().filter(|saved| *saved != port) {
        if version(running).is_some() {
            ensure_window(running);
            return Ok(running);
        }
    }
    let chrome = find_chrome().ok_or_else(|| {
        "No Chrome, Edge or Brave was found; set AGENT_RECORD_CHROME to a Chromium browser"
            .to_owned()
    })?;
    let profile = profile_for(&chrome);
    std::fs::create_dir_all(&profile)
        .map_err(|e| format!("cannot create {}: {e}", profile.display()))?;
    let headless = headless_requested();
    let mut command = Command::new(&chrome);
    command
        .args(launch_args_for(&chrome, port, &profile, headless))
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

fn saved_state() -> Option<Value> {
    let raw = std::fs::read(state_dir().join("chrome.json")).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn dedicated_pid() -> Option<u32> {
    u32::try_from(saved_state()?.get("pid")?.as_u64()?).ok()
}

/// The DevTools port the dedicated Chrome was last started with.
fn saved_port() -> Option<u16> {
    u16::try_from(saved_state()?.get("port")?.as_u64()?).ok()
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
pub const NO_DEVTOOLS_MESSAGE: &str = "Your daily browser (Chrome, Edge or Brave) has no DevTools port open, so agent-browser cannot drive it. \
The desktop commands drive it instead, in the background, through window messages (chromium_input): \
use agent-record snapshot/click/type/navigate on its window. \
To use agent-browser on it, turn on remote debugging in that browser (chrome://inspect/#remote-debugging in Chrome 144+; \
edge:// or brave:// in the others) and approve the prompt, or start it with --remote-debugging-port and a profile of its own.";

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
    KNOWN_BROWSERS
        .iter()
        .map(|(_, _, user_data)| user_data)
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
    fn chrome_then_edge_then_brave_or_only_the_named_one() {
        let bases = [PathBuf::from("PF"), PathBuf::from("LA")];
        let all = browser_candidates(&bases, None);
        assert_eq!(all.len(), 6);
        assert!(all[0].ends_with(r"Google\Chrome\Application\chrome.exe"));
        assert!(all[2].ends_with(r"Microsoft\Edge\Application\msedge.exe"));
        assert!(all[4].ends_with(r"BraveSoftware\Brave-Browser\Application\brave.exe"));
        assert!(all[5].starts_with("LA"), "a per-user install is found too");
        let brave = browser_candidates(&bases, Some("brave"));
        assert_eq!(brave.len(), 2);
        assert!(brave.iter().all(|path| path.ends_with("brave.exe")));
        assert!(browser_candidates(&bases, Some("firefox")).is_empty());
    }

    #[test]
    fn edge_is_kept_out_of_the_persons_account_in_one_disable_features_switch() {
        let edge = launch_args_for(Path::new(r"C:\x\msedge.exe"), 9333, Path::new("p"), false);
        let features: Vec<&String> = edge
            .iter()
            .filter(|arg| arg.starts_with("--disable-features="))
            .collect();
        assert_eq!(
            features,
            ["--disable-features=CalculateNativeWinOcclusion,msImplicitSignin"]
        );
        let chrome = launch_args_for(Path::new(r"C:\x\chrome.exe"), 9333, Path::new("p"), false);
        assert!(chrome.contains(&"--disable-features=CalculateNativeWinOcclusion".to_owned()));
        let brave = launch_args_for(Path::new(r"C:\x\brave.exe"), 9333, Path::new("p"), false);
        assert!(!brave.iter().any(|arg| arg.contains("msImplicitSignin")));
    }

    #[test]
    fn each_browser_has_a_dedicated_profile_of_its_own() {
        assert_eq!(
            profile_name(Path::new(r"C:\x\chrome.exe")),
            "chrome-profile"
        );
        assert_eq!(profile_name(Path::new(r"C:\x\msedge.exe")), "edge-profile");
        assert_eq!(profile_name(Path::new(r"C:\x\Brave.exe")), "brave-profile");
    }

    #[test]
    fn the_state_folder_takes_over_the_one_an_earlier_build_used() {
        let base = std::env::temp_dir().join(format!("dr-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("AgentRecordAdapter").join("chrome-profile")).unwrap();
        let dir = state_dir_in(&base);
        assert_eq!(dir, base.join("DesktopRecorder"));
        assert!(
            dir.join("chrome-profile").is_dir(),
            "the profile moves with it"
        );
        assert!(!base.join("AgentRecordAdapter").exists());
        assert_eq!(state_dir_in(&base), base.join("DesktopRecorder"));
        let _ = std::fs::remove_dir_all(&base);
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
