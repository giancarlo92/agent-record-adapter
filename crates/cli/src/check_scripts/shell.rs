// Part of Agent Record Adapter (see NOTICE).
//! Calls to agent-record, agent-desktop or agent-browser in a shell script
//! (sh/bash, PowerShell, cmd): a command line whose command is one of the
//! programs (by name, path or a variable holding it), or a function of the
//! script that runs one of them with its own arguments (`ar() { "$AR" "$@"; }`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::extract::{Arg, Call, Program, Source, program_named};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shell {
    Sh,
    Ps1,
    Cmd,
}

#[derive(Clone, Debug)]
enum Piece {
    Word {
        raw: String,
        text: String,
        dynamic: bool,
    },
    Op(String),
}

const SEPARATORS: &[&str] = &[";", "|", "||", "&&", "&", "(", ")", "$(", "{", "}"];
const REDIRECTS: &[&str] = &[
    ">", ">>", "<", "2>", "1>", "2>>", "2>&1", "1>&2", "*>", "*>>",
];
const PREFIXES: &[&str] = &[
    "if", "then", "do", "else", "elif", "while", "until", "time", "exec", "!", "command", "nohup",
    "env", "call", "start", "-not",
];
const REST: &[&str] = &["$@", "${@}", "$*", "@args", "$args", "%*"];

pub(crate) fn shell(text: &str, path: &Path, kind: Shell, home: Option<&Path>) -> Source {
    let lines = logical_lines(text, kind);
    let mut programs: HashMap<String, Program> = HashMap::new();
    let mut source = Source::default();
    let parsed: Vec<(usize, Vec<Piece>)> = lines
        .iter()
        .map(|(line, content)| (*line, pieces(content, kind)))
        .collect();
    // Variables holding a program.
    for (line, pieces) in &parsed {
        if let Some((name, value)) = assignment(pieces, kind) {
            if let Some(program) = program_of_value(&value, &programs) {
                let resolved = resolve(&value, kind, home, path);
                if program_named(&value).is_some() {
                    source.programs.push((*line, program.clone(), resolved));
                }
                programs.insert(name, program);
            }
        }
    }
    // Functions that run a program with their own arguments.
    let mut runners: HashMap<String, Program> = HashMap::new();
    for (index, (_, pieces)) in parsed.iter().enumerate() {
        let Some(name) = function_name(pieces) else {
            continue;
        };
        let mut depth = 0i32;
        let mut opened = false;
        let mut found = None;
        'body: for (_, body) in &parsed[index..] {
            for piece in body {
                match piece {
                    Piece::Op(op) if op == "{" => {
                        depth += 1;
                        opened = true;
                    }
                    Piece::Op(op) if op == "}" => depth -= 1,
                    _ => {}
                }
            }
            for segment in segments(body) {
                if let Some((program, args)) = command(&segment, &programs, &runners, kind) {
                    if args.contains(&Arg::Rest) {
                        found = Some(program);
                        break 'body;
                    }
                }
            }
            if opened && depth <= 0 {
                break;
            }
        }
        if let Some(program) = found {
            runners.insert(name, program);
        }
    }
    for (line, pieces) in &parsed {
        if function_name(pieces).is_some()
            || (kind != Shell::Sh && assignment(pieces, kind).is_some())
        {
            continue;
        }
        for segment in segments(pieces) {
            if let Some((program, args)) = command(&segment, &programs, &runners, kind) {
                if args.contains(&Arg::Rest) && !matches!(args.first(), Some(Arg::Lit(_))) {
                    continue;
                }
                if let [Arg::Lit(secret), Arg::Lit(fill), _, Arg::Lit(name), ..] = args.as_slice() {
                    if secret == "secret" && fill == "fill" {
                        source.secrets.push((*line, name.clone()));
                    }
                }
                source.calls.push(Call {
                    line: *line,
                    program,
                    args,
                    partial: false,
                });
            }
        }
    }
    source
}

/// Lines joined where they continue (`\`, a PowerShell backtick, `^`), with
/// comments dropped: (first line number, text).
fn logical_lines(text: &str, kind: Shell) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    for (index, raw) in text.lines().enumerate() {
        if current.is_empty() {
            start = index + 1;
        }
        let line = strip_comment(raw, kind);
        let continued = match kind {
            Shell::Sh => line.trim_end().strip_suffix('\\'),
            Shell::Ps1 => line.trim_end().strip_suffix('`'),
            Shell::Cmd => line.trim_end().strip_suffix('^'),
        };
        match continued {
            Some(head) => {
                current.push_str(head);
                current.push(' ');
            }
            None => {
                current.push_str(&line);
                out.push((start, std::mem::take(&mut current)));
            }
        }
    }
    if !current.is_empty() {
        out.push((start, current));
    }
    out
}

