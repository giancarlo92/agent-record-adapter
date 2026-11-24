// Part of Agent Record Adapter (see NOTICE).
use super::*;

fn line(value: serde_json::Value) -> String {
    value.to_string()
}

fn window() -> serde_json::Value {
    serde_json::json!({ "id": "w-1", "app_name": "chrome.exe", "title": "Sign up - Google Chrome",
        "path": "C:\\Chrome\\chrome.exe", "url": "C:/forms/signup.html" })
}

fn event(kind: &str, element: serde_json::Value, extra: serde_json::Value) -> String {
    let mut e =
        serde_json::json!({ "kind": kind, "t_ms": 1, "element": element, "window": window() });
    for (k, v) in extra.as_object().unwrap() {
        e[k] = v.clone();
    }
    line(e)
}

#[test]
fn a_form_recording_becomes_the_steps_a_replay_needs() {
    let recording = [
        line(serde_json::json!({ "kind": "recording_ready", "t_ms": 0 })),
        event("click", serde_json::json!({ "role": "textfield", "name": "Full name", "native_id": "fullName" }), serde_json::json!({})),
        event("press", serde_json::json!({ "role": "textfield", "name": "Full name", "native_id": "fullName" }), serde_json::json!({ "keys": "ctrl+a" })),
        event("type", serde_json::json!({ "role": "textfield", "name": "Full name", "native_id": "fullName" }), serde_json::json!({ "value": "Luc" })),
        event("type", serde_json::json!({ "role": "textfield", "name": "Full name", "native_id": "fullName" }), serde_json::json!({ "value": "Lucia Fernandez" })),
        event("click", serde_json::json!({ "role": "textfield", "name": "Date of birth", "native_id": "birthDate" }), serde_json::json!({})),
        event("type", serde_json::json!({ "role": "incrementor", "name": "Day Date of birth" }), serde_json::json!({ "value": "1" })),
        event("type", serde_json::json!({ "role": "incrementor", "name": "Month Date of birth" }), serde_json::json!({ "value": "12" })),
        event("type", serde_json::json!({ "role": "incrementor", "name": "Year Date of birth" }), serde_json::json!({ "value": "1990" })),
        event("type", serde_json::json!({ "role": "option", "name": "Peru" }), serde_json::json!({ "value": "" })),
        event("press", serde_json::json!({ "role": "combobox", "name": "Country", "native_id": "country" }), serde_json::json!({ "keys": "return" })),
        event("click", serde_json::json!({ "role": "checkbox", "name": "AI" }), serde_json::json!({ "state_after": { "checked": true } })),
        event("type", serde_json::json!({ "role": "textfield", "label": "Password" }), serde_json::json!({ "secure": true })),
        event("press", serde_json::json!({ "role": "unknown", "name": "" }), serde_json::json!({ "keys": "ctrl+v" })),
        event("click", serde_json::json!({ "role": "button", "name": "Send order", "native_id": "send" }), serde_json::json!({})),
    ]
    .join("\n");
    let steps = parse(&recording);
    let actions: Vec<&Action> = steps.iter().map(|s| &s.action).collect();
    let id = |id: &str| Locator {
        native_id: Some(id.into()),
        role: None,
        name: None,
        in_document: false,
    };
    assert!(
        matches!(actions[0], Action::OpenBrowser { url } if url == "file:///C:/forms/signup.html")
    );
    assert!(
        matches!(actions[1], Action::SetValue { at, value, .. } if at.native_id == id("fullName").native_id && value == "Lucia Fernandez")
    );
    assert!(
        matches!(actions[2], Action::SetValue { at, value, .. } if at.native_id.as_deref() == Some("birthDate") && value == "1990-12-01")
    );
    assert!(
        matches!(actions[3], Action::Select { at, value, .. } if at.native_id.as_deref() == Some("country") && value == "Peru")
    );
    assert!(matches!(actions[4], Action::Check { field, on: true, .. } if field == "AI"));
    assert!(
        matches!(actions[5], Action::Secret { at, name, .. } if at.name.as_deref() == Some("Password") && name == "chrome-login")
    );
    assert!(matches!(actions[6], Action::Skipped { what } if what.contains("ctrl+v")));
    assert!(matches!(
        actions[7],
        Action::Click {
            irreversible: true,
            ..
        }
    ));
    assert_eq!(steps.len(), 8);
}

