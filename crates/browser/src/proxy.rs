// Part of Agent Record Adapter (see NOTICE).
//! The DevTools proxy between agent-browser and a Chrome.
//!
//! HTTP discovery (`/json/version`, `/json/list`, ...) is passed to Chrome and
//! its answer pointed back at the proxy. Every WebSocket (agent-browser uses
//! the browser-level one with flattened page sessions) is relayed both ways,
//! with two changes made by [`crate::cdp`]: the commands that would bring the
//! browser to the front are answered here and never reach Chrome, and new
//! targets open in the background. While a recording runs, each agent action
//! is described (by a probe the proxy sends Chrome with an id of its own) and
//! noted in the recording's agent inbox before it is sent on.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::cdp::{self, ClientIntercept};
use crate::inbox::{self, InboxSource};

/// The header every HTTP answer of the proxy carries, so a running proxy is
/// told apart from a Chrome on the same port.
pub const PROXY_HEADER: &str = "X-Agent-Record-Proxy";
/// The proxy's own health endpoint: `{"proxy": true, "chrome_port": N}`.
pub const HEALTH_PATH: &str = "/agent-record/health";

/// Ids of the proxy's own probes start here, far above agent-browser's
/// counters, and stay inside a 32-bit integer.
const PROBE_ID_BASE: i64 = 2_000_000_000;
const PROBE_TIMEOUT: Duration = Duration::from_millis(700);
const HEAD_LIMIT: usize = 64 * 1024;

/// What a proxy serves.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Chrome's DevTools port on 127.0.0.1.
    pub chrome_port: u16,
    /// Where agent actions are noted.
    pub inbox: InboxSource,
    /// Stop when Chrome's port stops answering (a long-lived proxy process).
    pub exit_with_chrome: bool,
    /// The browser's WebSocket path (`/devtools/browser/<id>`) when Chrome
    /// serves no HTTP discovery (its own remote-debugging switch, Chrome
    /// 144+): the proxy then answers `/json/version` itself, and a bare
    /// `/devtools/browser` goes to this path.
    pub browser_ws_path: Option<String>,
}

struct Shared {
    config: ProxyConfig,
    port: u16,
    next_probe: AtomicI64,
}

/// Serves on `listener` until Chrome goes away (when `exit_with_chrome`) or
/// forever.
pub async fn serve(listener: TcpListener, config: ProxyConfig) -> io::Result<()> {
    let port = listener.local_addr()?.port();
    let shared = Arc::new(Shared {
        config,
        port,
        next_probe: AtomicI64::new(PROBE_ID_BASE),
    });
    let watch = shared.config.exit_with_chrome.then(|| {
        let chrome_port = shared.config.chrome_port;
        tokio::spawn(async move {
            let mut missed = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let up = tokio::time::timeout(
                    Duration::from_secs(2),
                    TcpStream::connect(("127.0.0.1", chrome_port)),
                )
                .await
                .is_ok_and(|connected| connected.is_ok());
                missed = if up { 0 } else { missed + 1 };
                if missed >= 3 {
                    return;
                }
            }
        })
    });
    let accept = async {
        loop {
            let (stream, _) = listener.accept().await?;
            let shared = shared.clone();
            tokio::spawn(async move {
                if let Err(error) = handle(stream, shared).await {
                    eprintln!("agent-record browser proxy: {error}");
                }
            });
        }
        #[allow(unreachable_code)]
        Ok::<(), io::Error>(())
    };
    match watch {
        Some(watch) => {
            tokio::select! {
                result = accept => result,
                _ = watch => Ok(()),
            }
        }
        None => accept.await,
    }
}

/// A proxy running on a thread of its own (tests, and a front that keeps the
/// proxy inside its own process).
pub struct RunningProxy {
    pub port: u16,
    _thread: std::thread::JoinHandle<()>,
}

