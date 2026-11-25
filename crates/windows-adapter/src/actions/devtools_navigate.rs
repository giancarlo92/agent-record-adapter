// Part of Agent Record Adapter (see NOTICE).
// Ported from the desktop-recorder-mcp fork of agent-desktop (cdp_page.rs,
// cdp_socket.rs, tcp_listener.rs, devtools.rs; Apache-2.0).
//! `navigate` in a browser window whose browser has a DevTools endpoint (one
//! started with `launch --cdp`): the tab is told to open the address through
//! that endpoint, keeping its session, and the window never comes forward.
//! With `--in-app` the address changes the way a single-page app's own links
//! do (`history.pushState` and a `popstate` event), so what the app keeps only
//! in memory stays. A full load that the app sends elsewhere (its router
//! starts at home) is followed by one in-app try, so the asked route is
//! reached. A browser without an endpoint is the person's own: its address
//! bar is used instead (see `chromium_input::navigate`).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent_desktop_core::{AdapterError, Deadline, DeliverySemantics, ErrorCode, WindowInfo};
use serde_json::{Value, json};

const NAVIGATE_TIMEOUT: Duration = Duration::from_secs(20);
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);
const LIST_TIMEOUT: Duration = Duration::from_millis(800);
/// A page loaded at an address other than the one asked for, and staying
/// there this long, was sent there by the app.
const SETTLED_ELSEWHERE: Duration = Duration::from_millis(2500);
const MAX_PORTS: usize = 16;
/// Once the navigation is sent, its result is checked at least this long,
/// even past the operation's deadline: a route changed and never checked
/// would be reported as not reached.
const MIN_CHECK: Duration = Duration::from_millis(1500);
/// What the in-app try after a full load the app sent elsewhere gets at least.
const MIN_RETRY: Duration = Duration::from_secs(2);

/// Changes the address the way a single-page app's own links do, when it is
/// on the same site; answers `"other"` for another site.
const IN_APP: &str = "function(u){var t=new URL(u,location.href);\
    if(t.protocol!==location.protocol||t.host!==location.host)return 'other';\
    history.pushState(history.state,'',t.href);\
    dispatchEvent(new PopStateEvent('popstate',{state:history.state}));return true}";

/// The tab a navigation is for: by its address, else by its title.
struct PageQuery<'a> {
    url: Option<&'a str>,
    title: Option<&'a str>,
}

enum Navigated {
    NotFound,
    At { url: String, reached: bool },
}

/// Navigates through DevTools; `None` when the window's browser has no
/// DevTools endpoint (the caller then uses the address bar).
pub(crate) fn navigate(
    window: &WindowInfo,
    url: &str,
    in_app: bool,
    deadline: Deadline,
) -> Option<Result<Value, AdapterError>> {
    let port = devtools_port(window.pid.get())?;
    let (page_url, page_title) = window_page(window);
    let from = page_url.clone();
    let outcome = navigate_page(
        port,
        &PageQuery {
            url: page_url.as_deref(),
            title: page_title.as_deref(),
        },
        url,
        in_app,
        deadline.remaining().min(NAVIGATE_TIMEOUT),
    );
    Some(match outcome {
        Ok(Navigated::At {
            url: now,
            reached: false,
        }) => Err(crate::actions::chromium_input::not_reached(
            url,
            Some(now.as_str()),
        )),
        Ok(Navigated::At { url: now, reached }) => Ok(json!({
            "window": window.id,
            "from": from,
            "url": now,
            "reached": reached,
            "in_app": in_app,
            "via": "devtools",
        })),
        Ok(Navigated::NotFound) => Err(AdapterError::new(
            ErrorCode::ElementNotFound,
            "The window's page could not be told apart from the browser's other tabs",
        )
        .with_suggestion("Close the other tabs with the same address, then navigate again")
        .with_disposition(DeliverySemantics::not_delivered())),
        Err(reason) => Err(AdapterError::new(ErrorCode::ActionFailed, reason)
            .with_disposition(DeliverySemantics::delivered_unverified())),
    })
}

/// The address and title of the page the window shows.
fn window_page(window: &WindowInfo) -> (Option<String>, Option<String>) {
    use uiautomation::patterns::UIValuePattern;
    use uiautomation::types::Handle;
    let Ok(hwnd) = crate::system::window::window_ops::parse_window_id(&window.id) else {
        return (None, None);
    };
    let Ok(client) = uiautomation::UIAutomation::new() else {
        return (None, None);
    };
    let Ok(root) = client.element_from_handle(Handle::from(hwnd)) else {
        return (None, None);
    };
    if let Some(document) = crate::actions::chromium_input::active_document(&root, &client) {
        let url = document
            .get_pattern::<UIValuePattern>()
            .and_then(|pattern| pattern.get_value())
            .ok()
            .filter(|url| !url.is_empty());
        let title = document.get_name().ok().filter(|title| !title.is_empty());
        return (url, title);
    }
    // A browser just started behind other windows takes a few seconds to
    // describe its page: the tab's title begins the window's ("tab - Chrome").
    let title = root
        .get_name()
        .ok()
        .and_then(|name| name.rsplit_once(" - ").map(|(tab, _)| tab.to_owned()))
        .filter(|tab| !tab.is_empty());
    (None, title)
}

