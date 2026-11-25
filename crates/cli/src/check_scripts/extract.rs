// Part of Agent Record Adapter (see NOTICE).
//! Finds, in a JavaScript, TypeScript or Python source, every call it makes
//! to agent-record, agent-desktop or agent-browser, with the arguments written
//! in it. Nothing is run. A call is found when:
//! - a function of a desktop-script.mjs copy (`run`) is called;
//! - a program is run directly (`execFileSync(bin, [...])`,
//!   `subprocess.run([exe, ...])`) where the program is written in the file;
//! - a function of the file that runs one of those, forwarding its arguments
//!   (`const call = (...args) => execFileSync(bin, args)`), is called.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::tokens::{Tok, Token, tokens};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Arg {
    /// Written in the script.
    Lit(String),
    /// Known only when the script runs.
    Dyn,
    /// Starts with this written text; the rest is known when it runs.
    Part(String),
    /// Any number of arguments known only when the script runs.
    Rest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Program {
    /// agent-record
    Front,
    /// agent-desktop (the engine)
    Engine,
    /// agent-browser
    Browser,
    /// `run` of a desktop-script.mjs copy: the program that copy finds.
    Helper(Option<PathBuf>),
}

#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub line: usize,
    pub program: Program,
    pub args: Vec<Arg>,
    /// Built in parts (an argument list and its `push`es): other parts may
    /// be added when it runs, so required arguments are not checked.
    pub partial: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Source {
    pub calls: Vec<Call>,
    /// desktop-script.mjs copies the file uses.
    pub helpers: Vec<(usize, PathBuf)>,
    /// Saved passwords named in the file.
    pub secrets: Vec<(usize, String)>,
    /// Program paths written in the file, as far as they can be worked out.
    pub programs: Vec<(usize, Program, Option<PathBuf>)>,
    /// The file is a desktop-script.mjs itself.
    pub is_helper: bool,
    /// What the file takes from desktop-script.mjs copies (`*` for all).
    pub helper_names: Vec<String>,
    /// Argument lists built in the file (`['find', '--window-id', id]`) and
    /// their `push`es, for a program: kept when they start with a command.
    pub lists: Vec<Call>,
}

/// The program a path or command name stands for.
pub(crate) fn program_named(text: &str) -> Option<Program> {
    let base = text
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(text)
        .trim()
        .to_ascii_lowercase();
    match base.strip_suffix(".exe").unwrap_or(&base) {
        "agent-record" => Some(Program::Front),
        "agent-desktop" => Some(Program::Engine),
        "agent-browser" => Some(Program::Browser),
        _ => None,
    }
}

const EXEC: &[&str] = &[
    "execFile",
    "execFileSync",
    "spawn",
    "spawnSync",
    "execa",
    "execaSync",
    "run",
    "call",
    "check_output",
    "check_call",
    "Popen",
];

struct Function {
    name: String,
    params: Vec<Option<(String, bool)>>,
    body: (usize, usize),
}

/// A function that runs a program with the arguments it is given from
/// its parameter number `offset` on.
#[derive(Clone)]
struct Runner {
    program: Program,
    offset: usize,
}

struct File<'a> {
    t: Vec<Token>,
    python: bool,
    dir: PathBuf,
    home: Option<&'a Path>,
    indents: Vec<usize>,
    programs: HashMap<String, Program>,
    paths: HashMap<String, Option<PathBuf>>,
}

pub(crate) fn code(text: &str, path: &Path, python: bool, home: Option<&Path>) -> Source {
    let mut file = File {
        t: tokens(text, python),
        python,
        dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        home,
        indents: std::iter::once(0)
            .chain(
                text.lines()
                    .map(|line| line.len() - line.trim_start().len()),
            )
            .collect(),
        programs: HashMap::new(),
        paths: HashMap::new(),
    };
    let mut source = Source::default();
    file.variables(&mut source);
    let mut runners = file.helper_imports(text, &mut source);
    if text.contains("export function agentDesktop") && text.contains("export function run") {
        source.is_helper = true;
        runners.insert(
            "run".into(),
            Runner {
                program: Program::Helper(Some(path.to_path_buf())),
                offset: 0,
            },
        );
    }
    let functions = file.functions();
    file.runners(&functions, &mut runners);
    file.calls(&runners, &mut source);
    if !python {
        let helper = source
            .is_helper
            .then(|| Program::Helper(Some(path.to_path_buf())));
        file.lists(&runners, helper.as_ref(), &mut source);
    }
    source
}

