// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn an_odd_size_is_made_even_for_the_encoder() {
    let area = Area::of(-9, -9, 1937, 1039).expect("an area");
    assert_eq!(
        (area.x, area.y, area.width, area.height),
        (-9, -9, 1936, 1038)
    );
}

#[test]
fn a_huge_area_is_capped_and_a_tiny_one_refused() {
    let area = Area::of(0, 0, 9000, 9000).expect("an area");
    assert_eq!((area.width, area.height), (4096, 2304));
    assert!(Area::of(0, 0, 10, 600).is_none());
    assert!(Area::of(0, 0, -5, 600).is_none());
}

#[test]
fn only_list_windows_ids_name_a_window() {
    assert!(window_handle("w-12345").is_some());
    assert!(window_handle("w-0").is_none());
    assert!(window_handle("12345").is_none());
    assert!(window_handle("w-abc").is_none());
}