fn strip_comment(line: &str, kind: Shell) -> String {
    if kind == Shell::Cmd {
        let trimmed = line.trim_start().to_ascii_lowercase();
        if trimmed.starts_with("rem ") || trimmed == "rem" || trimmed.starts_with("::") {
            return String::new();
        }
        return line.to_string();
    }
    let mut quote = None;
    let mut previous = ' ';
    for (index, ch) in line.char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '#' && previous.is_whitespace() => return line[..index].to_string(),
            None => {}
        }
        previous = ch;
    }
    line.to_string()
}

fn pieces(line: &str, kind: Shell) -> Vec<Piece> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut raw = String::new();
    let mut text = String::new();
    let mut dynamic = false;
    let flush = |out: &mut Vec<Piece>, raw: &mut String, text: &mut String, dynamic: &mut bool| {
        if !raw.is_empty() {
            out.push(Piece::Word {
                raw: std::mem::take(raw),
                text: std::mem::take(text),
                dynamic: std::mem::take(dynamic),
            });
        }
    };
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            flush(&mut out, &mut raw, &mut text, &mut dynamic);
            i += 1;
            continue;
        }
        if ch == '\'' && kind != Shell::Cmd {
            raw.push(ch);
            i += 1;
            while i < c.len() && c[i] != '\'' {
                raw.push(c[i]);
                text.push(c[i]);
                i += 1;
            }
            raw.push('\'');
            i += 1;
            continue;
        }
        if ch == '"' {
            raw.push(ch);
            i += 1;
            while i < c.len() && c[i] != '"' {
                if (c[i] == '\\' && kind == Shell::Sh) || (c[i] == '`' && kind == Shell::Ps1) {
                    if let Some(&next) = c.get(i + 1) {
                        raw.push(c[i]);
                        raw.push(next);
                        text.push(next);
                        i += 2;
                        continue;
                    }
                }
                if (c[i] == '$' && kind != Shell::Cmd) || (c[i] == '%' && kind == Shell::Cmd) {
                    dynamic = true;
                }
                raw.push(c[i]);
                text.push(c[i]);
                i += 1;
            }
            raw.push('"');
            i += 1;
            continue;
        }
        if ch == '$' && c.get(i + 1) == Some(&'(') && kind != Shell::Cmd {
            flush(&mut out, &mut raw, &mut text, &mut dynamic);
            out.push(Piece::Op("$(".into()));
            i += 2;
            continue;
        }
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        let four: String = c[i..(i + 4).min(c.len())].iter().collect();
        let op = if four == "2>&1" || four == "1>&2" {
            Some(four)
        } else if ["||", "&&", ">>", "2>", "1>", "*>"].contains(&two.as_str()) {
            Some(two)
        } else if ";|&()<>".contains(ch) || ((ch == '{' || ch == '}') && raw.is_empty()) {
            Some(ch.to_string())
        } else {
            None
        };
        if let Some(op) = op {
            // `{` and `}` are operators only as words of their own.
            let alone = !(op == "{" || op == "}")
                || c.get(i + 1)
                    .is_none_or(|next| next.is_whitespace() || *next == ';');
            if alone {
                flush(&mut out, &mut raw, &mut text, &mut dynamic);
                i += op.chars().count();
                out.push(Piece::Op(op));
                continue;
            }
        }
        if (ch == '$' && kind != Shell::Cmd) || (ch == '%' && kind == Shell::Cmd) || ch == '`' {
            dynamic = true;
        }
        raw.push(ch);
        text.push(ch);
        i += 1;
    }
    flush(&mut out, &mut raw, &mut text, &mut dynamic);
    out
}

fn segments(pieces: &[Piece]) -> Vec<Vec<Piece>> {
    let mut out = vec![Vec::new()];
    for piece in pieces {
        match piece {
            Piece::Op(op) if SEPARATORS.contains(&op.as_str()) => out.push(Vec::new()),
            other => {
                if let Some(last) = out.last_mut() {
                    last.push(other.clone());
                }
            }
        }
    }
    out.retain(|segment| !segment.is_empty());
    out
}

fn word(piece: &Piece) -> Option<(&str, &str, bool)> {
    match piece {
        Piece::Word { raw, text, dynamic } => Some((raw.as_str(), text.as_str(), *dynamic)),
        Piece::Op(_) => None,
    }
}

