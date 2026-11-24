// Part of Agent Record Adapter (see NOTICE).
use super::*;

fn at(x: f64, y: f64, at_ms: u64) -> StrokePoint {
    StrokePoint {
        point: Point { x, y },
        at_ms,
    }
}

#[test]
fn a_stroke_of_two_points_or_more_in_time_order_is_valid() {
    let stroke = StrokeParams {
        points: vec![at(10.0, 10.0, 0), at(40.0, 10.0, 30), at(40.0, 40.0, 60)],
    };
    assert!(stroke.validate().is_ok());
    assert_eq!(stroke.duration_ms(), 60);
}

#[test]
fn a_single_point_is_not_a_stroke() {
    let stroke = StrokeParams {
        points: vec![at(10.0, 10.0, 0)],
    };
    assert_eq!(stroke.validate().unwrap_err().code, ErrorCode::InvalidArgs);
}

#[test]
fn times_that_go_backwards_are_refused() {
    let stroke = StrokeParams {
        points: vec![at(10.0, 10.0, 50), at(20.0, 10.0, 10)],
    };
    assert_eq!(stroke.validate().unwrap_err().code, ErrorCode::InvalidArgs);
}

#[test]
fn a_stroke_longer_than_a_minute_is_refused() {
    let stroke = StrokeParams {
        points: vec![at(10.0, 10.0, 0), at(20.0, 10.0, MAX_STROKE_MS + 1)],
    };
    assert_eq!(stroke.validate().unwrap_err().code, ErrorCode::InvalidArgs);
}
