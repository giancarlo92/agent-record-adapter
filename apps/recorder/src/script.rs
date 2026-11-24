// Part of Agent Record Adapter (see NOTICE).
//! Writes the JavaScript script that repeats the steps, with desktop-script.mjs next to it.
//!
//! The values the person marked as changing every time become parameters whose default is the
//! recorded value, so the script repeats the task as it was when run with no values at all.

use crate::steps::{Action, Locator, Step, WindowRef};

/// The helper file the script imports, as the recording-to-script skill ships it.
pub const LIBRARY: &str =
    include_str!("../../../skills/recording-to-script/scripts/desktop-script.mjs");
pub const LIBRARY_NAME: &str = "desktop-script.mjs";

/// A value of the recording that can become a parameter: the index of its step and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub step: usize,
    pub name: String,
    pub question: String,
    pub value: String,
}

/// The values a script could take as parameters: what was typed or chosen (never a password).
pub fn candidates(steps: &[Step]) -> Vec<Param> {
    let mut used: Vec<String> = Vec::new();
    steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match &step.action {
            Action::SetValue { field, value, .. } | Action::Select { field, value, .. } => {
                let name = unique(snake(field), &mut used);
                Some(Param {
                    step: index,
                    name,
                    question: field.clone(),
                    value: value.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

/// What a step does, in plain words, for the list a person reads.
pub fn describe(step: &Step) -> String {
    match &step.action {
        Action::OpenBrowser { url } => format!("Open the page {url}"),
        Action::OpenApp { path } => {
            format!("Open {}", path.rsplit(['\\', '/']).next().unwrap_or(path))
        }
        Action::SetValue { field, value, .. } => {
            format!("Write \"{}\" in \"{field}\"", short(value))
        }
        Action::Select { field, value, .. } => format!("Choose \"{value}\" in \"{field}\""),
        Action::Check {
            field, on: true, ..
        } => format!("Tick \"{field}\""),
        Action::Check {
            field, on: false, ..
        } => format!("Untick \"{field}\""),
        Action::Click {
            field,
            irreversible: true,
            ..
        } => format!("Click \"{field}\" (asks before it runs)"),
        Action::Click { field, .. } => format!("Click \"{field}\""),
        Action::MenuItem { name } => format!("Choose \"{name}\" in the menu"),
        Action::ClickAt { dx, dy, .. } => {
            format!("Click the window at {dx}, {dy} (something with no name)")
        }
        Action::Key { keys } => format!("Press {keys} (through its menu item)"),
        Action::Paste { field, .. } => format!("Paste the clipboard into \"{field}\""),
        Action::Secret { field, name, .. } => {
            format!("Fill \"{field}\" with the saved password \"{name}\"")
        }
        Action::Skipped { what } => {
            format!("Not repeated: {what} (it needs the keyboard or the mouse)")
        }
    }
}

/// The whole script.
pub fn render(title: &str, recording: &str, steps: &[Step], params: &[Param]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// {title}\n// Made by Desktop Recorder from {recording}, without AI. Run it with node; --help lists its values.\n\
         import {{ task }} from './{LIBRARY_NAME}';\n\nawait task({{\n  title: {},\n  params: {{\n",
        js(title)
    ));
    for param in params {
        out.push_str(&format!(
            "    {}: {{ ask: {}, default: {} }},\n",
            param.name,
            js(&param.question),
            js(&param.value)
        ));
    }
    out.push_str("  },\n}, async ({ params, step, openBrowser, openApp, findWindow, confirm }) => {\n  const windows = {};\n");
    out.push_str(
        "  // The window a step acts in: the one opened above, else the window of its app whose title\n  // has the most of the parts the recording saw (the part naming a document changes).\n",
    );
    out.push_str("  const win = async (id, parts, app) => (windows[id] ??= await findWindow(parts, { app }));\n");
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        let what = js(&describe(step));
        let body = body(step, params.iter().find(|p| p.step == index));
        out.push_str(&format!(
            "  await step({number}, {what}, async () => {{\n{body}  }});\n"
        ));
    }
    out.push_str("});\n");
    out
}

fn body(step: &Step, param: Option<&Param>) -> String {
    let w = window_expr(&step.window);
    let value = |recorded: &str| match param {
        Some(p) => format!("params.{}", p.name),
        None => js(recorded),
    };
    match &step.action {
        Action::OpenBrowser { url } => format!(
            "    windows[{}] = await openBrowser({}, {{ title: {} }});\n",
            js(&step.window.id),
            js(url),
            js(&page_title(&step.window))
        ),
        Action::OpenApp { path } => format!(
            "    windows[{}] = await openApp({}, {{ title: {}, image: {} }});\n",
            js(&step.window.id),
            js(path),
            parts(&step.window),
            js(&step.window.app)
        ),
        // A document may be a page the app draws itself, whose text can only be read: `write`
        // types what it lacks there.
        Action::SetValue { at, value: v, .. } if at.role.as_deref() == Some("document") => {
            format!("    await {w}.write({}, {});\n", query(at), value(v))
        }
        Action::SetValue { at, value: v, .. } => format!(
            "    await (await {w}.find({})).setValue({});\n",
            query(at),
            value(v)
        ),
        Action::Select { at, value: v, .. } => format!(
            "    await (await {w}.find({})).select({});\n",
            query(at),
            value(v)
        ),
        Action::Check { at, on, .. } => {
            format!(
                "    await (await {w}.find({})).{}();\n",
                query(at),
                if *on { "check" } else { "uncheck" }
            )
        }
        Action::Click {
            at,
            field,
            irreversible,
        } => {
            let ask = if *irreversible {
                format!(
                    "    await confirm({});\n",
                    js(&format!("Click \"{field}\"?"))
                )
            } else {
                String::new()
            };
            format!("{ask}    await (await {w}.find({})).click();\n", query(at))
        }
        Action::MenuItem { name } => format!("    await {w}.chooseMenuItem({});\n", js(name)),
        Action::ClickAt { dx, dy, double } => format!(
            "    await {w}.clickAtOffset({dx}, {dy}{});\n",
            if *double { ", { double: true }" } else { "" }
        ),
        Action::Key { keys } => format!("    await {w}.shortcut({});\n", js(keys)),
        Action::Paste { at, .. } => {
            format!("    await (await {w}.find({})).paste();\n", query(at))
        }
        Action::Secret { at, name, .. } => format!(
            "    await (await {w}.find({})).fillSecret({});\n",
            query(at),
            js(name)
        ),
        Action::Skipped { what } => format!(
            "    console.log({});\n",
            js(&format!("   not repeated: {what}"))
        ),
    }
}

/// The window a step acts in: the one the script opened, else found by its title parts and app.
fn window_expr(window: &WindowRef) -> String {
    format!(
        "(await win({}, {}, {}))",
        js(&window.id),
        parts(window),
        js(&window.app)
    )
}

/// The parts of a window's title, as a JavaScript list: "*Report - Notepad" gives
/// ["Report", "Notepad"] (a page's title gives itself, without the browser's name).
fn parts(window: &WindowRef) -> String {
    let list: Vec<String> = if window.url.is_some() {
        vec![page_title(window)]
    } else {
        window
            .title
            .split(" - ")
            .map(|part| part.trim().trim_start_matches('*').trim().to_owned())
            .filter(|part| !part.is_empty())
            .collect()
    };
    format!(
        "[{}]",
        list.iter().map(|p| js(p)).collect::<Vec<_>>().join(", ")
    )
}

fn query(at: &Locator) -> String {
    if let Some(id) = &at.native_id {
        return format!("{{ nativeId: {} }}", js(id));
    }
    let mut parts = Vec::new();
    if let Some(role) = &at.role {
        parts.push(format!("role: {}", js(role)));
    }
    if let Some(name) = &at.name {
        parts.push(format!("name: {}, exact: true", js(name)));
    }
    if at.in_document {
        parts.push("inDocument: true".to_owned());
    }
    format!("{{ {} }}", parts.join(", "))
}

/// A web page's title without the browser's name.
fn page_title(window: &WindowRef) -> String {
    let title = &window.title;
    title
        .rsplit_once(" - ")
        .map_or(title.as_str(), |(page, _)| page)
        .to_owned()
}

fn short(value: &str) -> String {
    let flat = value.replace(['\r', '\n'], " ");
    if flat.chars().count() > 40 {
        format!("{}...", flat.chars().take(40).collect::<String>())
    } else {
        flat
    }
}

/// A JavaScript string literal.
pub fn js(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// `snake_case` ASCII from a field name ("Work email" gives `work_email`).
pub fn snake(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' | 'Á' => 'a',
            'é' | 'è' | 'ë' | 'ê' | 'É' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'Í' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'Ó' => 'o',
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' => 'u',
            'ñ' | 'Ñ' => 'n',
            other => other,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_owned();
    match out.chars().next() {
        None => "value".to_owned(),
        Some(first) if first.is_ascii_digit() => format!("v_{out}"),
        Some(_) => out,
    }
}

fn unique(name: String, used: &mut Vec<String>) -> String {
    let mut candidate = name.clone();
    let mut n = 2;
    while used.contains(&candidate) {
        candidate = format!("{name}_{n}");
        n += 1;
    }
    used.push(candidate.clone());
    candidate
}
