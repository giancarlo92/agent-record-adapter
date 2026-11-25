// Part of Agent Record Adapter (see NOTICE).
//! `check-scripts`: checks scripts that call agent-record, agent-desktop or
//! agent-browser against the programs installed here, without running them.
//!
//! What it checks, and how:
//! - every call written in a script (JavaScript, TypeScript, Python, sh,
//!   PowerShell, cmd), against the installed programs' own argument parsers
//!   (see `probe.rs`): the program, the command, its options and their values,
//!   and the required arguments;
//! - that a program path written in a script exists;
//! - every desktop-script.mjs copy a script uses: the program its own lookup
//!   finds here (it is run with Node.js only for that lookup), and its calls;
//! - that the passwords a script fills by name are saved (names only).
//!
//! What it cannot check: whether the app or page still has the elements the
//! script uses, and what the script does once it runs. Running the script
//! answers that.

pub(crate) mod extract;
mod probe;
pub(crate) mod shell;
pub(crate) mod tokens;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::args::CheckScriptsArgs;
use extract::{Arg, Call, Program, Source, program_named};
use probe::{Kind, Programs};
use shell::Shell;

const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    "coverage",
];

#[derive(Clone, Copy)]
pub(crate) enum Language {
    Code { python: bool },
    Script(Shell),
}

pub(crate) fn language(path: &Path) -> Option<Language> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match extension.as_str() {
        "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" => Language::Code { python: false },
        "py" => Language::Code { python: true },
        "sh" | "bash" => Language::Script(Shell::Sh),
        "ps1" | "psm1" => Language::Script(Shell::Ps1),
        "cmd" | "bat" => Language::Script(Shell::Cmd),
        _ => return None,
    })
}

fn collect(path: &Path, found: &mut Vec<PathBuf>) {
    if path.is_file() {
        if language(path).is_some() {
            found.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for entry in entries {
        let name = entry
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if entry.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                collect(&entry, found);
            }
        } else if language(&entry).is_some() {
            found.push(entry);
        }
    }
}

fn mentions(text: &str) -> bool {
    [
        "agent-record",
        "agent-desktop",
        "agent-browser",
        "desktop-script",
    ]
    .iter()
    .any(|name| text.contains(name))
}

pub(crate) fn analyse(path: &Path, text: &str, home: Option<&Path>) -> Option<Source> {
    Some(match language(path)? {
        Language::Code { python } => extract::code(text, path, python, home),
        Language::Script(kind) => shell::shell(text, path, kind, home),
    })
}

#[derive(Clone)]
struct Finding {
    line: usize,
    severity: &'static str,
    message: String,
    call: Option<String>,
}

impl Finding {
    fn to_json(&self) -> Value {
        let mut value =
            json!({ "line": self.line, "severity": self.severity, "message": self.message });
        if let Some(call) = &self.call {
            value["call"] = json!(call);
        }
        value
    }
}

#[derive(Default)]
struct Report {
    findings: Vec<Finding>,
    calls: usize,
    checked: usize,
    helper: bool,
}

impl Report {
    fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == "error")
            .count()
    }

    fn verdict(&self) -> &'static str {
        if self.errors() > 0 {
            "incompatible"
        } else if self.findings.iter().any(|f| f.severity == "unchecked") {
            "partly_checked"
        } else {
            "compatible"
        }
    }
}

/// What a desktop-script.mjs copy finds here: its program, or why not.
enum HelperFound {
    Program(Kind, PathBuf),
    Fails(String),
    Unknown(String),
}

