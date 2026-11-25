// Part of Agent Record Adapter (see NOTICE).
//! A date written into a web date field (`<input type="date">` or `"month"`)
//! of a Chromium browser.
//!
//! Chromium shows such a field as spin-button segments (day, month, year, in
//! the order of the browser's language) and ignores a value written through
//! accessibility, to the field or to a segment. A person clicks the first
//! segment and types the digits; Chromium moves to the next segment when one
//! is complete. That is done here, as window messages behind the person's
//! windows. Each segment is known by its range (day 1..31, month 1..12, year
//! beyond), never by its name, so any language and any order works. The
//! value asked for is the field's own format: `YYYY-MM-DD` (or `YYYY-MM`).

use agent_desktop_core::{ActionStep, AdapterError, Deadline, ErrorCode, StepMechanism};
use uiautomation::UIElement;
use uiautomation::patterns::{UIRangeValuePattern, UIValuePattern};
use uiautomation::types::{ControlType, TreeScope};

use crate::tree::element::UIAElement;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    Day,
    Month,
    Year,
}

/// Which part a segment is, by the largest value it takes.
fn part_of(maximum: f64) -> Option<Part> {
    if maximum == 31.0 {
        Some(Part::Day)
    } else if maximum == 12.0 {
        Some(Part::Month)
    } else if maximum >= 1000.0 {
        Some(Part::Year)
    } else {
        None
    }
}

/// The digits each part gets for a value in the field's own format
/// (`YYYY-MM-DD`, or `YYYY-MM` for a month field); `None` for another format.
fn digits(value: &str, parts: &[Part]) -> Option<Vec<String>> {
    let pieces: Vec<&str> = value.trim().split('-').collect();
    let number = |text: &str, width: usize| {
        (text.len() == width && text.chars().all(|c| c.is_ascii_digit())).then(|| text.to_string())
    };
    let (year, month, day) = match pieces.as_slice() {
        [year, month, day] => (number(year, 4)?, number(month, 2)?, Some(number(day, 2)?)),
        [year, month] => (number(year, 4)?, number(month, 2)?, None),
        _ => return None,
    };
    parts
        .iter()
        .map(|part| match part {
            Part::Year => Some(year.clone()),
            Part::Month => Some(month.clone()),
            Part::Day => day.clone(),
        })
        .collect()
}

/// The segments of a Chromium date field, in the order shown, with the part
/// each one is; `None` when `element` is not such a field.
fn segments_of(
    element: &UIElement,
    client: &uiautomation::UIAutomation,
) -> Option<(Vec<UIElement>, Vec<Part>)> {
    if element.get_framework_id().ok().as_deref()
        != Some(crate::actions::chromium_input::CHROMIUM_FRAMEWORK)
        || element.get_control_type().ok() != Some(ControlType::Edit)
    {
        return None;
    }
    let condition = client
        .create_property_condition(
            uiautomation::types::UIProperty::ControlType,
            uiautomation::variants::Variant::from(ControlType::Spinner as i32),
            None,
        )
        .ok()?;
    let segments: Vec<UIElement> = element
        .find_all(TreeScope::Descendants, &condition)
        .unwrap_or_default();
    let mut parts = Vec::new();
    for segment in &segments {
        let maximum = segment
            .get_pattern::<UIRangeValuePattern>()
            .and_then(|range| range.get_maximum())
            .ok();
        // A time or week segment: not a plain date field.
        parts.push(maximum.and_then(part_of)?);
    }
    let plain_date = matches!(parts.len(), 2 | 3)
        && parts.contains(&Part::Year)
        && parts.contains(&Part::Month)
        && (parts.len() == 2 || parts.contains(&Part::Day));
    plain_date.then_some((segments, parts))
}