/// Starts a proxy on 127.0.0.1:`port` (0 picks a free one) on a thread of
/// its own and returns once it listens.
pub fn spawn(port: u16, config: ProxyConfig) -> io::Result<RunningProxy> {
    let runtime = runtime()?;
    let listener = runtime.block_on(TcpListener::bind(("127.0.0.1", port)))?;
    let port = listener.local_addr()?.port();
    let thread = std::thread::Builder::new()
        .name("agent-record-proxy".into())
        .spawn(move || {
            if let Err(error) = runtime.block_on(serve(listener, config)) {
                eprintln!("agent-record browser proxy stopped: {error}");
            }
        })?;
    Ok(RunningProxy {
        port,
        _thread: thread,
    })
}

/// Serves on 127.0.0.1:`port` on this thread until the proxy stops.
pub fn serve_blocking(port: u16, config: ProxyConfig) -> io::Result<()> {
    let runtime = runtime()?;
    runtime.block_on(async {
        let listener = TcpListener::bind(("127.0.0.1", port)).await?;
        serve(listener, config).await
    })
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

fn ws_config() -> WebSocketConfig {
    // CDP messages (screenshots, big snapshots) exceed the default limits.
    WebSocketConfig::default()
        .max_message_size(None)
        .max_frame_size(None)
}

async fn handle(stream: TcpStream, shared: Arc<Shared>) -> io::Result<()> {
    let head = peek_head(&stream).await?;
    if is_websocket_upgrade(&head) {
        websocket(stream, shared).await
    } else {
        http(stream, shared).await
    }
}

/// The request head (up to the blank line), read without consuming it, so
/// the WebSocket handshake can still read it.
async fn peek_head(stream: &TcpStream) -> io::Result<String> {
    let mut buffer = vec![0u8; HEAD_LIMIT];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut seen = 0;
    loop {
        stream.readable().await?;
        let read = stream.peek(&mut buffer).await?;
        let text = String::from_utf8_lossy(&buffer[..read]);
        if let Some(end) = text.find("\r\n\r\n") {
            return Ok(text[..end].to_owned());
        }
        if read == 0 || read == HEAD_LIMIT || tokio::time::Instant::now() > deadline {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "no request head",
            ));
        }
        if read == seen {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        seen = read;
    }
}

/// Whether a request head asks for a WebSocket.
pub fn is_websocket_upgrade(head: &str) -> bool {
    head.lines().skip(1).any(|line| {
        let mut parts = line.splitn(2, ':');
        let name = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        name.eq_ignore_ascii_case("upgrade") && value.eq_ignore_ascii_case("websocket")
    })
}

async fn http(mut stream: TcpStream, shared: Arc<Shared>) -> io::Result<()> {
    let head = read_head(&mut stream).await?;
    let mut request_line = head.lines().next().unwrap_or("").split_whitespace();
    let method = request_line.next().unwrap_or("GET").to_owned();
    let path = request_line.next().unwrap_or("/").to_owned();
    if path == HEALTH_PATH {
        let body = json!({ "proxy": true, "chrome_port": shared.config.chrome_port }).to_string();
        return respond(&mut stream, "200 OK", "application/json", body.as_bytes()).await;
    }
    let chrome_port = shared.config.chrome_port;
    let answer = async {
        let mut chrome = TcpStream::connect(("127.0.0.1", chrome_port)).await?;
        chrome
            .write_all(
                format!(
                    "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{chrome_port}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await?;
        // Chrome keeps the connection open after answering: read the head,
        // then Content-Length bytes.
        let mut raw = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                match crate::chrome::content_length(&raw[..end]) {
                    Some(length) => {
                        while raw.len() < end + 4 + length {
                            let read = chrome.read(&mut chunk).await?;
                            if read == 0 {
                                break;
                            }
                            raw.extend_from_slice(&chunk[..read]);
                        }
                    }
                    None => {
                        chrome.read_to_end(&mut raw).await?;
                    }
                }
                break;
            }
            let read = chrome.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
        }
        Ok::<_, io::Error>(raw)
    };
    let raw = tokio::time::timeout(Duration::from_secs(10), answer)
        .await
        .ok()
        .and_then(Result::ok);
    let served = raw
        .as_ref()
        .is_some_and(|raw| raw.starts_with(b"HTTP/1.1 200"));
    if !served
        && path.starts_with("/json/version")
        && let Some(ws_path) = &shared.config.browser_ws_path
    {
        // Chrome's own remote-debugging switch serves only the WebSocket.
        let body = json!({
            "Browser": "Chrome",
            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{}{ws_path}", shared.port),
        })
        .to_string();
        return respond(&mut stream, "200 OK", "application/json", body.as_bytes()).await;
    }
    let Some(raw) = raw else {
        let body = format!("Chrome's DevTools port {chrome_port} does not answer");
        return respond(
            &mut stream,
            "502 Bad Gateway",
            "text/plain",
            body.as_bytes(),
        )
        .await;
    };
    let (status, content_type, body) = split_response(&raw);
    let body =
        cdp::rewrite_discovery_body(&String::from_utf8_lossy(body), chrome_port, shared.port);
    respond(&mut stream, &status, &content_type, body.as_bytes()).await
}