fn show(args: &[Arg]) -> String {
    args.iter()
        .map(|arg| match arg {
            Arg::Lit(text) if text.contains(' ') || text.is_empty() => format!("\"{text}\""),
            Arg::Lit(text) => text.clone(),
            Arg::Dyn => "…".into(),
            Arg::Part(prefix) => format!("{prefix}…"),
            Arg::Rest => "...".into(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn kind_of(program: &Program) -> Option<Kind> {
    match program {
        Program::Front => Some(Kind::Front),
        Program::Engine => Some(Kind::Engine),
        Program::Browser => Some(Kind::Browser),
        Program::Helper(_) => None,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

pub(crate) fn run(args: CheckScriptsArgs) -> ExitCode {
    let mut programs = match Programs::find() {
        Ok(programs) => programs,
        Err(message) => {
            return crate::run::emit(
                "check-scripts",
                Err(agent_desktop_core::AppError::Internal(message)),
            );
        }
    };
    let home = agent_desktop_core::refs::home_dir();
    let mut files = Vec::new();
    for path in &args.paths {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        collect(&path, &mut files);
    }
    let mut not_using = Vec::new();
    let mut sources: Vec<(PathBuf, Source)> = Vec::new();
    for path in &files {
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).to_string();
        if !mentions(&text) {
            not_using.push(path.clone());
            continue;
        }
        if let Some(source) = analyse(path, &text, home.as_deref()) {
            sources.push((path.clone(), source));
        }
    }
    // The desktop-script.mjs copies the scripts use, also outside the folders given.
    let mut helper_paths: Vec<PathBuf> = Vec::new();
    for (path, source) in &sources {
        if source.is_helper {
            helper_paths.push(path.clone());
        }
        for (_, helper) in &source.helpers {
            if helper.is_file() && !helper_paths.iter().any(|known| same_file(known, helper)) {
                helper_paths.push(helper.clone());
            }
        }
    }
    for helper in &helper_paths {
        if !sources.iter().any(|(path, _)| same_file(path, helper)) {
            let text =
                String::from_utf8_lossy(&std::fs::read(helper).unwrap_or_default()).to_string();
            if let Some(source) = analyse(helper, &text, home.as_deref()) {
                sources.push((helper.clone(), source));
            }
        }
    }
    let installed_helper = crate::skills::helper_source();
    let mut helpers: Vec<(PathBuf, HelperFound, bool)> = Vec::new();
    for helper in &helper_paths {
        let found = match probe::helper_program(helper) {
            Ok(Ok(program)) => {
                match program_named(&program.to_string_lossy()).and_then(|p| kind_of(&p)) {
                    Some(kind) => HelperFound::Program(kind, program),
                    None => HelperFound::Unknown(format!(
                        "it found {}, which is none of the programs",
                        program.display()
                    )),
                }
            }
            Ok(Err(message)) => HelperFound::Fails(message),
            Err(reason) => HelperFound::Unknown(reason),
        };
        let text = std::fs::read_to_string(helper).unwrap_or_default();
        let same = normalise(&text) == normalise(installed_helper);
        helpers.push((helper.clone(), found, same));
    }
    let secrets: HashSet<String> = agent_desktop_core::commands::secret_store::default_index()
        .map(|index| {
            agent_desktop_core::commands::secret_store::read_index(&index)
                .into_iter()
                .map(|info| info.name)
                .collect()
        })
        .unwrap_or_default();

    let mut reports: BTreeMap<PathBuf, Report> = BTreeMap::new();
    for (path, source) in &sources {
        let mut report = Report {
            helper: source.is_helper,
            ..Report::default()
        };
        if source.is_helper {
            match helper_state(&helpers, path).map(|(_, found, _)| found) {
                Some(HelperFound::Fails(message)) => report.findings.push(Finding {
                    line: 1,
                    severity: "error",
                    message: format!(
                        "this desktop-script.mjs cannot find a program it can use here: {message}. Replace it with the one agent-record installs (agent-record skills get recording-to-script --reference desktop-script.mjs)"
                    ),
                    call: None,
                }),
                Some(HelperFound::Unknown(reason)) => report.findings.push(Finding {
                    line: 1,
                    severity: "unchecked",
                    message: format!("the program this desktop-script.mjs finds was not checked: {reason}"),
                    call: None,
                }),
                _ => {}
            }
        }
        for (line, helper) in &source.helpers {
            if !helper.is_file() {
                report.findings.push(Finding {
                    line: *line,
                    severity: "error",
                    message: format!("it loads {}, which does not exist", helper.display()),
                    call: None,
                });
            }
        }
        for (line, program, path) in &source.programs {
            if let Some(path) = path {
                if path.is_absolute() && !path.exists() {
                    let name = match program {
                        Program::Engine => "agent-desktop",
                        Program::Browser => "agent-browser",
                        _ => "agent-record",
                    };
                    report.findings.push(Finding {
                        line: *line,
                        severity: "error",
                        message: format!(
                            "it runs {name} at {}, which does not exist",
                            path.display()
                        ),
                        call: None,
                    });
                }
            }
        }
        for call in &source.calls {
            check_call(call, &mut programs, &helpers, &mut report);
        }
        for list in &source.lists {
            if let Some(Arg::Lit(command)) = list.args.first() {
                if programs.is_command(command) {
                    check_call(list, &mut programs, &helpers, &mut report);
                }
            }
        }
        if source.calls.is_empty()
            && source.lists.is_empty()
            && source.helpers.is_empty()
            && !source.is_helper
        {
            report.findings.push(Finding {
                line: 1,
                severity: "unchecked",
                message: "it names agent-record, agent-desktop or agent-browser, but no call this check can follow was found (the program may be chosen when it runs)".into(),
                call: None,
            });
        }
        for (line, name) in &source.secrets {
            if !secrets.contains(name) {
                report.findings.push(Finding {
                    line: *line,
                    severity: "warning",
                    message: format!(
                        "no password is saved as '{name}': the first run will ask the person for it once"
                    ),
                    call: None,
                });
            }
        }
        reports.insert(path.clone(), report);
    }
    // A script is only as good as the parts of its desktop-script.mjs it uses.
    type Problems = (String, Vec<(Option<Scope>, String)>);
    let helper_problems: HashMap<PathBuf, Problems> = reports
        .iter()
        .filter(|(_, report)| report.helper)
        .map(|(path, report)| {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let mut problems: Vec<(Option<Scope>, String)> = Vec::new();
            for finding in report
                .findings
                .iter()
                .filter(|finding| finding.severity == "error")
            {
                let scope = finding
                    .call
                    .as_ref()
                    .and_then(|_| scope_at(&text, finding.line));
                let seen = problems.iter().any(|(known, message)| {
                    *message == finding.message
                        && known.as_ref().map(|k| &k.name) == scope.as_ref().map(|s| &s.name)
                });
                if !seen {
                    problems.push((scope, finding.message.clone()));
                }
            }
            (path.clone(), (text, problems))
        })
        .collect();
    for (path, source) in &sources {
        if source.is_helper {
            continue;
        }
        let text = std::fs::read_to_string(path).unwrap_or_default();
        for (line, helper) in &source.helpers {
            let Some((_, (helper_text, problems))) = helper_problems
                .iter()
                .find(|(known, _)| same_file(known, helper))
            else {
                continue;
            };
            for (scope, message) in problems {
                let (severity, note) = match scope {
                    None => (
                        "error",
                        format!(
                            "the desktop-script.mjs it loads ({}) fails here: {message}",
                            helper.display()
                        ),
                    ),
                    Some(scope) if scope.used_by(helper_text, &source.helper_names, &text) => (
                        "error",
                        format!(
                            "it uses {}() of its desktop-script.mjs, which fails here: {message}",
                            scope.name
                        ),
                    ),
                    Some(scope) => (
                        "warning",
                        format!(
                            "its desktop-script.mjs fails in {}(), which this script does not use: {message}",
                            scope.name
                        ),
                    ),
                };
                if let Some(report) = reports.get_mut(path) {
                    report.findings.push(Finding {
                        line: *line,
                        severity,
                        message: note,
                        call: None,
                    });
                }
            }
        }
    }

    let mut summary = json!({
        "files": files.len(),
        "using_agent_record": reports.len(),
        "not_using": not_using.len(),
        "compatible": 0, "partly_checked": 0, "incompatible": 0,
        "calls": 0, "checked_calls": 0,
    });
    let mut listed = Vec::new();
    let mut incompatible = false;
    for (path, mut report) in reports {
        report.findings.sort_by_key(|finding| finding.line);
        let verdict = report.verdict();
        incompatible |= verdict == "incompatible";
        summary[verdict] = json!(summary[verdict].as_u64().unwrap_or(0) + 1);
        summary["calls"] = json!(summary["calls"].as_u64().unwrap_or(0) + report.calls as u64);
        summary["checked_calls"] =
            json!(summary["checked_calls"].as_u64().unwrap_or(0) + report.checked as u64);
        let mut entry = json!({
            "path": path.display().to_string(),
            "verdict": verdict,
            "calls": report.calls,
            "checked_calls": report.checked,
            "findings": report.findings.iter().map(Finding::to_json).collect::<Vec<_>>(),
        });
        if report.helper {
            entry["helper"] = json!(true);
            if let Some((_, found, same)) = helper_state(&helpers, &path) {
                entry["same_as_installed_helper"] = json!(same);
                if let HelperFound::Program(_, program) = found {
                    entry["helper_program"] = json!(program.display().to_string());
                }
            }
        }
        listed.push(entry);
    }
    let data = json!({
        "program": programs.front.display().to_string(),
        "engine": programs.engine.display().to_string(),
        "summary": summary,
        "scripts": listed,
        "not_checked_here": "whether the app or page still has what each script uses, and what it does once it runs: run the script for that",
    });
    let code = crate::run::emit("check-scripts", Ok(data));
    if incompatible {
        ExitCode::from(1)
    } else {
        code
    }
}

fn helper_state<'a>(
    helpers: &'a [(PathBuf, HelperFound, bool)],
    path: &Path,
) -> Option<&'a (PathBuf, HelperFound, bool)> {
    helpers.iter().find(|(known, _, _)| same_file(known, path))
}

/// The function or method of a desktop-script.mjs a line is in.
struct Scope {
    name: String,
    method: bool,
}

/// Functions of desktop-script.mjs that give the script windows and
/// elements, whose methods it can then call.
const GIVES_OBJECTS: &[&str] = &[
    "task",
    "findWindow",
    "openApp",
    "openBrowser",
    "Window",
    "Element",
];

impl Scope {
    fn used_directly(&self, imported: &[String], text: &str) -> bool {
        let everything = imported.iter().any(|name| name == "*");
        if self.method {
            let reachable = everything
                || imported
                    .iter()
                    .any(|name| GIVES_OBJECTS.contains(&name.as_str()));
            reachable && text.contains(&format!(".{}(", self.name))
        } else {
            everything || imported.contains(&self.name)
        }
    }

    /// Whether the script uses this function or method, or one of the
    /// desktop-script.mjs functions and methods that call it.
    fn used_by(&self, helper: &str, imported: &[String], text: &str) -> bool {
        callers(helper, self)
            .iter()
            .any(|scope| scope.used_directly(imported, text))
    }
}

/// A function or method of desktop-script.mjs and everything there that
/// calls it, directly or through others.
fn callers(helper: &str, scope: &Scope) -> Vec<Scope> {
    let mut found = vec![Scope {
        name: scope.name.clone(),
        method: scope.method,
    }];
    let lines: Vec<&str> = helper.lines().collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (index, line) in lines.iter().enumerate() {
            let calls_one = found.iter().any(|known| {
                let name = &known.name;
                if known.method {
                    // Inside the helper, a method is called on its own object.
                    line.contains(&format!("this.{name}("))
                        || line.contains(&format!("this.#{name}("))
                } else {
                    line.match_indices(&format!("{name}(")).any(|(at, _)| {
                        line[..at].chars().last().is_none_or(|before| {
                            !(before.is_alphanumeric()
                                || before == '_'
                                || before == '.'
                                || before == '$')
                        })
                    })
                }
            });
            if !calls_one {
                continue;
            }
            if let Some(caller) = scope_at(helper, index + 1) {
                if !found.iter().any(|known| known.name == caller.name) {
                    found.push(caller);
                    changed = true;
                }
            }
        }
    }
    found
}

fn scope_at(text: &str, line: usize) -> Option<Scope> {
    let lines: Vec<&str> = text.lines().collect();
    for raw in lines.iter().take(line).rev() {
        let trimmed = raw.trim_start();
        let words: Vec<&str> = trimmed
            .split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$'))
            .filter(|word| !word.is_empty())
            .collect();
        if let Some(at) = words.iter().position(|word| *word == "function") {
            if raw.starts_with(|ch: char| !ch.is_whitespace()) {
                return words.get(at + 1).map(|name| Scope {
                    name: (*name).to_string(),
                    method: false,
                });
            }
        }
        let indented = raw.starts_with(|ch: char| ch.is_whitespace());
        let opens = trimmed.trim_end().ends_with('{');
        let first = words
            .iter()
            .find(|word| !matches!(**word, "async" | "static" | "get" | "set"))
            .copied();
        if indented && opens {
            if let Some(name) = first {
                let keyword = matches!(
                    name,
                    "if" | "for"
                        | "while"
                        | "switch"
                        | "catch"
                        | "else"
                        | "try"
                        | "do"
                        | "return"
                        | "const"
                        | "let"
                        | "await"
                );
                if !keyword && trimmed.contains(&format!("{name}(")) {
                    return Some(Scope {
                        name: name.to_string(),
                        method: true,
                    });
                }
            }
        }
    }
    None
}

fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

fn check_call(
    call: &Call,
    programs: &mut Programs,
    helpers: &[(PathBuf, HelperFound, bool)],
    report: &mut Report,
) {
    report.calls += 1;
    let shown = show(&call.args);
    let kind = match &call.program {
        Program::Helper(Some(helper)) => {
            match helper_state(helpers, helper).map(|(_, found, _)| found) {
                Some(HelperFound::Program(kind, _)) => *kind,
                // The copy's own problem is reported once, for the copy.
                Some(HelperFound::Fails(_)) => return,
                _ => {
                    report.findings.push(Finding {
                        line: call.line,
                        severity: "unchecked",
                        message: "the program its desktop-script.mjs uses is not known".into(),
                        call: Some(shown),
                    });
                    return;
                }
            }
        }
        Program::Helper(None) => {
            report.findings.push(Finding {
                line: call.line,
                severity: "warning",
                message: "the desktop-script.mjs it loads is chosen when it runs: checked against this agent-record".into(),
                call: Some(shown.clone()),
            });
            Kind::Front
        }
        other => kind_of(other).unwrap_or(Kind::Front),
    };
    let outcome = programs.check(kind, &call.args, call.partial);
    if outcome.unchecked.is_none() {
        report.checked += 1;
    }
    for message in outcome.errors {
        report.findings.push(Finding {
            line: call.line,
            severity: "error",
            message,
            call: Some(shown.clone()),
        });
    }
    for message in outcome.warnings {
        report.findings.push(Finding {
            line: call.line,
            severity: "warning",
            message,
            call: Some(shown.clone()),
        });
    }
    if let Some(reason) = outcome.unchecked {
        report.findings.push(Finding {
            line: call.line,
            severity: "unchecked",
            message: reason,
            call: Some(shown),
        });
    }
}

#[cfg(test)]
#[path = "check_scripts_tests.rs"]
mod tests;
