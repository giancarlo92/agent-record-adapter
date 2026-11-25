// Part of Agent Record Adapter (see NOTICE).
//! agent-browser (from its unmodified sources) as a library: agent-record.exe
//! runs it in a process of its own (`main`) instead of a separate executable.
#![allow(dead_code, unused_imports, unused_variables, unused_mut, clippy::all)]

include!(concat!(env!("OUT_DIR"), "/browser_root.rs"));