/// The loopback port of the browser process that answers as DevTools.
fn devtools_port(pid: u32) -> Option<u16> {
    listeners(pid).into_iter().take(MAX_PORTS).find(|port| {
        get_json(*port, "/json/version", PROBE_TIMEOUT)
            .is_some_and(|version| version["webSocketDebuggerUrl"].is_string())
    })
}

fn navigate_page(
    port: u16,
    page: &PageQuery<'_>,
    url: &str,
    in_app: bool,
    timeout: Duration,
) -> Result<Navigated, &'static str> {
    let until = Instant::now() + timeout;
    let Some(mut session) = only_page(port, page, until, timeout) else {
        return Ok(Navigated::NotFound);
    };
    if in_app {
        let window = session.object("window").ok_or("The page did not answer")?;
        let answer = session
            .call(
                "Runtime.callFunctionOn",
                json!({
                    "objectId": window,
                    "functionDeclaration": IN_APP,
                    "arguments": [{ "value": url }],
                    "returnByValue": true,
                    "userGesture": true,
                }),
            )
            .ok_or("The page did not confirm the navigation")?;
        if answer["result"]["value"] == "other" {
            return Err(
                "--in-app only changes the address within the same site; leave it out to load another site",
            );
        }
    } else {
        let answer = session
            .call("Page.navigate", json!({ "url": url }))
            .ok_or("The browser did not confirm the navigation")?;
        if answer["errorText"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
        {
            return Err("The browser could not open that address");
        }
    }
    let until = until.max(Instant::now() + MIN_CHECK);
    let wanted = url.trim_end_matches('/');
    let mut last = String::new();
    let mut settled: Option<Instant> = None;
    let start = page.url.unwrap_or_default();
    let asked = Instant::now();
    let mut begun = false;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(200));
        let Some(answer) = session.call(
            "Runtime.evaluate",
            json!({ "expression": "document.readyState+' '+location.href", "returnByValue": true }),
        ) else {
            continue;
        };
        let text = answer["result"]["value"].as_str().unwrap_or_default();
        let Some((state, href)) = text.split_once(' ') else {
            continue;
        };
        begun |= state != "complete"
            || (!start.is_empty() && href != start)
            || asked.elapsed() >= Duration::from_millis(1500);
        if href != last || state != "complete" {
            settled = (state == "complete").then(Instant::now);
        }
        last = href.to_owned();
        let here = href.trim_end_matches('/');
        if state == "complete" && (here == wanted || here.ends_with(wanted)) {
            // A single-page app draws the new view a moment after the address changes.
            std::thread::sleep(Duration::from_millis(500));
            return Ok(Navigated::At {
                url: href.to_owned(),
                reached: true,
            });
        }
        if begun && settled.is_some_and(|since| since.elapsed() >= SETTLED_ELSEWHERE) {
            break;
        }
    }
    // A full load that an app sends elsewhere (its router restores another
    // route on start): try once more the way its own links change routes.
    let left = until.saturating_duration_since(Instant::now());
    if !in_app && !last.is_empty() {
        let now = PageQuery {
            url: Some(last.as_str()),
            title: None,
        };
        let again = navigate_page(port, &now, url, true, left.max(MIN_RETRY));
        if let Ok(Navigated::At { reached: true, .. }) = again {
            return again;
        }
    }
    Ok(Navigated::At {
        url: last,
        reached: false,
    })
}

/// The DevTools session of the one page that is `page`.
fn only_page(
    port: u16,
    page: &PageQuery<'_>,
    until: Instant,
    timeout: Duration,
) -> Option<Session> {
    let targets = get_json(port, "/json/list", LIST_TIMEOUT.min(timeout))?;
    let mut pages = matching_pages(&targets, page);
    if pages.len() != 1 {
        return None;
    }
    Session::connect(&pages.remove(0), until)
}

