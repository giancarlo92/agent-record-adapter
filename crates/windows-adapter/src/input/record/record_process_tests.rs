// Part of Agent Record Adapter (see NOTICE).
use super::image_path;
use std::path::Path;

#[test]
fn reads_the_path_of_a_running_process() {
    let expected = std::env::current_exe().expect("current exe");
    let got = image_path(std::process::id()).expect("own process path");
    // Packaged (MSIX) hosts redirect AppData, so the same file can be reached
    // by two spellings: compare the file, not the text of the path.
    let got = Path::new(&got);
    assert!(got.is_file(), "{} is not a file", got.display());
    assert_eq!(
        got.file_name().map(|n| n.to_ascii_lowercase()),
        expected.file_name().map(|n| n.to_ascii_lowercase())
    );
    assert_eq!(
        std::fs::metadata(got).expect("metadata").len(),
        std::fs::metadata(&expected).expect("metadata").len()
    );
}

#[test]
fn has_no_path_for_pid_zero() {
    assert_eq!(image_path(0), None);
}
