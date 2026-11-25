// Part of Agent Record Adapter (see NOTICE).
// The engine as a library too: upstream's unmodified main.rs as its root (see
// crates/embed_root.rs), so agent-record.exe carries the engine inside.
include!("../embed_root.rs");

fn main() {
    embed_upstream_main(
        std::path::Path::new("../../vendor/agent-desktop/src/main.rs"),
        "engine_root.rs",
    );
}
