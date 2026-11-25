// Part of Agent Record Adapter (see NOTICE).
//
// Shared by the build scripts of the crates that carry an upstream program
// inside agent-record.exe (crates/engine, crates/web-engine), through
// `include!`. It writes, into OUT_DIR, upstream's unmodified `main.rs` as the
// root of a library: its `mod x;` lines point to upstream's own files by
// absolute path, its `fn main` is made public so agent-record can call it,
// and its inner attributes (which `include!` does not take) are left out.
// Nothing under vendor/ is changed or copied besides this one generated file.

/// Writes `<OUT_DIR>/<out_name>` from `main_rs` (upstream's main.rs).
fn embed_upstream_main(main_rs: &std::path::Path, out_name: &str) {
    let main_rs = main_rs
        .canonicalize()
        .unwrap_or_else(|error| panic!("{}: {error}", main_rs.display()));
    let src = main_rs.parent().expect("main.rs has a folder");
    println!("cargo:rerun-if-changed={}", main_rs.display());
    let text = std::fs::read_to_string(&main_rs).expect("upstream main.rs");
    let mut out = String::new();
    let mut pending_path = false;
    let mut made_public = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let top = line.len() == trimmed.len();
        // Inner attributes and inner doc comments belong to a crate root that
        // `include!` cannot have; the including lib.rs sets its own.
        if top && (trimmed.starts_with("#![") || trimmed.starts_with("//!")) {
            continue;
        }
        if top && trimmed.starts_with("#[path") {
            // An attribute giving a module's file relative to main.rs: made
            // absolute, as the generated file lives elsewhere.
            // (No let-chains: this file is also built as edition 2021.)
            let quoted = trimmed.find('"').and_then(|start| {
                trimmed[start + 1..]
                    .find('"')
                    .map(|end| &trimmed[start + 1..start + 1 + end])
            });
            if let Some(relative) = quoted {
                out.push_str(&format!("#[path = {:?}]\n", path_text(&src.join(relative))));
                pending_path = true;
                continue;
            }
        }
        let module = if top { module_name(trimmed) } else { None };
        if let Some(name) = module {
            if !pending_path {
                let file = src.join(format!("{name}.rs"));
                let folder = src.join(name).join("mod.rs");
                let found = if file.is_file() { file } else { folder };
                out.push_str(&format!("#[path = {:?}]\n", path_text(&found)));
            }
            pending_path = false;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if top && !trimmed.starts_with("#[") {
            pending_path = false;
        }
        if top && !made_public && trimmed.starts_with("fn main(") {
            out.push_str("pub ");
            made_public = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    assert!(made_public, "no `fn main(` at the top of {}", main_rs.display());
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out_dir.join(out_name), out).expect("write the generated root");
}

/// `x` for a line `mod x;` or `pub mod x;` (not an inline `mod x { ... }`).
fn module_name(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    let rest = rest.strip_prefix("mod ")?.trim();
    let name = rest.strip_suffix(';')?.trim();
    name.chars()
        .all(|c| c.is_alphanumeric() || c == '_')
        .then_some(name)
}

fn path_text(path: &std::path::Path) -> String {
    // A verbatim prefix from canonicalize is fine for rustc; plain is nicer.
    let text = path.to_string_lossy().to_string();
    text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
}
