// Part of Agent Record Adapter (see NOTICE).
//! Runs our own commands in process, with our background adapter, and prints
//! the same JSON envelope agent-desktop prints.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use agent_desktop_core::commands::{
    click_at, menu, navigate, record, record_background, secret, stroke, type_window,
};
use agent_desktop_core::context::CommandContext;
use agent_desktop_core::response::output::{ErrorPayload, Response};
use agent_desktop_core::session::resolve_active_session;
use agent_desktop_core::{Adapter, AppError};
use serde_json::Value;

use crate::args::{Commands, RecordAction, SecretAction, SecretSetArgs};

const OURS: &[&str] = &[
    "record",
    "secret",
    "stroke",
    "menu",
    "click-at",
    "navigate",
    "browser",
    "list-displays",
];

pub(crate) fn is_ours(command: &str) -> bool {
    OURS.contains(&command)
}

/// `type w-<id> TEXT`: types into a window, which has no ref.
pub(crate) fn is_window_type(args: &[String]) -> bool {
    args.iter()
        .skip_while(|arg| arg.as_str() != "type")
        .nth(1)
        .is_some_and(|target| type_window::is_window_target(target))
}

pub(crate) fn type_window(args: &[String]) -> ExitCode {
    let mut rest = args.iter().skip_while(|arg| arg.as_str() != "type").skip(1);
    let (Some(target), Some(text)) = (rest.next(), rest.next()) else {
        return emit(
            "type",
            Err(AppError::invalid_input(
                "type w-<id> needs the text to type",
            )),
        );
    };
    let result = with_adapter(|adapter| type_window::execute(target, text, adapter));
    emit("type", result)
}

pub(crate) fn ours(
    session: &Option<String>,
    agent_id: &Option<String>,
    headed: bool,
    command: Commands,
) -> ExitCode {
    let name = command_name(&command);
    let result = context(session, agent_id, headed)
        .and_then(|context| with_adapter(|adapter| dispatch(command, adapter, &context)));
    emit(name, result)
}

fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Record(_) => "record",
        Commands::Secret(_) => "secret",
        Commands::Stroke(_) => "stroke",
        Commands::Menu(_) => "menu",
        Commands::ClickAt(_) => "click-at",
        Commands::Navigate(_) => "navigate",
        Commands::Browser(_) => "browser",
        Commands::ListDisplays => "list-displays",
        Commands::External(_) => "external",
    }
}

fn context(
    session: &Option<String>,
    agent_id: &Option<String>,
    headed: bool,
) -> Result<CommandContext, AppError> {
    let session_id = resolve_active_session(
        session.as_deref(),
        std::env::var("AGENT_DESKTOP_SESSION").ok().as_deref(),
    )?;
    let agent_id = agent_id
        .clone()
        .or_else(|| std::env::var("AGENT_DESKTOP_AGENT_ID").ok());
    CommandContext::new(session_id, None, false)?
        .with_headed(headed)
        .with_agent_id(agent_id)
}

