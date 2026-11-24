// Part of Agent Record Adapter (see NOTICE).
use super::*;

#[test]
fn only_ctrl_c_and_ctrl_x_are_copies() {
    assert!(is_copy("ctrl+c"));
    assert!(is_copy("ctrl+shift+c"));
    assert!(is_copy("ctrl+x"));
    assert!(!is_copy("c"));
    assert!(!is_copy("ctrl+v"));
    assert!(!is_copy("alt+c"));
}

#[test]
fn a_table_is_measured_in_lines_and_columns() {
    let table = "1\tAna\tana@x.com\r\n2\tLuis\tluis@x.com\r\n3\tEva\teva@x.com\r\n";
    assert_eq!(shape(table)["lines"], 3);
    assert_eq!(shape(table)["columns"], 3);
    assert_eq!(shape("1762621")["lines"], 1);
    assert_eq!(shape("1762621")["columns"], 1);
    assert_eq!(shape("")["lines"], 0);
}

#[test]
fn a_paste_is_ctrl_v_or_shift_insert() {
    assert!(is_paste("ctrl+v"));
    assert!(is_paste("ctrl+shift+v"));
    assert!(is_paste("shift+insert"));
    assert!(!is_paste("v"));
    assert!(!is_paste("ctrl+alt+v"));
    assert!(!is_paste("win+v"));
    assert!(!is_paste("ctrl+insert"));
}
