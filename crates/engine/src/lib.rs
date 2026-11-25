// Part of Agent Record Adapter (see NOTICE).
//! The engine (upstream agent-desktop's own CLI and MCP server, from its
//! unmodified sources, with our adapter) as a library: agent-record.exe runs
//! it in a process of its own (`main`) instead of a separate executable.
#![allow(dead_code, unused_imports, unreachable_pub, clippy::all)]

include!(concat!(env!("OUT_DIR"), "/engine_root.rs"));
