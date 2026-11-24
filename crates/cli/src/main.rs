// Part of Agent Record Adapter (see NOTICE).
//! `agent-record`: one CLI and MCP server over the unmodified agent-desktop
//! (desktop apps) and agent-browser (web pages), plus our commands.
//!
//! - `record`, `secret`, `stroke`, `menu`, `click-at`, `navigate` and
//!   `type w-<id>` run here, in process, with our background adapter;
//! - `browser ...` runs agent-browser against the dedicated background Chrome;
//! - `mcp` serves all of them as one MCP server (see `mcp.rs`);
//! - every other command is agent-desktop's, run by the engine
//!   (`agent-desktop.exe` next to this program: upstream's own CLI built with
//!   our adapter) with the same arguments, output and exit code.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod args;
mod catalog;
mod engine;
mod labels;
mod launch_cdp;
mod mcp;
mod run;
mod skills;
mod stroke_args;

use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};

fn main() -> ExitCode {
    // With a work desktop set, the whole command runs there (and so does
    // every engine or browser process it starts).
    #[cfg(target_os = "windows")]
    if let Some(code) = agent_record_windows::run_on_work_desktop() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // Debug output on stderr when asked for (a background recording's stderr
    // is its .log file).
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .try_init();
    }
    let mut argv: Vec<String> = std::env::args().collect();
    if launch_cdp::applies(&argv[1..]) {
        return launch_cdp::run(&argv[1..]);
    }
    // `launch --no-attach` of a program given a profile of its own: upstream
    // refuses while another instance runs; our adapter starts it separately.
    if first_command(&argv[1..]) == Some("launch")
        && argv
            .iter()
            .any(|arg| arg.contains("--user-data-dir") || arg == "--arg=-data")
    {
        argv.retain(|arg| arg != "--no-attach");
    }
    let rest = &argv[1..];

    match first_command(rest) {
        Some("mcp") => return mcp::serve(rest),
        Some("type") if run::is_window_type(rest) => return run::type_window(rest),
        Some("list-displays") if !rest.iter().any(|arg| arg == "--help" || arg == "-h") => {
            return run::list_displays();
        }
        Some("skills") => {
            if let Some(code) = skills::run(rest) {
                return code;
            }
        }
        _ => {}
    }

    let matches = match args::Cli::command().try_get_matches_from(&argv) {
        Ok(matches) => matches,
        Err(error) => {
            let help = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            if help && first_command(rest).is_none_or(run::is_ours) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            // Help or a parse error of one of ours is clap's; anything else
            // (an upstream command's own flags) is the engine's to judge.
            if first_command(rest).is_some_and(run::is_ours) {
                let _ = error.print();
                return ExitCode::from(2);
            }
            return engine::run(rest);
        }
    };
    let cli = match args::Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(2);
        }
    };
    match cli.command {
        None => engine::run(rest),
        Some(args::Commands::External(_)) => engine::run(rest),
        Some(args::Commands::Browser(browser)) => agent_record_browser::run(browser.args),
        Some(command) => run::ours(&cli.session, &cli.agent_id, cli.headed, command),
    }
}

/// The first argument that is not a global flag (or a global flag's value).
pub(crate) fn first_command(args: &[String]) -> Option<&str> {
    const WITH_VALUE: &[&str] = &[
        "--session",
        "--agent-id",
        "--trace",
        "--wait-for",
        "--wait-for-gone",
        "--wait-timeout",
        "--screenshot",
    ];
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if WITH_VALUE.contains(&arg.as_str()) {
            skip = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return Some(arg.as_str());
    }
    None
}
