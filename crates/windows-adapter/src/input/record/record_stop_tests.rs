// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn the_session_stops_at_its_maximum_duration() {
    let start = Instant::now();
    let mut watch = StopWatch::new(start, 1000, None);

    assert_eq!(watch.reason(start + Duration::from_millis(999)), None);
    assert_eq!(
        watch.reason(start + Duration::from_millis(1000)),
        Some("max_duration")
    );
}

#[test]
fn the_session_ends_only_after_the_grace_period_that_follows_a_signal() {
    let start = Instant::now();
    let mut watch = StopWatch::new(start, 1000, None);

    assert_eq!(watch.finished(start + Duration::from_millis(1000)), None);
    assert_eq!(watch.finished(start + Duration::from_millis(1200)), None);
    assert_eq!(
        watch.finished(start + Duration::from_millis(1300)),
        Some("max_duration")
    );
}

#[test]
fn the_session_stops_when_the_stop_file_appears() {
    let path =
        std::env::temp_dir().join(format!("agent-desktop-record-stop-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let start = Instant::now();
    let mut watch = StopWatch::new(start, 60_000, Some(path.clone()));

    assert_eq!(watch.reason(start), None);
    std::fs::write(&path, b"").unwrap();
    assert_eq!(watch.reason(start + Duration::from_millis(50)), None);
    assert_eq!(
        watch.reason(start + Duration::from_millis(250)),
        Some("stop_file")
    );
    let _ = std::fs::remove_file(path);
}