fn dispatch(
    command: Commands,
    adapter: &dyn Adapter,
    context: &CommandContext,
) -> Result<Value, AppError> {
    match command {
        Commands::Record(args) => match args.action {
            Some(RecordAction::Start(start)) => record_background::start(
                record_background::StartArgs {
                    out: start.out,
                    app: start.scope.app,
                    window_id: start.scope.window_id,
                    display: start.scope.display,
                    max_duration_ms: start.scope.max_duration_ms,
                    include_injected: start.scope.include_injected,
                    video: start.video.video,
                    video_out: start.video.video_out,
                    indicator: !start.no_indicator,
                },
                adapter,
            ),
            Some(RecordAction::Stop(stop)) => {
                record_background::stop(stop.id.as_deref(), stop.timeout_ms)
            }
            Some(RecordAction::Status) => record_background::status(adapter),
            None => {
                let out = args.out.ok_or_else(|| {
                    AppError::invalid_input_with_suggestion(
                        "record needs --out",
                        "Pass --out FILE, or use 'record start' to record in the background",
                    )
                })?;
                let video = match (args.video.video_out, args.video.video) {
                    (Some(path), _) => Some(path),
                    (None, true) => Some(record::default_video_path(&out)),
                    (None, false) => None,
                };
                record::execute(
                    record::RecordArgs {
                        out,
                        app: args.scope.app,
                        window_id: args.scope.window_id,
                        display: args.scope.display,
                        max_duration_ms: args.scope.max_duration_ms,
                        stop_file: args.stop_file,
                        include_injected: args.scope.include_injected,
                        video,
                        indicator: !args.no_indicator,
                    },
                    adapter,
                )
            }
        },
        Commands::Secret(args) => match args.action {
            SecretAction::Set(set) => {
                secret::valid_name(&set.name)?;
                let value = read_secret(&set)?;
                secret::set(secret::SetArgs {
                    name: set.name,
                    value,
                    app: set.app,
                    site: set.site,
                    any_field: set.any_field,
                })
            }
            SecretAction::List => secret::list(),
            SecretAction::Delete(name) => secret::delete(&name.name),
            SecretAction::Fill(fill) => secret::fill(
                secret::FillArgs {
                    ref_id: fill.ref_id,
                    name: fill.name,
                    timeout_ms: fill.timeout_ms,
                },
                adapter,
                context,
            ),
        },
        Commands::Stroke(args) => {
            let points = crate::stroke_args::parse_points(&args)?;
            stroke::execute(stroke::StrokeArgs { points }, adapter, context)
        }
        Commands::Menu(args) => menu::execute(
            menu::MenuArgs {
                app: args.scope.app,
                window_id: args.scope.window_id,
                path: args.path,
                shortcut: args.shortcut,
                find: args.find,
            },
            adapter,
        ),
        Commands::ClickAt(args) => click_at::execute(
            click_at::ClickAtArgs {
                app: args.scope.app,
                window_id: args.scope.window_id,
                x: args.x,
                y: args.y,
                double: args.double,
                screen: args.screen,
            },
            adapter,
        ),
        Commands::Navigate(args) => navigate::execute(
            navigate::NavigateArgs {
                app: args.scope.app,
                window_id: args.scope.window_id,
                url: args.url,
                in_app: args.in_app,
            },
            adapter,
        ),
        Commands::ListDisplays => {
            let displays = adapter.list_displays(agent_desktop_core::Deadline::standard()?)?;
            Ok(agent_desktop_core::commands::record_display::numbered(
                &displays,
            ))
        }
        Commands::Browser(_) | Commands::External(_) => Err(AppError::Internal(
            "not a command agent-record runs itself".into(),
        )),
    }
}

