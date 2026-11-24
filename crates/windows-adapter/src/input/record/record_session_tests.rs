// Part of Agent Record Adapter (see NOTICE).
//! End-to-end check of the recording loop against a real browser form.
//!
//! The pointer presses and key strokes a person would make are handed to the
//! recorder directly instead of through the low-level hooks, so the test never
//! moves the cursor or types into whatever the user is doing. Each action's
//! effect on the page is produced through UI Automation (focus, value,
//! selection, toggle, invoke), the way the browser would answer the real
//! input. It needs Google Chrome, so it is ignored by default:
//! `cargo test -p agent-desktop-windows --lib record_session -- --ignored`.
//! Set `AGENT_DESKTOP_RECORD_LIVE_OUT` to also write the recording as JSON
//! Lines.

use super::*;
use agent_desktop_core::{Deadline, WindowFilter};
use std::path::PathBuf;
use std::process::{Child, Command};
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UISelectionItemPattern, UITogglePattern,
    UIValuePattern,
};
use uiautomation::types::{Handle, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement};

use crate::system::window::window_ops::list_windows_live;
use crate::tree::automation::automation_client;

const FORM_TITLE: &str = "Record live form";
const CONTAINER_ROLES: &[&str] = &[
    "group",
    "document",
    "pane",
    "window",
    "unknown",
    "statictext",
];
const FORM: &str = r#"<!doctype html><html lang="es"><head><meta charset="utf-8">
<title>Record live form</title></head><body style="font:16px sans-serif;margin:30px">
<h1 id="titulo">Formulario</h1>
<label for="nombre">Nombre</label> <input id="nombre" type="text"><br><br>
<label for="correo">Correo</label> <input id="correo" type="email"><br><br>
<label for="clave">Clave</label> <input id="clave" type="password"><br><br>
<label for="pais">País</label> <select id="pais"><option value="">Elige un país</option>
<option>Perú</option><option>México</option></select><br><br>
<fieldset><legend>Plan</legend>
<input id="plan-basico" type="radio" name="plan" checked><label for="plan-basico">Básico</label>
<input id="plan-pro" type="radio" name="plan"><label for="plan-pro">Pro</label></fieldset><br>
<input id="terminos" type="checkbox"><label for="terminos">Acepto los términos</label><br><br>
<label for="comentario">Comentario</label> <textarea id="comentario"></textarea><br><br>
<button id="enviar" type="button">Enviar</button></body></html>"#;

struct Browser {
    child: Child,
    dir: PathBuf,
}

