// Part of Agent Record Adapter (see NOTICE).
//! The `record` loop: turns raw transitions into `click`, `drag`, `type`,
//! `press` and `scroll` events.
//!
//! Typing is folded into one `type` event per field. Its `value` is the
//! field's content read once typing pauses, which is what a replay sets; the
//! keystroke text is kept beside it as `typed` while it is exact. A commit key
//! (Return, Tab, Escape, a function key) or a click elsewhere ends the run.

use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use agent_desktop_core::commands::record::RECORDING_READY_KIND;
use agent_desktop_core::commands::record_background::agent_inbox;
use agent_desktop_core::{AdapterError, RecordRequest, RecordSink, RecordSummary, RecordedEvent};
use serde_json::{Map, Value, json};

use super::record_copy::{copied, is_copy, is_paste};
use super::record_element::{FieldValue, Observer, Target};
use super::record_followup::{CLICK_SETTLE, ChoiceWatch, PendingClick, is_container};
use super::record_gesture::{
    ClickCounter, DragTracker, ScrollAccumulator, ScrollRun, system_click_rule, system_drag_slop,
    wheel_step,
};
use super::record_hook::{self, RawInput};
use super::record_indicator::Indicator;
use super::record_keys::{KeyClass, ModifierState, classify, key_name};
use super::record_stop::{InterruptGuard, StopWatch};
use super::record_text::typed_text;
use super::record_video::VideoRecorder;
use super::record_video_area::area_for;

const POLL: Duration = Duration::from_millis(50);
const SETTLE: Duration = Duration::from_millis(40);
const SCROLL_GAP: Duration = Duration::from_millis(600);
const SCROLL_SLOP: i32 = 40;
const VK_BACK: u32 = 0x08;

pub(super) fn run(
    request: &RecordRequest,
    sink: &mut RecordSink<'_>,
) -> Result<RecordSummary, AdapterError> {
    let observer = Observer::new(request)?;
    let _interrupt = InterruptGuard::install();
    let started = Instant::now();
    let mut stop = StopWatch::new(started, request.max_duration_ms, request.stop_file.clone());
    let (hooks, inputs) = record_hook::start(request.include_injected)?;
    let video = match &request.video {
        Some(path) => Some(VideoRecorder::start(path, area_for(request)?, started)?),
        None => None,
    };
    let indicator = request
        .indicator
        .then(|| area_for(request).ok())
        .flatten()
        .map(|area| Indicator::show(area, started));
    let mut recorder = Recorder::new(observer, started, sink);
    // agent-desktop's own actions while this background recording runs.
    let mut agent = request
        .stop_file
        .as_deref()
        .map(|stop| AgentInbox::new(agent_inbox(stop), super::record_agent::now_ms()));
    recorder.emit(RECORDING_READY_KIND, Instant::now(), Map::new())?;
    let stop_reason = loop {
        if let Some(reason) = stop.finished(Instant::now()) {
            break reason;
        }
        match inputs.recv_timeout(POLL) {
            Ok(input) => recorder.handle(input)?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(record_hook::hooks_lost()),
        }
        recorder.tick(Instant::now())?;
        if let Some(agent) = agent.as_mut() {
            for line in agent.take_new() {
                recorder.agent_action(&line, agent.started_ms)?;
            }
        }
    };
    drop(hooks);
    drop(indicator);
    while let Ok(input) = inputs.try_recv() {
        recorder.handle(input)?;
    }
    if let Some(agent) = agent.as_mut() {
        for line in agent.take_new() {
            recorder.agent_action(&line, agent.started_ms)?;
        }
    }
    recorder.finish()?;
    let duration_ms = millis_between(started, Instant::now());
    let video = match video.map(VideoRecorder::finish) {
        Some(Ok(video)) => Some(video),
        Some(Err(error)) => {
            tracing::warn!(%error, "record: the video could not be finished");
            None
        }
        None => None,
    };
    Ok(RecordSummary {
        duration_ms,
        stop_reason,
        video,
    })
}

/// The agent inbox of a background recording (see `record_agent`): the lines
/// agent-desktop appended since the last read.
struct AgentInbox {
    path: std::path::PathBuf,
    read: u64,
    /// When the recording started, in milliseconds since the Unix epoch.
    started_ms: u64,
}

