// Part of Agent Record Adapter (see NOTICE).
//! `agent-record mcp`: one MCP server made of three.
//!
//! - the engine's own MCP server (`agent-desktop mcp`: upstream's server, its
//!   `desktop_*` tools run with our background adapter);
//! - agent-browser's own MCP server (`agent_browser_*` tools), attached to the
//!   dedicated background Chrome through our DevTools proxy;
//! - our tools (`desktop_record_start`, `desktop_secret_fill`, `desktop_menu`,
//!   ...), each run as `agent-record <command>`.
//!
//! The two upstream servers run as child processes and are spoken to over
//! their stdio exactly as an MCP client would; requests are renumbered per
//! child and their answers mapped back, so neither upstream changes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::catalog;

const INSTRUCTIONS: &str = "Agent Record Adapter: desktop apps through the desktop_* tools (agent-desktop, with a background-first layer: actions do not take the person's mouse, keyboard or foreground), web pages through the agent_browser_* tools (agent-browser, in a dedicated Chrome that stays in the background). Pages in the person's own browser without DevTools are desktop windows: use desktop_snapshot/desktop_click there, and desktop_navigate to change its address. Extra tools: desktop_menu (a menu bar command without opening the menu), desktop_click_at (a point of a window), desktop_stroke, desktop_secret_list/desktop_secret_fill (passwords by name, never their value), desktop_record_start/stop/status (record a person's or your own actions to a JSON Lines file). Read desktop_skills_get with name \"background\" first, then \"recording\", \"recording-to-script\" or \"script-repair\" when the task is about recordings or scripts.";
const CHILD_TIMEOUT: Duration = Duration::from_secs(60);
/// Commands an MCP client must not run: a person types these at a terminal.
const NOT_TOOLS: &[&str] = &[
    "desktop_secret_set",
    "desktop_secret_delete",
    "desktop_record",
];

type Out = Arc<Mutex<std::io::Stdout>>;

struct Upstream {
    name: &'static str,
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    next_id: AtomicU64,
    _child: Mutex<Child>,
}

impl Upstream {
    fn spawn(name: &'static str, mut command: Command, out: Out) -> Result<Arc<Self>, String> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command
            .spawn()
            .map_err(|error| format!("Cannot start the {name} MCP server: {error}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let upstream = Arc::new(Self {
            name,
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            _child: Mutex::new(child),
        });
        let reader = Arc::clone(&upstream);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let is_response = message.get("method").is_none()
                    && (message.get("result").is_some() || message.get("error").is_some());
                if is_response && let Some(id) = message["id"].as_u64() {
                    let waiter = reader
                        .pending
                        .lock()
                        .ok()
                        .and_then(|mut map| map.remove(&id));
                    if let Some(waiter) = waiter {
                        let _ = waiter.send(message);
                    }
                } else if message.get("id").is_none() {
                    // A notification (progress, list changed): passed on.
                    write_line(&out, &message);
                }
            }
            // The child ended: every waiter gets an error.
            if let Ok(mut map) = reader.pending.lock() {
                for (_, waiter) in map.drain() {
                    let _ = waiter
                        .send(json!({ "error": { "code": -32603, "message": "server ended" } }));
                }
            }
        });
        Ok(upstream)
    }

    fn send(&self, message: &Value) {
        if let Ok(mut stdin) = self.stdin.lock() {
            let _ = serde_json::to_writer(&mut *stdin, message);
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
    }

    /// Sends a request and waits for its answer (`result` or `error`).
    fn request(&self, method: &str, params: &Value) -> Result<Value, Value> {
        let (id, answer) = self.start(method, params);
        let message = answer
            .recv_timeout(if method == "tools/call" { Duration::from_secs(3600) } else { CHILD_TIMEOUT })
            .map_err(|_| json!({ "code": -32603, "message": format!("{} did not answer {method}", self.name) }));
        if let Ok(mut map) = self.pending.lock() {
            map.remove(&id);
        }
        let message = message?;
        match message.get("error") {
            Some(error) => Err(error.clone()),
            None => Ok(message["result"].clone()),
        }
    }

    fn start(&self, method: &str, params: &Value) -> (u64, std::sync::mpsc::Receiver<Value>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        if let Ok(mut map) = self.pending.lock() {
            map.insert(id, tx);
        }
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        (id, rx)
    }
}

