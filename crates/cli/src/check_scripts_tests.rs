// Part of Agent Record Adapter (see NOTICE).
use std::path::{Path, PathBuf};

use super::extract::{Arg, Program, code};
use super::probe::{options, required, section_names};
use super::shell::{Shell, shell};

fn lit(text: &str) -> Arg {
    Arg::Lit(text.to_string())
}

const HOME: &str = "C:\\Users\\someone";

fn home() -> PathBuf {
    PathBuf::from(HOME)
}

#[test]
fn a_function_that_runs_the_engine_with_its_arguments_is_followed() {
    let text = r#"
import { execFileSync } from 'node:child_process';
const recorder = join(homedir(), '.desktop-recorder-mcp');
const bin = join(recorder, 'bin', 'agent-desktop.exe');
const call = (...args) => {
  try {
    return JSON.parse(execFileSync(bin, args, { encoding: 'utf8', windowsHide: true }));
  } catch (error) {
    return null;
  }
};
const snapshot = () => nodes(call('snapshot', '--window-id', appWindow, '--max-depth', '32'));
const clickPoint = ({ bounds: b }) => call('click-at', '--window-id', appWindow, '--screen', String(Math.round(b.x)), String(b.y));
"#;
    let source = code(
        text,
        Path::new("C:\\work\\replay.mjs"),
        false,
        Some(&home()),
    );
    assert_eq!(source.calls.len(), 2);
    assert_eq!(source.calls[0].program, Program::Engine);
    assert_eq!(
        source.calls[0].args,
        vec![
            lit("snapshot"),
            lit("--window-id"),
            Arg::Dyn,
            lit("--max-depth"),
            lit("32")
        ]
    );
    assert_eq!(source.calls[1].args[0], lit("click-at"));
    let (_, program, path) = &source.programs[0];
    assert_eq!(*program, Program::Engine);
    assert_eq!(
        path.as_deref(),
        Some(
            home()
                .join(".desktop-recorder-mcp")
                .join("bin")
                .join("agent-desktop.exe")
                .as_path()
        )
    );
}

#[test]
fn a_function_passing_on_a_later_parameter_is_followed() {
    let text = r#"
function native(window, ...args) {
  const recorder = join(homedir(), '.desktop-recorder-mcp');
  return execFileSync(join(recorder, 'bin', 'agent-record.exe'), args, { encoding: 'utf8' });
}
const capture = () => native(window, 'snapshot', '--window-id', window, '--max-depth', '24');
native(window, 'launch', `--arg=--user-data-dir=${dir}`);
"#;
    let source = code(text, Path::new("C:\\work\\p.mjs"), false, Some(&home()));
    assert_eq!(source.calls.len(), 2, "{:?}", source.calls);
    assert_eq!(source.calls[0].program, Program::Front);
    assert_eq!(source.calls[0].args[0], lit("snapshot"));
    assert_eq!(
        source.calls[1].args,
        vec![lit("launch"), Arg::Part("--arg=--user-data-dir=".into())]
    );
}

#[test]
fn run_imported_from_a_helper_copy_runs_what_that_copy_finds() {
    let text = "import { task, run } from './desktop-script.mjs';\nawait run('menu', '--window-id', id, 'File', 'Save');\n";
    let source = code(
        text,
        Path::new("C:\\work\\script.mjs"),
        false,
        Some(&home()),
    );
    let helper = PathBuf::from("C:\\work").join("./desktop-script.mjs");
    assert_eq!(source.helpers, vec![(1, helper.clone())]);
    assert_eq!(source.calls.len(), 1);
    assert_eq!(source.calls[0].program, Program::Helper(Some(helper)));
    assert_eq!(source.calls[0].line, 2);
}

#[test]
fn a_helper_loaded_at_run_time_is_found_from_its_fallback_path() {
    let text = r#"
export async function capture(page, windowId) {
  const helper = process.env.SGH_DESKTOP_SCRIPT
    ?? join(homedir(), '.codex', 'skills', 'recording-to-script', 'scripts', 'desktop-script.mjs');
  const { run } = await import(pathToFileURL(resolve(helper)).href);
  const record = await run('record', 'start', '--window-id', windowId, '--include-injected');
}
"#;
    let source = code(
        text,
        Path::new("C:\\work\\actions.mjs"),
        false,
        Some(&home()),
    );
    let helper = home()
        .join(".codex")
        .join("skills")
        .join("recording-to-script")
        .join("scripts")
        .join("desktop-script.mjs");
    assert_eq!(source.calls.len(), 1);
    assert_eq!(source.calls[0].program, Program::Helper(Some(helper)));
    assert_eq!(source.calls[0].args[1], lit("start"));
}

#[test]
fn python_run_with_star_args_is_followed() {
    let text = "import subprocess, os\nAR = os.path.expanduser('~/.desktop-recorder-mcp/bin/agent-record.exe')\n\ndef run(*args):\n    out = subprocess.run([AR, *args], capture_output=True)\n    return out\n\nrun('snapshot', '--app', 'notepad.exe')\nrun(f'{command}', key)\n";
    let source = code(text, Path::new("C:\\work\\task.py"), true, Some(&home()));
    assert_eq!(source.calls.len(), 1);
    assert_eq!(source.calls[0].program, Program::Front);
    assert_eq!(
        source.calls[0].args,
        vec![lit("snapshot"), lit("--app"), lit("notepad.exe")]
    );
}

#[test]
fn unrelated_calls_with_command_words_are_not_calls() {
    let text = "const label = (text) => text.trim();\nlabel('click');\nbutton.addEventListener('click', go);\nconst agent = 'agent-record';\nconsole.log('click', agent);\n";
    let source = code(text, Path::new("C:\\work\\page.js"), false, Some(&home()));
    assert!(source.calls.is_empty(), "{:?}", source.calls);
}