impl AgentInbox {
    fn new(path: std::path::PathBuf, started_ms: u64) -> Self {
        Self {
            path,
            read: 0,
            started_ms,
        }
    }

    /// The complete lines added since the last call; a line still being
    /// written is left for the next one.
    fn take_new(&mut self) -> Vec<Value> {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(self.read)).is_err() {
            return Vec::new();
        }
        let mut added = Vec::new();
        if file.read_to_end(&mut added).is_err() {
            return Vec::new();
        }
        let Some(end) = added.iter().rposition(|byte| *byte == b'\n') else {
            return Vec::new();
        };
        self.read += end as u64 + 1;
        added[..end]
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect()
    }
}

struct PendingText {
    target: Target,
    /// The field as it was when typing started: a dialog that closes on the
    /// commit key (a file name box) no longer answers when the text is flushed.
    evidence: Map<String, Value>,
    typed: String,
    exact: bool,
    started: Instant,
    last_key: Instant,
    settled: Option<FieldValue>,
}

struct Recorder<'s, 'a> {
    observer: Observer,
    started: Instant,
    sink: &'s mut RecordSink<'a>,
    clicks: ClickCounter,
    drags: DragTracker,
    scroll: ScrollAccumulator<Map<String, Value>>,
    modifiers: ModifierState,
    pending: Option<PendingText>,
    click: Option<PendingClick>,
    choice: Option<ChoiceWatch>,
}

impl<'s, 'a> Recorder<'s, 'a> {
    fn new(observer: Observer, started: Instant, sink: &'s mut RecordSink<'a>) -> Self {
        Self {
            observer,
            started,
            sink,
            clicks: ClickCounter::new(system_click_rule()),
            drags: DragTracker::new(system_drag_slop()),
            scroll: ScrollAccumulator::new(SCROLL_GAP, SCROLL_SLOP),
            modifiers: ModifierState::default(),
            pending: None,
            click: None,
            choice: None,
        }
    }

    fn handle(&mut self, input: RawInput) -> Result<(), AdapterError> {
        if let RawInput::Move { x, y, at } = input {
            self.drags.motion(x, y, at);
            return Ok(());
        }
        self.settle_click()?;
        self.poll_choice(Instant::now())?;
        match input {
            RawInput::Button {
                button,
                x,
                y,
                at,
                menu: Some((item, owner)),
            } => {
                tracing::debug!(button, x, y, "record: menu item press");
                self.on_menu_item(&item.text(), owner, (x, y), at)
            }
            RawInput::Button {
                button, x, y, at, ..
            } => {
                tracing::debug!(button, x, y, "record: pointer press");
                self.on_button(button, x, y, at)
            }
            RawInput::Release { x, y, at } => self.on_release(x, y, at),
            RawInput::Move { .. } => Ok(()),
            RawInput::Wheel {
                delta,
                horizontal,
                x,
                y,
                at,
            } => self.on_wheel(delta, horizontal, (x, y), at),
            RawInput::Key {
                vk,
                scan,
                down,
                at,
                clipboard,
            } => self.on_key(vk, scan, down, at, clipboard),
        }
    }

    fn tick(&mut self, now: Instant) -> Result<(), AdapterError> {
        if self
            .click
            .as_ref()
            .is_some_and(|click| now.saturating_duration_since(click.at) >= CLICK_SETTLE)
        {
            self.settle_click()?;
        }
        self.poll_choice(now)?;
        if let Some(run) = self.scroll.take_if_idle(now) {
            self.emit_scroll(run)?;
        }
        if let Some(pending) = self.pending.as_mut() {
            if pending.settled.is_none()
                && now.saturating_duration_since(pending.last_key) >= SETTLE
            {
                if let Some(element) = pending.target.element.as_ref() {
                    pending.settled = Some(self.observer.value(element));
                }
            }
        }
        Ok(())
    }

    fn settle_click(&mut self) -> Result<(), AdapterError> {
        let Some(click) = self.click.take() else {
            return Ok(());
        };
        let at = click.at;
        let (fields, watch) = click.resolve(&mut self.observer);
        self.poll_choice(Instant::now())?;
        let in_popup = self.choice.as_ref().is_some_and(|choice| {
            is_container(&fields) || (fields.contains_key("hit") && choice.targets(&fields))
        });
        if in_popup {
            return Ok(());
        }
        if watch.is_some() {
            self.choice = watch;
        }
        self.emit("click", at, fields)
    }

