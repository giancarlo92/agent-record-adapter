// Part of Agent Record Adapter (see NOTICE).
use std::path::Path;

use serde_json::{Value, json};

use super::read;

fn steps(name: &str, text: &str) -> Value {
    read(Path::new(name), text).expect("read")
}

const SIGN_UP: &str = r#"
import { task } from './desktop-script.mjs';

await task({
  title: 'Create an account',
  params: {
    url: { ask: 'The sign-up page', example: 'file:///C:/forms/signup.html' },
    plan: { ask: 'The plan', options: ['Basic', 'Pro'], default: 'Pro' },
    username: { ask: 'The username', example: 'lucia_f' },
    secret: { ask: 'The saved password', default: 'web-login' },
    phone: { ask: 'The phone', example: '+34 600' },
  },
}, async ({ params, step, openBrowser, confirm }) => {
  let page;
  await step(1, 'Open the sign-up page', async () => {
    page = await openBrowser(params.url, { title: 'Create your account', profile: process.env.SCRIPT_BROWSER_PROFILE });
  });
  await step(2, `Choose the plan ${params.plan}`, async () => {
    await (await page.find({ role: 'radiobutton', name: params.plan, exact: true })).click();
  });
  await step(3, 'Fill the password', async () => {
    const field = await page.find({ nativeId: 'password' });
    const { askedNow } = await field.fillSecret(params.secret);
    console.log(askedNow ? 'asked' : 'filled');
  });
  await step(4, 'Type the phone', async () => {
    const [code, ...digits] = params.phone.trim().split(/\s+/);
    await (await page.find({ nativeId: 'phone-code' })).select(code);
    await new Promise((done) => setTimeout(done, 1500));
  });
  await step(4.5, 'Create the account', async () => {
    await confirm(`Create the account ${params.username}?`);
    await (await page.find({ nativeId: 'create' })).click();
    await page.waitForText('Account created');
  });
  return `account ${params.username} created`;
});
"#;

