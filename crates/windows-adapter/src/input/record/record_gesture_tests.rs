// Part of Agent Record Adapter (see NOTICE).
use super::*;

fn rule() -> ClickRule {
    ClickRule {
        window: Duration::from_millis(500),
        slop_x: 4,
        slop_y: 4,
    }
}

#[test]
fn a_second_press_in_time_and_place_is_a_double_click() {
    let start = Instant::now();
    let mut counter = ClickCounter::new(rule());

    assert_eq!(counter.register("left", 100, 100, start), 1);
    assert_eq!(
        counter.register("left", 102, 99, start + Duration::from_millis(200)),
        2
    );
    assert_eq!(
        counter.register("left", 102, 99, start + Duration::from_millis(400)),
        3
    );
    assert_eq!(
        counter.register("left", 102, 99, start + Duration::from_millis(600)),
        3
    );
}

#[test]
fn a_late_far_or_other_button_press_starts_over() {
    let start = Instant::now();
    let mut counter = ClickCounter::new(rule());
    counter.register("left", 100, 100, start);

    assert_eq!(
        counter.register("left", 100, 100, start + Duration::from_millis(900)),
        1
    );
    assert_eq!(
        counter.register("left", 120, 100, start + Duration::from_millis(1000)),
        1
    );
    assert_eq!(
        counter.register("right", 120, 100, start + Duration::from_millis(1100)),
        1
    );
}

fn step(direction: &'static str, notches: u32) -> WheelStep {
    WheelStep { direction, notches }
}

#[test]
fn notches_join_a_run_until_the_direction_or_place_changes() {
    let start = Instant::now();
    let mut scroll: ScrollAccumulator<&str> =
        ScrollAccumulator::new(Duration::from_millis(600), 30);
    scroll.open(step("down", 1), (10, 10), start, "list");

    assert!(scroll.extend(step("down", 1), 12, 15, start + Duration::from_millis(100)));
    assert!(scroll.extend(step("down", 3), 12, 15, start + Duration::from_millis(200)));
    assert!(!scroll.extend(step("up", 1), 12, 15, start + Duration::from_millis(300)));
    assert!(!scroll.extend(step("down", 1), 300, 15, start + Duration::from_millis(300)));

    let run = scroll.take().expect("open run");
    assert_eq!(run.notches, 5);
    assert_eq!(run.direction, "down");
    assert_eq!(run.evidence, "list");
    assert!(scroll.take().is_none());
}

#[test]
fn an_idle_run_closes_only_after_the_gap() {
    let start = Instant::now();
    let mut scroll: ScrollAccumulator<()> = ScrollAccumulator::new(Duration::from_millis(600), 30);
    scroll.open(step("up", 1), (0, 0), start, ());

    assert!(
        scroll
            .take_if_idle(start + Duration::from_millis(500))
            .is_none()
    );
    assert!(
        scroll
            .take_if_idle(start + Duration::from_millis(700))
            .is_some()
    );
}

#[test]
fn wheel_deltas_map_to_directions_and_notches() {
    assert_eq!(wheel_step(120, false), Some(step("up", 1)));
    assert_eq!(wheel_step(-360, false), Some(step("down", 3)));
    assert_eq!(wheel_step(120, true), Some(step("right", 1)));
    assert_eq!(wheel_step(-240, true), Some(step("left", 2)));
    assert_eq!(wheel_step(30, false), Some(step("up", 1)));
    assert_eq!(wheel_step(0, false), None);
}

#[test]
fn a_release_away_from_the_press_is_a_drag() {
    let start = Instant::now();
    let mut drag = DragTracker::new((4, 4));
    drag.press(100, 100, start);
    assert_eq!(
        drag.release(103, 98, start),
        None,
        "a click wobble is not a drag"
    );
    drag.press(100, 100, start);
    let done = drag
        .release(400, 260, start + Duration::from_millis(300))
        .expect("a drag");
    assert_eq!(done.from, (100, 100));
    assert_eq!(done.path, vec![(100, 100, 0), (400, 260, 300)]);
    assert_eq!(
        drag.release(500, 500, start),
        None,
        "a release without its press is ignored"
    );
}

#[test]
fn a_drag_keeps_the_way_the_pointer_went() {
    let start = Instant::now();
    let mut drag = DragTracker::new((4, 4));
    drag.press(10, 10, start);
    drag.motion(10, 10, start + Duration::from_millis(5));
    drag.motion(60, 10, start + Duration::from_millis(50));
    drag.motion(60, 60, start + Duration::from_millis(100));
    let done = drag
        .release(10, 60, start + Duration::from_millis(150))
        .expect("a drag");
    assert_eq!(
        done.path,
        vec![(10, 10, 0), (60, 10, 50), (60, 60, 100), (10, 60, 150)],
        "a stroke keeps its corners, and a move to the same pixel adds nothing"
    );
}

#[test]
fn a_stroke_that_comes_back_to_its_start_is_still_a_drag() {
    let start = Instant::now();
    let mut drag = DragTracker::new((4, 4));
    drag.press(10, 10, start);
    drag.motion(80, 40, start + Duration::from_millis(40));
    let done = drag.release(11, 10, start + Duration::from_millis(90));
    assert!(
        done.is_some(),
        "a closed loop such as an O ends where it began"
    );
}

#[test]
fn a_long_stroke_is_thinned_but_keeps_its_ends() {
    let start = Instant::now();
    let mut drag = DragTracker::new((4, 4));
    drag.press(0, 0, start);
    for i in 1..=5000 {
        drag.motion(i, 0, start + Duration::from_millis(i as u64));
    }
    let done = drag
        .release(5001, 0, start + Duration::from_millis(5001))
        .expect("a drag");
    assert!(done.path.len() <= 2049, "{} points", done.path.len());
    assert_eq!(done.path.first(), Some(&(0, 0, 0)));
    assert_eq!(done.path.last(), Some(&(5001, 0, 5001)));
}
