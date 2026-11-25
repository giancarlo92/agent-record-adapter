// Part of Agent Record Adapter (see NOTICE).
//! Checks one call against the argument parsers of the programs installed
//! here, without running it:
//! - agent-desktop's commands: the engine itself parses the call with
//!   `--help` added at the end. Its parser rejects an unknown command, option
//!   or value before anything else, and with `--help` it only prints the
//!   command's help: nothing runs. The help's usage line then gives the
//!   required arguments;
//! - agent-record's own commands: their clap definition, in process;
//! - agent-browser's commands: its help (it does not refuse unknown options,
//!   so an option missing from the help is a warning, not an error).
//!
//! A value the script only knows when it runs is replaced by a placeholder;
//! when a placeholder is refused, other kinds are tried (a number, a word, a
//! ref, a window id), and a value still refused is reported as not checked.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::CommandFactory;

use super::extract::Arg;

const MARK: &str = "7531";
const PLACEHOLDERS: &[&str] = &["7531", "1", "x7531", "@e7531", "w-7531", "0.5"];
/// Global options that take a value, before the command.
const GLOBAL_WITH_VALUE: &[&str] = &[
    "--session",
    "--agent-id",
    "--trace",
    "--wait-for",
    "--wait-for-gone",
    "--wait-timeout",
    "--screenshot",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Front,
    Engine,
    Browser,
}

#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// Why the call could not be checked (completely).
    pub unchecked: Option<String>,
}

impl Outcome {
    fn error(message: impl Into<String>) -> Self {
        Self {
            errors: vec![message.into()],
            ..Self::default()
        }
    }

    fn unchecked(reason: impl Into<String>) -> Self {
        Self {
            unchecked: Some(reason.into()),
            ..Self::default()
        }
    }
}

enum Probe {
    Help(String),
    Invalid(String),
    NoAnswer(String),
}

pub(crate) struct Programs {
    pub front: PathBuf,
    pub engine: PathBuf,
    pub browser: Option<PathBuf>,
    engine_commands: Vec<String>,
    front_only: Vec<String>,
    probes: HashMap<Vec<String>, (bool, String)>,
    browser_help: HashMap<String, Option<String>>,
    browser_global: Option<String>,
}

/// Runs a program with no input and returns (success, stdout), or `None`
/// when it could not start or did not finish in time.
pub(crate) fn output(command: &mut Command, limit: Duration) -> Option<(bool, String)> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let start = Instant::now();
    let status = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break status;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let text = reader.join().unwrap_or_default();
    Some((status.success(), text))
}

fn clean(message: &str) -> String {
    let first = message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(message);
    first.trim().trim_start_matches("error:").trim().to_string()
}

impl Programs {
    pub(crate) fn find() -> Result<Self, String> {
        let front = std::env::current_exe()
            .map_err(|error| format!("Cannot locate agent-record: {error}"))?;
        let engine = crate::engine::path()?;
        let browser = Some(front.with_file_name(if cfg!(windows) {
            "agent-browser.exe"
        } else {
            "agent-browser"
        }))
        .filter(|path| path.is_file());
        let (ok, help) = output(Command::new(&engine).arg("--help"), Duration::from_secs(20))
            .ok_or_else(|| format!("The engine {} did not answer", engine.display()))?;
        if !ok {
            return Err(format!(
                "The engine {} did not show its help",
                engine.display()
            ));
        }
        let engine_commands = section_names(&help, "Commands:");
        let front_only = crate::args::Cli::command()
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .filter(|name| !name.is_empty() && !engine_commands.contains(name))
            .collect();
        Ok(Self {
            front,
            engine,
            browser,
            engine_commands,
            front_only,
            probes: HashMap::new(),
            browser_help: HashMap::new(),
            browser_global: None,
        })
    }

    /// Checks a call made to `kind` with these arguments.
    /// Whether a word is a command of agent-record or agent-desktop.
    pub(crate) fn is_command(&self, word: &str) -> bool {
        self.engine_commands.iter().any(|c| c == word) || self.front_only.iter().any(|c| c == word)
    }