#[test]
fn every_sample_recording_parses() {
    let dir = std::path::Path::new(&std::env::var("USERPROFILE").unwrap_or_default())
        .join("AprendeReplica-e2e")
        .join("recordings");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let steps = parse(&text);
        let script = crate::script::render("t", "r", &steps, &crate::script::candidates(&steps));
        assert!(script.contains("await task("), "{}", entry.path().display());
    }
}

fn desktop(
    kind: &str,
    window: serde_json::Value,
    element: serde_json::Value,
    extra: serde_json::Value,
) -> String {
    let mut e =
        serde_json::json!({ "kind": kind, "t_ms": 1, "element": element, "window": window });
    for (k, v) in extra.as_object().unwrap() {
        e[k] = v.clone();
    }
    line(e)
}

#[test]
fn a_desktop_recording_keeps_what_a_replay_can_repeat() {
    let main = serde_json::json!({ "id": "w-7", "app_name": "db.exe", "title": "DB 1.0 - <conn> Script-25",
        "path": "C:/DB/db.exe", "bounds": { "x": -9.0, "y": -9.0, "width": 1938.0, "height": 1038.0 } });
    let renamed = serde_json::json!({ "id": "w-7", "app_name": "db.exe", "title": "DB 1.0 - <conn> Script-26",
        "path": "C:/DB/db.exe", "bounds": { "x": -9.0, "y": -9.0, "width": 1938.0, "height": 1038.0 } });
    let popup = serde_json::json!({ "id": "w-9", "app_name": "db.exe", "title": "", "path": "C:/DB/db.exe" });
    let editor = serde_json::json!({ "role": "textfield", "native_id": "3345526" });
    let recording = [
        desktop("click", main.clone(), serde_json::json!({ "role": "treeitem", "name": "prv" }), serde_json::json!({ "button": "right" })),
        desktop("click", main.clone(), serde_json::json!({ "role": "menuitem", "name": "Open SQL script" }), serde_json::json!({ "menu": "popup" })),
        desktop("click", main.clone(), editor.clone(), serde_json::json!({})),
        desktop("type", main.clone(), editor.clone(), serde_json::json!({ "value": "SELECT *" })),
        desktop("press", main.clone(), editor.clone(), serde_json::json!({ "keys": "return" })),
        desktop("type", main.clone(), editor.clone(), serde_json::json!({ "value": "SELECT *\r\nFR" })),
        desktop("press", main.clone(), editor.clone(), serde_json::json!({ "keys": "ctrl+space" })),
        desktop("type", main.clone(), editor.clone(), serde_json::json!({ "value": "SELECT *\r\nFROM ALUMN" })),
        desktop("click", popup.clone(), serde_json::json!({ "role": "statictext", "name": "alumno a - Table", "native_id": "ListViewSubItem-0" }),
            serde_json::json!({ "ancestors": [{ "role": "option", "name": "alumno a - Table" }] })),
        desktop("click", popup, serde_json::json!({ "role": "statictext", "name": "alumno a - Table" }), serde_json::json!({})),
        desktop("click", renamed.clone(), editor.clone(), serde_json::json!({})),
        desktop("press", renamed.clone(), editor, serde_json::json!({ "keys": "ctrl+return" })),
        desktop("click", renamed.clone(), serde_json::json!({ "role": "group", "name": "grid" }), serde_json::json!({ "button": "right" })),
        desktop("click", renamed.clone(), serde_json::json!({}), serde_json::json!({ "point": { "x": 492, "y": 367 } })),
        desktop("click", renamed, serde_json::json!({}), serde_json::json!({ "point": { "x": 100, "y": 50 }, "count": 2 })),
    ]
    .join("\n");
    let steps = parse(&recording);
    let actions: Vec<&Action> = steps.iter().map(|s| &s.action).collect();
    assert!(matches!(actions[0], Action::OpenApp { path } if path == "C:/DB/db.exe"));
    // The right-click only opened the menu: its item is the step.
    assert!(matches!(actions[1], Action::MenuItem { name } if name == "Open SQL script"));
    // Typing around Enter and an autocompletion ends in one value; a window handle is no id.
    assert!(matches!(actions[2], Action::SetValue { at, value, .. }
            if value == "SELECT *\r\nFROM alumno a" && at.native_id.is_none() && at.in_document));
    assert!(matches!(actions[3], Action::Key { keys } if keys == "ctrl+return"));
    // A right-click whose item could not be read stays marked, and the click on nothing after
    // it (where the menu was) is not replayed; a later click on nothing is, at the same point.
    assert!(matches!(actions[4], Action::Skipped { what } if what.contains("right-click")));
    assert!(matches!(
        actions[5],
        Action::ClickAt {
            dx: 109,
            dy: 59,
            double: true
        }
    ));
    assert_eq!(steps.len(), 6);
}