#[test]
fn a_function_with_fixed_arguments_is_not_a_runner() {
    let text = "const bin = 'agent-record.exe';\nconst windows = (app) => execFileSync(bin, ['list-windows', '--app', 'x']);\nwindows('click');\n";
    let source = code(text, Path::new("C:\\work\\page.js"), false, Some(&home()));
    assert_eq!(source.calls.len(), 1);
    assert_eq!(source.calls[0].args[0], lit("list-windows"));
}

#[test]
fn secrets_filled_by_name_are_listed() {
    let text = "import { task } from './desktop-script.mjs';\nawait (await win.find({ role: 'textfield' })).fillSecret('bank-login');\n";
    let source = code(text, Path::new("C:\\work\\s.mjs"), false, Some(&home()));
    assert_eq!(source.secrets, vec![(2, "bank-login".to_string())]);
}

#[test]
fn regex_and_comments_do_not_hide_calls() {
    let text = "const ar = 'agent-record';\n// run('nothing')\nconst re = /'(x)/g; /* call('no') */\nexecFileSync(ar, ['press', '--window-id', w, 'ctrl+s']);\n";
    let source = code(text, Path::new("C:\\work\\a.js"), false, Some(&home()));
    assert_eq!(source.calls.len(), 1);
    assert_eq!(
        source.calls[0].args,
        vec![lit("press"), lit("--window-id"), Arg::Dyn, lit("ctrl+s")]
    );
}

#[test]
fn shell_calls_through_a_variable_and_a_function() {
    let text = "AR=\"$HOME/.desktop-recorder-mcp/bin/agent-record.exe\"\nar() { \"$AR\" \"$@\"; }\nar snapshot --app notepad.exe # look\nout=$(\"$AR\" press --window-id \"$W\" ctrl+s 2>/dev/null)\necho \"agent-record snapshot\"\n";
    let source = shell(text, Path::new("C:\\work\\t.sh"), Shell::Sh, Some(&home()));
    assert_eq!(source.calls.len(), 2, "{:?}", source.calls);
    assert_eq!(
        source.calls[0].args,
        vec![lit("snapshot"), lit("--app"), lit("notepad.exe")]
    );
    assert_eq!(
        source.calls[1].args,
        vec![lit("press"), lit("--window-id"), Arg::Dyn, lit("ctrl+s")]
    );
    assert_eq!(source.programs.len(), 1);
}

#[test]
fn powershell_call_operator() {
    let text = "$ar = \"$env:USERPROFILE\\.desktop-recorder-mcp\\bin\\agent-record.exe\"\n& $ar click-at --window-id $w 10 20 | Out-Null\n";
    let source = shell(
        text,
        Path::new("C:\\work\\t.ps1"),
        Shell::Ps1,
        Some(&home()),
    );
    assert_eq!(source.calls.len(), 1);
    assert_eq!(source.calls[0].program, Program::Front);
    assert_eq!(
        source.calls[0].args,
        vec![
            lit("click-at"),
            lit("--window-id"),
            Arg::Dyn,
            lit("10"),
            lit("20")
        ]
    );
}

const CLICK_HELP: &str = "Click element via accessibility press action\n\nUsage: agent-desktop.exe click [OPTIONS] <REF>\n\nArguments:\n  <REF>  Qualified ref\n\nOptions:\n      --timeout-ms <MS>  Deadline\n  -c, --compact          Compact\n      --session <SESSION>  Session\n";

#[test]
fn usage_gives_the_required_arguments() {
    let given = |text: &str| text.split(' ').map(str::to_string).collect::<Vec<_>>();
    assert!(
        required(CLICK_HELP, &given("click @e1 --timeout-ms 10 --help"))
            .errors
            .is_empty()
    );
    let missing = required(CLICK_HELP, &given("click --timeout-ms 10 --help"));
    assert_eq!(missing.errors, vec!["missing <REF>".to_string()]);
    let options = options(CLICK_HELP);
    assert!(options.contains(&("--timeout-ms".to_string(), true)));
    assert!(options.contains(&("-c".to_string(), false)));
    let help = "x\n\nCommands:\n  snapshot  a\n  click     b\n\nOptions:\n";
    assert_eq!(section_names(help, "Commands:"), vec!["snapshot", "click"]);
}

#[test]
fn a_value_that_may_come_from_stdin_is_needed_without_stdin() {
    let help = "Usage: agent-desktop.exe set-value [OPTIONS] <REF> [VALUE]\n\nOptions:\n      --stdin  Read from a pipe\n";
    let given = |text: &str| text.split(' ').map(str::to_string).collect::<Vec<_>>();
    assert_eq!(
        required(help, &given("set-value @e1 --help")).errors,
        vec!["missing [VALUE]".to_string()]
    );
    assert!(
        required(help, &given("set-value @e1 --stdin --help"))
            .errors
            .is_empty()
    );
    assert!(
        required(help, &given("set-value @e1 hello --help"))
            .errors
            .is_empty()
    );
}

#[test]
fn a_required_option_in_the_usage_is_checked() {
    let help = "Usage: agent-record record start [OPTIONS] --out <OUT>\n\nOptions:\n      --out <OUT>  File\n";
    let given = |text: &str| text.split(' ').map(str::to_string).collect::<Vec<_>>();
    assert_eq!(
        required(help, &given("record start --help")).errors,
        vec!["missing --out".to_string()]
    );
    assert!(
        required(help, &given("record start --out f.jsonl --help"))
            .errors
            .is_empty()
    );
}