    fn poll_choice(&mut self, now: Instant) -> Result<(), AdapterError> {
        let Some(mut watch) = self.choice.take() else {
            return Ok(());
        };
        let (changed, keep) = watch.poll(&self.observer, now);
        if keep {
            self.choice = Some(watch);
        }
        match changed {
            Some(fields) => self.emit("select", now, fields),
            None => Ok(()),
        }
    }

    fn finish(&mut self) -> Result<(), AdapterError> {
        self.settle_click()?;
        self.poll_choice(Instant::now())?;
        if let Some(run) = self.scroll.take() {
            self.emit_scroll(run)?;
        }
        self.flush_text()
    }

    fn on_button(
        &mut self,
        button: &'static str,
        x: i32,
        y: i32,
        at: Instant,
    ) -> Result<(), AdapterError> {
        if let Some(run) = self.scroll.take() {
            self.emit_scroll(run)?;
        }
        if button == "left" {
            self.drags.press(x, y, at);
        }
        let target = self.observer.at_point(x, y);
        let count = self.clicks.register(button, x, y, at);
        if self.typing_elsewhere(&target) {
            self.flush_text()?;
        }
        if !self.observer.keeps(&target) {
            tracing::debug!(
                x,
                y,
                listed = target.window.is_some(),
                "record: click outside the filter"
            );
            return Ok(());
        }
        let mut fields = self.observer.describe(&target);
        fields.insert("button".into(), json!(button));
        fields.insert("count".into(), json!(count));
        fields.insert("point".into(), json!({"x": x, "y": y}));
        self.click = Some(PendingClick::new(&mut self.observer, target, fields, at));
        Ok(())
    }

    /// A press on a command of a classic menu (a right-click menu): recorded
    /// as a click on a `menuitem` of the window that owns the menu, with
    /// `menu: "popup"`. The menu itself is gone by the time UI Automation
    /// could read it.
    fn on_menu_item(
        &mut self,
        name: &str,
        owner: isize,
        (x, y): (i32, i32),
        at: Instant,
    ) -> Result<(), AdapterError> {
        if let Some(run) = self.scroll.take() {
            self.emit_scroll(run)?;
        }
        self.flush_text()?;
        let target = self.observer.of_window(owner, (x, y));
        if !self.observer.keeps(&target) {
            return Ok(());
        }
        let mut fields = self.observer.describe(&target);
        fields.insert(
            "element".into(),
            json!({ "role": "menuitem", "name": name }),
        );
        fields.insert("menu".into(), json!("popup"));
        fields.insert("button".into(), json!("left"));
        fields.insert("count".into(), json!(1));
        fields.insert("point".into(), json!({"x": x, "y": y}));
        self.emit("click", at, fields)
    }

    /// A left release far from its press ends a drag, recorded after the
    /// press's `click` with the element under the release point: the cell a
    /// range selection ended on, or where something was dropped. That
    /// element is read now, so its name and state show what the drag left
    /// (a grid names the range it selected).
    fn on_release(&mut self, x: i32, y: i32, at: Instant) -> Result<(), AdapterError> {
        let Some(drag) = self.drags.release(x, y, at) else {
            return Ok(());
        };
        let (from_x, from_y) = drag.from;
        self.settle_click()?;
        let mut target = self.observer.at_point(x, y);
        for _ in 0..2 {
            if !self.observer.blank(&target) && target.window.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
            target = self.observer.at_point(x, y);
        }
        if !self.observer.keeps(&target) {
            return Ok(());
        }
        let mut fields = self.observer.describe(&target);
        fields.insert("from".into(), json!({"x": from_x, "y": from_y}));
        fields.insert("point".into(), json!({"x": x, "y": y}));
        let path = drag.path.iter().map(|&(px, py, ms)| json!([px, py, ms]));
        fields.insert("path".into(), Value::Array(path.collect()));
        self.emit("drag", at, fields)
    }