/// The value, typed by a person at a hidden prompt (twice), or the first line
/// of standard input with --stdin. Never an argument: arguments show up in
/// process lists and shell histories.
fn read_secret(set: &SecretSetArgs) -> Result<String, AppError> {
    if set.stdin {
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(AppError::Io)?;
        let line = line.trim_start_matches('\u{feff}');
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    if !std::io::stdin().is_terminal() {
        return Err(AppError::invalid_input_with_suggestion(
            "secret set asks a person for the value at a terminal",
            "Run it in a terminal, or pass --stdin to read the value from standard input",
        ));
    }
    let first = rpassword::prompt_password(format!("Value for '{}' (hidden): ", set.name))
        .map_err(AppError::Io)?;
    let again = rpassword::prompt_password("Type it again: ").map_err(AppError::Io)?;
    if first != again {
        return Err(AppError::invalid_input(
            "The two values differ; nothing was saved",
        ));
    }
    Ok(first)
}

#[cfg(target_os = "windows")]
fn with_adapter<T>(work: impl FnOnce(&dyn Adapter) -> Result<T, AppError>) -> Result<T, AppError> {
    let _ = agent_desktop_core::install_private_file_ops(Box::new(
        agent_record_windows::WindowsPrivateFile,
    ));
    agent_record_windows::ensure_owned_process_mta_and_dpi().map_err(AppError::from)?;
    let adapter = agent_record_windows::WindowsAdapter::new();
    work(&adapter)
}

#[cfg(not(target_os = "windows"))]
fn with_adapter<T>(_work: impl FnOnce(&dyn Adapter) -> Result<T, AppError>) -> Result<T, AppError> {
    Err(AppError::Internal(
        "agent-record's own commands run on Windows only".into(),
    ))
}

/// Prints the envelope agent-desktop prints and returns its exit code.
pub(crate) fn emit(command: &str, result: Result<Value, AppError>) -> ExitCode {
    let response = match result {
        Ok(data) => Response::ok(command, data),
        Err(error) => Response::err(command, ErrorPayload::from_app_error(&error)),
    };
    let ok = response.ok;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if serde_json::to_writer(&mut out, &response).is_err() || out.write_all(b"\n").is_err() {
        return ExitCode::FAILURE;
    }
    let _ = out.flush();
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A ref action of agent-desktop's: its command, ref and value.
pub(crate) struct RefAction {
    pub command: String,
    pub ref_id: String,
    pub value: Option<String>,
}

/// Recognizes `click @ref`, `set-value @ref VALUE`, `select @ref VALUE`...
pub(crate) fn ref_action(args: &[String]) -> Option<RefAction> {
    const COMMANDS: &[&str] = &[
        "click",
        "double-click",
        "right-click",
        "triple-click",
        "set-value",
        "clear",
        "focus",
        "select",
        "check",
        "uncheck",
        "toggle",
        "expand",
        "collapse",
        "scroll-to",
        "type",
        "hover",
    ];
    let command = crate::first_command(args)?;
    if !COMMANDS.contains(&command) {
        return None;
    }
    let mut rest = args
        .iter()
        .skip_while(|arg| arg.as_str() != command)
        .skip(1);
    let ref_id = rest.by_ref().find(|arg| arg.starts_with('@'))?.clone();
    let value = rest.find(|arg| !arg.starts_with("--")).cloned();
    Some(RefAction {
        command: command.to_string(),
        ref_id,
        value,
    })
}

fn action_of(action: &RefAction) -> Option<agent_desktop_core::Action> {
    use agent_desktop_core::Action;
    let value = || action.value.clone();
    Some(match action.command.as_str() {
        "click" => Action::Click,
        "double-click" => Action::DoubleClick,
        "right-click" => Action::RightClick,
        "triple-click" => Action::TripleClick,
        "set-value" => Action::SetValue(value()?),
        "clear" => Action::Clear,
        "focus" => Action::SetFocus,
        "select" => Action::Select(value()?),
        "check" => Action::Check,
        "uncheck" => Action::Uncheck,
        "toggle" => Action::Toggle,
        "expand" => Action::Expand,
        "collapse" => Action::Collapse,
        "scroll-to" => Action::ScrollTo,
        "type" => Action::TypeText(value()?),
        "hover" => Action::Hover,
        _ => return None,
    })
}

/// Runs a refused twin's action on the element found by its place.
#[cfg(target_os = "windows")]
pub(crate) fn by_place(args: &[String]) -> Option<ExitCode> {
    let parsed = ref_action(args)?;
    let action = action_of(&parsed)?;
    let session = flag_value(args, "--session");
    let result = context(&session, &None, false).and_then(|context| {
        let entry =
            agent_desktop_core::commands::helpers::load_ref_entry(&parsed.ref_id, &context)?;
        with_adapter(|_| Ok(()))?;
        let adapter = agent_record_windows::WindowsAdapter::new();
        let request = agent_desktop_core::actions::action_request::ActionRequest::headless(action);
        let result = agent_desktop_core::RecordOps::act_by_place(&adapter, &entry, request)?;
        let mut data =
            serde_json::to_value(&result).map_err(|error| AppError::Internal(error.to_string()))?;
        data["resolved_by"] = Value::from("place");
        Ok(data)
    });
    Some(emit(&parsed.command, result))
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn by_place(_args: &[String]) -> Option<ExitCode> {
    None
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let prefix = format!("{flag}=");
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == flag {
            args.get(index + 1).cloned()
        } else {
            arg.strip_prefix(&prefix).map(str::to_string)
        }
    })
}

/// `list-displays` with each display's number (1, 2, ...), which `record
/// --display N` and the Desktop Recorder app use.
pub(crate) fn list_displays() -> ExitCode {
    let result = with_adapter(|adapter| {
        let displays = adapter.list_displays(agent_desktop_core::Deadline::standard()?)?;
        Ok(agent_desktop_core::commands::record_display::numbered(
            &displays,
        ))
    });
    emit("list-displays", result)
}

/// `find ... --first` in background mode: when the first match has no ref
/// (nothing to act on), the first match of the same name that has one; and
/// for an editable combo box, its text box right after it (a file dialog's
/// "File name": the group around it comes first and has no ref, and the combo
/// box refuses the value). A match with another name is never taken instead.
/// `find`, with two background-mode additions: `--first` prefers a match an
/// action can use (`find_first`), and in one window, `--name` also finds an
/// unnamed field by the text before it (`labels.rs`).
pub(crate) fn find(args: &[String]) -> ExitCode {
    let first = args.iter().any(|arg| arg == "--first");
    let envelope = if first {
        find_first(args)
    } else {
        crate::engine::envelope(args)
    };
    let mut envelope = match envelope {
        Ok(envelope) => envelope,
        Err(message) => return emit("find", Err(AppError::Internal(message))),
    };
    let found_nothing = envelope["ok"] == true
        && (envelope["data"]["match"].is_null()
            && envelope["data"]["matches"]
                .as_array()
                .is_none_or(Vec::is_empty));
    if found_nothing
        && agent_desktop_core::background::enabled()
        && let (Some(name), Some(window)) =
            (flag_value(args, "--name"), flag_value(args, "--window-id"))
    {
        let mut snapshot = vec!["snapshot".to_string(), "--window-id".to_string(), window];
        if let Some(session) = flag_value(args, "--session") {
            snapshot.extend(["--session".to_string(), session]);
        }
        let exact = args.iter().any(|arg| arg == "--exact");
        let role = flag_value(args, "--role");
        if let Ok(tree) = crate::engine::envelope(&snapshot)
            && tree["ok"] == true
            && let Some(data) =
                crate::labels::find_answer(&tree, role.as_deref(), &name, exact, first)
        {
            envelope["data"] = data;
        }
    }
    print_envelope(&envelope)
}

fn find_first(args: &[String]) -> Result<Value, String> {
    let first = crate::engine::envelope(args)?;
    let lacks_ref = first["ok"] == true
        && !first["data"]["match"].is_null()
        && first["data"]["match"]["ref_id"].is_null();
    if !lacks_ref || !agent_desktop_core::background::enabled() {
        return Ok(first);
    }
    let mut wider: Vec<String> = args
        .iter()
        .filter(|arg| arg.as_str() != "--first")
        .cloned()
        .collect();
    wider.extend(["--limit".to_string(), "20".to_string()]);
    let Ok(mut all) = crate::engine::envelope(&wider) else {
        return Ok(first);
    };
    let Some(matches) = all["data"]["matches"].as_array().cloned() else {
        return Ok(first);
    };
    let Some(chosen) = first_actionable(&matches) else {
        return Ok(first);
    };
    if let Some(data) = all["data"].as_object_mut() {
        data.remove("matches");
        data.insert("match".into(), chosen);
    }
    Ok(all)
}

/// The choice `find_first` makes, on the matches' JSON.
pub(crate) fn first_actionable(matches: &[Value]) -> Option<Value> {
    let first = matches.first()?;
    let name = &first["name"];
    let mut chosen = matches
        .iter()
        .position(|found| !found["ref_id"].is_null() && &found["name"] == name)?;
    if matches[chosen]["role"] == "combobox"
        && let Some(next) = matches.get(chosen + 1)
        && next["role"] == "textfield"
        && !next["ref_id"].is_null()
        && next["name"] == matches[chosen]["name"]
    {
        chosen += 1;
    }
    Some(matches[chosen].clone())
}

/// `wait --text TEXT --window-id W`: reads only that window (upstream reads
/// the app's default window, the wrong one when the app has several instances
/// running, such as a browser with a profile of its own beside the person's).
pub(crate) fn wait_text_in_window(args: &[String]) -> ExitCode {
    let value = |flag: &str| flag_value(args, flag);
    let (Some(text), Some(window)) = (value("--text"), value("--window-id")) else {
        return emit(
            "wait",
            Err(AppError::invalid_input("wait needs --text and --window-id")),
        );
    };
    let timeout = value("--timeout")
        .and_then(|ms| ms.parse::<u64>().ok())
        .unwrap_or(30_000);
    let mut snapshot = vec![
        "snapshot".to_string(),
        "--window-id".to_string(),
        window.clone(),
    ];
    if let Some(session) = value("--session") {
        snapshot.extend(["--session".to_string(), session]);
    }
    let start = std::time::Instant::now();
    loop {
        if let Ok(envelope) = crate::engine::envelope(&snapshot)
            && envelope["ok"] == true
            && let Some(found) = find_text(&envelope["data"]["tree"], &text)
        {
            return emit(
                "wait",
                Ok(serde_json::json!({
                    "found": true,
                    "text": text,
                    "ref": found["ref_id"],
                    "role": found["role"],
                    "snapshot_id": envelope["data"]["snapshot_id"],
                    "window_id": window,
                    "elapsed_ms": start.elapsed().as_millis() as u64,
                })),
            );
        }
        if start.elapsed().as_millis() as u64 >= timeout {
            let error = agent_desktop_core::AdapterError::new(
                agent_desktop_core::ErrorCode::Timeout,
                format!("Text '{text}' did not appear in window {window} within {timeout} ms"),
            )
            .with_disposition(agent_desktop_core::DeliverySemantics::not_delivered());
            return emit("wait", Err(AppError::Adapter(error)));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
}

/// The first node whose name, value or description contains `text`.
pub(crate) fn find_text<'a>(node: &'a Value, text: &str) -> Option<&'a Value> {
    let has = |field: &str| {
        node[field]
            .as_str()
            .is_some_and(|value| value.contains(text))
    };
    if has("name") || has("value") || has("description") {
        return Some(node);
    }
    node["children"]
        .as_array()?
        .iter()
        .find_map(|child| find_text(child, text))
}

fn print_envelope(envelope: &Value) -> ExitCode {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = serde_json::to_writer(&mut out, envelope);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
    if envelope["ok"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
