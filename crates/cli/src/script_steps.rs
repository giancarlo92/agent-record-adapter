// Part of Agent Record Adapter (see NOTICE).
//! `script-steps`: the steps of a script made from a recording, as JSON, so
//! another program (a bot runner) can repeat them without AI. Nothing is run.
//!
//! A JavaScript script that uses desktop-script.mjs is read as the helper's
//! calls: `task`'s title and values, each `step(...)` and what it does
//! (open a browser, find an element and set its value, a menu command...).
//! Any other script (Python, PowerShell, a shell, JavaScript without the
//! helper) gives each agent-record call written in it as a `command` action.
//!
//! What cannot be known without running the script (a loop, a condition, a
//! value computed at run time) is listed under `unsupported` with its line,
//! never guessed.

mod parse;

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Map, Value, json};

use crate::args::ScriptStepsArgs;
use crate::check_scripts::extract::{Arg, Program};
use crate::check_scripts::shell::Shell;
use crate::check_scripts::tokens::tokens;
use crate::check_scripts::{self, Language};
use parse::{Body, Expr, Kind, Pattern, Prop, Stmt};

/// The functions desktop-script.mjs gives a task.
const HELPERS: &[&str] = &[
    "task",
    "step",
    "confirm",
    "run",
    "findWindow",
    "openApp",
    "openBrowser",
    "readClipboard",
];

/// Globals that only compute a value (no step).
const PURE: &[&str] = &[
    "String",
    "Number",
    "Boolean",
    "JSON",
    "Math",
    "Date",
    "Object",
    "Array",
    "parseInt",
    "parseFloat",
    "isNaN",
    "encodeURIComponent",
    "decodeURIComponent",
];

pub(crate) fn run(args: ScriptStepsArgs) -> ExitCode {
    let path = std::path::absolute(&args.file).unwrap_or_else(|_| args.file.clone());
    let result = match std::fs::read(&path) {
        Ok(bytes) => read(&path, &String::from_utf8_lossy(&bytes)),
        Err(error) => Err(format!("{} could not be read: {error}", path.display())),
    };
    crate::run::emit(
        "script-steps",
        result.map_err(|message| {
            agent_desktop_core::AppError::Adapter(agent_desktop_core::AdapterError::new(
                agent_desktop_core::ErrorCode::InvalidArgs,
                message,
            ))
        }),
    )
}

