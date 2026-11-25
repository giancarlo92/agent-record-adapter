// Part of Agent Record Adapter (see NOTICE).
//! What the bar does: record and stop (writing the script), list the recordings, play one.
//! How it looks is in `view.rs`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::script;
use crate::steps::{self, Action};
use crate::texts::{Texts, texts};
use crate::tool::{self, Target};

pub const SCRIPT: &str = "script.mjs";

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Panel {
    None,
    Settings,
    List,
}

pub struct Active {
    pub id: String,
    pub dir: PathBuf,
    pub started: Instant,
    pub checked: Instant,
    /// Stop was pressed: agent-record is finishing the files (a video takes a few seconds).
    pub stopping: bool,
    /// A status call is on its way.
    pub asking: bool,
}

/// What a call to agent-record answered. Every call runs on its own thread, so the bar never
/// waits for one (listing windows, or stopping while a video is written, can take seconds).
pub enum Done {
    Targets(Vec<Target>),
    Started(Result<String, String>, PathBuf),
    Stopped(Result<(), String>, PathBuf),
    Status(String, bool),
    /// The latest published version, if npm answered.
    Latest(Option<String>),
    /// How the update went.
    Updated(Result<(), String>),
}

/// A recording in the list.
pub struct Item {
    pub dir: PathBuf,
    pub when: String,
    pub steps: usize,
    pub irreversible: Vec<String>,
    /// Its steps after the first: place in the step list, what it does, and for a click whether it
    /// asks before it runs.
    pub lines: Vec<(usize, String, Option<bool>)>,
    /// Where its last play stopped, read from its log (it outlives the app).
    pub failed: Option<String>,
}

/// A recording playing, and what it last said.
pub struct Playing {
    pub dir: PathBuf,
    pub lines: Receiver<String>,
    pub last: String,
}