/// The DevTools addresses of the pages that are `page`: those at its address
/// (compared without the fragment), or failing that those with its title.
fn matching_pages(targets: &Value, page: &PageQuery<'_>) -> Vec<String> {
    let pages: Vec<&Value> = targets
        .as_array()
        .map(|all| {
            all.iter()
                .filter(|target| target["type"] == "page")
                .collect()
        })
        .unwrap_or_default();
    let socket = |target: &&Value| target["webSocketDebuggerUrl"].as_str().map(str::to_owned);
    let without_fragment = |url: &str| url.split('#').next().unwrap_or_default().to_owned();
    if let Some(url) = page.url.filter(|url| !url.is_empty()) {
        let wanted = without_fragment(url);
        let by_url: Vec<String> = pages
            .iter()
            .filter(|target| {
                target["url"]
                    .as_str()
                    .is_some_and(|url| without_fragment(url) == wanted)
            })
            .filter_map(socket)
            .collect();
        if !by_url.is_empty() {
            return by_url;
        }
    }
    match page.title.filter(|title| !title.is_empty()) {
        Some(title) => pages
            .iter()
            .filter(|target| target["title"].as_str() == Some(title))
            .filter_map(socket)
            .collect(),
        // One tab only: it is the window's.
        None if pages.len() == 1 => pages.iter().filter_map(socket).collect(),
        None => Vec::new(),
    }
}

/// The JSON a DevTools HTTP path answers with, or `None`.
fn get_json(port: u16, path: &str, timeout: Duration) -> Option<Value> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = Vec::new();
    let _ = stream.take(8 * 1024 * 1024).read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") {
        return None;
    }
    serde_json::from_str(body.trim()).ok()
}