#[test]
fn a_key_typed_with_a_value_before_it_keeps_that_value() {
    let main = serde_json::json!({ "id": "w-7", "app_name": "db.exe", "title": "DB", "path": "C:/DB/db.exe" });
    let editor = serde_json::json!({ "role": "textfield", "name": "Query" });
    let recording = [
        desktop(
            "type",
            main.clone(),
            editor.clone(),
            serde_json::json!({ "value": "FROM ALUMN" }),
        ),
        desktop(
            "press",
            main.clone(),
            editor,
            serde_json::json!({ "keys": "ctrl+return", "value_before": "FROM alumno a" }),
        ),
    ]
    .join("\n");
    let steps = parse(&recording);
    assert!(matches!(&steps[1].action, Action::SetValue { value, .. } if value == "FROM alumno a"));
    assert!(matches!(&steps[2].action, Action::Key { keys } if keys == "ctrl+return"));
}

#[test]
fn a_page_address_keeps_its_scheme_and_a_bare_local_one_gets_http() {
    assert_eq!(
        full_url("http://127.0.0.1:4200/auth/login"),
        "http://127.0.0.1:4200/auth/login"
    );
    assert_eq!(
        full_url("127.0.0.1:4200/auth/login"),
        "http://127.0.0.1:4200/auth/login"
    );
    assert_eq!(full_url("localhost:3000"), "http://localhost:3000");
    assert_eq!(full_url("192.168.1.20/app"), "http://192.168.1.20/app");
    assert_eq!(full_url("[::1]:8080/x"), "http://[::1]:8080/x");
    assert_eq!(full_url("example.com/signup"), "https://example.com/signup");
    assert_eq!(full_url("C:/forms/a.html"), "file:///C:/forms/a.html");
}

#[test]
fn a_file_picker_opened_by_the_page_is_acted_on_in_its_own_window() {
    let page = serde_json::json!({ "id": "w-1", "app_name": "chrome.exe", "title": "Import - Google Chrome",
        "path": "C:/Chrome/chrome.exe", "url": "http://127.0.0.1:4200/import" });
    let picker = serde_json::json!({ "id": "w-9", "app_name": "chrome.exe", "title": "Open",
        "path": "C:/Chrome/chrome.exe", "owner_id": "w-1" });
    let recording = [
        desktop(
            "click",
            page,
            serde_json::json!({ "role": "button", "name": "Archivo" }),
            serde_json::json!({}),
        ),
        desktop(
            "type",
            picker.clone(),
            serde_json::json!({ "role": "textfield", "name": "File name:" }),
            serde_json::json!({ "value": "C:/data/people.csv" }),
        ),
        desktop(
            "click",
            picker,
            serde_json::json!({ "role": "button", "name": "Open", "native_id": "1" }),
            serde_json::json!({}),
        ),
    ]
    .join("\n");
    let steps = parse(&recording);
    assert!(
        matches!(&steps[0].action, Action::OpenBrowser { url } if url == "http://127.0.0.1:4200/import")
    );
    assert!(matches!(&steps[1].action, Action::Click { field, .. } if field == "Archivo"));
    assert!(
        matches!(&steps[2].action, Action::SetValue { value, .. } if value == "C:/data/people.csv")
    );
    assert_eq!(steps[2].window.title, "Open");
    assert!(matches!(&steps[3].action, Action::Click { field, .. } if field == "Open"));
    assert_eq!(steps.len(), 4);
}

