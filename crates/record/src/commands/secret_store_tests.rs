// Part of Agent Record Adapter (see NOTICE).
use super::*;

fn info(name: &str) -> SecretInfo {
    SecretInfo {
        name: name.into(),
        app: None,
        site: Some("example.com".into()),
        any_field: false,
        saved_ms: 1,
    }
}

#[test]
fn the_index_keeps_one_entry_per_name_in_order() {
    let entries = with_entry(vec![info("bank"), info("mail")], info("bank"));
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["bank", "mail"]);
    let entries = with_entry(entries, info("alpha"));
    assert_eq!(entries[0].name, "alpha");
}

#[test]
fn the_index_round_trips_and_holds_no_value() {
    let dir = std::env::temp_dir().join(format!("ad-secret-index-{}", std::process::id()));
    let path = index_path(&dir);
    write_index(&path, &[info("bank")]).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        !raw.contains("secret\""),
        "the index never has a value field: {raw}"
    );
    assert_eq!(read_index(&path), vec![info("bank")]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_or_broken_index_reads_as_empty() {
    let dir = std::env::temp_dir().join(format!("ad-secret-broken-{}", std::process::id()));
    let path = index_path(&dir);
    assert!(read_index(&path).is_empty());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"not json").unwrap();
    assert!(read_index(&path).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
