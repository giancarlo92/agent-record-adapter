// Part of Agent Record Adapter (see NOTICE).
use super::*;

fn display(x: f64, width: f64) -> Rect {
    Rect {
        x,
        y: 0.0,
        width,
        height: 1080.0,
    }
}

#[test]
fn a_point_on_the_seam_between_two_displays_belongs_to_the_right_one() {
    let left = display(0.0, 1920.0);
    let right = display(1920.0, 1920.0);

    assert!(contains(&left, 1919, 500));
    assert!(!contains(&left, 1920, 500));
    assert!(contains(&right, 1920, 500));
}

#[test]
fn points_outside_a_display_are_refused() {
    let primary = display(0.0, 1920.0);

    assert!(!contains(&primary, -1, 10));
    assert!(!contains(&primary, 10, 1080));
    assert!(contains(&primary, 0, 0));
}