async fn read_head(stream: &mut TcpStream) -> io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > HEAD_LIMIT || stream.read(&mut byte).await? == 0 {
            break;
        }
        head.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// Status (`200 OK`), content type and body of a raw HTTP answer.
fn split_response(raw: &[u8]) -> (String, String, &[u8]) {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(raw.len(), |at| at + 4);
    let head = String::from_utf8_lossy(&raw[..end.min(raw.len())]);
    let status = head
        .lines()
        .next()
        .and_then(|line| {
            line.split_once(' ')
                .map(|(_, status)| status.trim().to_owned())
        })
        .unwrap_or_else(|| "200 OK".into());
    let content_type = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-type")
                .then(|| value.trim().to_owned())
        })
        .unwrap_or_else(|| "application/json; charset=UTF-8".into());
    (status, content_type, &raw[end.min(raw.len())..])
}

async fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{PROXY_HEADER}: 1\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}

type Pending = Arc<std::sync::Mutex<HashMap<i64, oneshot::Sender<Value>>>>;

async fn websocket(stream: TcpStream, shared: Arc<Shared>) -> io::Result<()> {
    let mut path = String::from("/");
    let client = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        |request: &Request, response: Response| {
            path = request
                .uri()
                .path_and_query()
                .map_or_else(|| "/".to_owned(), |p| p.as_str().to_owned());
            Ok(response)
        },
        Some(ws_config()),
    )
    .await
    .map_err(io::Error::other)?;
    if path == "/devtools/browser"
        && let Some(known) = &shared.config.browser_ws_path
    {
        path = known.clone();
    }
    let url = format!("ws://127.0.0.1:{}{path}", shared.config.chrome_port);
    let (chrome, _) = tokio_tungstenite::connect_async_with_config(url, Some(ws_config()), false)
        .await
        .map_err(io::Error::other)?;
    let (client_sink, mut client_stream) = client.split();
    let (mut chrome_sink, mut chrome_stream) = chrome.split();
    let client_sink = Arc::new(Mutex::new(client_sink));
    let pending: Pending = Arc::new(std::sync::Mutex::new(HashMap::new()));
    // Everything for Chrome goes through one queue, in order: agent-browser's
    // commands and the proxy's own (probes, focus emulation).
    let (to_chrome, mut chrome_queue) = mpsc::unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(message) = chrome_queue.recv().await {
            let close = matches!(message, Message::Close(_));
            if chrome_sink.send(message).await.is_err() || close {
                break;
            }
        }
        let _ = chrome_sink.close().await;
    });

    let to_client = {
        let client_sink = client_sink.clone();
        let pending = pending.clone();
        let to_chrome = to_chrome.clone();
        let shared = shared.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = chrome_stream.next().await {
                if let Message::Text(text) = &message {
                    let text = text.as_str();
                    if is_own_answer(&pending, text) {
                        continue;
                    }
                    if text.contains("Target.attachedToTarget")
                        && let Ok(event) = serde_json::from_str::<Value>(text)
                        && let Some(session) = cdp::attached_page_session(&event)
                    {
                        emulate_focus(&shared, &to_chrome, &session);
                    }
                }
                let close = matches!(message, Message::Close(_));
                if matches!(message, Message::Ping(_) | Message::Pong(_)) {
                    continue;
                }
                if client_sink.lock().await.send(message).await.is_err() || close {
                    break;
                }
            }
            let _ = client_sink.lock().await.close().await;
        })
    };

    while let Some(Ok(message)) = client_stream.next().await {
        match message {
            Message::Text(text) => match cdp::intercept_client_message(text.as_str()) {
                ClientIntercept::Answer(answer) => {
                    // The page does not come to the front, but it behaves as
                    // the focused page (focus, typing, timers).
                    if let Some(session) = cdp::session_of(text.as_str()) {
                        emulate_focus(&shared, &to_chrome, &session);
                    }
                    if client_sink
                        .lock()
                        .await
                        .send(Message::text(answer))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                ClientIntercept::Forward(text) => {
                    note_action(&shared, &pending, &to_chrome, &text).await;
                    if to_chrome.send(Message::text(text)).is_err() {
                        break;
                    }
                }
            },
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(frame) => {
                let _ = to_chrome.send(Message::Close(frame));
                break;
            }
            other => {
                if to_chrome.send(other).is_err() {
                    break;
                }
            }
        }
    }
    drop(to_chrome);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    to_client.abort();
    Ok(())
}