/// The name of agent-browser's upstream server.
const BROWSER: &str = "agent-browser";

struct Server {
    /// The dedicated Chrome and proxy of agent-browser's server, started at
    /// its first tool call that needs a page.
    browser: Option<agent_record_browser::LazyBrowser>,
    upstreams: Vec<Arc<Upstream>>,
    ours: Vec<catalog::Tool>,
    globals: Vec<String>,
    routes: Mutex<HashMap<String, usize>>,
    notes: Vec<String>,
}

pub(crate) fn serve(args: &[String]) -> ExitCode {
    let out: Out = Arc::new(Mutex::new(std::io::stdout()));
    let globals: Vec<String> = args
        .iter()
        .filter(|arg| arg.as_str() != "mcp")
        .cloned()
        .collect();
    let mut upstreams = Vec::new();
    let mut notes = Vec::new();
    let mut engine_args = globals.clone();
    engine_args.push("mcp".into());
    match crate::engine::command(&engine_args)
        .and_then(|command| Upstream::spawn("agent-desktop", command, Arc::clone(&out)))
    {
        Ok(upstream) => upstreams.push(upstream),
        Err(message) => notes.push(message),
    }
    // agent-browser's server starts now, its browser only at the first web
    // tool call: an agent that never opens a page never sees a Chrome.
    let mut browser = None;
    match agent_record_browser::mcp_command().and_then(|(command, lazy)| {
        Upstream::spawn(BROWSER, command, Arc::clone(&out)).map(|upstream| (upstream, lazy))
    }) {
        Ok((upstream, lazy)) => {
            upstreams.push(upstream);
            browser = Some(lazy);
        }
        Err(message) => notes.push(format!("Web page tools are not available: {message}")),
    }
    let server = Arc::new(Server {
        browser,
        upstreams,
        ours: catalog::tools()
            .into_iter()
            .filter(|tool| !NOT_TOOLS.contains(&tool.name.as_str()))
            .collect(),
        globals,
        routes: Mutex::new(HashMap::new()),
        notes,
    });

    let stdin = std::io::stdin();
    let mut running = Vec::new();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let message = match serde_json::from_str::<Value>(&line) {
            Ok(message) if message.is_object() => message,
            _ => {
                write_line(&out, &error_response(Value::Null, -32700, "Parse error"));
                continue;
            }
        };
        let server = Arc::clone(&server);
        let out = Arc::clone(&out);
        running.retain(|handle: &std::thread::JoinHandle<()>| !handle.is_finished());
        running.push(std::thread::spawn(move || server.handle(&out, message)));
    }
    for handle in running {
        let _ = handle.join();
    }
    ExitCode::SUCCESS
}