#[test]
fn a_helper_script_gives_its_values_windows_and_steps() {
    let data = steps("sign-up.mjs", SIGN_UP);
    assert_eq!(data["title"], "Create an account");
    assert_eq!(data["language"], "javascript");
    assert_eq!(data["helper"], true);
    let params = data["params"].as_array().unwrap();
    assert_eq!(params.len(), 5);
    assert_eq!(params[1]["options"], json!(["Basic", "Pro"]));
    assert_eq!(params[1]["default"], "Pro");
    assert_eq!(params[0]["default"], Value::Null);
    assert_eq!(params[3]["secret"], true);
    assert_eq!(params[2]["secret"], false);
    assert_eq!(
        data["windows"],
        json!([{"var": "page", "kind": "browser", "url": {"param": "url"},
                "title": {"text": "Create your account"}, "profile": {"env": "SCRIPT_BROWSER_PROFILE"}}])
    );
    let steps = data["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 5);
    assert_eq!(steps[0]["actions"][0]["kind"], "open_browser");
    assert_eq!(steps[0]["actions"][0]["window"], "page");
    assert_eq!(steps[1]["title"], "Choose the plan {plan}");
    assert_eq!(
        steps[1]["actions"][0]["target"],
        json!({"role": {"text": "radiobutton"}, "name": {"param": "plan"}, "exact": true})
    );
    assert_eq!(steps[2]["actions"][0]["kind"], "fill_secret");
    assert_eq!(steps[2]["actions"][0]["secret"], json!({"param": "secret"}));
    assert_eq!(steps[2]["actions"].as_array().unwrap().len(), 1);
    // The phone code is computed at run time: never guessed, listed instead.
    assert_eq!(steps[3]["actions"], json!([{"kind": "wait", "ms": 1500, "line": steps[3]["actions"][0]["line"]}]));
    let unsupported = data["unsupported"].as_array().unwrap();
    assert_eq!(unsupported.len(), 1, "{unsupported:?}");
    assert_eq!(unsupported[0]["step"], 4);
    assert!(unsupported[0]["code"].as_str().unwrap().contains("select(code)"));
    assert_eq!(steps[4]["number"], 4.5);
    let last: Vec<&str> = steps[4]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    assert_eq!(last, ["confirm", "click", "wait_text"]);
    assert_eq!(
        steps[4]["actions"][0]["message"],
        json!({"template": "Create the account {username}?"})
    );
}

const GENERATED: &str = r#"
import { task } from './desktop-script.mjs';

await task({
  title: "agent-record",
  params: {
    apellido: { ask: "Apellido", default: "Quispe" },
  },
}, async ({ params, step, openBrowser, openApp, findWindow, confirm }) => {
  const windows = {};
  const win = async (id, parts, app) => (windows[id] ??= await findWindow(parts, { app }));
  await step(1, "Open the page", async () => {
    windows["w-1"] = await openBrowser("file:///C:/x.html", { title: "Test" });
  });
  await step(2, "Write in Apellido", async () => {
    await (await (await win("w-1", ["Test"], "chrome.exe")).find({ role: "textfield", name: "Apellido", exact: true })).setValue(params.apellido);
  });
  await step(3, "Run the menu", async () => {
    await (await win("w-2", ["Untitled", "Notepad"], "notepad.exe")).menu("File", "Save");
    await (await win("w-2", ["Untitled", "Notepad"], "notepad.exe")).shortcut("ctrl+s");
  });
});
"#;

#[test]
fn a_generated_script_names_its_windows_by_their_recorded_id() {
    let data = steps("script.mjs", GENERATED);
    assert_eq!(data["unsupported"], json!([]));
    assert_eq!(data["windows"][0]["var"], "w-1");
    assert_eq!(data["windows"][0]["kind"], "browser");
    assert_eq!(
        data["windows"][1],
        json!({"var": "w-2", "kind": "find", "title_parts": [{"text": "Untitled"}, {"text": "Notepad"}], "app": {"text": "notepad.exe"}})
    );
    let steps = data["steps"].as_array().unwrap();
    assert_eq!(steps[1]["actions"][0]["window"], "w-1");
    assert_eq!(steps[1]["actions"][0]["value"], json!({"param": "apellido"}));
    let third: Vec<&str> = steps[2]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    // The window is found once, where it is first used.
    assert_eq!(third, ["find_window", "menu", "shortcut"]);
}

#[test]
fn reads_and_waits_for_new_text_keep_their_names() {
    let data = steps(
        "rows.mjs",
        r#"
import { task } from './desktop-script.mjs';
await task({ title: 'Rows', params: {} }, async ({ params, step, findWindow }) => {
  let app;
  await step(1, 'Find it', async () => { app = await findWindow('DBeaver', { app: 'dbeaver.exe' }); });
  await step(2, 'Run', async () => {
    const status = { role: 'statictext', text: 'row(s) fetched' };
    const before = await app.read(status, { timeoutMs: 1000 }).catch(() => '');
    await app.menu('SQL Editor', 'Execute SQL query');
    await app.waitForNew(status, before);
    if (!before) throw new Error('x');
  });
});
"#,
    );
    let actions = data["steps"][1]["actions"].as_array().unwrap();
    assert_eq!(actions[0]["kind"], "read");
    assert_eq!(actions[0]["into"], "before");
    assert_eq!(actions[0]["optional"], true);
    assert_eq!(actions[0]["timeout_ms"], 1000);
    assert_eq!(actions[2]["kind"], "wait_new");
    assert_eq!(actions[2]["before"], "before");
    assert_eq!(data["windows"][0]["title"], json!({"text": "DBeaver"}));
    assert_eq!(data["unsupported"][0]["reason"], "a condition, decided when the script runs");
}

#[test]
fn a_script_without_the_helper_gives_each_call_as_a_command() {
    let data = steps(
        "flow.py",
        r#"
import subprocess, sys
subprocess.run(["agent-record", "click-at", "w-123", "40", "50"])
subprocess.run(["agent-record", "type", "w-123", sys.argv[1]])
"#,
    );
    assert_eq!(data["language"], "python");
    assert_eq!(data["helper"], false);
    let steps = data["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["actions"][0]["command"], "click-at");
    assert_eq!(
        steps[0]["actions"][0]["args"],
        json!([{"text": "w-123"}, {"text": "40"}, {"text": "50"}])
    );
    assert_eq!(data["unsupported"][0]["line"], 4);
}
