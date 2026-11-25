// Part of Agent Record Adapter (see NOTICE).
// agent-browser as a library: runs upstream's own build script (it generates
// the DevTools protocol types into OUT_DIR) from upstream's folder, mirrors
// the dashboard folder upstream embeds, and writes upstream's main.rs as the
// library root (see crates/embed_root.rs).
include!("../../embed_root.rs");

mod upstream {
    include!("../../../vendor/agent-browser/cli/build.rs");

    pub fn run() {
        main();
    }
}

fn main() {
    let ours = std::env::current_dir().expect("the package folder");
    let cli = ours.join("../../../vendor/agent-browser/cli");
    std::env::set_current_dir(&cli).expect("vendor/agent-browser/cli");
    upstream::run();
    std::env::set_current_dir(&ours).expect("the package folder");
    mirror(&cli.join("../packages/dashboard/out"), &ours.join("../packages/dashboard/out"));
    embed_upstream_main(&cli.join("src/main.rs"), "browser_root.rs");
}

/// Copies `from` into `to` (files and folders), replacing what was there.
fn mirror(from: &std::path::Path, to: &std::path::Path) {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to).expect("the dashboard mirror");
    let Ok(entries) = std::fs::read_dir(from) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            mirror(&path, &target);
        } else {
            let _ = std::fs::copy(&path, &target);
        }
    }
}