/// Turns on focus emulation in page session `session` (the answer is the
/// proxy's and is dropped).
fn emulate_focus(shared: &Shared, to_chrome: &mpsc::UnboundedSender<Message>, session: &str) {
    let id = shared.next_probe.fetch_add(1, Ordering::Relaxed);
    let _ = to_chrome.send(Message::text(cdp::focus_emulation(session, id).to_string()));
}

/// Whether `text` answers a command of the proxy's own (an id from
/// `PROBE_ID_BASE` up). A probe's answer is handed to the probe waiting for
/// it; neither reaches agent-browser.
fn is_own_answer(pending: &Pending, text: &str) -> bool {
    if !text.starts_with("{\"id\":") {
        return false;
    }
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let Some(id) = value.get("id").and_then(Value::as_i64) else {
        return false;
    };
    if id < PROBE_ID_BASE {
        return false;
    }
    if let Some(sender) = pending.lock().ok().and_then(|mut w| w.remove(&id)) {
        let _ = sender.send(value);
    }
    true
}

/// Notes the action `text` performs in every recording running, after asking
/// Chrome what element it targets. Does nothing (no probe, no delay) when no
/// recording runs.
async fn note_action(
    shared: &Shared,
    pending: &Pending,
    chrome: &mpsc::UnboundedSender<Message>,
    text: &str,
) {
    if !text.contains("\"method\"") {
        return;
    }
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some((action, session)) = cdp::agent_action(&message) else {
        return;
    };
    let inboxes = shared.config.inbox.inboxes();
    if inboxes.is_empty() {
        return;
    }
    let id = shared.next_probe.fetch_add(1, Ordering::Relaxed);
    let element: Option<Map<String, Value>> = match cdp::probe_for(&action, session.as_deref(), id)
    {
        Some(probe) => {
            let (sender, receiver) = oneshot::channel();
            if let Ok(mut waiting) = pending.lock() {
                waiting.insert(id, sender);
            }
            let sent = chrome.send(Message::text(probe.to_string())).is_ok();
            let answer = if sent {
                tokio::time::timeout(PROBE_TIMEOUT, receiver)
                    .await
                    .ok()
                    .and_then(Result::ok)
            } else {
                None
            };
            if let Ok(mut waiting) = pending.lock() {
                waiting.remove(&id);
            }
            answer.as_ref().and_then(cdp::probe_description)
        }
        None => None,
    };
    let line = cdp::inbox_line(&action, element.as_ref(), inbox::now_ms());
    for inbox_path in inboxes {
        inbox::append(&inbox_path, &line);
    }
}

/// Whether something on 127.0.0.1:`port` is one of our proxies, and for
/// which Chrome port.
pub fn probe_running(port: u16) -> Option<u16> {
    let body = crate::chrome::http_get(port, HEALTH_PATH, Duration::from_millis(800))?;
    let value: Value = serde_json::from_str(&body).ok()?;
    (value.get("proxy")? == &Value::Bool(true))
        .then(|| value.get("chrome_port")?.as_u64())
        .flatten()
        .and_then(|p| u16::try_from(p).ok())
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod tests;