impl File<'_> {
    fn matching(&self, open: usize) -> Option<usize> {
        let mut depth = 0i32;
        for i in open..self.t.len() {
            match self.t[i].tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The comma-separated items between `open` and its closing bracket.
    fn items(&self, open: usize) -> Option<(Vec<(usize, usize)>, usize)> {
        let close = self.matching(open)?;
        let mut parts = Vec::new();
        let mut depth = 0i32;
        let mut start = open + 1;
        for i in open + 1..close {
            match self.t[i].tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => depth -= 1,
                Tok::Punct(',') if depth == 0 => {
                    parts.push((start, i));
                    start = i + 1;
                }
                _ => {}
            }
        }
        if start < close {
            parts.push((start, close));
        }
        Some((parts, close))
    }

    fn continues(&self, last: &Token, next: &Token) -> bool {
        let operator = |token: &Token, set: &str| match token.tok {
            Tok::Punct(p) => set.contains(p),
            Tok::Arrow => true,
            _ => false,
        };
        operator(last, "+-*/=?:,.(|&<>\\")
            || operator(next, ".?:+-*/|&")
            || (self.python && matches!(next.ident(), Some("or" | "and" | "if" | "else")))
    }

    /// Where the statement starting at `from` ends (exclusive).
    fn statement_end(&self, from: usize) -> usize {
        let mut depth = 0i32;
        let mut i = from;
        while i < self.t.len() {
            match self.t[i].tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => {
                    if depth == 0 {
                        return i;
                    }
                    depth -= 1;
                }
                Tok::Punct(';') if depth == 0 => return i,
                _ => {}
            }
            if depth == 0
                && i + 1 < self.t.len()
                && self.t[i + 1].line > self.t[i].line
                && !self.continues(&self.t[i], &self.t[i + 1])
            {
                return i + 1;
            }
            i += 1;
        }
        self.t.len()
    }

    fn first_on_line(&self, i: usize) -> bool {
        i == 0 || self.t[i - 1].line < self.t[i].line
    }

    /// Variables holding a program or a path.
    fn variables(&mut self, source: &mut Source) {
        let mut i = 0;
        while i + 2 < self.t.len() {
            let assigned = if !self.python
                && matches!(self.t[i].ident(), Some("const" | "let" | "var"))
                && self.t[i + 1].ident().is_some()
                && self.t[i + 2].is('=')
            {
                Some((i + 1, i + 3))
            } else if self.python
                && self.first_on_line(i)
                && self.t[i].ident().is_some()
                && self.t[i + 1].is('=')
                && !self.t[i + 2].is('=')
            {
                Some((i, i + 2))
            } else {
                None
            };
            let Some((name_at, start)) = assigned else {
                i += 1;
                continue;
            };
            let end = self.statement_end(start);
            let name = self.t[name_at].ident().unwrap_or_default().to_string();
            let path = self.resolve(start, end);
            if let Some((program, written)) = self.program_in(start, end) {
                if written {
                    source
                        .programs
                        .push((self.t[name_at].line, program.clone(), path.clone()));
                }
                self.programs.insert(name.clone(), program);
            }
            self.paths.insert(name, path);
            i += 1;
        }
    }

    /// The program in t[start..end]: (program, written here as a name or path).
    fn program_in(&self, start: usize, end: usize) -> Option<(Program, bool)> {
        for i in start..end {
            match &self.t[i].tok {
                Tok::Str(text) => {
                    if let Some(program) = program_named(text) {
                        return Some((program, true));
                    }
                }
                Tok::Ident(name) if !self.member(i) && !self.called(i) => {
                    if let Some(program) = self.programs.get(name) {
                        return Some((program.clone(), false));
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn member(&self, i: usize) -> bool {
        i > 0 && self.t[i - 1].is('.')
    }

    fn called(&self, i: usize) -> bool {
        self.t.get(i + 1).is_some_and(|next| next.is('('))
    }

    /// The path t[start..end] builds, when it is made of written parts, the
    /// home folder, the file's folder and other such variables. With a
    /// fallback (`env ?? join(...)`, `a or b`), the last alternative.
    fn resolve(&self, start: usize, end: usize) -> Option<PathBuf> {
        let mut from = start;
        let mut depth = 0i32;
        for i in start..end {
            match &self.t[i].tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => depth -= 1,
                Tok::Punct('?' | '|')
                    if depth == 0 && self.t.get(i + 1).map(|t| &t.tok) == Some(&self.t[i].tok) =>
                {
                    from = i + 2;
                }
                Tok::Ident(word) if depth == 0 && self.python && word == "or" => from = i + 1,
                _ => {}
            }
        }
        // A choice (`a ? b : c`, `b if a else c`) or a comparison: not a path.
        let mut depth = 0i32;
        for i in from..end {
            match &self.t[i].tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => depth -= 1,
                Tok::Punct('=') if depth == 0 => return None,
                Tok::Punct('?') if depth == 0 => {
                    let doubled = self.t.get(i + 1).is_some_and(|t| t.is('?') || t.is('.'))
                        || (i > from && self.t[i - 1].is('?'));
                    if !doubled {
                        return None;
                    }
                }
                Tok::Ident(word) if depth == 0 && self.python && word == "if" => return None,
                _ => {}
            }
        }
        let mut pieces: Vec<PathBuf> = Vec::new();
        for i in from..end {
            match &self.t[i].tok {
                Tok::Tmpl(_) => return None,
                Tok::Str(text) => {
                    if let Some(rest) = text.strip_prefix('~') {
                        pieces.push(self.home?.to_path_buf());
                        let rest = rest.trim_start_matches(['/', '\\']);
                        if !rest.is_empty() {
                            pieces.push(PathBuf::from(rest));
                        }
                    } else {
                        pieces.push(PathBuf::from(text));
                    }
                }
                Tok::Ident(name) => match name.as_str() {
                    "homedir" if self.called(i) => pieces.push(self.home?.to_path_buf()),
                    "home" if self.member(i) && self.called(i) => {
                        pieces.push(self.home?.to_path_buf());
                    }
                    "__dirname" => pieces.push(self.dir.clone()),
                    "env" | "environ" | "getenv" | "argv" | "cwd" => return None,
                    _ if !self.member(i) && !self.called(i) => {
                        if let Some(known) = self.paths.get(name) {
                            pieces.push(known.clone()?);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if pieces.is_empty() {
            return None;
        }
        let mut path = PathBuf::new();
        for piece in pieces {
            path.push(piece);
        }
        Some(if path.is_absolute() {
            path
        } else {
            self.dir.join(path)
        })
    }

    /// `import { run } from './desktop-script.mjs'`, `import * as ds from ...`
    /// and `const { run } = await import(helper)`.
    fn helper_imports(&self, text: &str, source: &mut Source) -> HashMap<String, Runner> {
        let mut runners = HashMap::new();
        if !text.contains("desktop-script") {
            return runners;
        }
        let t = &self.t;
        for i in 0..t.len() {
            if t[i].ident() == Some("import") && !self.called(i) && !self.member(i) {
                let Some(from) = (i..t.len().min(i + 64)).find(|&k| t[k].ident() == Some("from"))
                else {
                    continue;
                };
                let Some(Tok::Str(module)) = t.get(from + 1).map(|token| &token.tok) else {
                    continue;
                };
                if !module.contains("desktop-script") {
                    continue;
                }
                let helper = self.dir.join(module);
                source.helpers.push((t[i].line, helper.clone()));
                if t.get(i + 1).is_some_and(|token| token.is('*')) {
                    source.helper_names.push("*".into());
                    if let Some(name) = t.get(i + 3).and_then(Token::ident) {
                        runners.insert(
                            format!("{name}.run"),
                            Runner {
                                program: Program::Helper(Some(helper)),
                                offset: 0,
                            },
                        );
                    }
                    continue;
                }
                for (name, alias) in self.imported_names(i + 1, from) {
                    source.helper_names.push(name.clone());
                    if name == "run" {
                        runners.insert(
                            alias,
                            Runner {
                                program: Program::Helper(Some(helper.clone())),
                                offset: 0,
                            },
                        );
                    }
                }
            }
            // const { run } = await import(EXPR)
            if t[i].ident() == Some("import") && self.called(i) && !self.member(i) {
                let Some(close) = self.matching(i + 1) else {
                    continue;
                };
                let mut k = i;
                while k > 0 && !t[k].is('=') && i - k < 4 {
                    k -= 1;
                }
                if !t[k].is('=') || k == 0 || !t[k - 1].is('}') {
                    continue;
                }
                let Some(open) = (0..k).rev().find(|&j| t[j].is('{')) else {
                    continue;
                };
                let names = self.imported_names(open, k - 1);
                let helper = self
                    .resolve(i + 2, close)
                    .filter(|path| path.to_string_lossy().contains("desktop-script"));
                if let Some(path) = &helper {
                    source.helpers.push((t[i].line, path.clone()));
                }
                for (name, alias) in names {
                    source.helper_names.push(name.clone());
                    if name == "run" {
                        runners.insert(
                            alias,
                            Runner {
                                program: Program::Helper(helper.clone()),
                                offset: 0,
                            },
                        );
                    }
                }
            }
        }
        runners
    }

    /// Names between braces from `start` to `end`: (name, local name).
    fn imported_names(&self, start: usize, end: usize) -> Vec<(String, String)> {
        let mut names = Vec::new();
        let mut i = start;
        while i < end {
            if let Some(name) = self.t[i].ident() {
                let renamed = matches!(self.t.get(i + 1).and_then(Token::ident), Some("as"))
                    || self.t.get(i + 1).is_some_and(|token| token.is(':'));
                if renamed {
                    if let Some(alias) = self.t.get(i + 2).and_then(Token::ident) {
                        names.push((name.to_string(), alias.to_string()));
                        i += 3;
                        continue;
                    }
                }
                names.push((name.to_string(), name.to_string()));
            }
            i += 1;
        }
        names
    }

    fn param(&self, (start, end): (usize, usize)) -> Option<(String, bool)> {
        let t = &self.t[start..end];
        match t {
            [] => None,
            [first, rest @ ..] if first.tok == Tok::Spread || (self.python && first.is('*')) => {
                rest.first()
                    .and_then(Token::ident)
                    .map(|name| (name.to_string(), true))
            }
            [first, ..] => first.ident().map(|name| (name.to_string(), false)),
        }
    }

    fn functions(&self) -> Vec<Function> {
        let t = &self.t;
        let mut found = Vec::new();
        for i in 0..t.len() {
            if !self.python
                && matches!(t[i].ident(), Some("const" | "let" | "var"))
                && t.get(i + 2).is_some_and(|token| token.is('='))
            {
                let Some(name) = t.get(i + 1).and_then(Token::ident) else {
                    continue;
                };
                let mut j = i + 3;
                if t.get(j).and_then(Token::ident) == Some("async") {
                    j += 1;
                }
                let (params, arrow) = if t.get(j).is_some_and(|token| token.is('(')) {
                    let Some((params, close)) = self.items(j) else {
                        continue;
                    };
                    (params.iter().map(|&p| self.param(p)).collect(), close + 1)
                } else if t.get(j).and_then(Token::ident).is_some() {
                    (vec![self.param((j, j + 1))], j + 1)
                } else {
                    continue;
                };
                if t.get(arrow).map(|token| &token.tok) != Some(&Tok::Arrow) {
                    continue;
                }
                let start = arrow + 1;
                let body = if t.get(start).is_some_and(|token| token.is('{')) {
                    (start, self.matching(start).unwrap_or(t.len()))
                } else {
                    (start, self.statement_end(start))
                };
                found.push(Function {
                    name: name.to_string(),
                    params,
                    body,
                });
            }
            let def = if self.python { "def" } else { "function" };
            if t[i].ident() == Some(def) && t.get(i + 2).is_some_and(|token| token.is('(')) {
                let Some(name) = t.get(i + 1).and_then(Token::ident) else {
                    continue;
                };
                let Some((params, close)) = self.items(i + 2) else {
                    continue;
                };
                let params: Vec<Option<(String, bool)>> =
                    params.iter().map(|&p| self.param(p)).collect();
                let body = if self.python {
                    let line = t[i].line;
                    let indent = self.indents.get(line).copied().unwrap_or(0);
                    let mut end = close + 1;
                    while end < t.len()
                        && (t[end].line == line
                            || !self.first_on_line(end)
                            || self.indents.get(t[end].line).copied().unwrap_or(0) > indent)
                    {
                        end += 1;
                    }
                    (close + 1, end)
                } else {
                    let Some(open) = (close + 1..t.len()).find(|&k| t[k].is('{')) else {
                        continue;
                    };
                    (open, self.matching(open).unwrap_or(t.len()))
                };
                found.push(Function {
                    name: name.to_string(),
                    params,
                    body,
                });
            }
        }
        found
    }

    /// The program an expression runs: a program written in it, or a
    /// variable holding one.
    fn expr_program(&self, (start, end): (usize, usize)) -> Option<Program> {
        self.program_in(start, end).map(|(program, _)| program)
    }

    fn is_list(&self, (start, end): (usize, usize)) -> bool {
        end > start && self.t[start].is('[') && self.matching(start) == Some(end - 1)
    }

    /// Functions that run a program with the arguments they are given.
    fn runners(&self, functions: &[Function], runners: &mut HashMap<String, Runner>) {
        for _ in 0..6 {
            let mut changed = false;
            for function in functions {
                if runners.contains_key(&function.name) {
                    continue;
                }
                if let Some(runner) = self.runs(function, runners) {
                    runners.insert(function.name.clone(), runner);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// The parameter an item passes on as it is (`args`, `...args`).
    fn passed_param(&self, function: &Function, item: (usize, usize)) -> Option<usize> {
        let (name, spread) = self.param(item)?;
        let len = item.1 - item.0;
        if !((len == 1 && !spread) || (len == 2 && spread)) {
            return None;
        }
        function
            .params
            .iter()
            .position(|param| param.as_ref().is_some_and(|(p, _)| *p == name))
    }

    /// How a function runs a program with the arguments it is given: it
    /// runs a program with one of its parameters (`execFileSync(bin, args)`),
    /// or passes one on to another such function (`call(...args)`).
    fn runs(&self, function: &Function, runners: &HashMap<String, Runner>) -> Option<Runner> {
        let (start, end) = function.body;
        for i in start..end.min(self.t.len()) {
            let Some(name) = self.t[i].ident() else {
                continue;
            };
            if !self.called(i) {
                continue;
            }
            let Some((items, _)) = self.items(i + 1) else {
                continue;
            };
            let Some(&first) = items.first() else {
                continue;
            };
            if EXEC.contains(&name) {
                if let Some(program) = self.expr_program(first) {
                    let used = |param: &str| {
                        items
                            .iter()
                            .any(|&(s, e)| (s..e).any(|k| self.t[k].ident() == Some(param)))
                    };
                    let offset = function
                        .params
                        .iter()
                        .position(|param| param.as_ref().is_some_and(|(p, _)| used(p)));
                    if let Some(offset) = offset {
                        return Some(Runner { program, offset });
                    }
                }
            }
            if let Some(inner) = runners.get(name).filter(|_| !self.member(i)) {
                let passed = items
                    .get(inner.offset)
                    .and_then(|&item| self.passed_param(function, item));
                if let Some(offset) = passed {
                    return Some(Runner {
                        program: inner.program.clone(),
                        offset,
                    });
                }
            }
        }
        None
    }

    fn arg(&self, (start, end): (usize, usize)) -> Option<Arg> {
        let t = &self.t[start..end];
        match t {
            [] => None,
            [only] => Some(match &only.tok {
                Tok::Str(text) | Tok::Num(text) => Arg::Lit(text.clone()),
                Tok::Tmpl(prefix) if !prefix.is_empty() => Arg::Part(prefix.clone()),
                _ => Arg::Dyn,
            }),
            [sign, number] if sign.is('-') => match &number.tok {
                Tok::Num(text) => Some(Arg::Lit(format!("-{text}"))),
                _ => Some(Arg::Dyn),
            },
            [first, ..] if first.tok == Tok::Spread || (self.python && first.is('*')) => {
                Some(Arg::Rest)
            }
            _ => Some(Arg::Dyn),
        }
    }

    fn args(&self, items: &[(usize, usize)]) -> Vec<Arg> {
        items.iter().filter_map(|&item| self.arg(item)).collect()
    }

    fn calls(&self, runners: &HashMap<String, Runner>, source: &mut Source) {
        let t = &self.t;
        let definitions = ["function", "def"];
        for i in 0..t.len() {
            let Some(name) = t[i].ident() else {
                continue;
            };
            if name == "fillSecret" && self.member(i) && self.called(i) {
                if let Some(Tok::Str(secret)) = t.get(i + 2).map(|token| &token.tok) {
                    source.secrets.push((t[i].line, secret.clone()));
                }
            }
            if !self.called(i) || (i > 0 && definitions.contains(&t[i - 1].ident().unwrap_or(""))) {
                continue;
            }
            let Some((items, _)) = self.items(i + 1) else {
                continue;
            };
            let namespaced = if self.member(i) && i >= 2 {
                t[i - 2].ident().map(|ns| format!("{ns}.{name}"))
            } else {
                None
            };
            let runner = match namespaced {
                Some(key) => runners.get(&key),
                None if !self.member(i) => runners.get(name),
                None => None,
            };
            let found = if let Some(runner) = runner {
                let given = items.get(runner.offset..).unwrap_or_default();
                let args = match given {
                    [only] if self.is_list(*only) => self
                        .items(only.0)
                        .map(|(inner, _)| self.args(&inner))
                        .unwrap_or_default(),
                    _ => self.args(given),
                };
                Some((runner.program.clone(), args))
            } else if EXEC.contains(&name) {
                match items.as_slice() {
                    [program, list, ..] if self.is_list(*list) => {
                        self.expr_program(*program).and_then(|program| {
                            self.items(list.0)
                                .map(|(inner, _)| (program, self.args(&inner)))
                        })
                    }
                    [list, ..] if self.is_list(*list) => {
                        self.items(list.0).and_then(|(inner, _)| {
                            let (&head, rest) = inner.split_first()?;
                            let program = self.expr_program(head)?;
                            Some((program, self.args(rest)))
                        })
                    }
                    _ => None,
                }
            } else {
                None
            };
            let Some((program, args)) = found else {
                continue;
            };
            if !matches!(args.first(), Some(Arg::Lit(_))) {
                continue;
            }
            if let [Arg::Lit(secret), Arg::Lit(fill), _, Arg::Lit(name), ..] = args.as_slice() {
                if secret == "secret" && fill == "fill" {
                    source.secrets.push((t[i].line, name.clone()));
                }
            }
            source.calls.push(Call {
                line: t[i].line,
                program,
                args,
                partial: false,
            });
        }
    }

    /// Argument lists kept in a variable and passed to a runner, or built in
    /// a desktop-script.mjs: the list, then the list with each `push`.
    fn lists(
        &self,
        runners: &HashMap<String, Runner>,
        helper: Option<&Program>,
        source: &mut Source,
    ) {
        let t = &self.t;
        for i in 0..t.len() {
            let declared = matches!(t[i].ident(), Some("const" | "let" | "var"))
                && t.get(i + 2).is_some_and(|token| token.is('='))
                && t.get(i + 3).is_some_and(|token| token.is('['));
            if !declared {
                continue;
            }
            let Some(name) = t.get(i + 1).and_then(Token::ident) else {
                continue;
            };
            let Some((items, close)) = self.items(i + 3) else {
                continue;
            };
            let base: Vec<Arg> = items
                .iter()
                .filter(|&&(s, _)| t[s].tok != Tok::Spread)
                .filter_map(|&item| self.arg(item))
                .collect();
            if !matches!(base.first(), Some(Arg::Lit(_))) {
                continue;
            }
            // The runner it is passed to (`run(...args)`), else the helper itself.
            let mut program = helper.cloned();
            for k in close..t.len() {
                let Some(runner) = t[k].ident().and_then(|r| runners.get(r)) else {
                    continue;
                };
                if self.member(k) || !self.called(k) {
                    continue;
                }
                let Some((given, _)) = self.items(k + 1) else {
                    continue;
                };
                let passes = given.get(runner.offset).is_some_and(|&(s, e)| {
                    (s..e).any(|j| t[j].ident() == Some(name)) && e - s <= 2
                });
                if passes {
                    program = Some(runner.program.clone());
                    break;
                }
            }
            let Some(program) = program else {
                continue;
            };
            source.lists.push(Call {
                line: t[i].line,
                program: program.clone(),
                args: base.clone(),
                partial: true,
            });
            for k in close..t.len() {
                let pushed = t[k].ident() == Some(name)
                    && t.get(k + 1).is_some_and(|token| token.is('.'))
                    && t.get(k + 2).and_then(Token::ident) == Some("push")
                    && t.get(k + 3).is_some_and(|token| token.is('('));
                if !pushed {
                    continue;
                }
                let Some((extra, _)) = self.items(k + 3) else {
                    continue;
                };
                let mut args = base.clone();
                args.extend(
                    extra
                        .iter()
                        .filter(|&&(s, _)| t[s].tok != Tok::Spread)
                        .filter_map(|&item| self.arg(item)),
                );
                source.lists.push(Call {
                    line: t[k].line,
                    program: program.clone(),
                    args,
                    partial: true,
                });
            }
        }
    }
}
