// Part of Agent Record Adapter (see NOTICE).
//! The points of `stroke --path`: `x,y;x,y;...` or a JSON array of `[x, y]`
//! or `[x, y, ms]` (a recorded drag's path).

use agent_desktop_core::AppError;

use crate::args::StrokeArgs;

const DEFAULT_STEP_MS: u64 = 16;

pub(crate) fn parse_points(args: &StrokeArgs) -> Result<Vec<(f64, f64, u64)>, AppError> {
    let text = match (&args.path, &args.path_file) {
        (Some(path), None) => path.clone(),
        (None, Some(file)) => std::fs::read_to_string(file).map_err(|error| {
            AppError::invalid_input(format!("Cannot read {}: {error}", file.display()))
        })?,
        _ => return Err(AppError::invalid_input("Provide --path or --path-file")),
    };
    let step = args.step_ms.unwrap_or(DEFAULT_STEP_MS);
    let (dx, dy) = match &args.offset {
        Some(offset) => parse_xy(offset)?,
        None => (0.0, 0.0),
    };
    let raw = if text.trim_start().starts_with('[') {
        json_points(&text)?
    } else {
        pair_points(&text)?
    };
    Ok(raw
        .into_iter()
        .enumerate()
        .map(|(index, (x, y, ms))| {
            (
                x + dx,
                y + dy,
                ms.unwrap_or(step.saturating_mul(index as u64)),
            )
        })
        .collect())
}

fn json_points(text: &str) -> Result<Vec<(f64, f64, Option<u64>)>, AppError> {
    let values: Vec<Vec<f64>> = serde_json::from_str(text).map_err(|error| {
        AppError::invalid_input(format!("The path is not a JSON array of points: {error}"))
    })?;
    values
        .into_iter()
        .map(|point| match point.as_slice() {
            [x, y] => Ok((*x, *y, None)),
            [x, y, ms] if *ms >= 0.0 => Ok((*x, *y, Some(*ms as u64))),
            _ => Err(AppError::invalid_input(
                "Each point is [x, y] or [x, y, ms]",
            )),
        })
        .collect()
}

fn pair_points(text: &str) -> Result<Vec<(f64, f64, Option<u64>)>, AppError> {
    text.split(';')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(|pair| parse_xy(pair).map(|(x, y)| (x, y, None)))
        .collect()
}

/// `x,y` as two numbers.
pub(crate) fn parse_xy(s: &str) -> Result<(f64, f64), AppError> {
    let (x_raw, y_raw) = s.split_once(',').ok_or_else(|| {
        AppError::invalid_input("Invalid coordinates. Expected format: x,y (e.g., 500,300)")
    })?;
    if y_raw.contains(',') {
        return Err(AppError::invalid_input(
            "Invalid coordinates. Expected exactly one comma",
        ));
    }
    let x: f64 = x_raw
        .trim()
        .parse()
        .map_err(|_| AppError::invalid_input("Invalid x coordinate"))?;
    let y: f64 = y_raw
        .trim()
        .parse()
        .map_err(|_| AppError::invalid_input("Invalid y coordinate"))?;
    if !x.is_finite() || !y.is_finite() {
        return Err(AppError::invalid_input(
            "Coordinates must be finite numbers",
        ));
    }
    Ok((x, y))
}

#[cfg(test)]
#[path = "stroke_tests.rs"]
mod tests;
