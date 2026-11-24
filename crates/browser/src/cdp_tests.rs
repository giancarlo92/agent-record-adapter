// Part of Agent Record Adapter (see NOTICE).
use super::*;
use serde_json::json;

#[test]
fn bring_to_front_is_answered_not_forwarded() {
    let sent = r#"{"id":41,"method":"Page.bringToFront","sessionId":"S1"}"#;
    let ClientIntercept::Answer(answer) = intercept_client_message(sent) else {
        panic!("forwarded");
    };
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer, json!({"id": 41, "result": {}, "sessionId": "S1"}));
}

#[test]
fn activate_target_is_answered_without_session() {
    let sent = r#"{"id":7,"method":"Target.activateTarget","params":{"targetId":"T"}}"#;
    let ClientIntercept::Answer(answer) = intercept_client_message(sent) else {
        panic!("forwarded");
    };
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer, json!({"id": 7, "result": {}}));
}

#[test]
fn other_commands_pass_unchanged() {
    let sent = r#"{"id":3,"method":"Runtime.evaluate","params":{"expression":"1"}}"#;
    assert_eq!(
        intercept_client_message(sent),
        ClientIntercept::Forward(sent.to_owned())
    );
    assert_eq!(
        intercept_client_message("not json"),
        ClientIntercept::Forward("not json".to_owned())
    );
}

