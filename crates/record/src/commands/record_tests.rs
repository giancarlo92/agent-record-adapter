// Part of Agent Record Adapter (see NOTICE).
use super::*;
use crate::{ActionOps, InputOps, ObservationOps, RecordOps, RecordSink, SystemOps};
use serde_json::Map;
use std::sync::Mutex;

struct ScriptedRecorder {
    events: Vec<RecordedEvent>,
    seen: Mutex<Option<RecordRequest>>,
}

impl ScriptedRecorder {
    fn emitting(events: Vec<RecordedEvent>) -> Self {
        Self {
            events,
            seen: Mutex::new(None),
        }
    }
}

impl ObservationOps for ScriptedRecorder {}
impl ActionOps for ScriptedRecorder {}
impl SystemOps for ScriptedRecorder {}
impl InputOps for ScriptedRecorder {}
impl RecordOps for ScriptedRecorder {
    fn record_input(
        &self,
        request: &RecordRequest,
        sink: &mut RecordSink<'_>,
    ) -> Result<RecordSummary, AdapterError> {
        *self.seen.lock().unwrap() = Some(request.clone());
        for event in &self.events {
            sink(event.clone())?;
        }
        Ok(RecordSummary {
            video: None,
            duration_ms: 1234,
            stop_reason: "stop_file",
        })
    }
}

struct UnsupportedRecorder;
impl ObservationOps for UnsupportedRecorder {}
impl ActionOps for UnsupportedRecorder {}
impl SystemOps for UnsupportedRecorder {}
impl InputOps for UnsupportedRecorder {}
impl RecordOps for UnsupportedRecorder {}

fn click(elapsed_ms: u64, name: &str) -> RecordedEvent {
    let mut fields = Map::new();
    fields.insert("element".into(), json!({"role": "button", "name": name}));
    fields.insert("kind".into(), json!("spoofed"));
    RecordedEvent {
        kind: "click",
        elapsed_ms,
        fields,
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agent-desktop-record-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn args(out: PathBuf) -> RecordArgs {
    RecordArgs {
        out,
        app: Some("Calculator".into()),
        window_id: None,
        display: None,
        max_duration_ms: Some(5000),
        stop_file: None,
        include_injected: false,
        video: None,
        indicator: false,
    }
}

fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn streams_a_header_every_event_and_a_trailer() {
    let dir = scratch("stream");
    let out = dir.join("nested").join("demo.jsonl");
    let recorder = ScriptedRecorder::emitting(vec![click(10, "Two"), click(250, "Plus")]);

    let result = execute(args(out.clone()), &recorder).unwrap();

    assert_eq!(result["events"], 2);
    assert_eq!(result["stop_reason"], "stop_file");
    assert_eq!(result["format"], RECORDING_FORMAT);
    let written = lines(&out);
    assert_eq!(written.len(), 4);
    assert_eq!(written[0]["kind"], "recording_started");
    assert_eq!(written[0]["app"], "Calculator");
    assert_eq!(written[1]["kind"], "click");
    assert_eq!(written[1]["t_ms"], 10);
    assert_eq!(written[1]["element"]["name"], "Two");
    assert_eq!(written[2]["element"]["name"], "Plus");
    assert_eq!(written[3]["kind"], "recording_stopped");
    assert_eq!(written[3]["events"], 2);
    let seen = recorder.seen.lock().unwrap().clone().unwrap();
    assert_eq!(seen.max_duration_ms, 5000);
    assert_eq!(seen.app.as_deref(), Some("Calculator"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unsupported_platform_leaves_no_empty_recording_behind() {
    let dir = scratch("unsupported");
    let out = dir.join("demo.jsonl");

    let error = execute(args(out.clone()), &UnsupportedRecorder).unwrap_err();

    assert_eq!(error.code(), "PLATFORM_NOT_SUPPORTED");
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_existing_stop_file_is_refused_before_anything_is_written() {
    let dir = scratch("stop-exists");
    std::fs::create_dir_all(&dir).unwrap();
    let stop = dir.join("stop");
    std::fs::write(&stop, b"").unwrap();
    let out = dir.join("demo.jsonl");
    let mut request = args(out.clone());
    request.stop_file = Some(stop);

    let error = execute(request, &ScriptedRecorder::emitting(Vec::new())).unwrap_err();

    assert_eq!(error.code(), "INVALID_ARGS");
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_duration_outside_the_bounds_is_refused() {
    for duration in [0, MAX_RECORD_DURATION_MS + 1] {
        let mut request = args(scratch("bounds").join("demo.jsonl"));
        request.max_duration_ms = Some(duration);

        let error = execute(request, &ScriptedRecorder::emitting(Vec::new())).unwrap_err();

        assert_eq!(error.code(), "INVALID_ARGS");
    }
}

#[test]
fn only_one_scope_is_accepted() {
    for (window_id, display) in [(Some("w-1"), None), (None, Some("monitor-1"))] {
        let mut request = args(scratch("both").join("demo.jsonl"));
        request.window_id = window_id.map(Into::into);
        request.display = display.map(Into::into);

        let error = execute(request, &ScriptedRecorder::emitting(Vec::new())).unwrap_err();

        assert_eq!(error.code(), "INVALID_ARGS");
    }
}

#[test]
fn a_display_scope_reaches_the_adapter_and_the_header() {
    let dir = scratch("display");
    let out = dir.join("demo.jsonl");
    let mut request = args(out.clone());
    request.app = None;
    request.display = Some("monitor-2".into());
    let recorder = ScriptedRecorder::emitting(Vec::new());

    execute(request, &recorder).unwrap();

    let seen = recorder.seen.lock().unwrap().clone().unwrap();
    assert_eq!(seen.display.as_deref(), Some("monitor-2"));
    assert_eq!(lines(&out)[0]["display"], "monitor-2");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_ready_line_is_written_but_not_counted() {
    let dir = scratch("ready");
    let out = dir.join("demo.jsonl");
    let ready = RecordedEvent {
        kind: RECORDING_READY_KIND,
        elapsed_ms: 3,
        fields: Map::new(),
    };
    let recorder = ScriptedRecorder::emitting(vec![ready, click(10, "Two")]);

    let result = execute(args(out.clone()), &recorder).unwrap();

    assert_eq!(result["events"], 1);
    let written = lines(&out);
    assert_eq!(written[1]["kind"], RECORDING_READY_KIND);
    assert_eq!(written[2]["kind"], "click");
    assert_eq!(written[3]["events"], 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_event_cannot_override_its_kind_or_time() {
    let line = click(42, "Two").to_json();

    assert_eq!(line["kind"], "click");
    assert_eq!(line["t_ms"], 42);
}