/// `NAME=value`, `$NAME = value`, `set NAME=value`: (name, value as written).
fn assignment(pieces: &[Piece], kind: Shell) -> Option<(String, String)> {
    let words: Vec<&str> = pieces
        .iter()
        .filter_map(word)
        .map(|(raw, _, _)| raw)
        .collect();
    match kind {
        Shell::Sh => {
            let first = if matches!(words.first(), Some(&"export" | &"local" | &"readonly")) {
                words.get(1)?
            } else {
                words.first()?
            };
            let (name, value) = first.split_once('=')?;
            valid_name(name).then(|| (name.to_string(), unquote(value)))
        }
        Shell::Ps1 => {
            let name = words.first()?.strip_prefix('$')?;
            let name = name.strip_prefix("env:").unwrap_or(name);
            (*words.get(1)? == "=" && valid_name(name))
                .then(|| (name.to_string(), unquote(&words[2..].join(" "))))
        }
        Shell::Cmd => {
            if !words.first()?.eq_ignore_ascii_case("set") {
                return None;
            }
            let rest = unquote(&words[1..].join(" "));
            let (name, value) = rest.split_once('=')?;
            valid_name(name).then(|| (name.to_string(), value.to_string()))
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches(['"', '\'']).to_string()
}

fn program_of_value(value: &str, programs: &HashMap<String, Program>) -> Option<Program> {
    program_named(value).or_else(|| variable(value).and_then(|name| programs.get(&name).cloned()))
}

/// The variable a word stands for: `$AR`, `${AR}`, `"$AR"`, `$env:AR`, `%AR%`.
fn variable(raw: &str) -> Option<String> {
    let bare = raw.trim_matches(['"', '\'']);
    let name = if let Some(rest) = bare.strip_prefix("${") {
        rest.strip_suffix('}')?
    } else if let Some(rest) = bare.strip_prefix("$env:") {
        rest
    } else if let Some(rest) = bare.strip_prefix('$') {
        rest
    } else if let Some(rest) = bare.strip_prefix('%') {
        rest.strip_suffix('%')?
    } else {
        return None;
    };
    valid_name(name).then(|| name.to_string())
}

fn resolve(value: &str, kind: Shell, home: Option<&Path>, script: &Path) -> Option<PathBuf> {
    let home_text = home?.to_string_lossy().to_string();
    let mut text = value.to_string();
    for marker in [
        "${HOME}",
        "$HOME",
        "$env:USERPROFILE",
        "$USERPROFILE",
        "%USERPROFILE%",
        "~",
    ] {
        if let Some(rest) = text.strip_prefix(marker) {
            text = format!("{home_text}{rest}");
        }
    }
    let dynamic = match kind {
        Shell::Cmd => text.contains('%'),
        _ => text.contains('$') || text.contains('`'),
    };
    if dynamic {
        return None;
    }
    let path = PathBuf::from(text);
    Some(if path.is_absolute() {
        path
    } else {
        script.parent().unwrap_or(Path::new(".")).join(path)
    })
}

/// A function definition's name: `name() {`, `function name {`.
fn function_name(pieces: &[Piece]) -> Option<String> {
    let words: Vec<(&str, &str, bool)> = pieces.iter().filter_map(word).collect();
    if let Some((first, _, _)) = words.first() {
        if first.eq_ignore_ascii_case("function") {
            let name = words.get(1)?.0.trim_end_matches("()");
            return valid_name(name.trim_end_matches(['(', ')'])).then(|| name.to_string());
        }
    }
    // name ( ) {
    match pieces {
        [
            Piece::Word { raw, .. },
            Piece::Op(open),
            Piece::Op(close),
            ..,
        ] if open == "(" && close == ")" => valid_name(raw).then(|| raw.clone()),
        [Piece::Word { raw, .. }, ..] if raw.ends_with("()") => {
            let name = raw.trim_end_matches("()");
            valid_name(name).then(|| name.to_string())
        }
        _ => None,
    }
}

/// The program a command line runs and its arguments.
fn command(
    segment: &[Piece],
    programs: &HashMap<String, Program>,
    runners: &HashMap<String, Program>,
    kind: Shell,
) -> Option<(Program, Vec<Arg>)> {
    let mut i = 0;
    while let Some((raw, _, _)) = segment.get(i).and_then(word) {
        let lower = raw.to_ascii_lowercase();
        let assignment = kind == Shell::Sh
            && raw
                .split_once('=')
                .is_some_and(|(name, _)| valid_name(name));
        if PREFIXES.contains(&lower.as_str()) || assignment {
            i += 1;
            continue;
        }
        if lower == "timeout" {
            i += 2;
            continue;
        }
        break;
    }
    let (raw, text, _) = segment.get(i).and_then(word)?;
    let program = program_named(text)
        .filter(|_| !text.contains(' '))
        .or_else(|| variable(raw).and_then(|name| programs.get(&name).cloned()))
        .or_else(|| runners.get(raw).cloned())?;
    let mut args = Vec::new();
    let mut k = i + 1;
    while k < segment.len() {
        match &segment[k] {
            Piece::Op(op) if REDIRECTS.contains(&op.as_str()) => {
                if !op.contains('&') {
                    k += 1;
                }
            }
            Piece::Op(_) => break,
            Piece::Word { raw, text, dynamic } => {
                let bare = raw.trim_matches('"');
                args.push(if REST.contains(&bare) {
                    Arg::Rest
                } else if *dynamic {
                    Arg::Dyn
                } else {
                    Arg::Lit(text.clone())
                });
            }
        }
        k += 1;
    }
    Some((program, args))
}
