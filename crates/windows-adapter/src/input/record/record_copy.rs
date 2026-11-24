// Part of Agent Record Adapter (see NOTICE).
//! What a copy shortcut put on the clipboard, as a shape only.
//!
//! A `press` of ctrl+c or ctrl+x says which keys were pressed but not how
//! much they copied: one cell and a whole table look the same. Once the
//! application has handled the key, the clipboard's text is measured in lines
//! and tab-separated columns. The text itself is never recorded.

use std::time::{Duration, Instant};

use agent_desktop_core::{ClipboardContent, ClipboardFormat, Deadline};
use serde_json::{Value, json};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

use crate::input::clipboard::get_clipboard_content;

const COPY_WAIT: Duration = Duration::from_millis(500);
const COPY_POLL: Duration = Duration::from_millis(20);
const READ_TIMEOUT_MS: u64 = 500;

/// Whether `keys` (a recorded combo such as `ctrl+c`) copies or cuts.
pub(super) fn is_copy(keys: &str) -> bool {
    let mut parts = keys.split('+');
    let key = parts.next_back();
    let ctrl = keys.split('+').any(|part| part == "ctrl");
    ctrl && matches!(key, Some("c" | "x"))
}

/// Whether a key combination pastes: Ctrl+V or Shift+Insert, with Shift
/// allowed beside Ctrl (paste as plain text).
pub(super) fn is_paste(keys: &str) -> bool {
    let parts: Vec<&str> = keys.split('+').collect();
    let (key, held) = parts.split_last().map_or(("", &[][..]), |(k, h)| (*k, h));
    let only = |allowed: &[&str]| held.iter().all(|part| allowed.contains(part));
    match key {
        "v" => held.contains(&"ctrl") && only(&["ctrl", "shift"]),
        "insert" => held == ["shift"],
        _ => false,
    }
}

/// The shape of the clipboard text once it changed from the sequence number
/// `before`, or `None` when the copy put nothing new, or no text, there.
pub(super) fn copied(before: u32) -> Option<Value> {
    let until = Instant::now() + COPY_WAIT;
    while unsafe { GetClipboardSequenceNumber() } == before {
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(COPY_POLL);
    }
    let deadline = Deadline::after(READ_TIMEOUT_MS).ok()?;
    match get_clipboard_content(ClipboardFormat::Text, deadline) {
        Ok(Some(ClipboardContent::Text(text))) => Some(shape(&text)),
        _ => None,
    }
}

/// The lines, the most tab-separated columns in a line, and the characters
/// of `text`. A final line break does not count as an empty last line.
pub(super) fn shape(text: &str) -> Value {
    let body = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let lines: Vec<&str> = if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').collect()
    };
    let columns = lines
        .iter()
        .map(|line| line.split('\t').count())
        .max()
        .unwrap_or(0);
    json!({
        "lines": lines.len(),
        "columns": columns,
        "characters": text.chars().count(),
    })
}

#[cfg(test)]
#[path = "record_copy_tests.rs"]
mod tests;
