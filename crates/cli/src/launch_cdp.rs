// Part of Agent Record Adapter (see NOTICE).
//! `launch <browser> --cdp [PORT]` for a browser given a profile of its own
//! (`--arg=--user-data-dir=...`). Upstream refuses `--cdp` while any instance
//! of that browser runs (the person's own Chrome, say), although a profile of
//! its own makes a separate instance with its own DevTools port. So the port
//! switches are passed as plain arguments, the browser is started by our
//! adapter as a separate instance, and the endpoint is verified here and
//! reported in upstream's shape (`data.cdp`).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Whether these `launch` arguments ask for `--cdp` with a profile of its own.
pub(crate) fn applies(args: &[String]) -> bool {
    crate::first_command(args) == Some("launch")
        && args
            .iter()
            .any(|arg| arg == "--cdp" || arg.starts_with("--cdp="))
        && args.iter().any(|arg| arg.contains("--user-data-dir"))
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    let mut rest = Vec::new();
    let mut port: Option<u16> = None;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--no-attach" {
            continue;
        }
        if let Some(value) = arg.strip_prefix("--cdp=") {
            port = value.parse().ok();
            continue;
        }
        if arg == "--cdp" {
            if let Some(next) = iter.peek()
                && let Ok(value) = next.parse::<u16>()
            {
                port = Some(value);
                iter.next();
            }
            continue;
        }
        rest.push(arg.clone());
    }
    let port = match port.filter(|port| *port != 0) {
        Some(port) => port,
        None => match free_port() {
            Some(port) => port,
            None => return fail("No free port for the DevTools endpoint"),
        },
    };
    rest.push(format!("--arg=--remote-debugging-port={port}"));
    rest.push("--arg=--remote-debugging-address=127.0.0.1".to_string());
    let mut envelope = match crate::engine::envelope(&rest) {
        Ok(envelope) => envelope,
        Err(message) => return fail(&message),
    };
    if envelope["ok"] == true {
        match endpoint(port, Duration::from_secs(20)) {
            Some(cdp) => envelope["data"]["cdp"] = cdp,
            None => {
                return fail(&format!(
                    "The browser started, but its DevTools endpoint on port {port} did not answer"
                ));
            }
        }
    }
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, &envelope);
    let _ = out.write_all(b"\n");
    if envelope["ok"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fail(message: &str) -> ExitCode {
    crate::run::emit(
        "launch",
        Err(agent_desktop_core::AppError::Internal(message.to_string())),
    )
}

fn free_port() -> Option<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    listener.local_addr().ok().map(|address| address.port())
}

/// `/json/version` of the endpoint, as upstream reports it.
fn endpoint(port: u16, wait: Duration) -> Option<Value> {
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if let Some(version) = json_version(port) {
            return Some(json!({
                "port": port,
                "http_endpoint": format!("http://127.0.0.1:{port}"),
                "websocket_url": version["webSocketDebuggerUrl"],
                "product": version["Browser"],
            }));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    None
}

fn json_version(port: u16) -> Option<Value> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .write_all(b"GET /json/version HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .ok()?;
    // Chrome may keep the connection open: read until the body is whole or
    // the read times out, then parse what arrived.
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => bytes.extend_from_slice(&buffer[..read]),
        }
        let text = String::from_utf8_lossy(&bytes);
        if let Some(body) = text.split("\r\n\r\n").nth(1)
            && serde_json::from_str::<Value>(body).is_ok()
        {
            break;
        }
    }
    let text = String::from_utf8_lossy(&bytes);
    let body = text.split("\r\n\r\n").nth(1)?;
    serde_json::from_str(body).ok()
}
