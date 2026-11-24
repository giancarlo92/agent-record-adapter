// Part of Agent Record Adapter (see NOTICE).
//! The proxy against a fake Chrome: a local server that serves discovery
//! and echoes WebSocket commands with an answer, so nothing real is driven.
use super::*;
use std::sync::Mutex as StdMutex;

/// A fake Chrome on a free port. It answers `/json/version` naming its own
/// port, and answers every WebSocket command with `{"id", "result":
/// {"echo": method}}`; every command it received is kept.
struct FakeChrome {
    port: u16,
    received: Arc<StdMutex<Vec<Value>>>,
}

fn fake_chrome(runtime: &tokio::runtime::Runtime) -> FakeChrome {
    let listener = runtime
        .block_on(TcpListener::bind(("127.0.0.1", 0)))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(StdMutex::new(Vec::new()));
    let seen = received.clone();
    runtime.spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let seen = seen.clone();
            tokio::spawn(async move {
                let head = peek_head(&stream).await.unwrap_or_default();
                if is_websocket_upgrade(&head) {
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    while let Some(Ok(Message::Text(text))) = socket.next().await {
                        let command: Value = serde_json::from_str(text.as_str()).unwrap();
                        seen.lock().unwrap().push(command.clone());
                        let mut answer = json!({ "id": command["id"], "result": {
                            "echo": command["method"] } });
                        if command["method"] == "Runtime.evaluate" {
                            answer["result"] = json!({ "result": { "type": "object",
                                "value": { "tag": "input", "type": "password", "secure": true,
                                           "url": "file:///login.html" } } });
                        }
                        if let Some(session) = command.get("sessionId") {
                            answer["sessionId"] = session.clone();
                        }
                        socket.send(Message::text(answer.to_string())).await.unwrap();
                    }
                } else {
                    let mut stream = stream;
                    let _ = read_head(&mut stream).await;
                    let body = format!(
                        r#"{{"Browser":"Fake/1","webSocketDebuggerUrl":"ws://127.0.0.1:{port}/devtools/browser/fake"}}"#
                    );
                    let _ = respond(&mut stream, "200 OK", "application/json", body.as_bytes()).await;
                }
            });
        }
    });
    FakeChrome { port, received }
}

fn proxy_for(chrome: &FakeChrome, inbox: InboxSource) -> RunningProxy {
    spawn(
        0,
        ProxyConfig {
            chrome_port: chrome.port,
            inbox,
            exit_with_chrome: false,
            browser_ws_path: None,
        },
    )
    .unwrap()
}

async fn ask(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    command: Value,
) -> Value {
    socket
        .send(Message::text(command.to_string()))
        .await
        .unwrap();
    loop {
        if let Some(Ok(Message::Text(text))) = socket.next().await {
            return serde_json::from_str(text.as_str()).unwrap();
        }
    }
}

#[test]
fn discovery_through_the_proxy_names_the_proxy() {
    let runtime = runtime().unwrap();
    let chrome = fake_chrome(&runtime);
    let proxy = proxy_for(&chrome, InboxSource::None);
    let body =
        crate::chrome::http_get(proxy.port, "/json/version", Duration::from_secs(3)).unwrap();
    assert!(
        body.contains(&format!(
            "ws://127.0.0.1:{}/devtools/browser/fake",
            proxy.port
        )),
        "{body}"
    );
    assert_eq!(probe_running(proxy.port), Some(chrome.port));
    assert_eq!(probe_running(chrome.port), None);
}

#[test]
fn bring_to_front_never_reaches_chrome_and_the_rest_does() {
    let runtime = runtime().unwrap();
    let chrome = fake_chrome(&runtime);
    let proxy = proxy_for(&chrome, InboxSource::None);
    let url = format!("ws://127.0.0.1:{}/devtools/browser/fake", proxy.port);
    runtime.block_on(async {
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let answer = ask(
            &mut socket,
            json!({"id":1,"method":"Page.bringToFront","sessionId":"S"}),
        )
        .await;
        assert_eq!(answer, json!({"id":1,"result":{},"sessionId":"S"}));
        let answer = ask(
            &mut socket,
            json!({"id":2,"method":"Target.activateTarget","params":{"targetId":"T"}}),
        )
        .await;
        assert_eq!(answer, json!({"id":2,"result":{}}));
        let answer = ask(
            &mut socket,
            json!({"id":3,"method":"Target.createTarget","params":{"url":"about:blank"}}),
        )
        .await;
        assert_eq!(answer["result"]["echo"], json!("Target.createTarget"));
    });
    let received = chrome.received.lock().unwrap().clone();
    let methods: Vec<&str> = received
        .iter()
        .filter_map(|c| c["method"].as_str())
        .collect();
    // Instead of bringing the page forward, the proxy makes it behave as
    // focused (once, for the bringToFront that named session S).
    assert_eq!(
        methods,
        vec!["Emulation.setFocusEmulationEnabled", "Target.createTarget"]
    );
    assert_eq!(received[0]["sessionId"], json!("S"));
    assert!(received[0]["id"].as_i64().unwrap() >= PROBE_ID_BASE);
    assert_eq!(received[1]["params"]["background"], json!(true));
}

#[test]
fn a_recording_gets_the_agent_action_without_the_secret() {
    let dir = std::env::temp_dir().join(format!("agent-record-proxy-inbox-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let inbox_file = dir.join("rec.agent.jsonl");
    let runtime = runtime().unwrap();
    let chrome = fake_chrome(&runtime);
    let proxy = proxy_for(&chrome, InboxSource::Files(vec![inbox_file.clone()]));
    let url = format!("ws://127.0.0.1:{}/devtools/browser/fake", proxy.port);
    runtime.block_on(async {
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let answer = ask(
            &mut socket,
            json!({"id":5,"method":"Input.insertText","sessionId":"S","params":{"text":"hunter2"}}),
        )
        .await;
        // The probe's answer is the proxy's; agent-browser sees only its own.
        assert_eq!(answer["id"], json!(5));
        assert_eq!(answer["result"]["echo"], json!("Input.insertText"));
    });
    let text = std::fs::read_to_string(&inbox_file).unwrap();
    assert!(!text.contains("hunter2"), "{text}");
    let line: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(line["kind"], json!("type"));
    assert_eq!(line["fields"]["secure"], json!(true));
    assert_eq!(line["fields"]["by"], json!("agent"));
    assert_eq!(line["fields"]["driver"], json!("browser"));
    assert_eq!(line["fields"]["url"], json!("file:///login.html"));
    let received = chrome.received.lock().unwrap().clone();
    assert_eq!(received[0]["method"], json!("Runtime.evaluate"));
    assert!(received[0]["id"].as_i64().unwrap() >= PROBE_ID_BASE);
    assert_eq!(received[1]["method"], json!("Input.insertText"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn websocket_upgrade_is_told_from_plain_http() {
    assert!(is_websocket_upgrade(
        "GET /devtools/browser/x HTTP/1.1\r\nHost: a\r\nUpgrade: websocket\r\nConnection: Upgrade"
    ));
    assert!(!is_websocket_upgrade(
        "GET /json/version HTTP/1.1\r\nHost: a"
    ));
}