#[test]
fn new_targets_open_in_the_background() {
    let sent = r#"{"id":9,"method":"Target.createTarget","params":{"url":"about:blank"}}"#;
    let ClientIntercept::Forward(text) = intercept_client_message(sent) else {
        panic!("answered");
    };
    let sent: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(sent["params"]["background"], json!(true));
    assert_eq!(sent["params"]["url"], json!("about:blank"));
    let bare = r#"{"id":10,"method":"Target.createTarget"}"#;
    let ClientIntercept::Forward(text) = intercept_client_message(bare) else {
        panic!("answered");
    };
    assert!(text.contains(r#""background":true"#));
}

#[test]
fn discovery_points_at_the_proxy() {
    let body = r#"{"webSocketDebuggerUrl":"ws://127.0.0.1:9333/devtools/browser/abc",
        "devtoolsFrontendUrl":"/devtools/inspector.html?ws=localhost:9333/devtools/page/X"}"#;
    let rewritten = rewrite_discovery_body(body, 9333, 41000);
    assert!(rewritten.contains("ws://127.0.0.1:41000/devtools/browser/abc"));
    assert!(rewritten.contains("ws=127.0.0.1:41000/devtools/page/X"));
    assert!(!rewritten.contains("9333"));
}

#[test]
fn mouse_press_is_a_click_and_release_is_not() {
    let pressed = json!({"id":1,"method":"Input.dispatchMouseEvent","sessionId":"S",
        "params":{"type":"mousePressed","x":10.5,"y":20.0,"button":"left","clickCount":2}});
    let (action, session) = agent_action(&pressed).unwrap();
    assert_eq!(
        action,
        AgentAction::Click {
            x: 10.5,
            y: 20.0,
            button: "left".into(),
            count: 2
        }
    );
    assert_eq!(session.as_deref(), Some("S"));
    let released = json!({"id":2,"method":"Input.dispatchMouseEvent",
        "params":{"type":"mouseReleased","x":10.5,"y":20.0}});
    assert!(agent_action(&released).is_none());
    let moved = json!({"id":3,"method":"Input.dispatchMouseEvent",
        "params":{"type":"mouseMoved","x":1,"y":2}});
    assert!(agent_action(&moved).is_none());
}

#[test]
fn fill_through_the_value_setter_is_a_set_value() {
    let fill = json!({"id":5,"method":"Runtime.callFunctionOn","params":{
        "objectId":"O-1",
        "functionDeclaration":"function(value) { const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; }",
        "arguments":[{"value":"Ana"}]}});
    let (action, _) = agent_action(&fill).unwrap();
    assert_eq!(
        action,
        AgentAction::SetValue {
            object_id: "O-1".into(),
            value: "Ana".into()
        }
    );
    let other = json!({"id":6,"method":"Runtime.callFunctionOn","params":{
        "objectId":"O-1","functionDeclaration":"function(){return 1}"}});
    assert!(agent_action(&other).is_none());
}

#[test]
fn navigate_upload_and_insert_text() {
    let nav = json!({"id":1,"method":"Page.navigate","params":{"url":"file:///C:/forms/a.html"}});
    assert_eq!(
        agent_action(&nav).unwrap().0,
        AgentAction::Navigate {
            url: "file:///C:/forms/a.html".into()
        }
    );
    let up = json!({"id":2,"method":"DOM.setFileInputFiles","params":{"files":["C:\\a.pdf"],"backendNodeId":3}});
    assert_eq!(
        agent_action(&up).unwrap().0,
        AgentAction::Upload {
            files: vec!["C:\\a.pdf".into()]
        }
    );
    let ins = json!({"id":3,"method":"Input.insertText","params":{"text":"hola"}});
    assert_eq!(
        agent_action(&ins).unwrap().0,
        AgentAction::InsertText {
            text: "hola".into()
        }
    );
}

#[test]
fn secure_fields_never_keep_their_text() {
    let action = AgentAction::InsertText {
        text: "hunter2".into(),
    };
    let mut element = Map::new();
    element.insert("secure".into(), json!(true));
    element.insert("type".into(), json!("password"));
    element.insert("url".into(), json!("file:///login.html"));
    let line = inbox_line(&action, Some(&element), 5);
    let text = line.to_string();
    assert!(!text.contains("hunter2"), "{text}");
    assert_eq!(line["fields"]["secure"], json!(true));
    assert_eq!(line["fields"]["url"], json!("file:///login.html"));
    assert_eq!(line["kind"], json!("type"));
}

#[test]
fn an_undescribed_field_withholds_its_text() {
    let action = AgentAction::SetValue {
        object_id: "O".into(),
        value: "secret?".into(),
    };
    let line = inbox_line(&action, None, 5);
    assert!(!line.to_string().contains("secret?"));
    assert_eq!(line["fields"]["value_withheld"], json!(true));
}

#[test]
fn plain_fields_keep_their_text_and_the_agent_mark() {
    let action = AgentAction::SetValue {
        object_id: "O".into(),
        value: "Ana".into(),
    };
    let mut element = Map::new();
    element.insert("secure".into(), json!(false));
    element.insert("tag".into(), json!("input"));
    element.insert("label".into(), json!("Name"));
    let line = inbox_line(&action, Some(&element), 9);
    assert_eq!(line["fields"]["value"], json!("Ana"));
    assert_eq!(line["fields"]["source"], json!("value"));
    assert_eq!(line["fields"]["by"], json!("agent"));
    assert_eq!(line["fields"]["driver"], json!("browser"));
    assert_eq!(line["fields"]["element"]["label"], json!("Name"));
    assert_eq!(line["at_ms"], json!(9));
}

#[test]
fn keys_with_modifiers_or_no_text_are_presses() {
    let enter = AgentAction::Key {
        key: "Enter".into(),
        modifiers: 0,
        text: Some("\r".into()),
    };
    assert_eq!(inbox_line(&enter, None, 0)["kind"], json!("press"));
    let save = AgentAction::Key {
        key: "s".into(),
        modifiers: 2,
        text: Some("s".into()),
    };
    let line = inbox_line(&save, None, 0);
    assert_eq!(line["kind"], json!("press"));
    assert_eq!(line["fields"]["keys"], json!("ctrl+s"));
    let capital = AgentAction::Key {
        key: "A".into(),
        modifiers: 8,
        text: Some("A".into()),
    };
    assert_eq!(inbox_line(&capital, None, 0)["kind"], json!("type"));
}

#[test]
fn probes_carry_their_own_id_and_the_session() {
    let click = AgentAction::Click {
        x: 1.0,
        y: 2.0,
        button: "left".into(),
        count: 1,
    };
    let probe = probe_for(&click, Some("S9"), 2_000_000_001).unwrap();
    assert_eq!(probe["id"], json!(2_000_000_001_i64));
    assert_eq!(probe["sessionId"], json!("S9"));
    assert!(
        probe["params"]["expression"]
            .as_str()
            .unwrap()
            .contains("elementFromPoint(1, 2)")
    );
    let nav = AgentAction::Navigate { url: "x".into() };
    assert!(probe_for(&nav, None, 1).is_none());
    let answer = json!({"id":1,"result":{"result":{"type":"object","value":{"tag":"a"}}}});
    assert_eq!(probe_description(&answer).unwrap()["tag"], json!("a"));
}

#[test]
fn attached_pages_get_focus_emulation() {
    let event = json!({"method":"Target.attachedToTarget","params":{"sessionId":"S2",
        "targetInfo":{"type":"page","targetId":"T"},"waitingForDebugger":false}});
    assert_eq!(attached_page_session(&event).as_deref(), Some("S2"));
    let worker = json!({"method":"Target.attachedToTarget","params":{"sessionId":"W",
        "targetInfo":{"type":"service_worker"}}});
    assert!(attached_page_session(&worker).is_none());
    let command = focus_emulation("S2", 2_000_000_005);
    assert_eq!(
        command["method"],
        json!("Emulation.setFocusEmulationEnabled")
    );
    assert_eq!(command["params"]["enabled"], json!(true));
    assert_eq!(command["sessionId"], json!("S2"));
    assert_eq!(
        session_of(r#"{"id":1,"method":"Page.bringToFront","sessionId":"Z"}"#).as_deref(),
        Some("Z")
    );
}

#[test]
fn agent_browser_select_is_an_action_with_its_values() {
    let message = json!({
        "id": 7,
        "method": "Runtime.callFunctionOn",
        "sessionId": "S1",
        "params": {
            "objectId": "obj-1",
            "functionDeclaration": "async function(vals, by) { if (!(this instanceof HTMLSelectElement)) {} }",
            "arguments": [{ "value": ["Peru"] }, { "value": null }]
        }
    });
    let (action, session) = agent_action(&message).expect("select is recognized");
    assert_eq!(session.as_deref(), Some("S1"));
    assert_eq!(
        action,
        AgentAction::Select {
            object_id: "obj-1".into(),
            values: vec!["Peru".into()]
        }
    );
    let line = inbox_line(&action, None, 5);
    assert_eq!(line["kind"], "select");
    assert_eq!(line["fields"]["value"], "Peru");
}