    fn on_wheel(
        &mut self,
        delta: i16,
        horizontal: bool,
        point: (i32, i32),
        at: Instant,
    ) -> Result<(), AdapterError> {
        let Some(step) = wheel_step(delta, horizontal) else {
            return Ok(());
        };
        let (x, y) = point;
        if self.scroll.extend(step, x, y, at) {
            return Ok(());
        }
        if let Some(run) = self.scroll.take() {
            self.emit_scroll(run)?;
        }
        let target = self.observer.at_point(x, y);
        if self.observer.keeps(&target) {
            let evidence = self.observer.describe(&target);
            self.scroll.open(step, point, at, evidence);
        }
        Ok(())
    }

    fn on_key(
        &mut self,
        vk: u32,
        scan: u32,
        down: bool,
        at: Instant,
        clipboard: u32,
    ) -> Result<(), AdapterError> {
        let class = classify(vk);
        if let KeyClass::Modifier(key) = class {
            self.modifiers.apply(key, down);
            return Ok(());
        }
        if !down {
            return Ok(());
        }
        if self.modifiers.is_shortcut() {
            self.flush_text()?;
            return self.emit_press(vk, at, clipboard);
        }
        match class {
            KeyClass::Commit => {
                self.flush_text()?;
                self.emit_press(vk, at, clipboard)
            }
            KeyClass::Navigation if self.pending.is_none() => self.emit_press(vk, at, clipboard),
            _ => self.on_typing(vk, scan, class, at),
        }
    }

    fn on_typing(
        &mut self,
        vk: u32,
        scan: u32,
        class: KeyClass,
        at: Instant,
    ) -> Result<(), AdapterError> {
        let target = self.observer.focused();
        if !self.observer.keeps(&target) {
            return self.flush_text();
        }
        if self.typing_elsewhere(&target) {
            self.flush_text()?;
        }
        let text = match class {
            KeyClass::Text => typed_text(vk, scan, &self.modifiers),
            _ => None,
        };
        if self.pending.is_none() {
            let evidence = self.observer.describe(&target);
            self.pending = Some(PendingText {
                target,
                evidence,
                typed: String::new(),
                exact: true,
                started: at,
                last_key: at,
                settled: None,
            });
        }
        let Some(pending) = self.pending.as_mut() else {
            return Ok(());
        };
        match (class, text) {
            (KeyClass::Text, Some(text)) => pending.typed.push_str(&text),
            (KeyClass::Text, None) => {}
            (KeyClass::Editing, _) if vk == VK_BACK && pending.typed.pop().is_some() => {}
            _ => pending.exact = false,
        }
        pending.last_key = at;
        pending.settled = None;
        Ok(())
    }