/// The loopback TCP ports process `pid` listens on. The system's listener
/// table is asked for directly: in the table of all connections some Windows
/// builds report a listener's state as 0, so filtering by state finds none.
fn listeners(pid: u32) -> Vec<u16> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    const LOOPBACK: u32 = 0x7f00_0001;
    let mut size = 0;
    // SAFETY: a null buffer asks for the size only.
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if size < std::mem::size_of::<u32>() as u32 {
        return Vec::new();
    }
    // Listeners can appear between the two calls: leave room for a few more.
    size += 16 * std::mem::size_of::<MIB_TCPROW_OWNER_PID>() as u32;
    let mut buffer = vec![0_u32; size.div_ceil(4) as usize];
    // SAFETY: the buffer holds `size` bytes, aligned for the table.
    let read = unsafe {
        GetExtendedTcpTable(
            buffer.as_mut_ptr().cast(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if read != 0 {
        return Vec::new();
    }
    // SAFETY: the call filled the buffer with a table of `dwNumEntries` rows.
    let table = unsafe { &*buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>() };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    rows.iter()
        .filter(|row| u32::from_be(row.dwLocalAddr) == LOOPBACK && row.dwOwningPid == pid)
        .map(|row| u16::from_be(row.dwLocalPort as u16))
        .collect()
}

struct Session {
    socket: Socket,
    next: u64,
    until: Instant,
}

impl Session {
    fn connect(url: &str, until: Instant) -> Option<Self> {
        let timeout = until.checked_duration_since(Instant::now())?;
        Some(Self {
            socket: Socket::connect(url, timeout)?,
            next: 0,
            until,
        })
    }

    /// One protocol call; `None` when it fails, errors or times out.
    fn call(&mut self, method: &str, params: Value) -> Option<Value> {
        self.next += 1;
        let id = self.next;
        let message = json!({ "id": id, "method": method, "params": params }).to_string();
        if !self.socket.send_text(&message) {
            return None;
        }
        loop {
            let reply: Value = serde_json::from_str(&self.socket.read_text(self.until)?).ok()?;
            if reply["id"].as_u64() == Some(id) {
                return reply.get("result").cloned();
            }
        }
    }

    fn object(&mut self, expression: &str) -> Option<String> {
        let result = self.call(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": false }),
        )?;
        result["result"]["objectId"].as_str().map(str::to_owned)
    }
}

/// A minimal WebSocket client for a DevTools endpoint on the loopback
/// interface: text messages only, no extensions, no TLS.
struct Socket {
    stream: TcpStream,
    mask: u32,
}

const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_HANDSHAKE_BYTES: usize = 8 * 1024;
/// Any 16 bytes, base64: the answer is not checked, the peer being a
/// loopback DevTools endpoint probed over HTTP just before.
const HANDSHAKE_KEY: &str = "YWdlbnQtZGVza3RvcC1jZA==";
const TEXT: u8 = 0x1;
const CONTINUATION: u8 = 0x0;
const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;
const PONG: u8 = 0xA;

impl Socket {
    /// Connects to `ws://127.0.0.1:PORT/PATH`. Any other host is refused.
    fn connect(url: &str, timeout: Duration) -> Option<Self> {
        let (authority, path) = url.strip_prefix("ws://")?.split_once('/')?;
        let (host, port) = authority.rsplit_once(':')?;
        if host != "127.0.0.1" && host != "localhost" {
            return None;
        }
        let port: u16 = port.parse().ok()?;
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = TcpStream::connect_timeout(&address, timeout).ok()?;
        stream.set_read_timeout(Some(timeout)).ok()?;
        stream.set_write_timeout(Some(timeout)).ok()?;
        let _ = stream.set_nodelay(true);
        let request = format!(
            "GET /{path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: {HANDSHAKE_KEY}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).ok()?;
        let mut head = Vec::new();
        let mut byte = [0_u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > MAX_HANDSHAKE_BYTES {
                return None;
            }
            stream.read_exact(&mut byte).ok()?;
            head.push(byte[0]);
        }
        if !head.starts_with(b"HTTP/1.1 101") {
            return None;
        }
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x9E37_79B9, |since| since.subsec_nanos());
        Some(Self {
            stream,
            mask: seed | 1,
        })
    }

    fn send_text(&mut self, text: &str) -> bool {
        self.send_frame(TEXT, text.as_bytes())
    }

    /// The next whole text message, or `None` when the peer closes, sends
    /// something malformed, or nothing arrives before `until`.
    fn read_text(&mut self, until: Instant) -> Option<String> {
        let mut message = Vec::new();
        loop {
            let remaining = until.checked_duration_since(Instant::now())?;
            if remaining.is_zero() {
                return None;
            }
            self.stream.set_read_timeout(Some(remaining)).ok()?;
            let mut head = [0_u8; 2];
            self.stream.read_exact(&mut head).ok()?;
            let finished = head[0] & 0x80 != 0;
            let opcode = head[0] & 0x0F;
            let masked = head[1] & 0x80 != 0;
            let length = match head[1] & 0x7F {
                126 => {
                    let mut bytes = [0_u8; 2];
                    self.stream.read_exact(&mut bytes).ok()?;
                    u64::from(u16::from_be_bytes(bytes))
                }
                127 => {
                    let mut bytes = [0_u8; 8];
                    self.stream.read_exact(&mut bytes).ok()?;
                    u64::from_be_bytes(bytes)
                }
                short => u64::from(short),
            };
            let length = usize::try_from(length).ok()?;
            if length > MAX_MESSAGE_BYTES || message.len() + length > MAX_MESSAGE_BYTES {
                return None;
            }
            let mut key = [0_u8; 4];
            if masked {
                self.stream.read_exact(&mut key).ok()?;
            }
            let mut payload = vec![0_u8; length];
            self.stream.read_exact(&mut payload).ok()?;
            if masked {
                payload
                    .iter_mut()
                    .enumerate()
                    .for_each(|(index, byte)| *byte ^= key[index % 4]);
            }
            match opcode {
                TEXT | CONTINUATION => {
                    message.extend_from_slice(&payload);
                    if finished {
                        return String::from_utf8(message).ok();
                    }
                }
                CLOSE => return None,
                PING => {
                    if !self.send_frame(PONG, &payload) {
                        return None;
                    }
                }
                _ => {}
            }
        }
    }

    /// A client frame is always masked (RFC 6455, section 5.3).
    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> bool {
        let mut frame = Vec::with_capacity(payload.len() + 14);
        frame.push(0x80 | opcode);
        match payload.len() {
            length if length < 126 => frame.push(0x80 | length as u8),
            length if length <= usize::from(u16::MAX) => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(length as u16).to_be_bytes());
            }
            length => {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(length as u64).to_be_bytes());
            }
        }
        self.mask ^= self.mask << 13;
        self.mask ^= self.mask >> 17;
        self.mask ^= self.mask << 5;
        let key = self.mask.to_be_bytes();
        frame.extend_from_slice(&key);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ key[index % 4]),
        );
        self.stream.write_all(&frame).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_is_found_by_address_then_by_title() {
        let targets = json!([
            { "type": "page", "url": "http://a/x#1", "title": "A", "webSocketDebuggerUrl": "ws://127.0.0.1:1/a" },
            { "type": "page", "url": "http://b/", "title": "B", "webSocketDebuggerUrl": "ws://127.0.0.1:1/b" },
            { "type": "service_worker", "url": "http://a/x", "webSocketDebuggerUrl": "ws://127.0.0.1:1/w" },
        ]);
        let by_url = PageQuery {
            url: Some("http://a/x"),
            title: None,
        };
        assert_eq!(
            matching_pages(&targets, &by_url),
            vec!["ws://127.0.0.1:1/a"]
        );
        let by_title = PageQuery {
            url: Some("http://gone/"),
            title: Some("B"),
        };
        assert_eq!(
            matching_pages(&targets, &by_title),
            vec!["ws://127.0.0.1:1/b"]
        );
        let neither = PageQuery {
            url: None,
            title: None,
        };
        assert!(matching_pages(&targets, &neither).is_empty());
    }
}
