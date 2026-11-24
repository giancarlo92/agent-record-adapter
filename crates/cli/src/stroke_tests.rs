// Part of Agent Record Adapter (see NOTICE).
use super::*;

fn args(path: &str) -> StrokeArgs {
    StrokeArgs {
        path: Some(path.into()),
        path_file: None,
        step_ms: None,
        offset: None,
    }
}

#[test]
fn pairs_get_evenly_spaced_times() {
    let points = parse_points(&args("10,20; 30,40;50,60")).unwrap();
    assert_eq!(
        points,
        vec![(10.0, 20.0, 0), (30.0, 40.0, 16), (50.0, 60.0, 32)]
    );
}

#[test]
fn a_recorded_path_keeps_its_times() {
    let points = parse_points(&args("[[10,20,0],[30,40,120],[50,60,250]]")).unwrap();
    assert_eq!(
        points,
        vec![(10.0, 20.0, 0), (30.0, 40.0, 120), (50.0, 60.0, 250)]
    );
}

#[test]
fn an_offset_moves_every_point() {
    let mut stroke = args("[[10,20],[30,40]]");
    stroke.offset = Some("-10,5".into());
    stroke.step_ms = Some(50);
    let points = parse_points(&stroke).unwrap();
    assert_eq!(points, vec![(0.0, 25.0, 0), (20.0, 45.0, 50)]);
}

#[test]
fn a_path_is_required() {
    let mut stroke = args("");
    stroke.path = None;
    assert!(parse_points(&stroke).is_err());
}

#[test]
fn a_point_with_one_number_is_refused() {
    assert!(parse_points(&args("[[10],[30,40]]")).is_err());
}