    fn typing_elsewhere(&self, target: &Target) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        match (pending.target.element.as_ref(), target.element.as_ref()) {
            (Some(typing), Some(next)) => !self.observer.same(typing, next),
            _ => true,
        }
    }

    fn flush_text(&mut self) -> Result<(), AdapterError> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let value = match pending.settled {
            Some(value) => value,
            None => pending
                .target
                .element
                .as_ref()
                .map_or(FieldValue::Unavailable, |element| {
                    self.observer.value(element)
                }),
        };
        let mut fields = pending.evidence;
        match value {
            FieldValue::Secret => {
                fields.insert("secure".into(), json!(true));
            }
            // Fork: keys typed into something that shows no text (a slide or a page an app
            // draws itself, read as an empty window or document) are what was typed.
            FieldValue::Text(text) if text.trim().is_empty() && !pending.typed.is_empty() => {
                fields.insert("typed".into(), json!(pending.typed));
                fields.insert("source".into(), json!("keys"));
            }
            FieldValue::Text(text) => {
                fields.insert("value".into(), json!(text));
                fields.insert("source".into(), json!("value"));
                if pending.exact && !pending.typed.is_empty() {
                    fields.insert("typed".into(), json!(pending.typed));
                }
            }
            FieldValue::Unavailable if pending.typed.is_empty() => return Ok(()),
            FieldValue::Unavailable => {
                fields.insert("typed".into(), json!(pending.typed));
                fields.insert("source".into(), json!("keys"));
            }
        }
        self.emit("type", pending.started, fields)
    }

    /// A key press. A copy (ctrl+c, ctrl+x) also records the shape of what it
    /// put on the clipboard, `clipboard` being the sequence number before it.
    fn emit_press(&mut self, vk: u32, at: Instant, clipboard: u32) -> Result<(), AdapterError> {
        let target = self.observer.focused();
        if !self.observer.keeps(&target) {
            return Ok(());
        }
        let mut fields = self.observer.describe(&target);
        let mut readable = false;
        // What the field holds as the key goes down: an autocompletion or a paste changes a field
        // without typing, and a replay needs the text as it was when, say, Ctrl+Enter ran it.
        if let Some(FieldValue::Text(text)) = target
            .element
            .as_ref()
            .map(|element| self.observer.value(element))
        {
            fields.insert("value_before".into(), json!(text));
            readable = true;
        }
        let mut pasted = false;
        match key_name(vk) {
            Some(name) => {
                let keys = self.modifiers.combo(&name);
                if is_copy(&keys) {
                    if let Some(shape) = copied(clipboard) {
                        fields.insert("clipboard".into(), shape);
                    }
                }
                pasted = is_paste(&keys);
                fields.insert("keys".into(), json!(keys))
            }
            None => fields.insert("vk".into(), json!(vk)),
        };
        self.emit("press", at, fields)?;
        // A paste into a field is followed like typing: the text it left is
        // recorded at the next flush (another field, a click, Stop), so a
        // replay sets that text instead of pasting whatever the clipboard holds.
        if pasted && readable {
            let evidence = self.observer.describe(&target);
            self.pending = Some(PendingText {
                target,
                evidence,
                typed: String::new(),
                exact: false,
                started: at,
                last_key: at,
                settled: None,
            });
        }
        Ok(())
    }

    fn emit_scroll(&mut self, run: ScrollRun<Map<String, Value>>) -> Result<(), AdapterError> {
        let mut fields = run.evidence;
        fields.insert("direction".into(), json!(run.direction));
        fields.insert("amount".into(), json!(run.notches));
        fields.insert("point".into(), json!({"x": run.x, "y": run.y}));
        self.emit("scroll", run.started, fields)
    }

    /// An action agent-desktop did while this recording runs (an agent's
    /// click through accessibility, which the hooks do not hear), recorded as
    /// the person's would be when it is in the session's scope.
    fn agent_action(&mut self, line: &Value, started_ms: u64) -> Result<(), AdapterError> {
        let kind = match line.get("kind").and_then(Value::as_str) {
            Some("click") => "click",
            Some("type") => "type",
            Some("select") => "select",
            Some("press") => "press",
            Some("scroll") => "scroll",
            // A page an agent drove through agent-browser (the DevTools proxy).
            Some("navigate") => "navigate",
            Some("upload") => "upload",
            _ => return Ok(()),
        };
        let Some(Value::Object(fields)) = line.get("fields").cloned() else {
            return Ok(());
        };
        let browser = fields.get("driver").and_then(Value::as_str) == Some("browser");
        if browser {
            // Its point is in the page, not on the screen, and its window is
            // the dedicated Chrome's: kept unless the recording is narrowed
            // to a window or to an app that is not a browser.
            if !self.observer.keeps_browser() {
                return Ok(());
            }
        } else {
            let spot = line.get("spot").and_then(Value::as_array).and_then(|xy| {
                let x = i32::try_from(xy.first()?.as_i64()?).ok()?;
                let y = i32::try_from(xy.get(1)?.as_i64()?).ok()?;
                Some((x, y))
            });
            if !self.observer.keeps_described(&fields, spot) {
                return Ok(());
            }
        }
        // What the person did before it comes first.
        self.settle_click()?;
        self.flush_text()?;
        let at_ms = line
            .get("at_ms")
            .and_then(Value::as_u64)
            .unwrap_or(started_ms);
        let at = self.started + Duration::from_millis(at_ms.saturating_sub(started_ms));
        self.emit(kind, at, fields)
    }

    fn emit(
        &mut self,
        kind: &'static str,
        at: Instant,
        fields: Map<String, Value>,
    ) -> Result<(), AdapterError> {
        (self.sink)(RecordedEvent {
            kind,
            elapsed_ms: millis_between(self.started, at),
            fields,
        })
    }
}

fn millis_between(start: Instant, at: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(start).as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "record_session_tests.rs"]
mod tests;
