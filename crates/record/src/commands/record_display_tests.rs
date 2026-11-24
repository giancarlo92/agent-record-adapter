// Part of Agent Record Adapter (see NOTICE).
use super::*;
use crate::Rect;

fn display(id: &str, x: f64, primary: bool) -> DisplayInfo {
    DisplayInfo {
        id: id.into(),
        bounds: Rect {
            x,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        },
        is_primary: primary,
        scale: 1.25,
    }
}

fn three() -> Vec<DisplayInfo> {
    vec![
        display("monitor-2", 1920.0, false),
        display("monitor-1", 0.0, true),
        display("monitor-3", 3840.0, false),
    ]
}

#[test]
fn a_display_is_named_by_primary_its_number_or_its_id() {
    let displays = three();
    assert_eq!(resolve("primary", &displays).unwrap().id, "monitor-1");
    assert_eq!(resolve("1", &displays).unwrap().id, "monitor-2");
    assert_eq!(resolve(" 3 ", &displays).unwrap().id, "monitor-3");
    assert_eq!(resolve("monitor-3", &displays).unwrap().id, "monitor-3");
}

#[test]
fn a_number_or_id_that_names_no_display_is_refused() {
    let displays = three();
    assert!(resolve("0", &displays).is_err());
    assert!(resolve("4", &displays).is_err());
    assert!(resolve("monitor-9", &displays).is_err());
}

#[test]
fn list_displays_numbers_them_in_order() {
    let listed = numbered(&three());
    assert_eq!(listed[0]["number"], 1);
    assert_eq!(listed[2]["id"], "monitor-3");
    assert_eq!(listed[2]["number"], 3);
    let named = describe(&three(), &three()[1]);
    assert_eq!(named["number"], 2);
    assert_eq!(named["primary"], true);
}