    pub(crate) fn check(&mut self, kind: Kind, args: &[Arg], partial: bool) -> Outcome {
        let mut at = 0;
        while let Some(Arg::Lit(flag)) = args.get(at) {
            if !flag.starts_with('-') {
                break;
            }
            at += if GLOBAL_WITH_VALUE.contains(&flag.as_str()) {
                2
            } else {
                1
            };
        }
        let command = match args.get(at) {
            Some(Arg::Lit(command)) => command.clone(),
            Some(_) => return Outcome::unchecked("the command is chosen when the script runs"),
            None => return Outcome::unchecked("no command in the call"),
        };
        match kind {
            Kind::Browser => self.check_browser(&args[at..]),
            Kind::Engine if self.front_only.contains(&command) => Outcome::error(format!(
                "`{command}` is a command of agent-record, not of agent-desktop (the engine): run agent-record.exe instead of agent-desktop.exe"
            )),
            Kind::Engine if command == "type" && window_target(args.get(at + 1)) => Outcome::error(
                "`type w-<id>` (typing into a window) is agent-record's: agent-desktop.exe takes a ref after `type`; run agent-record.exe",
            ),
            Kind::Front if command == "browser" => self.check_browser(&args[at + 1..]),
            Kind::Front if self.front_only.contains(&command) => self.check_front(args, partial),
            Kind::Front if command == "type" && window_target(args.get(at + 1)) => {
                if args.len() > at + 2 {
                    Outcome::default()
                } else {
                    Outcome::error("`type w-<id>` needs the text to type")
                }
            }
            _ if !self.engine_commands.contains(&command) && command != "help" => Outcome::error(
                format!("`{command}` is not a command of agent-record or agent-desktop"),
            ),
            _ => self.check_engine(args, partial),
        }
    }

    fn probe_engine(&mut self, args: &[String]) -> Probe {
        let mut argv = args.to_vec();
        argv.push("--help".into());
        if !self.probes.contains_key(&argv) {
            let answer = output(
                Command::new(&self.engine).args(&argv),
                Duration::from_secs(20),
            );
            let Some(answer) = answer else {
                return Probe::NoAnswer("the engine did not answer".into());
            };
            self.probes.insert(argv.clone(), answer);
        }
        let Some((ok, text)) = self.probes.get(&argv) else {
            return Probe::NoAnswer("the engine did not answer".into());
        };
        if *ok {
            return Probe::Help(text.clone());
        }
        match serde_json::from_str::<serde_json::Value>(text.trim()) {
            Ok(envelope) => Probe::Invalid(clean(
                envelope["error"]["message"]
                    .as_str()
                    .unwrap_or("the engine refused the arguments"),
            )),
            Err(_) => Probe::NoAnswer("the engine gave no answer it could read".into()),
        }
    }

