// Part of Agent Record Adapter (see NOTICE).
//! The proxy against a real headless Chrome with a throwaway profile, on a
//! local test page. Needs Chrome or Edge installed, so it is ignored by
//! default: `cargo test -p agent-record-browser -- --ignored`.
//!
//! `AGENT_RECORD_TEST_FORMS` points at the test pages folder (signup-v1.html);
//! without it the Aprende Replica fixture next to this repository is used.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_record_browser::{InboxSource, ProxyConfig, chrome, proxy};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Ends the test's Chrome with all its processes, even when the test fails
/// (its children would otherwise keep running and hold the test's output).
struct ChromeTree(std::process::Child);

impl Drop for ChromeTree {
    fn drop(&mut self) {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &self.0.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.0.wait();
    }
}

struct Cdp {
    socket: Socket,
    next: i64,
}

impl Cdp {
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Value {
        self.next += 1;
        let id = self.next;
        let mut command = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            command["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::text(command.to_string()))
            .await
            .unwrap();
        loop {
            let Some(Ok(Message::Text(text))) = self.socket.next().await else {
                panic!("socket closed waiting for {method}");
            };
            let value: Value = serde_json::from_str(text.as_str()).unwrap();
            if value["id"] == id {
                return value;
            }
        }
    }

    async fn eval(&mut self, expression: &str, session: &str) -> Value {
        self.call(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true }),
            Some(session),
        )
        .await["result"]["result"]["value"]
            .clone()
    }
}

fn forms_dir() -> PathBuf {
    std::env::var_os("AGENT_RECORD_TEST_FORMS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(r"..\..\..\Proyecto Aprende Replica\app\testdata\forms")
        })
}

fn file_url(path: &std::path::Path) -> String {
    let path = path.canonicalize().unwrap();
    let text = path.display().to_string();
    let text = text.trim_start_matches(r"\\?\").replace('\\', "/");
    format!("file:///{text}")
}

#[test]
#[ignore = "starts a real headless Chrome"]
fn real_chrome_through_the_proxy() {
    let chrome_exe = chrome::find_chrome().expect("Chrome or Edge installed");
    let page = forms_dir().join("signup-v1.html");
    assert!(page.is_file(), "missing test page {}", page.display());
    let scratch = std::env::temp_dir().join(format!("agent-record-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let profile = scratch.join("profile");
    let inbox = scratch.join("live.agent.jsonl");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let browser = ChromeTree(
        Command::new(chrome_exe)
            .args(chrome::launch_args(port, &profile, true))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let end = Instant::now() + Duration::from_secs(20);
    while chrome::version(port).is_none() {
        assert!(Instant::now() < end, "Chrome never answered");
        std::thread::sleep(Duration::from_millis(100));
    }
    let running = proxy::spawn(
        0,
        ProxyConfig {
            chrome_port: port,
            inbox: InboxSource::Files(vec![inbox.clone()]),
            exit_with_chrome: false,
            browser_ws_path: None,
        },
    )
    .unwrap();
    let version = chrome::version(running.port).expect("discovery through the proxy");
    let ws = version["webSocketDebuggerUrl"].as_str().unwrap().to_owned();
    assert!(ws.contains(&format!("127.0.0.1:{}", running.port)), "{ws}");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (socket, _) = tokio_tungstenite::connect_async(ws).await.unwrap();
        let mut cdp = Cdp { socket, next: 0 };
        let created = cdp
            .call("Target.createTarget", json!({ "url": file_url(&page) }), None)
            .await;
        let target = created["result"]["targetId"].as_str().unwrap().to_owned();
        let attached = cdp
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target, "flatten": true }),
                None,
            )
            .await;
        let session = attached["result"]["sessionId"].as_str().unwrap().to_owned();
        let end = Instant::now() + Duration::from_secs(10);
        while cdp.eval("!!document.getElementById('fullName') && document.readyState === 'complete'", &session).await != json!(true) {
            assert!(Instant::now() < end, "page never loaded");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Bringing the tab to the front is answered by the proxy itself.
        let front = cdp.call("Page.bringToFront", json!({}), Some(&session)).await;
        assert_eq!(front["result"], json!({}));

        // A plain field: its text is noted.
        cdp.eval("document.getElementById('fullName').focus()", &session).await;
        cdp.call("Input.insertText", json!({ "text": "Ana Live" }), Some(&session))
            .await;
        let typed = cdp
            .eval("document.getElementById('fullName').value", &session)
            .await;
        assert_eq!(typed, json!("Ana Live"));

        // A click on the username field: noted with the element under it.
        let rect = cdp
            .eval(
                "(() => { const e = document.getElementById('username'); e.scrollIntoView({block: 'center'}); \
                 const r = e.getBoundingClientRect(); \
                 return [r.left + r.width / 2, r.top + r.height / 2]; })()",
                &session,
            )
            .await;
        let (x, y) = (rect[0].as_f64().unwrap(), rect[1].as_f64().unwrap());
        for kind in ["mousePressed", "mouseReleased"] {
            cdp.call(
                "Input.dispatchMouseEvent",
                json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }),
                Some(&session),
            )
            .await;
        }

        // A password field (a local data: page): its text is never noted.
        cdp.call(
            "Page.navigate",
            json!({ "url": "data:text/html,<input id=p type=password autofocus>" }),
            Some(&session),
        )
        .await;
        let end = Instant::now() + Duration::from_secs(10);
        while cdp.eval("!!document.getElementById('p')", &session).await != json!(true) {
            assert!(Instant::now() < end, "data page never loaded");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        cdp.eval("document.getElementById('p').focus()", &session).await;
        cdp.call("Input.insertText", json!({ "text": "hunter2-live" }), Some(&session))
            .await;
        let _ = cdp.call("Target.closeTarget", json!({ "targetId": target }), None).await;
    });
    drop(browser);

    let text = std::fs::read_to_string(&inbox).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(!text.contains("hunter2-live"), "{text}");
    let typed = lines
        .iter()
        .find(|l| l["fields"]["typed"] == json!("Ana Live"))
        .unwrap_or_else(|| panic!("no typed line: {text}"));
    assert_eq!(typed["fields"]["element"]["id"], json!("fullName"));
    assert_eq!(typed["fields"]["by"], json!("agent"));
    assert_eq!(typed["fields"]["driver"], json!("browser"));
    let click = lines
        .iter()
        .find(|l| l["kind"] == json!("click"))
        .unwrap_or_else(|| panic!("no click line: {text}"));
    assert_eq!(
        click["fields"]["element"]["id"],
        json!("username"),
        "{click}"
    );
    assert!(
        lines.iter().any(|l| l["kind"] == json!("navigate")),
        "no navigate line: {text}"
    );
    let secret = lines
        .iter()
        .find(|l| l["fields"]["secure"] == json!(true))
        .unwrap_or_else(|| panic!("no secure line: {text}"));
    assert_eq!(secret["kind"], json!("type"));
}