/// The date a Chromium date field shows, in its own format (`YYYY-MM-DD`),
/// from its segments: Chromium may leave the field's own value unreported
/// for a while (on a work desktop it does not report it at all) after the
/// date was typed, while each segment shows its number at once. `None` when
/// the element is not such a field or a segment is empty.
pub(crate) fn shown_date(element: &UIElement) -> Option<String> {
    let client = uiautomation::UIAutomation::new().ok()?;
    let (segments, parts) = segments_of(element, &client)?;
    let mut year = None;
    let mut month = None;
    let mut day = None;
    for (segment, part) in segments.iter().zip(&parts) {
        let shown = segment
            .get_pattern::<UIRangeValuePattern>()
            .and_then(|range| range.get_value())
            .ok()
            .filter(|value| *value >= 1.0)? as u32;
        match part {
            Part::Year => year = Some(shown),
            Part::Month => month = Some(shown),
            Part::Day => day = Some(shown),
        }
    }
    let (year, month) = (year?, month?);
    Some(match day {
        Some(day) => format!("{year:04}-{month:02}-{day:02}"),
        None => format!("{year:04}-{month:02}"),
    })
}

/// Writes `value` into a Chromium date field; `None` when `element` is not
/// one or `value` is not in its format (the caller goes on to its other ways).
pub(crate) fn set(
    element: &UIAElement,
    value: &str,
    deadline: Deadline,
) -> Result<Option<Vec<ActionStep>>, AdapterError> {
    let Ok(client) = uiautomation::UIAutomation::new() else {
        return Ok(None);
    };
    let Some((segments, parts)) = segments_of(&element.0, &client) else {
        return Ok(None);
    };
    let Some(typed) = digits(value, &parts) else {
        return Ok(None);
    };
    let Some(top) = crate::actions::chromium_input::top_window(&element.0, &client) else {
        return Ok(None);
    };
    // The first segment takes the keyboard focus as a person's click gives it.
    if crate::actions::chromium_input::pointer_click_control(
        &UIAElement::from(segments[0].clone()),
        deadline,
    )?
    .is_none()
    {
        return Ok(None);
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    let mut keys = Vec::new();
    for digit in typed.concat().encode_utf16() {
        keys.extend(crate::actions::posted_keys::key_messages(
            digit,
            Some(digit),
            false,
        ));
    }
    if !crate::actions::chromium_input::send_keys(top, &keys) {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "The browser did not take the date's digits",
        ));
    }
    // Each segment shows its number at once; the field reports its value in
    // its own format a moment later. Either one proves the date arrived.
    let wanted = value.trim();
    let numbers: Vec<f64> = typed
        .iter()
        .map(|digits| digits.parse::<f64>().unwrap_or(-1.0))
        .collect();
    let end = std::time::Instant::now() + std::time::Duration::from_millis(4000);
    let mut now = String::new();
    while std::time::Instant::now() < end && !deadline.is_expired() {
        now = element
            .0
            .get_pattern::<UIValuePattern>()
            .and_then(|pattern| pattern.get_value())
            .unwrap_or_default();
        let segments_hold = segments.iter().zip(&numbers).all(|(segment, number)| {
            segment
                .get_pattern::<UIRangeValuePattern>()
                .and_then(|range| range.get_value())
                .is_ok_and(|shown| shown == *number)
        });
        if now == wanted || segments_hold {
            let step = ActionStep::succeeded("DateField.TypeSegments")
                .with_mechanism(StepMechanism::SemanticApi)
                .with_verified(true);
            return Ok(Some(vec![step]));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(AdapterError::new(
        ErrorCode::ActionFailed,
        format!("The date field holds '{now}' after typing {wanted}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_follow_the_order_of_the_segments() {
        let day_first = [Part::Day, Part::Month, Part::Year];
        assert_eq!(
            digits("1990-05-14", &day_first).unwrap().concat(),
            "14051990"
        );
        let month_first = [Part::Month, Part::Day, Part::Year];
        assert_eq!(
            digits("1990-05-14", &month_first).unwrap().concat(),
            "05141990"
        );
        let year_first = [Part::Year, Part::Month, Part::Day];
        assert_eq!(
            digits("1990-05-14", &year_first).unwrap().concat(),
            "19900514"
        );
        assert_eq!(
            digits("1990-05", &[Part::Month, Part::Year])
                .unwrap()
                .concat(),
            "051990"
        );
        assert!(digits("14/05/1990", &day_first).is_none());
        assert!(digits("1990-5-14", &day_first).is_none());
        assert_eq!(part_of(31.0), Some(Part::Day));
        assert_eq!(part_of(275760.0), Some(Part::Year));
        assert_eq!(part_of(59.0), None);
    }
}
