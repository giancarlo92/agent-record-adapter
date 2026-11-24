// Part of Agent Record Adapter (see NOTICE).
//! Naming a display the way a person does.
//!
//! A display's id (`monitor-262245`) is the window system's handle for it
//! and changes between runs, so `record --display` and `record start
//! --display` also take a display's `number` (its position in
//! `list-displays`, from 1) or `primary`.

use serde_json::{Value, json};

use crate::{AppError, DisplayInfo};

/// The display a selector names: `primary`, a number from 1, or an id.
pub fn resolve<'a>(
    selector: &str,
    displays: &'a [DisplayInfo],
) -> Result<&'a DisplayInfo, AppError> {
    let wanted = selector.trim();
    let found = if wanted.eq_ignore_ascii_case("primary") {
        displays.iter().find(|display| display.is_primary)
    } else if let Ok(number) = wanted.parse::<usize>() {
        number.checked_sub(1).and_then(|index| displays.get(index))
    } else {
        displays.iter().find(|display| display.id == wanted)
    };
    found.ok_or_else(|| {
        AppError::invalid_input_with_suggestion(
            format!("No display is '{wanted}' ({} connected)", displays.len()),
            "Pass primary, a number from list-displays (1, 2, ...) or one of its ids",
        )
    })
}

/// The displays as `list-displays` shows them: each with its `number`.
pub fn numbered(displays: &[DisplayInfo]) -> Value {
    Value::Array(
        displays
            .iter()
            .enumerate()
            .map(|(index, display)| {
                let mut entry = json!(display);
                entry["number"] = json!(index + 1);
                entry
            })
            .collect(),
    )
}

/// One display, as a recording's result names it.
pub fn describe(displays: &[DisplayInfo], display: &DisplayInfo) -> Value {
    let number = displays
        .iter()
        .position(|d| d.id == display.id)
        .map(|i| i + 1);
    json!({
        "number": number,
        "id": display.id,
        "primary": display.is_primary,
        "bounds": display.bounds,
    })
}

#[cfg(test)]
#[path = "record_display_tests.rs"]
mod tests;