impl Server {
    fn handle(&self, out: &Out, message: Value) {
        let method = message["method"].as_str().unwrap_or_default().to_string();
        let Some(id) = message.get("id").cloned() else {
            // Notifications go to every upstream server.
            for upstream in &self.upstreams {
                upstream.send(&message);
            }
            return;
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let answer = match method.as_str() {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.list() })),
            "tools/call" => self.call(&params),
            _ => match self.upstreams.first() {
                Some(engine) => engine.request(&method, &params),
                None => Err(json!({ "code": -32601, "message": "Method not found" })),
            },
        };
        let response = match answer {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        };
        write_line(out, &response);
    }

    fn initialize(&self, params: &Value) -> Value {
        let mut instructions = vec![INSTRUCTIONS.to_string()];
        let mut protocol = params["protocolVersion"].clone();
        for upstream in &self.upstreams {
            if let Ok(result) = upstream.request("initialize", params) {
                if upstream.name == "agent-desktop" {
                    protocol = result["protocolVersion"].clone();
                }
                if let Some(text) = result["instructions"].as_str() {
                    instructions.push(format!("[{}] {text}", upstream.name));
                }
            }
        }
        instructions.extend(self.notes.iter().cloned());
        json!({
            "protocolVersion": protocol,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "desktop-recorder", "version": env!("CARGO_PKG_VERSION") },
            "instructions": instructions.join("\n\n"),
        })
    }

    fn list(&self) -> Vec<Value> {
        let mut tools = Vec::new();
        let mut routes = HashMap::new();
        let ours: Vec<&str> = self.ours.iter().map(|tool| tool.name.as_str()).collect();
        for (index, upstream) in self.upstreams.iter().enumerate() {
            let Ok(result) = upstream.request("tools/list", &json!({})) else {
                continue;
            };
            for tool in result["tools"].as_array().cloned().unwrap_or_default() {
                let Some(name) = tool["name"].as_str() else {
                    continue;
                };
                // Ours wins over an upstream tool of the same name.
                if ours.contains(&name) {
                    continue;
                }
                routes.insert(name.to_string(), index);
                tools.push(tool);
            }
        }
        tools.extend(self.ours.iter().map(catalog::Tool::definition));
        if let Ok(mut table) = self.routes.lock() {
            *table = routes;
        }
        tools
    }

    fn call(&self, params: &Value) -> Result<Value, Value> {
        let name = params["name"].as_str().unwrap_or_default().to_string();
        if let Some(tool) = self.ours.iter().find(|tool| tool.name == name) {
            let arguments = params["arguments"].as_object().cloned().unwrap_or_default();
            return Ok(self.call_ours(tool, &arguments));
        }
        // The skills tools serve ours too, as the CLI's `skills` does.
        if name == "desktop_skills_get" || name == "desktop_skills_list" {
            let arguments = params["arguments"].as_object().cloned().unwrap_or_default();
            return Ok(self.call_skills(&name, &arguments));
        }
        let route = |server: &Self| {
            server
                .routes
                .lock()
                .ok()
                .and_then(|table| table.get(&name).copied())
        };
        let index = match route(self) {
            Some(index) => Some(index),
            None => {
                self.list();
                route(self)
            }
        };
        match index.and_then(|index| self.upstreams.get(index)) {
            Some(upstream) => {
                if upstream.name == BROWSER
                    && agent_record_browser::tool_needs_browser(&name)
                    && let Some(browser) = &self.browser
                    && let Err(message) = browser.ensure()
                {
                    return Ok(tool_error(&format!(
                        "The browser could not start: {message}"
                    )));
                }
                upstream.request("tools/call", params)
            }
            None => Ok(tool_error(&format!("Unknown tool '{name}'"))),
        }
    }

    fn call_skills(&self, name: &str, arguments: &Map<String, Value>) -> Value {
        let mut argv = vec!["skills".to_string()];
        if name == "desktop_skills_list" {
            argv.push("list".into());
        } else {
            argv.push("get".into());
            let Some(skill) = arguments.get("name").and_then(Value::as_str) else {
                return tool_error("desktop_skills_get needs name");
            };
            argv.push(skill.to_string());
            if let Some(reference) = arguments.get("reference").and_then(Value::as_str) {
                argv.push(reference.to_string());
            }
            if arguments.get("full").and_then(Value::as_bool) == Some(true) {
                argv.push("--full".into());
            }
        }
        self.run_self(&argv, &argv.join(" "))
    }

    fn call_ours(&self, tool: &catalog::Tool, arguments: &Map<String, Value>) -> Value {
        let argv = match tool.argv(arguments) {
            Ok(argv) => argv,
            Err(message) => return tool_error(&message),
        };
        self.run_self(&argv, &tool.path.join(" "))
    }

    /// Runs `agent-record <argv>` with the server's global flags and maps its
    /// JSON envelope to a tool result.
    fn run_self(&self, argv: &[String], what: &str) -> Value {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => return tool_error(&format!("Cannot locate agent-record: {error}")),
        };
        let output = Command::new(exe)
            .args(&self.globals)
            .args(argv)
            .stdin(Stdio::null())
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => return tool_error(&format!("Cannot run agent-record: {error}")),
        };
        match serde_json::from_slice::<Value>(&output.stdout) {
            Ok(envelope) => {
                let failed = envelope["ok"] != Value::Bool(true);
                json!({
                    "content": [{ "type": "text", "text": envelope.to_string() }],
                    "structuredContent": envelope,
                    "isError": failed,
                })
            }
            Err(_) => tool_error(&format!(
                "agent-record {} exited with {} and no JSON envelope: {}",
                what,
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )),
        }
    }
}

fn tool_error(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn write_line(out: &Mutex<std::io::Stdout>, message: &Value) {
    if let Ok(out) = out.lock() {
        let mut lock = out.lock();
        let _ = serde_json::to_writer(&mut lock, message);
        let _ = lock.write_all(b"\n");
        let _ = lock.flush();
    }
}