#[test]
fn two_choices_in_one_list_are_replayed_in_order() {
    let page = serde_json::json!({ "id": "w-1", "app_name": "chrome.exe", "title": "Print - Google Chrome",
        "path": "C:/Chrome/chrome.exe", "url": "chrome://print/" });
    let list = serde_json::json!({ "role": "combobox", "name": "Destination" });
    let recording = [
        desktop(
            "select",
            page.clone(),
            list.clone(),
            serde_json::json!({ "value": "Microsoft Print to PDF" }),
        ),
        desktop(
            "select",
            page,
            list,
            serde_json::json!({ "value": "Save as PDF" }),
        ),
    ]
    .join("\n");
    let steps = parse(&recording);
    let chosen: Vec<&str> = steps
        .iter()
        .filter_map(|s| match &s.action {
            Action::Select { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(chosen, ["Microsoft Print to PDF", "Save as PDF"]);
}

#[test]
fn space_on_a_radio_and_enter_on_a_button_are_replayed_as_clicks() {
    let page = serde_json::json!({ "id": "w-1", "app_name": "chrome.exe", "title": "SGH - Google Chrome",
        "path": "C:/Chrome/chrome.exe", "url": "http://127.0.0.1:4200/caja" });
    let recording = [
        desktop(
            "press",
            page.clone(),
            serde_json::json!({ "role": "radiobutton", "name": "Pago directo" }),
            serde_json::json!({ "keys": "space" }),
        ),
        desktop(
            "press",
            page,
            serde_json::json!({ "role": "button", "name": "Continuar" }),
            serde_json::json!({ "keys": "return" }),
        ),
    ]
    .join("\n");
    let steps = parse(&recording);
    assert!(
        matches!(&steps[1].action, Action::Check { field, on: true, .. } if field == "Pago directo")
    );
    assert!(matches!(&steps[2].action, Action::Click { field, .. } if field == "Continuar"));
}

#[test]
fn caret_keys_and_a_paste_followed_by_text_are_replayed_by_that_text() {
    let doc = serde_json::json!({ "id": "w-3", "app_name": "wordcraft.exe", "title": "Document1 - WordCraft",
        "path": "C:/WordCraft/wordcraft.exe" });
    let body = serde_json::json!({ "role": "document", "name": "Document1" });
    let pasted = [
        desktop(
            "press",
            doc.clone(),
            body.clone(),
            serde_json::json!({ "keys": "ctrl+end" }),
        ),
        desktop(
            "press",
            doc.clone(),
            body.clone(),
            serde_json::json!({ "keys": "return" }),
        ),
        desktop(
            "press",
            doc.clone(),
            body.clone(),
            serde_json::json!({ "keys": "ctrl+v", "value_before": "Invoice\n" }),
        ),
        desktop(
            "type",
            doc.clone(),
            body.clone(),
            serde_json::json!({ "value": "Invoice\nFactura 2002" }),
        ),
    ]
    .join("\n");
    let steps = parse(&pasted);
    assert!(
        matches!(&steps[1].action, Action::SetValue { value, .. } if value == "Invoice\nFactura 2002")
    );
    assert_eq!(steps.len(), 2);
}