impl Drop for Browser {
    fn drop(&mut self) {
        let pid = self.child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .output();
        let _ = self.child.wait();
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn chrome() -> PathBuf {
    std::env::var("AGENT_DESKTOP_TEST_CHROME").map_or_else(
        |_| PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
        PathBuf::from,
    )
}

fn open_form() -> (Browser, String) {
    let dir =
        std::env::temp_dir().join(format!("agent-desktop-record-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let page = dir.join("form.html");
    std::fs::write(&page, FORM).unwrap();
    let child = Command::new(chrome())
        .arg(format!("--user-data-dir={}", dir.join("profile").display()))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--force-renderer-accessibility",
            "--new-window",
            "--window-position=60,40",
            "--window-size=760,980",
        ])
        .arg(format!(
            "file:///{}",
            page.display().to_string().replace('\\', "/")
        ))
        .spawn()
        .expect("Google Chrome starts");
    let browser = Browser { child, dir };
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        let deadline = Deadline::detached_after(2_000).unwrap();
        let windows = list_windows_live(&WindowFilter::default(), deadline).unwrap_or_default();
        if let Some(window) = windows.iter().find(|w| w.title.starts_with(FORM_TITLE)) {
            return (browser, window.id.clone());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("the form window did not appear");
}

struct Page {
    client: UIAutomation,
    root: UIElement,
}

impl Page {
    fn new(window_id: &str) -> Self {
        let client = automation_client().unwrap();
        let hwnd: usize = window_id.trim_start_matches("w-").parse().unwrap();
        let root = client
            .element_from_handle(Handle::from(hwnd as isize))
            .unwrap();
        Self { client, root }
    }

    fn by(&self, scope: &UIElement, property: UIProperty, value: &str) -> UIElement {
        let condition = self
            .client
            .create_property_condition(property, Variant::from(value), None)
            .unwrap();
        let started = Instant::now();
        loop {
            if let Ok(found) = scope.find_first(TreeScope::Descendants, &condition) {
                return found;
            }
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "{value} not found"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn id(&self, id: &str) -> UIElement {
        self.by(&self.root, UIProperty::AutomationId, id)
    }
}

fn center(element: &UIElement) -> (i32, i32) {
    let rect = element.get_bounding_rectangle().unwrap();
    (
        (rect.get_left() + rect.get_right()) / 2,
        (rect.get_top() + rect.get_bottom()) / 2,
    )
}

fn label_point(control: &UIElement) -> (i32, i32) {
    let rect = control.get_bounding_rectangle().unwrap();
    (
        rect.get_right() + 14,
        (rect.get_top() + rect.get_bottom()) / 2,
    )
}

fn press(recorder: &mut Recorder<'_, '_>, (x, y): (i32, i32)) {
    let at = Instant::now();
    let button = "left";
    recorder
        .handle(RawInput::Button {
            button,
            x,
            y,
            at,
            menu: None,
        })
        .unwrap();
}

fn key(recorder: &mut Recorder<'_, '_>, vk: u32) {
    for down in [true, false] {
        let at = Instant::now();
        recorder
            .handle(RawInput::Key {
                vk,
                scan: 0,
                down,
                at,
                clipboard: 0,
            })
            .unwrap();
    }
}

fn release(recorder: &mut Recorder<'_, '_>, (x, y): (i32, i32)) {
    let at = Instant::now();
    recorder.handle(RawInput::Release { x, y, at }).unwrap();
}

/// Selects across the comment box by dragging, then copies a small table: the
/// test puts the table on the clipboard itself, the way the page would answer
/// ctrl+c, and restores what the clipboard held before.
fn drag_and_copy(page: &Page, recorder: &mut Recorder<'_, '_>) {
    let rect = page.id("comentario").get_bounding_rectangle().unwrap();
    let y = (rect.get_top() + rect.get_bottom()) / 2;
    press(recorder, (rect.get_left() + 4, y));
    release(recorder, (rect.get_right() - 4, y));
    settle(recorder);
    let deadline = || Deadline::after(2_000).unwrap();
    let saved = crate::input::clipboard::get_clipboard_content(
        agent_desktop_core::ClipboardFormat::Text,
        deadline(),
    )
    .ok()
    .flatten();
    let table = agent_desktop_core::ClipboardContent::Text(
        "1	Ana
2	Luis
"
        .into(),
    );
    crate::input::clipboard::set_content(&table, deadline()).unwrap();
    for (vk, down) in [(0xA2, true), (0x43, true), (0x43, false), (0xA2, false)] {
        recorder
            .handle(RawInput::Key {
                vk,
                scan: 0,
                down,
                at: Instant::now(),
                clipboard: 0,
            })
            .unwrap();
    }
    if let Some(saved) = saved {
        let _ = crate::input::clipboard::set_content(&saved, deadline());
    }
}

fn settle(recorder: &mut Recorder<'_, '_>) {
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(50));
        recorder.tick(Instant::now()).unwrap();
    }
}

fn set_value(element: &UIElement, value: &str) {
    element
        .get_pattern::<UIValuePattern>()
        .unwrap()
        .set_value(value)
        .unwrap();
}

fn type_into(page: &Page, recorder: &mut Recorder<'_, '_>, id: &str, value: &str) {
    let field = page.id(id);
    field.set_focus().unwrap();
    set_value(&field, value);
    let letters = value.chars().filter(char::is_ascii_alphabetic);
    for vk in letters.map(|c| c.to_ascii_uppercase() as u32) {
        key(recorder, vk);
    }
    settle(recorder);
}

fn demonstrate(page: &Page, recorder: &mut Recorder<'_, '_>) {
    press(recorder, center(&page.id("nombre")));
    page.id("nombre").set_focus().unwrap();
    settle(recorder);
    type_into(page, recorder, "nombre", "Ana");
    key(recorder, 0x09);
    type_into(page, recorder, "correo", "ana@example.com");
    key(recorder, 0x09);
    type_into(page, recorder, "clave", "secreto");
    let pais = page.id("pais");
    press(recorder, center(&pais));
    settle(recorder);
    let expander = pais.get_pattern::<UIExpandCollapsePattern>().unwrap();
    expander.expand().unwrap();
    let desktop = page.client.get_root_element().unwrap();
    let peru = page.by(&desktop, UIProperty::Name, "Perú");
    press(recorder, center(&page.id("titulo")));
    peru.get_pattern::<UISelectionItemPattern>()
        .unwrap()
        .select()
        .unwrap();
    let _ = expander.collapse();
    settle(recorder);
    let pro = page.id("plan-pro");
    press(recorder, label_point(&pro));
    pro.set_focus().unwrap();
    pro.get_pattern::<UISelectionItemPattern>()
        .unwrap()
        .select()
        .unwrap();
    settle(recorder);
    let terminos = page.id("terminos");
    for _ in 0..3 {
        press(recorder, label_point(&terminos));
        terminos.set_focus().unwrap();
        terminos
            .get_pattern::<UITogglePattern>()
            .unwrap()
            .toggle()
            .unwrap();
        settle(recorder);
    }
    press(recorder, center(&page.id("comentario")));
    type_into(page, recorder, "comentario", "Hola");
    drag_and_copy(page, recorder);
    let enviar = page.id("enviar");
    press(recorder, center(&enviar));
    enviar
        .get_pattern::<UIInvokePattern>()
        .unwrap()
        .invoke()
        .unwrap();
    settle(recorder);
    recorder.finish().unwrap();
}

fn of<'e>(events: &'e [Value], kind: &str, native_id: &str) -> Vec<&'e Value> {
    events
        .iter()
        .filter(|e| e["kind"] == kind && e["element"]["native_id"] == native_id)
        .collect()
}

#[test]
#[ignore = "drives Google Chrome; run with --ignored"]
fn a_browser_form_demonstration_records_every_step() {
    crate::tree::fixture::ensure_test_apartment();
    let (_browser, window_id) = open_form();
    let page = Page::new(&window_id);
    let request = RecordRequest {
        app: None,
        window_id: Some(window_id.clone()),
        display: None,
        max_duration_ms: 60_000,
        stop_file: None,
        include_injected: false,
        video: None,
        indicator: false,
    };
    let mut recorded: Vec<RecordedEvent> = Vec::new();
    let mut sink = |event: RecordedEvent| {
        recorded.push(event);
        Ok(())
    };
    let observer = Observer::new(&request).unwrap();
    let mut recorder = Recorder::new(observer, Instant::now(), &mut sink);
    demonstrate(&page, &mut recorder);
    drop(recorder);
    let events: Vec<Value> = recorded.iter().map(RecordedEvent::to_json).collect();
    if let Ok(path) = std::env::var("AGENT_DESKTOP_RECORD_LIVE_OUT") {
        let lines: Vec<String> = events.iter().map(Value::to_string).collect();
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    assert_eq!(
        of(&events, "type", "nombre")[0]["value"],
        "Ana",
        "{events:#?}"
    );
    assert_eq!(of(&events, "type", "nombre")[0]["typed"], "ana");
    assert_eq!(of(&events, "type", "correo")[0]["value"], "ana@example.com");
    let secret = of(&events, "type", "clave")[0];
    assert_eq!(secret["secure"], true);
    assert!(secret.get("value").is_none() && secret.get("typed").is_none());
    assert_eq!(
        of(&events, "select", "pais")[0]["value"],
        "Perú",
        "{events:#?}"
    );
    let pro = of(&events, "click", "plan-pro");
    assert_eq!(pro.len(), 1, "{events:#?}");
    assert_eq!(pro[0]["state_after"]["selected"], true);
    let checks: Vec<&Value> = of(&events, "click", "terminos")
        .into_iter()
        .map(|e| &e["state_after"]["checked"])
        .collect();
    assert_eq!(
        checks,
        [&json!(true), &json!(false), &json!(true)],
        "{events:#?}"
    );
    assert_eq!(of(&events, "type", "comentario")[0]["value"], "Hola");
    assert_eq!(of(&events, "click", "enviar").len(), 1);
    let drags = of(&events, "drag", "comentario");
    assert_eq!(drags.len(), 1, "{events:#?}");
    assert!(drags[0]["from"]["x"].as_i64() < drags[0]["point"]["x"].as_i64());
    let copy: Vec<&Value> = events.iter().filter(|e| e["keys"] == "ctrl+c").collect();
    assert_eq!(copy.len(), 1, "{events:#?}");
    assert_eq!(copy[0]["clipboard"]["lines"], 2, "{events:#?}");
    assert_eq!(copy[0]["clipboard"]["columns"], 2);
    let stray: Vec<&Value> = events
        .iter()
        .filter(|e| {
            e["kind"] == "click"
                && e["element"]["role"]
                    .as_str()
                    .is_some_and(|role| CONTAINER_ROLES.contains(&role))
        })
        .collect();
    assert!(stray.is_empty(), "clicks left on containers: {stray:#?}");
}
