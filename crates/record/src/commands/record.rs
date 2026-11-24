// Part of Agent Record Adapter (see NOTICE).
use crate::{
    AdapterError, AppError, ErrorCode, RecordRequest, RecordSummary, RecordedEvent,
    adapter::PlatformAdapter,
};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// The `format` tag of the first line of every recording file.
pub const RECORDING_FORMAT: &str = "agent-desktop.recording/v1";
/// The `kind` of the line an adapter emits once it observes input. Actions
/// before it are not recorded, so a caller starts the demonstration after it
/// appears. It is not counted as an event.
pub const RECORDING_READY_KIND: &str = "recording_ready";
/// How long a session runs when the caller sets no `--max-duration-ms`.
pub const DEFAULT_RECORD_DURATION_MS: u64 = 600_000;
/// The longest session a caller can ask for.
pub const MAX_RECORD_DURATION_MS: u64 = 3_600_000;

pub struct RecordArgs {
    pub out: PathBuf,
    pub app: Option<String>,
    pub window_id: Option<String>,
    pub display: Option<String>,
    pub max_duration_ms: Option<u64>,
    pub stop_file: Option<PathBuf>,
    pub include_injected: bool,
    /// The MP4 video to write beside the events; `None` records no video.
    pub video: Option<PathBuf>,
    /// Show the on-screen recording indicator, with its Stop button.
    pub indicator: bool,
}

/// Streams the session to `args.out` as JSON lines: a `recording_started`
/// header, one line per observed action, and a `recording_stopped` trailer.
/// Each line is flushed as it is written, so an interrupted process still
/// leaves every action it saw on disk.
pub fn execute(args: RecordArgs, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let mut request = record_request(&args)?;
    let display = match &args.display {
        Some(selector) => match adapter.list_displays(crate::Deadline::standard()?) {
            Ok(displays) => {
                let chosen = crate::commands::record_display::resolve(selector, &displays)?;
                request.display = Some(chosen.id.clone());
                Some(crate::commands::record_display::describe(&displays, chosen))
            }
            Err(_) => None,
        },
        None => None,
    };
    let mut writer = open_recording(&args.out)?;
    let started = write_line(&mut writer, &header(&request));
    if let Err(error) = started {
        return Err(unwritable(&args.out, error));
    }
    let mut events: u64 = 0;
    let outcome = adapter.record_input(&request, &mut |event: RecordedEvent| {
        write_line(&mut writer, &event.to_json()).map_err(sink_failure)?;
        if event.kind != RECORDING_READY_KIND {
            events += 1;
        }
        Ok(())
    });
    let summary = match outcome {
        Ok(summary) => summary,
        Err(error) => {
            drop(writer);
            if events == 0 {
                let _ = std::fs::remove_file(&args.out);
            }
            return Err(error.into());
        }
    };
    write_line(&mut writer, &trailer(&summary, events)).map_err(|e| unwritable(&args.out, e))?;
    let mut result = json!({
        "out": args.out.to_string_lossy(),
        "format": RECORDING_FORMAT,
        "events": events,
        "duration_ms": summary.duration_ms,
        "stop_reason": summary.stop_reason,
    });
    if let Some(video) = &summary.video {
        result["video"] = json!(video);
    }
    if let Some(display) = display {
        result["display"] = display;
    }
    Ok(result)
}

fn record_request(args: &RecordArgs) -> Result<RecordRequest, AppError> {
    let max_duration_ms = args.max_duration_ms.unwrap_or(DEFAULT_RECORD_DURATION_MS);
    if max_duration_ms == 0 || max_duration_ms > MAX_RECORD_DURATION_MS {
        return Err(AppError::invalid_input(format!(
            "--max-duration-ms must be between 1 and {MAX_RECORD_DURATION_MS}"
        )));
    }
    let scopes = [&args.app, &args.window_id, &args.display]
        .iter()
        .filter(|scope| scope.is_some())
        .count();
    if scopes > 1 {
        return Err(AppError::invalid_input(
            "record accepts one of --app, --window-id or --display",
        ));
    }
    if let Some(stop_file) = &args.stop_file {
        if stop_file.exists() {
            return Err(AppError::invalid_input_with_suggestion(
                format!("The stop file '{}' already exists", stop_file.display()),
                "Delete it before recording; the session stops when it appears",
            ));
        }
        if stop_file == &args.out {
            return Err(AppError::invalid_input(
                "--stop-file must differ from --out",
            ));
        }
    }
    if let Some(video) = &args.video {
        validate_video_path(video, args)?;
    }
    Ok(RecordRequest {
        app: args.app.clone(),
        window_id: args.window_id.clone(),
        display: args.display.clone(),
        max_duration_ms,
        stop_file: args.stop_file.clone(),
        include_injected: args.include_injected,
        video: args.video.clone(),
        indicator: args.indicator,
    })
}

/// The video's path, when `--video-out` is not given: the recording's, with
/// the `.mp4` extension.
pub fn default_video_path(out: &Path) -> PathBuf {
    out.with_extension("mp4")
}

fn validate_video_path(video: &Path, args: &RecordArgs) -> Result<(), AppError> {
    let mp4 = video
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
    if !mp4 {
        return Err(AppError::invalid_input(format!(
            "The video '{}' must be an .mp4 file",
            video.display()
        )));
    }
    if video == args.out || args.stop_file.as_deref() == Some(video) {
        return Err(AppError::invalid_input(
            "The video must differ from --out and --stop-file",
        ));
    }
    if crate::refs::is_symlink(video) {
        return Err(AppError::invalid_input(format!(
            "Refusing to write the video through the symbolic link '{}'",
            video.display()
        )));
    }
    if let Some(parent) = video.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::refs::unwritable_output("the video", video, AppError::Io(e)))?;
    }
    Ok(())
}

fn open_recording(path: &Path) -> Result<BufWriter<File>, AppError> {
    if crate::refs::is_symlink(path) {
        return Err(AppError::invalid_input(format!(
            "Refusing to write the recording through the symbolic link '{}'",
            path.display()
        )));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| unwritable(path, e))?;
    }
    let file = File::create(path).map_err(|e| unwritable(path, e))?;
    Ok(BufWriter::new(file))
}

fn header(request: &RecordRequest) -> Value {
    let mut line = json!({
        "kind": "recording_started",
        "format": RECORDING_FORMAT,
        "os": std::env::consts::OS,
        "max_duration_ms": request.max_duration_ms,
    });
    if let Some(app) = &request.app {
        line["app"] = json!(app);
    }
    if let Some(window_id) = &request.window_id {
        line["window_id"] = json!(window_id);
    }
    if let Some(display) = &request.display {
        line["display"] = json!(display);
    }
    if let Some(video) = &request.video {
        line["video"] = json!(video.to_string_lossy());
    }
    line
}

fn trailer(summary: &RecordSummary, events: u64) -> Value {
    let mut line = json!({
        "kind": "recording_stopped",
        "t_ms": summary.duration_ms,
        "stop_reason": summary.stop_reason,
        "events": events,
    });
    if let Some(video) = &summary.video {
        line["video"] = json!(video);
    }
    line
}

fn write_line(writer: &mut BufWriter<File>, line: &Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, line)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn sink_failure(error: std::io::Error) -> AdapterError {
    AdapterError::new(
        ErrorCode::Internal,
        format!("Could not append to the recording: {error}"),
    )
}

fn unwritable(path: &Path, error: std::io::Error) -> AppError {
    crate::refs::unwritable_output("the recording", path, AppError::Io(error))
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
