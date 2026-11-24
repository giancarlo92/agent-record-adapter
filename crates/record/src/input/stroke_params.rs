// Part of Agent Record Adapter (see NOTICE).
use serde::{Deserialize, Serialize};

use crate::{AdapterError, ErrorCode, Point};

pub const MAX_STROKE_POINTS: usize = 4_096;
pub const MAX_STROKE_MS: u64 = 60_000;

/// One point of a stroke: where the pointer is and when, in milliseconds
/// after the button went down.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrokePoint {
    pub point: Point,
    pub at_ms: u64,
}

/// A path the pointer follows with the left button held, such as a line
/// drawn on a canvas. The button goes down at the first point and up at the
/// last.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrokeParams {
    pub points: Vec<StrokePoint>,
}

impl StrokeParams {
    pub fn validate(&self) -> Result<(), AdapterError> {
        let count = self.points.len();
        if !(2..=MAX_STROKE_POINTS).contains(&count) {
            return Err(invalid(format!(
                "A stroke needs 2 to {MAX_STROKE_POINTS} points, got {count}"
            )));
        }
        let mut previous = 0;
        for stroke_point in &self.points {
            stroke_point.point.validate()?;
            if stroke_point.at_ms < previous {
                return Err(invalid("Stroke times must not go backwards".into()));
            }
            previous = stroke_point.at_ms;
        }
        if previous > MAX_STROKE_MS {
            return Err(invalid(format!(
                "A stroke must not last more than {MAX_STROKE_MS}ms"
            )));
        }
        Ok(())
    }

    /// Milliseconds from the press to the release.
    pub fn duration_ms(&self) -> u64 {
        self.points.last().map_or(0, |last| last.at_ms)
    }
}

fn invalid(message: String) -> AdapterError {
    AdapterError::new(ErrorCode::InvalidArgs, message)
}

#[cfg(test)]
#[path = "stroke_params_tests.rs"]
mod tests;