pub struct App {
    pub t: &'static Texts,
    pub panel: Panel,
    pub folder: String,
    pub video: bool,
    pub targets: Vec<Target>,
    pub target: usize,
    pub choosing_target: bool,
    pub active: Option<Active>,
    pub items: Vec<Item>,
    pub playing: Option<Playing>,
    pub results: Vec<(PathBuf, String)>,
    pub confirm: Option<PathBuf>,
    /// The recording whose steps are shown, to mark the ones that ask before they run.
    pub steps_open: Option<PathBuf>,
    pub error: Option<String>,
    pub hidden_from_capture: bool,
    /// Input events held back for the next frame (see `view::steady_clicks`).
    pub held_input: Vec<egui::Event>,
    /// The recording whose repair prompt was just copied, and when (the row says so for a while).
    pub copied: Option<(PathBuf, Instant)>,
    /// The recording whose repair button the pointer is on (its row shows what it does).
    pub hovered_repair: Option<PathBuf>,
    /// Record was pressed and agent-record has not answered yet.
    pub starting: bool,
    /// The bar closes once the recording being stopped is saved.
    pub close_when_saved: bool,
    listing: bool,
    /// This app's version, and a newer one when there is one.
    pub version: String,
    pub newer: Option<String>,
    /// The update was started: the app closes and the installer opens it again.
    pub updating: bool,
    /// Asking npm for the latest version right now.
    pub checking: bool,
    /// The last time npm was asked, it did not answer.
    pub offline: bool,
    checked_update: Option<Instant>,
    ctx: egui::Context,
    done: (Sender<Done>, Receiver<Done>),
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        crate::look::fonts(ctx);
        egui_extras::install_image_loaders(ctx);
        let t = texts();
        let (folder, video) = tool::load_settings();
        let mut app = Self {
            t,
            panel: Panel::None,
            folder: folder.unwrap_or_else(|| default_folder().display().to_string()),
            video,
            targets: Vec::new(),
            target: 0,
            choosing_target: false,
            active: None,
            items: Vec::new(),
            playing: None,
            results: Vec::new(),
            confirm: None,
            steps_open: None,
            error: tool::agent_desktop()
                .is_none()
                .then(|| t.no_tool.to_owned()),
            hidden_from_capture: false,
            held_input: Vec::new(),
            copied: None,
            hovered_repair: None,
            starting: false,
            close_when_saved: false,
            listing: false,
            version: crate::update::current(),
            newer: None,
            updating: false,
            checking: false,
            offline: false,
            checked_update: None,
            ctx: ctx.clone(),
            done: channel(),
        };
        app.list_targets();
        app.refresh();
        if let Some(error) = crate::update::failure() {
            app.error = Some(format!("{} {error}", t.update_failed));
        }
        app
    }

    /// Asks npm for the latest version, in the background (at start, then every six hours).
    fn check_update(&mut self) {
        if self
            .checked_update
            .is_some_and(|at| at.elapsed() < Duration::from_secs(6 * 60 * 60))
        {
            return;
        }
        self.check_now();
    }

    /// Asks npm now (the version row in Settings, when there is no update to install).
    pub fn check_now(&mut self) {
        if self.checking || self.updating {
            return;
        }
        self.checking = true;
        self.checked_update = Some(Instant::now());
        self.spawn(|| Done::Latest(crate::update::latest()));
    }

    /// Installs the newer version, in the background; when it is in, the new app opens and this
    /// one closes. Not while it records or plays.
    pub fn update(&mut self) {
        let Some(version) = self.newer.clone() else {
            return;
        };
        if self.updating || self.active.is_some() || self.starting || self.playing.is_some() {
            return;
        }
        self.updating = true;
        self.error = None;
        self.spawn(move || Done::Updated(crate::update::run(&version)));
    }

    /// Why agent-record cannot run: missing, or not the one this app was
    /// installed with (its SHA-256 is checked once while the file is unchanged).
    fn tool_problem(&self) -> Option<String> {
        match tool::checked_agent_record() {
            Ok(_) => None,
            Err(_) if tool::agent_desktop().is_none() => Some(self.t.no_tool.to_owned()),
            Err(_) => Some(self.t.changed_tool.to_owned()),
        }
    }

    /// Runs `call` on its own thread; its answer is taken in `poll`.
    fn spawn(&self, call: impl FnOnce() -> Done + Send + 'static) {
        let send = self.done.0.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = send.send(call());
            ctx.request_repaint();
        });
    }

    /// Lists the displays and windows again, in the background: the list shows at once with what
    /// was there before and updates when the answer comes.
    pub fn list_targets(&mut self) {
        if self.listing {
            return;
        }
        self.listing = true;
        let word = self.t.display;
        self.spawn(move || Done::Targets(tool::targets(word)));
    }

    pub fn refresh(&mut self) {
        let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&self.folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().join("recording.jsonl").exists())
            .map(|entry| {
                (
                    entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH),
                    entry.path(),
                )
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        self.items = found
            .into_iter()
            .take(6)
            .map(|(_, dir)| self.item(dir))
            .collect();
    }

    fn item(&self, dir: PathBuf) -> Item {
        let text = std::fs::read_to_string(dir.join("recording.jsonl")).unwrap_or_default();
        let steps = steps::parse_dir(&dir, &text);
        let irreversible = steps
            .iter()
            .filter(|s| {
                matches!(
                    s.action,
                    Action::Click {
                        irreversible: true,
                        ..
                    }
                )
            })
            .map(script::describe)
            .collect();
        // Every step after the first (which opens the app or the page), and for a click whether
        // it asks before it runs: the person can change that in the list.
        let lines = steps
            .iter()
            .enumerate()
            .skip(1)
            .map(|(index, step)| {
                let ask = match step.action {
                    Action::Click { irreversible, .. } => Some(irreversible),
                    _ => None,
                };
                (index, script::describe(step), ask)
            })
            .collect();
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Item {
            when: when(&name, self.t.today),
            steps: steps.len().saturating_sub(1),
            irreversible,
            lines,
            failed: crate::repair::failure(&dir).map(|line| format!("{} {line}", self.t.failed)),
            dir,
        }
    }

    /// Turns "ask before it runs" on or off for one click of a recording, keeping the others as
    /// they are, and saves it next to the recording.
    pub fn toggle_ask_before(&mut self, dir: &Path, step: usize) {
        let Some(at) = self.items.iter().position(|i| i.dir == dir) else {
            return;
        };
        let mut marked: Vec<usize> = self.items[at]
            .lines
            .iter()
            .filter(|(_, _, ask)| *ask == Some(true))
            .map(|(index, _, _)| *index)
            .collect();
        if let Some(found) = marked.iter().position(|index| *index == step) {
            marked.remove(found);
        } else {
            marked.push(step);
            marked.sort_unstable();
        }
        if steps::write_ask_before(dir, &marked).is_ok() {
            self.items[at] = self.item(dir.to_path_buf());
        }
    }

    pub fn start(&mut self) {
        if self.starting || self.active.is_some() {
            return;
        }
        let Some(target) = self.targets.get(self.target).cloned() else {
            if let Some(problem) = self.tool_problem() {
                self.error = Some(problem);
            }
            self.list_targets();
            return;
        };
        tool::save_settings(&self.folder, self.video);
        let dir = Path::new(&self.folder).join(folder_name());
        let video = self.video;
        self.starting = true;
        self.panel = Panel::None;
        self.error = None;
        self.spawn(move || Done::Started(tool::start(&target, video, &dir), dir));
    }

    /// Stops the recording; its script is written once agent-record has saved it.
    pub fn stop(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.stopping {
            return;
        }
        active.stopping = true;
        let (id, dir) = (active.id.clone(), active.dir.clone());
        self.spawn(move || Done::Stopped(tool::stop(&id).map(|_| ()), dir));
    }

    fn answered(&mut self, done: Done) {
        match done {
            Done::Targets(targets) => {
                self.listing = false;
                // No display at all: agent-record did not run (a changed one is
                // refused), so say why instead of a Record button that does nothing.
                if targets.is_empty()
                    && let Some(problem) = self.tool_problem()
                {
                    self.error = Some(problem);
                }
                // Keep what was chosen when the list changes under it.
                let chosen = self.targets.get(self.target).map(|t| t.args.clone());
                self.target = chosen
                    .and_then(|args| targets.iter().position(|t| t.args == args))
                    .unwrap_or(0);
                self.targets = targets;
            }
            Done::Started(Ok(id), dir) => {
                self.starting = false;
                self.active = Some(Active {
                    id,
                    dir,
                    started: Instant::now(),
                    checked: Instant::now(),
                    stopping: false,
                    asking: false,
                });
            }
            Done::Started(Err(error), _) => {
                self.starting = false;
                self.error = Some(error);
            }
            Done::Stopped(result, dir) => {
                self.active = None;
                match result {
                    Err(error) if !dir.join("recording.jsonl").exists() => {
                        self.error = Some(error);
                    }
                    _ => {
                        write_script(&dir);
                        self.refresh();
                        self.panel = Panel::List;
                    }
                }
                if self.close_when_saved {
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Done::Updated(result) => {
                self.updating = false;
                match result.and_then(|()| crate::update::reopen()) {
                    Ok(()) => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(error) => self.error = Some(format!("{} {error}", self.t.update_failed)),
                }
            }
            Done::Latest(latest) => {
                self.checking = false;
                self.offline = latest.is_none();
                self.newer = latest.filter(|v| crate::update::newer(v, &self.version));
            }
            Done::Status(id, running) => {
                let Some(active) = self.active.as_mut().filter(|a| a.id == id) else {
                    return;
                };
                active.asking = false;
                // Stopped somewhere else (a script, the MCP server): save it the same way.
                if !running {
                    self.stop();
                }
            }
        }
    }

    /// Plays a recording, after asking when one of its steps cannot be undone.
    pub fn ask_to_play(&mut self, dir: &Path) {
        let risky = self
            .items
            .iter()
            .find(|i| i.dir == dir)
            .is_some_and(|i| !i.irreversible.is_empty());
        if risky && self.confirm.as_deref() != Some(dir) {
            self.confirm = Some(dir.to_path_buf());
            return;
        }
        self.confirm = None;
        write_script(dir);
        let values = if risky {
            vec!["--yes".to_owned()]
        } else {
            Vec::new()
        };
        let (send, lines) = channel();
        tool::run_script(dir, SCRIPT, values, send);
        self.results.retain(|(d, _)| d != dir);
        self.playing = Some(Playing {
            dir: dir.to_path_buf(),
            lines,
            last: self.t.playing.to_owned(),
        });
    }

    pub fn choose_folder(&mut self) {
        if let Some(folder) = tool::pick_folder(&self.folder) {
            self.folder = folder;
            tool::save_settings(&self.folder, self.video);
            self.refresh();
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.done.1.try_recv() {
            self.answered(done);
        }
        self.check_update();
        let mut ask = None;
        if let Some(active) = self.active.as_mut() {
            ctx.request_repaint_after(Duration::from_millis(250));
            if !active.stopping
                && !active.asking
                && active.checked.elapsed() > Duration::from_secs(2)
            {
                active.checked = Instant::now();
                active.asking = true;
                ask = Some(active.id.clone());
            }
        }
        if let Some(id) = ask {
            self.spawn(move || {
                let running = tool::running(&id);
                Done::Status(id, running)
            });
        }
        let mut finished = None;
        if let Some(playing) = self.playing.as_mut() {
            ctx.request_repaint_after(Duration::from_millis(200));
            while let Ok(line) = playing.lines.try_recv() {
                if let Some(code) = line.strip_prefix('\u{0}') {
                    let result = if code == "0" {
                        self.t.played.to_owned()
                    } else {
                        format!("{} {}", self.t.failed, playing.last)
                    };
                    finished = Some((playing.dir.clone(), result));
                    break;
                }
                if !line.trim().is_empty() {
                    playing.last = line.trim().to_owned();
                }
            }
        }
        if let Some(result) = finished {
            self.playing = None;
            self.results.push(result);
            self.refresh();
        }
        if self
            .copied
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(4))
        {
            self.copied = None;
        }
        if self.copied.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

/// While it records, the bar is left out of every capture, so it is not in the video.
pub fn hide_from_capture(frame: &eframe::Frame, hide: bool) {
    // Tests that capture the bar while it records set this; nobody else should.
    if std::env::var_os("DESKTOP_RECORDER_CAPTURABLE").is_some() {
        return;
    }
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        const WDA_NONE: u32 = 0;
        const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
        if let Ok(handle) = frame.window_handle()
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            let affinity = if hide {
                WDA_EXCLUDEFROMCAPTURE
            } else {
                WDA_NONE
            };
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity(
                    win32.hwnd.get() as _,
                    affinity,
                )
            };
        }
    }
    #[cfg(not(windows))]
    let _ = (frame, hide);
}