    fn check_engine(&mut self, args: &[Arg], partial: bool) -> Outcome {
        let rest = args.contains(&Arg::Rest);
        let given: Vec<Arg> = args
            .iter()
            .filter(|arg| **arg != Arg::Rest)
            .cloned()
            .collect();
        let attempt = |choice: &[usize]| -> Vec<String> {
            let mut slot = 0;
            given
                .iter()
                .map(|arg| match arg {
                    Arg::Lit(text) => text.clone(),
                    other => {
                        let value = PLACEHOLDERS[choice.get(slot).copied().unwrap_or(0)];
                        slot += 1;
                        match other {
                            Arg::Part(prefix) => format!("{prefix}{value}"),
                            _ => value.to_string(),
                        }
                    }
                })
                .collect()
        };
        let marked =
            |probe: &Probe| matches!(probe, Probe::Invalid(message) if placeholder_in(message));
        let slots = given
            .iter()
            .filter(|arg| matches!(arg, Arg::Dyn | Arg::Part(_)))
            .count();
        let choice = vec![0; slots];
        let mut argv = attempt(&choice);
        let mut probe = self.probe_engine(&argv);
        // A placeholder refused: try other kinds of value, one slot at a time.
        'slots: for slot in 0..slots {
            if !marked(&probe) {
                break;
            }
            for kind in 1..PLACEHOLDERS.len() {
                let mut tried = choice.clone();
                tried[slot] = kind;
                let tried_argv = attempt(&tried);
                let tried_probe = self.probe_engine(&tried_argv);
                if !marked(&tried_probe) {
                    argv = tried_argv;
                    probe = tried_probe;
                    break 'slots;
                }
            }
        }
        // Arguments passed on at run time may hold the value an option needs.
        if rest {
            if let Probe::Invalid(message) = &probe {
                if message.contains("a value is required") {
                    argv.push(MARK.into());
                    probe = self.probe_engine(&argv);
                }
            }
        }
        match probe {
            Probe::Help(_) if rest => Outcome::unchecked(
                "some arguments are passed on when the script runs: only the ones written were checked",
            ),
            Probe::Help(_) if partial => Outcome::default(),
            Probe::Help(help) => required(&help, &argv),
            Probe::Invalid(message) if placeholder_in(&message) => Outcome::unchecked(format!(
                "a value given when the script runs decides it: {}",
                hide_placeholders(&message)
            )),
            Probe::Invalid(message) if partial && message.contains("required") => {
                Outcome::default()
            }
            Probe::Invalid(message) if rest && message.contains("required") => Outcome::unchecked(
                "some arguments are passed on when the script runs: only the ones written were checked",
            ),
            Probe::Invalid(message) => Outcome::error(message),
            Probe::NoAnswer(reason) => Outcome::unchecked(reason),
        }
    }

    fn check_front(&mut self, args: &[Arg], partial: bool) -> Outcome {
        let rest = args.contains(&Arg::Rest);
        let given: Vec<&Arg> = args.iter().filter(|arg| **arg != Arg::Rest).collect();
        let mut last = String::new();
        for placeholder in PLACEHOLDERS {
            let argv =
                std::iter::once("agent-record".to_string()).chain(given.iter().map(
                    |arg| match arg {
                        Arg::Lit(text) => text.clone(),
                        Arg::Part(prefix) => format!("{prefix}{placeholder}"),
                        _ => placeholder.to_string(),
                    },
                ));
            match crate::args::Cli::command().try_get_matches_from(argv) {
                Ok(_) => return Outcome::default(),
                Err(error) => {
                    use clap::error::ErrorKind;
                    match error.kind() {
                        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                            return Outcome::default();
                        }
                        ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand
                            if partial =>
                        {
                            return Outcome::default();
                        }
                        ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand
                            if rest =>
                        {
                            return Outcome::unchecked(
                                "some arguments are passed on when the script runs: only the ones written were checked",
                            );
                        }
                        _ => {}
                    }
                    let message = clean(&error.to_string());
                    if !placeholder_in(&message) {
                        return Outcome::error(message);
                    }
                    last = message;
                }
            }
        }
        Outcome::unchecked(format!(
            "a value given when the script runs decides it: {}",
            hide_placeholders(&last)
        ))
    }

    fn browser_command_help(&mut self, command: &str) -> Option<String> {
        if let Some(found) = self.browser_help.get(command) {
            return found.clone();
        }
        let browser = self.browser.clone()?;
        let answer = output(
            Command::new(&browser).args([command, "--help"]),
            Duration::from_secs(20),
        );
        let help = answer.filter(|(ok, _)| *ok).map(|(_, text)| text);
        self.browser_help.insert(command.to_string(), help.clone());
        help
    }

    fn check_browser(&mut self, args: &[Arg]) -> Outcome {
        if self.browser.is_none() {
            return Outcome::unchecked("agent-browser is not installed next to agent-record");
        }
        let mut at = 0;
        while let Some(Arg::Lit(flag)) = args.get(at) {
            if !flag.starts_with('-') {
                break;
            }
            at += 1;
        }
        let command = match args.get(at) {
            Some(Arg::Lit(command)) => command.clone(),
            Some(_) => {
                return Outcome::unchecked(
                    "the agent-browser command is chosen when the script runs",
                );
            }
            None => return Outcome::unchecked("no agent-browser command in the call"),
        };
        let Some(help) = self.browser_command_help(&command) else {
            return Outcome::error(format!("`{command}` is not a command of agent-browser"));
        };
        if self.browser_global.is_none() {
            let browser = self.browser.clone();
            self.browser_global = browser
                .and_then(|path| output(Command::new(path).arg("--help"), Duration::from_secs(20)))
                .map(|(_, text)| text);
        }
        let global = self.browser_global.clone().unwrap_or_default();
        let mut outcome = Outcome::default();
        for arg in args {
            if let Arg::Lit(flag) = arg {
                let name = flag.split('=').next().unwrap_or(flag);
                if name.starts_with("--") && !help.contains(name) && !global.contains(name) {
                    outcome.warnings.push(format!(
                        "`{name}` is not in agent-browser's help for `{command}` (agent-browser does not refuse unknown options: it may be ignored)"
                    ));
                }
            }
        }
        if args.contains(&Arg::Rest) {
            outcome.unchecked = Some("some arguments are passed on when the script runs".into());
        }
        outcome
    }
}

/// The refusal names one of the values put in for those known at run time.
fn placeholder_in(message: &str) -> bool {
    message.contains(MARK)
        || PLACEHOLDERS
            .iter()
            .any(|p| message.contains(&format!("'{p}'")))
}

fn hide_placeholders(message: &str) -> String {
    let mut text = message.to_string();
    for placeholder in PLACEHOLDERS.iter().rev() {
        text = text.replace(&format!("'{placeholder}'"), "'…'");
    }
    text.replace(MARK, "…")
}

fn window_target(arg: Option<&Arg>) -> bool {
    matches!(arg, Some(Arg::Lit(target)) if target.starts_with("w-"))
}

/// The first word of each line in a help section (`Commands:`).
pub(crate) fn section_names(help: &str, title: &str) -> Vec<String> {
    help.lines()
        .skip_while(|line| line.trim() != title)
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect()
}