/// The steps of the script at `path`, whose text is `text`.
pub(crate) fn read(path: &Path, text: &str) -> Result<Value, String> {
    let language = check_scripts::language(path).ok_or_else(|| {
        format!(
            "{} is not a script this reads (.js, .mjs, .ts, .py, .ps1, .sh, .cmd)",
            path.display()
        )
    })?;
    let home = agent_desktop_core::refs::home_dir();
    let source = check_scripts::analyse(path, text, home.as_deref()).unwrap_or_default();
    let helper = !source.helpers.is_empty() || !source.helper_names.is_empty();
    let name = match language {
        Language::Code { python: false } => "javascript",
        Language::Code { python: true } => "python",
        Language::Script(Shell::Sh) => "shell",
        Language::Script(Shell::Ps1) => "powershell",
        Language::Script(Shell::Cmd) => "cmd",
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut data = if helper && matches!(language, Language::Code { python: false }) {
        Reader::new(text).helper_script().unwrap_or_else(|| {
            json!({"title": stem, "params": [], "windows": [], "steps": [], "unsupported": [
                {"line": 1, "code": "", "reason": "no task(...) call of desktop-script.mjs was found", "step": null}
            ]})
        })
    } else {
        commands(text, &source.calls, &stem)
    };
    data["language"] = json!(name);
    data["helper"] = json!(helper);
    Ok(data)
}

/// A script without the helper: each agent-record call written in it is a
/// step with one `command` action.
fn commands(text: &str, calls: &[check_scripts::extract::Call], stem: &str) -> Value {
    let lines: Vec<&str> = text.lines().collect();
    let line_text = |line: usize| short(lines.get(line.wrapping_sub(1)).copied().unwrap_or(""));
    let mut steps = Vec::new();
    let mut unsupported = Vec::new();
    for call in calls {
        let program = match call.program {
            Program::Front => "agent-record",
            Program::Engine => "agent-desktop",
            Program::Browser => "agent-browser",
            Program::Helper(_) => "agent-record",
        };
        let written: Option<Vec<&String>> = call
            .args
            .iter()
            .map(|arg| match arg {
                Arg::Lit(text) => Some(text),
                _ => None,
            })
            .collect();
        let Some(written) = written.filter(|w| !w.is_empty() && !call.partial) else {
            unsupported.push(json!({
                "line": call.line,
                "code": line_text(call.line),
                "reason": "a value of this call is known only when the script runs",
                "step": null,
            }));
            continue;
        };
        let shown = written.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
        steps.push(json!({
            "number": steps.len() + 1,
            "title": format!("{program} {shown}"),
            "line": call.line,
            "actions": [{
                "kind": "command",
                "program": program,
                "command": written[0],
                "args": written[1..].iter().map(|s| json!({"text": s})).collect::<Vec<_>>(),
                "line": call.line,
            }],
        }));
    }
    json!({"title": stem, "params": [], "windows": [], "steps": steps, "unsupported": unsupported})
}

fn short(code: &str) -> String {
    let code = code.split_whitespace().collect::<Vec<_>>().join(" ");
    if code.chars().count() > 160 {
        format!("{}…", code.chars().take(160).collect::<String>())
    } else {
        code
    }
}

fn snake(key: &str) -> String {
    let mut out = String::new();
    for ch in key.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// A written value.
#[derive(Clone, Debug)]
enum Val {
    Text(String),
    Param(String),
    /// Text with values in braces: `Create the account {username}?`
    Template(String),
    Env(String),
    Num(String),
    Bool(bool),
    Null,
}

/// What an expression stands for while the script is read.
#[derive(Clone, Debug)]
enum Sym {
    Val(Val),
    Window(String),
    Element(String, Map<String, Value>),
    Object(Vec<(String, Sym)>),
    Array(Vec<Sym>),
    /// A `read` action of the statement being read (its index), until it is
    /// given a name.
    ReadPending(usize),
    /// The text a `read` action kept under this name.
    ReadVar(String),
    Params,
    Helper,
    /// The generated `win(id, parts, app)`: the window opened under `id`,
    /// else its app's window found by title.
    Lookup,
    /// The generated `windows = {}`.
    WindowMap,
    Unset(String),
    /// Known only when the script runs: why.
    Computed(String),
}

struct Reader {
    chars: Vec<char>,
    lines: Vec<String>,
    env: HashMap<String, Sym>,
    helpers: HashMap<String, &'static str>,
    windows: Vec<Value>,
    steps: Vec<Value>,
    unsupported: Vec<Value>,
    secrets: Vec<String>,
    fresh: usize,
    /// The actions of the statement being read.
    pending: Vec<Map<String, Value>>,
    /// The actions of the step being read, when inside one.
    step_actions: Option<Vec<Value>>,
    step: Option<Value>,
    line: usize,
}

type Read<T> = Result<T, String>;

impl Reader {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            lines: text.lines().map(str::to_string).collect(),
            env: HashMap::new(),
            helpers: HELPERS.iter().map(|h| ((*h).to_string(), *h)).collect(),
            windows: Vec::new(),
            steps: Vec::new(),
            unsupported: Vec::new(),
            secrets: Vec::new(),
            fresh: 0,
            pending: Vec::new(),
            step_actions: None,
            step: None,
            line: 0,
        }
    }

    fn helper_script(mut self) -> Option<Value> {
        let text: String = self.chars.iter().collect();
        let program = parse::program(&tokens(&text, false));
        let args = program.iter().find_map(|stmt| match &stmt.kind {
            Kind::Expr(e) | Kind::Decl(_, Some(e)) => task_call(e),
            _ => None,
        })?;
        let spec = args.first();
        let title = match spec {
            Some(Expr::Object(props)) => prop(props, "title").map(|e| self.title_text(e)),
            _ => None,
        }
        .unwrap_or_else(|| "Task".to_string());
        let mut params = Vec::new();
        if let Some(Expr::Object(props)) = spec
            && let Some(Expr::Object(items)) = prop(props, "params")
        {
            for item in items {
                let Prop::Pair(name, Expr::Object(fields)) = item else {
                    continue;
                };
                let text_of = |key: &str| prop(fields, key).and_then(|e| self.plain(e));
                let options = match prop(fields, "options") {
                    Some(Expr::Array(items)) => {
                        items.iter().filter_map(|e| self.plain(e)).collect::<Vec<_>>()
                    }
                    _ => Vec::new(),
                };
                params.push(json!({
                    "name": name,
                    "ask": text_of("ask").unwrap_or_default(),
                    "example": text_of("example"),
                    "options": options,
                    "default": text_of("default"),
                    "secret": false,
                }));
            }
        }
        match args.get(1) {
            Some(Expr::Arrow(patterns, body)) => {
                if let Some(Pattern::Object(entries)) = patterns.first() {
                    for (key, local) in entries {
                        let local = local.clone().unwrap_or_else(|| key.clone());
                        if key == "params" {
                            self.env.insert(local, Sym::Params);
                        } else if let Some(helper) = HELPERS.iter().find(|h| **h == key) {
                            self.helpers.insert(local, helper);
                        }
                    }
                }
                match body {
                    Body::Block(stmts) => self.statements(stmts),
                    Body::Expr(expr) => {
                        let stmt = Stmt {
                            line: 1,
                            span: (0, 0),
                            kind: Kind::Expr(expr.as_ref().clone()),
                        };
                        self.statements(std::slice::from_ref(&stmt));
                    }
                }
            }
            _ => self.unsupported.push(json!({
                "line": 1, "code": "", "reason": "the task's steps are not written as a function", "step": null,
            })),
        }
        for param in &mut params {
            if let Some(name) = param["name"].as_str()
                && self.secrets.iter().any(|s| s == name)
            {
                param["secret"] = json!(true);
            }
        }
        Some(json!({
            "title": title,
            "params": params,
            "windows": self.windows,
            "steps": self.steps,
            "unsupported": self.unsupported,
        }))
    }

    // ---- statements ----

    fn statements(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match &stmt.kind {
                Kind::Expr(e) if self.step_actions.is_none() && step_call(e, &self.helpers).is_some() => {
                    let args = step_call(e, &self.helpers).unwrap_or_default();
                    self.step(stmt, args);
                }
                Kind::Block(inner) => self.statements(inner),
                Kind::Empty => {}
                Kind::Other(word) => self.skip(stmt, reason_for(word)),
                _ => self.statement(stmt),
            }
        }
    }

    fn step(&mut self, stmt: &Stmt, args: &[Expr]) {
        let number = match args.first() {
            Some(Expr::Num(n)) => n
                .parse::<i64>()
                .map(Value::from)
                .ok()
                .or_else(|| n.parse::<f64>().ok().map(Value::from)),
            _ => None,
        };
        let Some(number) = number else {
            self.skip(stmt, "a step whose number is computed when the script runs".into());
            return;
        };
        let title = args.get(1).map(|e| self.title_text(e)).unwrap_or_default();
        self.step = Some(number.clone());
        self.step_actions = Some(Vec::new());
        match args.get(2) {
            Some(Expr::Arrow(_, Body::Block(body))) => self.statements(body),
            Some(Expr::Arrow(_, Body::Expr(expr))) => {
                let inner = Stmt {
                    line: stmt.line,
                    span: stmt.span,
                    kind: Kind::Expr(expr.as_ref().clone()),
                };
                self.statement(&inner);
            }
            _ => self.skip(stmt, "a step whose action is not written as a function".into()),
        }
        let actions = self.step_actions.take().unwrap_or_default();
        self.steps.push(json!({
            "number": number,
            "title": title,
            "line": stmt.line,
            "actions": actions,
        }));
        self.step = None;
    }

    fn statement(&mut self, stmt: &Stmt) {
        self.pending.clear();
        self.line = stmt.line;
        let done = match &stmt.kind {
            Kind::Decl(pattern, init) => self.declare(pattern, init.as_ref()),
            Kind::Expr(Expr::Assign(op, target, value)) if op == "=" => {
                self.assign(target, value)
            }
            Kind::Expr(e) => self.eval(e).map(|_| ()),
            Kind::Return(Some(e)) => self.eval(e).map(|_| ()),
            _ => Ok(()),
        };
        match done {
            Ok(()) => self.commit(stmt),
            Err(reason) => {
                // What it would have set is not known either.
                for name in set_names(&stmt.kind) {
                    let why = format!("{name} is set by code the steps cannot express (line {})", stmt.line);
                    self.env.insert(name, Sym::Computed(why));
                }
                self.skip(stmt, reason);
            }
        }
        self.pending.clear();
    }

    fn commit(&mut self, stmt: &Stmt) {
        let actions: Vec<Map<String, Value>> = self.pending.drain(..).collect();
        if actions.is_empty() {
            return;
        }
        let Some(step_actions) = self.step_actions.as_mut() else {
            self.skip(stmt, "an action outside a step".into());
            return;
        };
        for mut action in actions {
            if action.get("into").is_some_and(Value::is_null) {
                action.remove("into");
            }
            let kind = action["kind"].as_str().unwrap_or_default().to_string();
            let window_kind = match kind.as_str() {
                "open_browser" => Some("browser"),
                "open_app" => Some("app"),
                "find_window" => Some("find"),
                _ => None,
            };
            if let Some(window_kind) = window_kind {
                let var = action["window"].clone();
                if !self.windows.iter().any(|w| w["var"] == var) {
                    let mut entry = Map::new();
                    entry.insert("var".into(), var);
                    entry.insert("kind".into(), json!(window_kind));
                    for (key, value) in &action {
                        if !matches!(key.as_str(), "kind" | "window" | "line") {
                            entry.insert(key.clone(), value.clone());
                        }
                    }
                    self.windows.push(Value::Object(entry));
                }
            }
            step_actions.push(Value::Object(action));
        }
    }

    fn skip(&mut self, stmt: &Stmt, reason: String) {
        let code = if stmt.span.1 > stmt.span.0 {
            self.chars[stmt.span.0..stmt.span.1.min(self.chars.len())]
                .iter()
                .collect::<String>()
        } else {
            self.lines.get(stmt.line.wrapping_sub(1)).cloned().unwrap_or_default()
        };
        self.unsupported.push(json!({
            "line": stmt.line,
            "code": short(&code),
            "reason": reason,
            "step": self.step,
        }));
    }

    fn declare(&mut self, pattern: &Pattern, init: Option<&Expr>) -> Read<()> {
        match (pattern, init) {
            (Pattern::Name(name), None) => {
                self.env.insert(name.clone(), Sym::Unset(name.clone()));
            }
            (Pattern::Name(name), Some(arrow @ Expr::Arrow(params, _))) => {
                let lookup = params.len() == 3 && parse::calls(arrow, "findWindow");
                let sym = if lookup {
                    Sym::Lookup
                } else {
                    Sym::Computed(format!("calls {name}, a function of the script"))
                };
                self.env.insert(name.clone(), sym);
            }
            (Pattern::Name(name), Some(Expr::Object(props))) if props.is_empty() => {
                self.env.insert(name.clone(), Sym::WindowMap);
            }
            (Pattern::Name(name), Some(init)) => {
                let sym = self.eval(init)?;
                self.bind(name, sym);
            }
            (Pattern::Object(entries), Some(init)) => {
                let sym = self.eval(init)?;
                for (key, local) in entries {
                    let Some(local) = local else { continue };
                    let value = match sym {
                        Sym::Params => Sym::Val(Val::Param(key.clone())),
                        _ => Sym::Computed(format!("{local} is known only when the script runs")),
                    };
                    self.env.insert(local.clone(), value);
                }
            }
            (Pattern::Array(items), Some(init)) => {
                self.eval(init)?;
                for local in items.iter().flatten() {
                    self.env.insert(
                        local.clone(),
                        Sym::Computed(format!("{local} is computed when the script runs")),
                    );
                }
            }
            (_, Some(init)) => {
                self.eval(init)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn assign(&mut self, target: &Expr, value: &Expr) -> Read<()> {
        match target {
            Expr::Ident(name) => {
                let sym = self.eval(value)?;
                self.bind(name, sym);
            }
            Expr::Index(map, key)
                if matches!(map.as_ref(), Expr::Ident(m) if matches!(self.env.get(m), Some(Sym::WindowMap))) =>
            {
                let Expr::Str(id) = key.as_ref() else {
                    return Err("keeps a window under a name computed at run time".into());
                };
                match self.eval(value)? {
                    Sym::Window(var) => self.rename(&var, id),
                    _ => return Err("keeps something other than a window in the windows".into()),
                }
            }
            _ => {
                self.eval(value)?;
            }
        }
        Ok(())
    }

    /// `name` now stands for `sym`; a window or a read of this statement
    /// takes that name.
    fn bind(&mut self, name: &str, sym: Sym) {
        let sym = match sym {
            Sym::Window(var) if self.opened_here(&var) => {
                self.rename(&var, name);
                Sym::Window(name.to_string())
            }
            Sym::ReadPending(index) => {
                if let Some(action) = self.pending.get_mut(index) {
                    action.insert("into".into(), json!(name));
                }
                Sym::ReadVar(name.to_string())
            }
            other => other,
        };
        self.env.insert(name.to_string(), sym);
    }

    fn opened_here(&self, var: &str) -> bool {
        self.pending.iter().any(|a| {
            a.get("window").and_then(Value::as_str) == Some(var)
                && matches!(
                    a.get("kind").and_then(Value::as_str),
                    Some("open_browser" | "open_app" | "find_window")
                )
        })
    }

    fn rename(&mut self, from: &str, to: &str) {
        for action in &mut self.pending {
            if action.get("window").and_then(Value::as_str) == Some(from) {
                action.insert("window".into(), json!(to));
            }
        }
    }

    // ---- expressions ----

    fn eval(&mut self, expr: &Expr) -> Read<Sym> {
        Ok(match expr {
            Expr::Str(text) => Sym::Val(Val::Text(text.clone())),
            Expr::Tmpl(start, end) => self.template(*start, *end),
            Expr::Num(n) => Sym::Val(Val::Num(n.clone())),
            Expr::Ident(name) => match name.as_str() {
                "true" => Sym::Val(Val::Bool(true)),
                "false" => Sym::Val(Val::Bool(false)),
                "null" | "undefined" => Sym::Val(Val::Null),
                _ => {
                    if let Some(sym) = self.env.get(name) {
                        sym.clone()
                    } else if self.helpers.contains_key(name) {
                        Sym::Helper
                    } else {
                        Sym::Computed(format!("{name} is known only when the script runs"))
                    }
                }
            },
            Expr::Member(object, name) => {
                if let Expr::Member(inner, env) = object.as_ref()
                    && env == "env"
                    && matches!(inner.as_ref(), Expr::Ident(p) if p == "process")
                {
                    return Ok(Sym::Val(Val::Env(name.clone())));
                }
                match self.eval(object)? {
                    Sym::Params => Sym::Val(Val::Param(name.clone())),
                    _ => Sym::Computed(format!("{name} is read when the script runs")),
                }
            }
            Expr::Index(object, index) => match (self.eval(object)?, index.as_ref()) {
                (Sym::Params, Expr::Str(key)) => Sym::Val(Val::Param(key.clone())),
                (Sym::WindowMap, Expr::Str(id)) => {
                    if self.known_window(id) {
                        Sym::Window(id.clone())
                    } else {
                        Sym::Unset(id.clone())
                    }
                }
                _ => Sym::Computed("a value picked when the script runs".into()),
            },
            Expr::Call(callee, args) => self.call(callee, args)?,
            Expr::Await(inner) => self.eval(inner)?,
            Expr::New(inner) => {
                if let Some(ms) = sleep_ms(inner) {
                    self.act("wait", None, vec![("ms", number(&ms))]);
                    Sym::Val(Val::Null)
                } else {
                    Sym::Computed("an object made when the script runs".into())
                }
            }
            Expr::Object(props) => {
                let mut pairs = Vec::new();
                for item in props {
                    match item {
                        Prop::Pair(key, value) => pairs.push((key.clone(), self.eval(value)?)),
                        _ => return Ok(Sym::Computed("an object built when the script runs".into())),
                    }
                }
                Sym::Object(pairs)
            }
            Expr::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    match item {
                        Expr::Spread(_) => {
                            return Ok(Sym::Computed("a list built when the script runs".into()));
                        }
                        _ => out.push(self.eval(item)?),
                    }
                }
                Sym::Array(out)
            }
            Expr::Unary(op, inner) if op == "-" => match self.eval(inner)? {
                Sym::Val(Val::Num(n)) => Sym::Val(Val::Num(format!("-{n}"))),
                _ => Sym::Computed("a value computed when the script runs".into()),
            },
            Expr::Binary(op, left, right) if op == "+" => {
                let (left, right) = (self.eval(left)?, self.eval(right)?);
                match (braced(&left), braced(&right)) {
                    (Some(a), Some(b)) => template_val(format!("{a}{b}")),
                    _ => Sym::Computed("a value computed when the script runs".into()),
                }
            }
            Expr::Cond(_, yes, no) => {
                if acts(yes) || acts(no) {
                    return Err("chooses between two ways when the script runs".into());
                }
                Sym::Computed("a value chosen when the script runs".into())
            }
            Expr::Assign(op, target, value) => {
                if op == "=" {
                    self.assign(target, value)?;
                    self.eval(value)?
                } else {
                    return Err("changes a value of the script".into());
                }
            }
            Expr::Arrow(..) => Sym::Computed("a function of the script".into()),
            _ => Sym::Computed("a value computed when the script runs".into()),
        })
    }

    fn known_window(&self, id: &str) -> bool {
        self.windows.iter().any(|w| w["var"] == id) || self.opened_here(id)
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> Read<Sym> {
        match callee {
            Expr::Member(object, method) => {
                if method == "catch" {
                    let before = self.pending.len();
                    let sym = self.eval(object)?;
                    for action in &mut self.pending[before..] {
                        action.insert("optional".into(), json!(true));
                    }
                    return Ok(sym);
                }
                if matches!(method.as_str(), "then" | "finally") {
                    return Err("works on the result when the script runs".into());
                }
                if let Expr::Ident(name) = object.as_ref() {
                    if name == "console" {
                        return Ok(Sym::Val(Val::Null));
                    }
                    if !self.env.contains_key(name)
                        && !self.helpers.contains_key(name)
                        && !PURE.contains(&name.as_str())
                    {
                        return Err(format!(
                            "calls {name}.{method}, which runs in the script itself, not as a step"
                        ));
                    }
                }
                match self.eval(object)? {
                    Sym::Window(window) => self.window_method(&window, method, args),
                    Sym::Element(window, target) => {
                        self.element_method(&window, target, method, args)
                    }
                    Sym::Unset(name) => Err(format!("uses {name} before a step sets it")),
                    Sym::Computed(why) if acting(method) => Err(why),
                    Sym::Lookup | Sym::Helper => Err(format!("calls {method} on a function")),
                    _ => Ok(Sym::Computed(format!("{method} is computed when the script runs"))),
                }
            }
            Expr::Ident(name) => match self.env.get(name).cloned() {
                Some(Sym::Lookup) => self.lookup(args),
                Some(_) => Err(format!("calls {name}, a function of the script")),
                None => match self.helpers.get(name).copied() {
                    Some(helper) => self.helper(helper, args),
                    None if PURE.contains(&name.as_str()) => {
                        Ok(Sym::Computed(format!("{name} is computed when the script runs")))
                    }
                    None => Err(format!(
                        "calls {name}, which runs in the script itself, not as a step"
                    )),
                },
            },
            _ => Err("code the steps cannot express".into()),
        }
    }

    fn arg(&mut self, args: &[Expr], index: usize) -> Read<Option<Sym>> {
        match args.get(index) {
            Some(Expr::Spread(_)) => Err("passes values spread from a list".into()),
            Some(expr) => self.eval(expr).map(Some),
            None => Ok(None),
        }
    }

    fn helper(&mut self, helper: &'static str, args: &[Expr]) -> Read<Sym> {
        match helper {
            "openBrowser" | "openApp" => {
                let first = self.arg(args, 0)?;
                let first = value(first.as_ref())?;
                let options = options(self.arg(args, 1)?.as_ref())?;
                self.fresh += 1;
                let window = format!("window{}", self.fresh);
                let (kind, key) = if helper == "openBrowser" {
                    ("open_browser", "url")
                } else {
                    ("open_app", "app")
                };
                let mut fields = vec![(key, first)];
                fields.extend(options.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.act(kind, Some(&window), fields);
                Ok(Sym::Window(window))
            }
            "findWindow" => {
                let title = self.arg(args, 0)?;
                let options = options(self.arg(args, 1)?.as_ref())?;
                self.fresh += 1;
                let window = format!("window{}", self.fresh);
                let mut fields = Vec::new();
                match title {
                    Some(Sym::Array(parts)) => fields.push(("title_parts", values(&parts)?)),
                    Some(title) => fields.push(("title", value(Some(&title))?)),
                    None => {}
                }
                fields.extend(options.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.act("find_window", Some(&window), fields);
                Ok(Sym::Window(window))
            }
            "confirm" => {
                let message = value(self.arg(args, 0)?.as_ref())?;
                self.act("confirm", None, vec![("message", message)]);
                Ok(Sym::Val(Val::Null))
            }
            "run" => {
                let mut all = Vec::new();
                for index in 0..args.len() {
                    all.push(self.arg(args, index)?);
                }
                let command = match all.first() {
                    Some(Some(Sym::Val(Val::Text(command)))) => command.clone(),
                    _ => return Err("runs a command whose name is computed at run time".into()),
                };
                let rest: Vec<Value> = all[1..]
                    .iter()
                    .map(|sym| value(sym.as_ref()))
                    .collect::<Read<_>>()?;
                self.act(
                    "command",
                    None,
                    vec![
                        ("program", json!("agent-record")),
                        ("command", json!(command)),
                        ("args", Value::Array(rest)),
                    ],
                );
                Ok(Sym::Computed(format!("the answer of {command}, known when the script runs")))
            }
            "readClipboard" => Ok(Sym::Computed("the clipboard, read when the script runs".into())),
            "step" => Err("a step inside a step".into()),
            _ => Err(format!("calls {helper} where a step cannot")),
        }
    }

    /// The generated `win(id, parts, app)`.
    fn lookup(&mut self, args: &[Expr]) -> Read<Sym> {
        let Some(Sym::Val(Val::Text(id))) = self.arg(args, 0)? else {
            return Err("looks up a window under a name computed at run time".into());
        };
        if self.known_window(&id) {
            return Ok(Sym::Window(id));
        }
        let mut fields = Vec::new();
        match self.arg(args, 1)? {
            Some(Sym::Array(parts)) => fields.push(("title_parts", values(&parts)?)),
            Some(title @ Sym::Val(_)) => fields.push(("title", value(Some(&title))?)),
            _ => {}
        }
        if let Some(app) = self.arg(args, 2)?
            && !matches!(app, Sym::Val(Val::Null))
        {
            fields.push(("app", value(Some(&app))?));
        }
        self.act("find_window", Some(&id), fields);
        Ok(Sym::Window(id))
    }

    fn window_method(&mut self, window: &str, method: &str, args: &[Expr]) -> Read<Sym> {
        let w = Some(window);
        match method {
            "find" => {
                let target = target(self.arg(args, 0)?.as_ref())?;
                Ok(Sym::Element(window.to_string(), target))
            }
            "read" => {
                let target = target(self.arg(args, 0)?.as_ref())?;
                let mut fields = vec![("target", Value::Object(target)), ("into", Value::Null)];
                let options = options(self.arg(args, 1)?.as_ref())?;
                fields.extend(options.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.act("read", w, fields);
                Ok(Sym::ReadPending(self.pending.len() - 1))
            }
            "waitForNew" => {
                let target = target(self.arg(args, 0)?.as_ref())?;
                let before = match self.arg(args, 1)? {
                    Some(Sym::ReadVar(name)) => json!(name),
                    None | Some(Sym::Val(Val::Null)) => Value::Null,
                    _ => return Err("waits for a text other than one read before".into()),
                };
                let mut fields = vec![("target", Value::Object(target)), ("before", before)];
                let options = options(self.arg(args, 2)?.as_ref())?;
                fields.extend(options.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.act("wait_new", w, fields);
                Ok(Sym::Computed("the new text, read when the script runs".into()))
            }
            "waitForText" => {
                let text = value(self.arg(args, 0)?.as_ref())?;
                let mut fields = vec![("text", text)];
                let options = options(self.arg(args, 1)?.as_ref())?;
                fields.extend(options.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.act("wait_text", w, fields);
                Ok(Sym::Val(Val::Null))
            }
            "write" => {
                let target = target(self.arg(args, 0)?.as_ref())?;
                let text = value(self.arg(args, 1)?.as_ref())?;
                self.act("write", w, vec![("target", Value::Object(target)), ("text", text)]);
                Ok(Sym::Val(Val::Null))
            }
            "menu" => {
                let mut path = Vec::new();
                for index in 0..args.len() {
                    path.push(value(self.arg(args, index)?.as_ref())?);
                }
                self.act("menu", w, vec![("path", Value::Array(path))]);
                Ok(Sym::Val(Val::Null))
            }
            "shortcut" => {
                let keys = value(self.arg(args, 0)?.as_ref())?;
                self.act("shortcut", w, vec![("keys", keys)]);
                Ok(Sym::Val(Val::Null))
            }
            "chooseMenuItem" => {
                let name = value(self.arg(args, 0)?.as_ref())?;
                self.act("choose_menu_item", w, vec![("name", name)]);
                Ok(Sym::Val(Val::Null))
            }
            "clickAt" | "clickAtOffset" => {
                let (kind, xk, yk) = if method == "clickAt" {
                    ("click_at", "x", "y")
                } else {
                    ("click_at_offset", "dx", "dy")
                };
                let x = coordinate(self.arg(args, 0)?)?;
                let y = coordinate(self.arg(args, 1)?)?;
                let mut fields = vec![(xk, x), (yk, y)];
                let options = options(self.arg(args, 2)?.as_ref())?;
                fields.extend(options.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.act(kind, w, fields);
                Ok(Sym::Val(Val::Null))
            }
            "clickIn" => {
                let target = target(self.arg(args, 0)?.as_ref())?;
                let dx = coordinate(self.arg(args, 1)?)?;
                let dy = coordinate(self.arg(args, 2)?)?;
                let mut fields = vec![("target", Value::Object(target)), ("dx", dx), ("dy", dy)];
                let options = options(self.arg(args, 3)?.as_ref())?;
                fields.extend(options.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.act("click_in", w, fields);
                Ok(Sym::Val(Val::Null))
            }
            "screenshot" => {
                let file = value(self.arg(args, 0)?.as_ref())?;
                self.act("screenshot", w, vec![("file", file)]);
                Ok(Sym::Val(Val::Null))
            }
            "close" => {
                self.act("close", w, Vec::new());
                Ok(Sym::Val(Val::Null))
            }
            "boundsOf" | "has" | "currentTitle" => Ok(Sym::Computed(format!(
                "{method} is read when the script runs"
            ))),
            _ => Err(format!("{method} is not a step of desktop-script.mjs")),
        }
    }

    fn element_method(
        &mut self,
        window: &str,
        target: Map<String, Value>,
        method: &str,
        args: &[Expr],
    ) -> Read<Sym> {
        let w = Some(window);
        let target = ("target", Value::Object(target));
        match method {
            "click" | "clear" | "check" | "uncheck" | "paste" | "expand" | "collapse" => {
                self.act(method, w, vec![target]);
                Ok(Sym::Val(Val::Null))
            }
            "setValue" | "select" => {
                let given = self.arg(args, 0)?;
                let key = if method == "setValue" { "value" } else { "option" };
                let kind = if method == "setValue" { "set_value" } else { "select" };
                self.act(kind, w, vec![target, (key, value(given.as_ref())?)]);
                Ok(Sym::Val(Val::Null))
            }
            "fillSecret" => {
                let given = self.arg(args, 0)?;
                if let Some(Sym::Val(Val::Param(name))) = &given {
                    self.secrets.push(name.clone());
                }
                self.act("fill_secret", w, vec![target, ("secret", value(given.as_ref())?)]);
                Ok(Sym::Computed("whether it was asked for, known when the script runs".into()))
            }
            "value" | "text" | "bounds" | "is" => Ok(Sym::Computed(format!(
                "{method} is read when the script runs"
            ))),
            _ => Err(format!("{method} is not a step of desktop-script.mjs")),
        }
    }

    fn act(&mut self, kind: &str, window: Option<&str>, fields: Vec<(&str, Value)>) {
        let mut action = Map::new();
        action.insert("kind".into(), json!(kind));
        if let Some(window) = window {
            action.insert("window".into(), json!(window));
        }
        for (key, value) in fields {
            action.insert(key.to_string(), value);
        }
        action.insert("line".into(), json!(self.line));
        self.pending.push(action);
    }

    // ---- written text ----

    /// A template literal: `{name}` for each `${params.name}`; any other
    /// filled part makes it a value computed at run time.
    fn template(&mut self, start: usize, end: usize) -> Sym {
        match self.template_text(start, end, false) {
            Some((text, true)) => Sym::Val(Val::Template(text)),
            Some((text, false)) => Sym::Val(Val::Text(unbrace(&text))),
            None => Sym::Computed("a text built when the script runs".into()),
        }
    }

    /// The template's text with values in braces, and whether it has any.
    /// `loose`: a part that is not a value is written as its code in braces
    /// (for titles) instead of failing.
    fn template_text(&mut self, start: usize, end: usize, loose: bool) -> Option<(String, bool)> {
        let inner: Vec<char> = self.chars[(start + 1).min(end)..end.saturating_sub(1).max(start + 1)].to_vec();
        let mut out = String::new();
        let mut filled = false;
        let mut i = 0;
        while i < inner.len() {
            let ch = inner[i];
            if ch == '\\' && i + 1 < inner.len() {
                let next = inner[i + 1];
                out.push(match next {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
                i += 2;
                continue;
            }
            if ch == '$' && inner.get(i + 1) == Some(&'{') {
                let mut depth = 0;
                let mut j = i + 1;
                while j < inner.len() {
                    match inner[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let code: String = inner[i + 2..j.min(inner.len())].iter().collect();
                let program = parse::program(&tokens(&code, false));
                let sym = match program.as_slice() {
                    [Stmt { kind: Kind::Expr(expr), .. }] => {
                        let saved = self.pending.len();
                        let sym = self.eval(expr).ok();
                        self.pending.truncate(saved);
                        sym
                    }
                    _ => None,
                };
                match sym {
                    Some(Sym::Val(Val::Param(name))) => {
                        out.push('{');
                        out.push_str(&name);
                        out.push('}');
                        filled = true;
                    }
                    Some(Sym::Val(Val::Text(text) | Val::Num(text))) => out.push_str(&brace(&text)),
                    Some(Sym::Val(Val::Template(text))) => {
                        out.push_str(&text);
                        filled = true;
                    }
                    _ if loose => {
                        out.push('{');
                        out.push_str(code.trim());
                        out.push('}');
                        filled = true;
                    }
                    _ => return None,
                }
                i = j + 1;
                continue;
            }
            match ch {
                '{' => out.push_str("{{"),
                '}' => out.push_str("}}"),
                _ => out.push(ch),
            }
            i += 1;
        }
        Some((out, filled))
    }

    /// A step's or task's title as text (`{name}` where a value goes).
    fn title_text(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Str(text) => text.clone(),
            Expr::Tmpl(start, end) => self
                .template_text(*start, *end, true)
                .map(|(text, filled)| if filled { text } else { unbrace(&text) })
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// A written text, number or yes/no as a plain string.
    fn plain(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Str(text) | Expr::Num(text) => Some(text.clone()),
            Expr::Ident(word) if word == "true" || word == "false" => Some(word.clone()),
            _ => None,
        }
    }
}

/// The names a statement sets (`const x = ...`, `x = ...`).
fn set_names(kind: &Kind) -> Vec<String> {
    match kind {
        Kind::Decl(Pattern::Name(name), _) => vec![name.clone()],
        Kind::Decl(Pattern::Object(entries), _) => entries
            .iter()
            .filter_map(|(key, local)| local.clone().or_else(|| Some(key.clone())))
            .collect(),
        Kind::Decl(Pattern::Array(items), _) => items.iter().flatten().cloned().collect(),
        Kind::Expr(Expr::Assign(_, target, _)) => match target.as_ref() {
            Expr::Ident(name) => vec![name.clone()],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The arguments of `task(...)`, when `expr` is that call.
fn task_call(expr: &Expr) -> Option<&[Expr]> {
    match expr {
        Expr::Await(inner) => task_call(inner),
        Expr::Call(callee, args) if matches!(callee.as_ref(), Expr::Ident(n) if n == "task") => {
            Some(args)
        }
        _ => None,
    }
}

/// The arguments of `step(...)`, when `expr` is that call.
fn step_call<'a>(expr: &'a Expr, helpers: &HashMap<String, &'static str>) -> Option<&'a [Expr]> {
    match expr {
        Expr::Await(inner) => step_call(inner, helpers),
        Expr::Call(callee, args)
            if matches!(callee.as_ref(), Expr::Ident(n) if helpers.get(n) == Some(&"step")) =>
        {
            Some(args)
        }
        _ => None,
    }
}

fn prop<'a>(props: &'a [Prop], key: &str) -> Option<&'a Expr> {
    props.iter().find_map(|p| match p {
        Prop::Pair(k, e) if k == key => Some(e),
        _ => None,
    })
}

/// `new Promise((done) => setTimeout(done, 1500))`: the milliseconds.
fn sleep_ms(expr: &Expr) -> Option<String> {
    let Expr::Call(callee, args) = expr else { return None };
    if !matches!(callee.as_ref(), Expr::Ident(n) if n == "Promise") {
        return None;
    }
    let Some(Expr::Arrow(_, body)) = args.first() else { return None };
    let call = match body {
        Body::Expr(e) => e.as_ref(),
        Body::Block(stmts) => match stmts.as_slice() {
            [Stmt { kind: Kind::Expr(e), .. }] => e,
            _ => return None,
        },
    };
    let Expr::Call(timer, timer_args) = call else { return None };
    if !matches!(timer.as_ref(), Expr::Ident(n) if n == "setTimeout") {
        return None;
    }
    match timer_args.get(1) {
        Some(Expr::Num(ms)) => Some(ms.clone()),
        _ => None,
    }
}

/// Whether evaluating `expr` could act (it calls or awaits something).
fn acts(expr: &Expr) -> bool {
    match expr {
        Expr::Call(..) | Expr::Await(_) | Expr::New(_) => true,
        Expr::Member(inner, _) | Expr::Unary(_, inner) | Expr::Spread(inner) => acts(inner),
        Expr::Index(a, b) | Expr::Binary(_, a, b) | Expr::Assign(_, a, b) => acts(a) || acts(b),
        Expr::Cond(a, b, c) => acts(a) || acts(b) || acts(c),
        _ => false,
    }
}

/// Whether a method of the helper's windows and elements acts.
fn acting(method: &str) -> bool {
    matches!(
        method,
        "click"
            | "setValue"
            | "clear"
            | "select"
            | "check"
            | "uncheck"
            | "fillSecret"
            | "paste"
            | "find"
            | "read"
            | "waitForNew"
            | "waitForText"
            | "write"
            | "menu"
            | "shortcut"
            | "chooseMenuItem"
            | "clickAt"
            | "clickAtOffset"
            | "clickIn"
            | "expand"
            | "collapse"
            | "screenshot"
            | "close"
    )
}

fn reason_for(word: &str) -> String {
    match word {
        "if" | "switch" => "a condition, decided when the script runs",
        "for" | "while" | "do" | "break" | "continue" => "a loop",
        "try" => "error handling",
        "throw" => "stops the script with an error",
        _ => "code the steps cannot express",
    }
    .to_string()
}

fn number(text: &str) -> Value {
    text.parse::<i64>()
        .map(Value::from)
        .or_else(|_| text.parse::<f64>().map(Value::from))
        .unwrap_or_else(|_| json!(text))
}

fn brace(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

fn unbrace(text: &str) -> String {
    text.replace("{{", "{").replace("}}", "}")
}

/// A value as template text (`{name}` for a param), for joining with `+`.
fn braced(sym: &Sym) -> Option<String> {
    match sym {
        Sym::Val(Val::Text(text) | Val::Num(text)) => Some(brace(text)),
        Sym::Val(Val::Param(name)) => Some(format!("{{{name}}}")),
        Sym::Val(Val::Template(text)) => Some(text.clone()),
        _ => None,
    }
}

fn template_val(text: String) -> Sym {
    if text.replace("{{", "").replace("}}", "").contains('{') {
        Sym::Val(Val::Template(text))
    } else {
        Sym::Val(Val::Text(unbrace(&text)))
    }
}

/// A text the script gives: `{"text"}`, `{"param"}`, `{"template"}` or `{"env"}`.
fn value(sym: Option<&Sym>) -> Read<Value> {
    match sym {
        Some(Sym::Val(val)) => match val {
            Val::Text(text) => Ok(json!({"text": text})),
            Val::Num(text) => Ok(json!({"text": text})),
            Val::Bool(b) => Ok(json!({"text": b.to_string()})),
            Val::Param(name) => Ok(json!({"param": name})),
            Val::Template(text) => Ok(json!({"template": text})),
            Val::Env(name) => Ok(json!({"env": name})),
            Val::Null => Err("passes no value".into()),
        },
        Some(Sym::Computed(why)) => Err(why.clone()),
        Some(Sym::ReadVar(name)) => Err(format!("uses the text read into {name} when the script runs")),
        Some(Sym::Unset(name)) => Err(format!("uses {name} before a step sets it")),
        None => Err("passes no value".into()),
        Some(_) => Err("passes something other than a text".into()),
    }
}

fn values(items: &[Sym]) -> Read<Value> {
    Ok(Value::Array(
        items.iter().map(|s| value(Some(s))).collect::<Read<Vec<_>>>()?,
    ))
}

/// An option or a number: numbers and yes/no as they are, texts as values.
fn raw(sym: &Sym) -> Read<Value> {
    match sym {
        Sym::Val(Val::Num(n)) => Ok(number(n)),
        Sym::Val(Val::Bool(b)) => Ok(json!(b)),
        Sym::Val(Val::Null) => Ok(Value::Null),
        Sym::Array(items) => Ok(Value::Array(items.iter().map(raw).collect::<Read<Vec<_>>>()?)),
        Sym::Object(pairs) => {
            let mut map = Map::new();
            for (key, sym) in pairs {
                map.insert(snake(key), raw(sym)?);
            }
            Ok(Value::Object(map))
        }
        other => value(Some(other)),
    }
}

fn coordinate(sym: Option<Sym>) -> Read<Value> {
    match sym {
        Some(Sym::Val(Val::Num(n))) => Ok(number(&n)),
        _ => Err("clicks at a point computed when the script runs".into()),
    }
}

/// The options object of a call (`{ title, profile }`), keys in snake_case.
fn options(sym: Option<&Sym>) -> Read<Vec<(String, Value)>> {
    match sym {
        None | Some(Sym::Val(Val::Null)) => Ok(Vec::new()),
        Some(Sym::Object(pairs)) => pairs
            .iter()
            .map(|(key, sym)| {
                raw(sym)
                    .map(|v| (snake(key), v))
                    .map_err(|_| format!("its {key} is known only when the script runs"))
            })
            .collect(),
        Some(_) => Err("passes options built when the script runs".into()),
    }
}

/// The query of an element (`{ nativeId: 'email' }`), keys in snake_case.
fn target(sym: Option<&Sym>) -> Read<Map<String, Value>> {
    match sym {
        Some(Sym::Object(pairs)) => {
            let mut map = Map::new();
            for (key, sym) in pairs {
                let value = raw(sym).map_err(|_| {
                    format!("finds an element whose {key} is known only when the script runs")
                })?;
                map.insert(snake(key), value);
            }
            Ok(map)
        }
        Some(Sym::Computed(_)) => Err("finds an element with a query built when the script runs".into()),
        _ => Err("finds an element with a query built when the script runs".into()),
    }
}

#[cfg(test)]
#[path = "script_steps_tests.rs"]
mod tests;