/// Where the app notes what it wrote, to tell its own script from one changed since.
const WRITTEN: &str = ".desktop-recorder.json";

/// Writes the script of a recording (every value a parameter whose default is what was recorded,
/// so it plays as recorded) and the helper file it imports. A script changed since the app wrote
/// it (repaired by hand or by an agent) is kept as it is, with its helper file.
pub fn write_script(dir: &Path) {
    let current = std::fs::read_to_string(dir.join(SCRIPT)).ok();
    let noted: Option<String> = std::fs::read_to_string(dir.join(WRITTEN))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v["script"].as_str().map(str::to_owned));
    let ours = match (&current, &noted) {
        (None, _) => true,
        (Some(text), Some(hash)) => fingerprint(text) == *hash,
        // Written by a version that noted nothing: its first line says the app made it.
        (Some(text), None) => text
            .lines()
            .nth(1)
            .is_some_and(|line| line.starts_with("// Made by Desktop Recorder")),
    };
    if !ours {
        return;
    }
    let text = std::fs::read_to_string(dir.join("recording.jsonl")).unwrap_or_default();
    let steps = steps::parse_dir(dir, &text);
    let params = script::candidates(&steps);
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let script = script::render(&name, "recording.jsonl", &steps, &params);
    let _ = std::fs::write(dir.join(SCRIPT), &script);
    let _ = std::fs::write(dir.join(script::LIBRARY_NAME), script::LIBRARY);
    let _ = std::fs::write(
        dir.join(WRITTEN),
        serde_json::json!({ "script": fingerprint(&script) }).to_string(),
    );
}

/// A short fingerprint of a text (FNV-1a, 64 bits), the same in every version of the app.
fn fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn default_folder() -> PathBuf {
    tool::app_home().join("Documents").join("Desktop Recorder")
}

/// "Today 10:42" or "18/11 10:42", from a folder named "Recording 2026-11-18 10.42.07".
fn when(name: &str, today_word: &str) -> String {
    let mut parts = name.split_whitespace().skip(1);
    let (Some(date), Some(time)) = (parts.next(), parts.next()) else {
        return name.to_owned();
    };
    let hm = time.replace('.', ":").chars().take(5).collect::<String>();
    let ymd: Vec<&str> = date.split('-').collect();
    if ymd.len() != 3 {
        return name.to_owned();
    }
    if folder_name().split_whitespace().nth(1) == Some(date) {
        format!("{today_word} {hm}")
    } else {
        format!("{}/{} {hm}", ymd[2], ymd[1])
    }
}

/// "Recording 2026-11-19 10.42.07", in local time.
fn folder_name() -> String {
    #[cfg(windows)]
    {
        let mut now = unsafe { std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut now) };
        format!(
            "Recording {:04}-{:02}-{:02} {:02}.{:02}.{:02}",
            now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond
        )
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("Recording {secs}")
    }
}