/// Options of a help text: (long or short name, takes a value).
pub(crate) fn options(help: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    for line in help.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            continue;
        }
        let head = trimmed.split("  ").next().unwrap_or(trimmed);
        let takes_value = head.contains('<');
        for part in head.split([',', ' ']) {
            let name = part.split(['=', '[']).next().unwrap_or(part);
            if name.starts_with('-') && name.len() > 1 {
                found.push((name.to_string(), takes_value));
            }
        }
    }
    found
}

/// Checks the required arguments the help's usage line names.
pub(crate) fn required(help: &str, argv: &[String]) -> Outcome {
    let Some(usage) = help
        .lines()
        .find_map(|line| line.trim().strip_prefix("Usage:"))
    else {
        return Outcome::default();
    };
    let words: Vec<&str> = usage.split_whitespace().collect();
    // The program, then the command path (plain words).
    let path_len = words
        .iter()
        .skip(1)
        .take_while(|word| {
            word.chars().all(|ch| ch.is_alphanumeric() || ch == '-') && !word.starts_with('-')
        })
        .count();
    let mut required_positionals = Vec::new();
    let mut required_options = Vec::new();
    let mut k = 1 + path_len;
    while k < words.len() {
        let word = words[k];
        // A value that can also come from a pipe (`[VALUE]` with `--stdin`):
        // one of the two is needed, so without `--stdin` the value is.
        let from_pipe = word.starts_with('[')
            && !word.starts_with("[OPTIONS]")
            && k + 1 == words.len()
            && help.contains("--stdin")
            && !argv.iter().any(|arg| arg == "--stdin");
        if from_pipe || (word.starts_with('<') && !word.starts_with("<--")) {
            required_positionals.push(word.trim_end_matches("...").to_string());
        } else if word.starts_with("--") {
            required_options.push(word.to_string());
            if words.get(k + 1).is_some_and(|value| value.starts_with('<')) {
                k += 1;
            }
        }
        k += 1;
    }
    let options = options(help);
    // The given arguments after the command path.
    let command_at = argv
        .iter()
        .position(|arg| !arg.starts_with('-') && words.get(1).is_some_and(|w| w == arg));
    let Some(command_at) = command_at else {
        return Outcome::default();
    };
    let mut given_positionals = 0;
    let mut i = command_at + path_len;
    let mut only_positionals = false;
    let mut present: Vec<&str> = Vec::new();
    while i < argv.len() {
        let arg = argv[i].as_str();
        if only_positionals {
            given_positionals += 1;
        } else if arg == "--" {
            only_positionals = true;
        } else if arg.starts_with('-') && arg.parse::<f64>().is_err() {
            let name = arg.split('=').next().unwrap_or(arg);
            present.push(name);
            let takes = options
                .iter()
                .any(|(option, value)| option == name && *value);
            if takes && !arg.contains('=') {
                i += 1;
            }
        } else {
            given_positionals += 1;
        }
        i += 1;
    }
    let mut outcome = Outcome::default();
    if given_positionals < required_positionals.len() {
        let missing = required_positionals[given_positionals..].join(" ");
        outcome.errors.push(format!("missing {missing}"));
    }
    for option in required_options {
        if !present.contains(&option.as_str()) {
            outcome.errors.push(format!("missing {option}"));
        }
    }
    outcome
}

/// Where a helper copy finds its program: runs only the copy's own lookup
/// (`agentDesktop()`), which asks a program for its help and nothing else.
pub(crate) fn helper_program(helper: &Path) -> Result<Result<PathBuf, String>, String> {
    const LOOKUP: &str = "const m = await import(process.argv.at(-1)); \
        if (typeof m.agentDesktop !== 'function') { console.log(JSON.stringify({ kind: 'none' })); } \
        else { try { console.log(JSON.stringify({ kind: 'ok', program: m.agentDesktop() })); } \
        catch (e) { console.log(JSON.stringify({ kind: 'error', message: String((e && e.message) || e) })); } }";
    let url = file_url(helper);
    let answer = output(
        Command::new("node").args(["--input-type=module", "-e", LOOKUP, &url]),
        Duration::from_secs(60),
    )
    .ok_or_else(|| "Node.js did not run (is it installed?)".to_string())?;
    let line = answer
        .1
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'));
    let Some(found) = line.and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    else {
        return Err("Node.js could not load it".into());
    };
    match found["kind"].as_str() {
        Some("ok") => Ok(Ok(PathBuf::from(
            found["program"].as_str().unwrap_or_default(),
        ))),
        Some("error") => Ok(Err(found["message"]
            .as_str()
            .unwrap_or_default()
            .to_string())),
        _ => Err("it has no agentDesktop() lookup".into()),
    }
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from(if text.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for byte in text.bytes() {
        let plain = byte.is_ascii_alphanumeric() || b"/:-_.~".contains(&byte);
        if plain {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}
