---
name: recording-to-script
tags: automation-script, rpa, record-and-replay, desktop-automation, ai-agent
requirements:
  - agent-record
description: >
  ALWAYS use this skill before writing any script, bot or automation that repeats a task done in
  a desktop app or a web page: a task you just did in this conversation with the desktop tools
  (agent-record, its desktop-recorder MCP server, agent-browser), a task the person recorded (agent-record record), or one
  they describe. The script repeats the same steps in the same app, without AI and in the
  background, with the values that change as parameters and passwords filled by name.
  JavaScript by default; any language the person asks for (Python, Go, PowerShell...).
  Triggers on: "create a script for this", "crea un script", "crea un script para replicar esto",
  "hazme un script", "automate this", "automatiza esto", "make it repeatable", "hazlo repetible",
  "repeat this every day", "que se ejecute solo", "turn the recording into a script",
  "convierte la grabación en script", "make a bot of this", "the customer and the amount change
  every time", "write it in Python". When such a script later fails, use script-repair.
---
<!-- Part of Agent Record Adapter (see NOTICE). -->

# A script that repeats a task

The person wants a task done in a desktop app or web page to run again without doing it by hand, and without AI. You write a script that repeats it with agent-record commands. The script opens what the task needs, finds each element by what it is (not by where it was on the screen), does the same actions and checks each one worked. It stops with a clear message when something is not as expected. It runs in the background: the person keeps using their computer meanwhile.

The script repeats the task as it was done. When the app or page changes, it stops at the step that no longer matches: then follow the `script-repair` skill.

**Desktop apps and web pages.** Desktop apps are driven with agent-record's desktop commands (`click`, `set-value`, `press --window-id`, `menu`, ...). A web page in a browser started with DevTools goes through the official agent-browser, run as `agent-record browser <agent-browser command>` (MCP: the browser tools of the same server); it also works in the background. The person's own browser without DevTools (where they are already signed in) is driven with the desktop commands (`navigate --window-id`, `click`), which reach its pages as window messages.

## Where the steps come from

- **A task you just did in this conversation** ("now make a script to replicate this"): your own successful desktop tool calls are the steps, in order. Leave out the attempts that failed, the reads you only made to look around (snapshots, screenshots, finds), and anything you would not need again. Keep how you found each element (its window, role and name or id), never a ref: refs belong to one snapshot.
- **A recording** (`record`, see the `desktop-recording` skill): read it as section 1 says.
- **A task the person describes**: do it once with the desktop tools, or walk through it with them, then write the steps.

## Follow the same path through the app

The script goes the way the task went: the same app, the same screens, the same menus and buttons. The app already holds what the task needs (its saved connections and sessions, the person's login, its settings), so a script that drives the app needs no user name, password or connection string. Do not replace the app with another route (a direct database connection, an API, a file the app reads) unless the person asks for that route. Never read, copy or decrypt credentials an app stored for itself (DBeaver's `credentials-config.json`, a browser's saved passwords): when a step needs a password the app does not fill in itself, use `secret fill`.

## 1. Read the recording

The recording is a JSON Lines file: `recording_started`, `recording_ready`, one line per action, `recording_stopped`. Read every action in order and first write it down in plain words, one line each. Show this list to the person: it is what they can correct before any code exists.

```
1. Chrome, page northwind.example/signup: type "Lucia Fernandez" in "Full name"
2. same page: choose "Peru" in "Country"
3. click the button "Create account"
```

What matters in each action line:

- `kind`: `click`, `type`, `select`, `press`, `scroll`, `drag`.
- `element`: `role`, `name`, `native_id` (the automation id, or a web element's `id`), `help_text` (often a placeholder), `label` (the text beside a field with no name), `bounds`.
- `ancestors` and `match_count`: above 1, several elements look the same and the nearest named ancestor tells them apart.
- `window`: `title`, `app_name`, `path` (the program), `bounds`, and `url` for a web page, scheme included: open it as recorded (`http://` stays `http://`). `owner_id` marks a dialog another window opened, such as the file picker of a page's upload button: in the script, click the button, wait for that app's window with the dialog's title (`list-windows --app <app_name>`), act in it (`set-value` on the file name, `click` on Open) and wait for it to close.
- `by: "agent"` marks an agent's own action. An agent's `click-at` keeps the element under the point when there is one (replay it by that element) and `window_point`; a `press` of Space on a radio button or checkbox, or Enter on a button, is replayed as a click on it, since keys may be ignored in the background.
- `value` on `type` and `select`: what the field ended up holding. `secure: true` is a password: no text was recorded.
- `state_after.checked` on a click: the box ended checked or not.
- `video.offset_ms` in the last line: an action at `t_ms` is at `t_ms - offset_ms` in the video. Watch the video around an action you do not understand.

## 2. Keep what the task needs

Leave out what a replay must not repeat:

- Clicks that only moved the focus or did nothing, and repeated attempts at the same thing (keep the last one that worked).
- Mistakes the person corrected: a value typed and then changed keeps only the last value; a menu or dialog opened and closed without choosing anything is left out.
- Clicks on the taskbar or the Start menu (`explorer.exe`): the script opens the app itself.
- Minimize, maximize and restore.
- Keys that only move the caret or select inside a field (Home, Ctrl+End, Shift+arrows) and a paste whose text the next value of the field already holds: the value that follows is what the replay writes.

Keep everything that changes the result: every value, every option, every file opened or saved, every confirmation. A part that has no direct background command is still done, another way (section 6); never drop it. What the person says about the task (a step they forgot, a value that changes, something to ignore) wins over the recording.

## 3. Decide the parameters

A value that will be different next time is a parameter: a name, an email, an amount, a date, a file, a search term. Ask the person when it is not clear. Give each one an English `snake_case` name, a short question that explains it and the recorded value as its example. A choice among fixed options lists them.

A password (`secure: true`) is never written in the script, never a parameter and never invented. The script fills it by name with `agent-record secret fill <field> <name>`, with an English name that says what it is for (`bank-login`). The first run opens a small window where the person types it once; it is saved in the system's secret store, bound to that app and site, and later runs fill it on their own. Whoever runs a shared script is asked for their own password the first time.

## 4. Choose the language

**JavaScript is the default**: Node is already on the computer (the tool was installed with `npx`), it runs the same on Windows, macOS and Linux, and `scripts/desktop-script.mjs` in this skill already does the rules below. Use it unless the person asks for something else.

**If the person asks for another language** (Python, Go, PowerShell, C#, TypeScript...), write the script in that language, with the structure that suits it. The rules in the next section are what make a script work; they hold in any language. Section 8 shows the few helpers to write.

## 5. The rules every script keeps

These were tested on real forms, a password prompt and a desktop app, with a person typing and clicking meanwhile.

1. **Run agent-record and read its JSON.** Every command prints one envelope: `{"ok": true, "data": ...}` or `{"ok": false, "error": {"code", "message", "details", "disposition"}}`. A step fails when `ok` is false. Call the program directly with an argument list, never through a shell string (values with spaces, quotes or `&` stay intact).
2. **Use the installed program.** `$AGENT_RECORD_BIN` if set, else `~/.desktop-recorder-mcp/bin/agent-record(.exe)`, else `agent-record` on PATH. Check that `agent-record navigate --help` works (only agent-record has it): the official agent-desktop would move the mouse and bring windows forward. Scripts run it with `AGENT_DESKTOP_BACKGROUND=1`.
3. **Open, never attach to, what the person is using.** A desktop app: `launch <program>` (it never takes the focus), then wait for its window by title. A web page: `launch <browser> --no-attach --cdp --arg=--user-data-dir=<a folder of the script's own> --arg=--no-first-run --arg=<url>`. The DevTools endpoint lets clicks, values, list choices and checkboxes on the page happen without the browser ever coming forward, and the separate profile leaves the person's own browser alone. Find its window by the `pid` that `launch` returned.
4. **Find each element right before acting on it**, inside the task's window (`find --window-id <id> --first ...`). Refs belong to one snapshot and go stale. Prefer, in this order: `--native-id`, then `--role` with `--name` (`--exact` for short names such as "AI"), then `--name` with the `help_text` or `label`, then an ancestor when `match_count` was above 1. Retry `find` for a while (15 s) instead of sleeping a fixed time: pages and apps answer at different speeds.
5. **A text that is only read has no ref.** A message, a total or a status is read from the `find` match itself (`match.value`, else `match.name`), not with `get`.
6. **Act with background commands** (table in section 6): `click`, `set-value`, `clear`, `select`, `check`, `uncheck`, `secret fill`. `press <key> --window-id` presses Enter, Escape, Tab or an arrow in the background, and `navigate` opens an address. Use the background commands where they work; when a step needs the original agent-desktop's physical input (`--headed`, `type`, `double-click`, `right-click`, `hover`, `drag`, `mouse-click --xy`), use it: the step must be done. Other ways are fine too: the app's own automation (COM for Excel, Word or Outlook), its command line, its files or database, or an API.
7. **Check every step.** `set-value`, `select`, `check` and `secret fill` report `data.disposition.delivery`. `delivered_verified` means agent-record read the result back (for a web field, from the page itself: a date field shows its parts, not the value, to accessibility). Otherwise read it with `get <ref> --property value` or `is <ref> --property checked`. After an action that opens or changes something, wait for what the next step needs (`wait --text <text> --window-id <id>`, or a retried `find`).
8. **Retry an action only when `disposition.retry` is `safe`.** Any other answer means it may have happened: read the screen and decide, never repeat it blindly.
9. **Ask before what cannot be undone** (send, post, delete, buy, pay), unless the person asked for it to run on its own (a `--yes` option). Never retry those steps, and never try two ways of doing one.
10. **Say where it failed.** Number the steps and give each a short description. On failure print `Step N (what) failed: <why>` and exit with a non-zero code. A `select` with no such option lists the options there are (`error.details.options`): print them. Leave the screen as it is: it is where a repair starts.
11. **Let a repair stop before a step**: an option such as `--stop-before N` that runs the steps before N and leaves the screen there.
12. **Clean up only what it opened.** On success, close the browser it started (by its `pid`, never all browsers). Before opening its browser, close the one an earlier run left open with the same profile.

## 6. Turn each action into a command

| Recorded | In the script |
|---|---|
| `click` on a button, link, tab, menu item, radio button | `click` the element |
| `click` on a `menuitem` with `menu: "popup"` (an item of a right-click menu) | the same command from the menu bar: `menu --window-id <id> --find "<item>"` (most right-click commands are in the menu bar too, and act on what is selected); a `menuitem` of a menu the app draws (Notepad's File menu): `click` its menu (File), then the item |
| a `click` with `button: "right"` and no menu item after it | the recording could not read what was chosen: say so in the step list, never click where the menu was |
| `click` with `state_after.checked` | `check` or `uncheck` (they do nothing when it is already so) |
| `type` into a field | `set-value` with the value (or the parameter), then check it |
| `type` with `secure: true` | `secret fill` the field with the secret's name |
| `type` into a `document` that is only read (`states` has `readonly`, no ref: a page the app draws itself, as in a word processor) | `window.write(query, text)`: it types at the document's end what its text lacks (a new line as Enter), with the window's own keys in the background; the app may take typed text only while its window is active |
| `type` into a grid cell or canvas editor | select the cell (`select` or `click`), then `set-value` the cell or its editor; a grid that only accepts typing is written through the app's automation (COM for Excel) or its file |
| `select` | `select` the list with the option's text (or value) |
| `press` (`return`, `ctrl+s`, `ctrl+enter`) | `click` the button that does the same (Save, OK, Send, Submit), or run the menu item that shows those keys: `menu --window-id <id> --shortcut ctrl+return` (it finds "Execute SQL query  Ctrl+Enter" in any submenu) |
| `press` in a field followed by more `type` in the same field (Enter for a new line, Ctrl+Space for an autocompletion) | nothing: the last typed value already holds it; a `press` with `value_before` tells what the field held when the key went down (an autocompletion picked with the mouse) |
| `press` `ctrl+v` in a field | `set-value` the field to what it held plus the clipboard's text (`clipboard-get`) |
| a menu-bar item | `menu --window-id <id> <item> <item>`: it runs without opening the menu |
| `scroll` | usually nothing: `find` reaches elements out of view; `scroll-to` if one must be visible |
| `drag` that selected cells or text | select the same range with `select` |
| a click on content with no element (a results grid, a canvas) | `click-at --window-id <id> <x> <y>` at pixels of the window's screenshot; in a script, work the point out from an element next to it (its `bounds` minus the window's position) rather than fixed pixels, so it survives a window of another size |
| copy (`ctrl+c`) with `clipboard` | the app's own Copy (`menu ... Edit Copy`), then read the clipboard (`clipboard-get`) and write it where it goes, as it is; or read the source (`get`, the `find` match) |

## 7. JavaScript (the default)

Copy `scripts/desktop-script.mjs` from this skill next to the script. An agent without the skill folder gets it with `agent-record skills get recording-to-script desktop-script.mjs` (MCP: `desktop_skills_get` with `{"name": "recording-to-script", "reference": "desktop-script.mjs"}`) and saves the `content`. It has no dependencies and needs Node 18 or later.

```js
// create-account.mjs: node create-account.mjs --full-name "Lucia Fernandez" --country Peru
import { task } from './desktop-script.mjs';

await task({
  title: 'Create an account on Northwind Cloud',
  params: {
    full_name: { ask: "The person's first and last name", example: 'Lucia Fernandez' },
    country: { ask: 'Their country', options: ['Spain', 'Peru', 'Mexico'], example: 'Peru' },
    plan: { ask: 'The plan', options: ['Basic', 'Pro'], default: 'Pro' },
  },
}, async ({ params, step, openBrowser, confirm }) => {
  let page;
  await step(1, 'Open the sign-up page', async () => {
    page = await openBrowser('https://northwind.example/signup', { title: 'Create your account' });
  });
  await step(2, 'Type the full name', async () => {
    await (await page.find({ nativeId: 'fullName' })).setValue(params.full_name);
  });
  await step(3, 'Choose the country', async () => {
    await (await page.find({ nativeId: 'country' })).select(params.country);
  });
  await step(4, 'Choose the plan', async () => {
    await (await page.find({ role: 'radiobutton', name: params.plan, exact: true })).click();
  });
  await step(5, 'Accept the terms', async () => {
    await (await page.find({ nativeId: 'terms' })).check();
  });
  await step(6, 'Create the account', async () => {
    await confirm(`Create the account for ${params.full_name}?`);
    await (await page.find({ role: 'button', name: 'Create account' })).click();
    await page.waitForText('Account created');
  });
  return page.read({ name: 'Reference' });
});
```

What `desktop-script.mjs` gives:

| | |
|---|---|
| `task({ title, params }, body)` | Reads `--name value` (snake_case names also as `--kebab-case`), prints `--help` with each question, refuses missing values (exit 2), runs the body, prints `Step N (what) failed: why` (exit 1) or `Done` |
| `step(n, what, fn)` | One numbered step; `--stop-before N` stops there and leaves the screen |
| `confirm(question)` | Asks y/n in the terminal; `--yes` answers for the person |
| `openApp(program, { title, image, args })` | Its window: one already open with the whole `title`, else the new window `launch` opens (an app that keeps one window: the one it has) |
| `openBrowser(url, { title, profile, browser })` | Chrome (or Edge) with a profile of the script's own and DevTools; closed when the task finishes (`--keep-open` keeps it) |
| `findWindow(title, { app, pid })` | A window already open. `title` can be a list of the title's parts as recorded (`['Report.txt', 'Notepad']`): the window of `app` with the most of them wins, as the part naming a document changes |
| `window.find(query)` | An element to act on, waiting for it. `query`: `nativeId` (with its `role`: ids an app numbers, such as a dialog's Save button 1, repeat on the items of its file list), `role`, `name`, `text`, `exact`, `within` (only inside an element whose name contains that text: a tab, a panel, a dialog), and `inDocument: true` (inside the document the window's title names now: a code editor with no name) |
| `window.boundsOf(query)` | Where any element is on screen, also one you cannot act on (a panel) |
| `window.waitForNew(query, before)` | Waits until a text that is only read differs from what it was (a status line an earlier run left) |
| `window.read(query)` | The text of an element that is only read |
| `window.has(query)` | The element if it is there now, else `null`: for a dialog that only sometimes appears |
| `window.waitForText(text)` | Waits until the window shows it |
| `window.menu(...path)` | A menu-bar command, without opening the menu |
| `window.shortcut(keys)` | What pressing `keys` does (`'ctrl+return'`), in the background: the menu item or button that shows them, else the keys to what has the focus in that window |
| `window.chooseMenuItem(name)` | A menu item by name: in an open menu, in a classic menu bar (any submenu), or by opening each menu of the window's own menu bar in turn |
| `element.paste()` | Ctrl+V: the clipboard's text added to what the field holds |
| `window.write(query, text)` | Makes a document hold `text`: sets it when it takes a value, else types what it lacks at its end |
| `window.clickAtOffset(dx, dy, { double })` | A click at a point a recording noted, measured from the window's top-left corner where it is now |
| `window.clickAt(x, y, { double, screen })` | A click on content with no element: at pixels of the window's screenshot, or with `screen: true` at screen pixels (as `boundsOf` gives them) |
| `findWindow(title, { app, pid })` restores a minimized window | A minimized window shows no elements; it is shown again without coming forward |
| `window.screenshot(file)`, `window.currentTitle()` | A PNG of the window (even behind others); its title now |
| `readClipboard()` | The text the app copied |
| `element.click()`, `.setValue(v)`, `.clear()`, `.select(option)`, `.check()`, `.uncheck()`, `.fillSecret(name)`, `.value()`, `.is(state)`, `.bounds()` | The actions, each checked; where the element is |
| `element.expand()`, `.collapse()` | Opens or closes a tree item (a click only selects it) |
| `window.clickIn(query, dx, dy, { double })` | A click at a point with no element of its own inside a panel that has a name (a cell of a results grid inside its results tab), measured from the panel's corner, once the panel is there (it need not be something you can act on) |
| `window.close()` | Closes the window as its Close button does (an app that asks to save shows its dialog) |
| `run(...args)` | Any other agent-record command; returns `data` or throws |

A desktop app driven through its menus, the way a task done in DBeaver goes (the connection is the one the app already has, so the script needs no login):

```js
// export-rows.mjs: node export-rows.mjs --connection "PRV Staging" --table alumno --out C:\Users\me\Desktop\alumno.txt
import { task } from './desktop-script.mjs';
import { writeFileSync } from 'node:fs';

await task({
  title: 'Copy the first rows of a table from DBeaver into a text file',
  params: {
    connection: { ask: 'The DBeaver connection', example: 'PRV Staging' },
    table: { ask: 'The table', example: 'alumno' },
    rows: { ask: 'How many rows', default: '10' },
    out: { ask: 'The text file to write', example: 'C:\\Users\\me\\Desktop\\alumno.txt' },
  },
}, async ({ params, step, findWindow, readClipboard }) => {
  let app, editor;
  await step(1, 'Find DBeaver', async () => {
    app = await findWindow('DBeaver', { app: 'dbeaver.exe' });   // restored if it was minimized
  });
  await step(2, `Select the connection ${params.connection}`, async () => {
    await (await app.find({ role: 'treeitem', name: params.connection, exact: true })).select(params.connection);
  });
  await step(3, 'Open a new SQL script for it', async () => {
    await app.menu('SQL Editor', 'New SQL script');
    await new Promise((done) => setTimeout(done, 1500));
    const title = await app.currentTitle();
    // Never run anything on another connection: the title names the script's connection.
    if (!title.includes(`<${params.connection}>`)) throw new Error(`the new script is not on ${params.connection}: ${title}`);
    editor = await app.find({ role: 'textfield', within: title.split(' - ').pop() });
  });
  await step(4, 'Write and run the query', async () => {
    const status = { role: 'statictext', text: 'row(s) fetched' };
    const before = await app.read(status, { timeoutMs: 1000 }).catch(() => '');
    await editor.setValue(`SELECT TOP ${params.rows} * FROM ${params.table};`);
    await app.menu('SQL Editor', 'Execute SQL query');
    await app.waitForNew(status, before);   // not the status an earlier run left
  });
  await step(5, 'Copy the whole result and write it as it is', async () => {
    // The results panel; the grid's top-left corner is 50 px in and 84 px down from its corner,
    // and a click there selects every cell.
    const at = await app.boundsOf({ role: 'tablist', name: `${params.table} 1`, exact: true });
    await app.clickAt(at.x + 50, at.y + 84, { screen: true });
    await app.menu('Edit', 'Copy');
    writeFileSync(params.out, await readClipboard());
  });
  return params.out;
});
```

This script was run as it is against DBeaver 26 (SQL Server connection, two displays at 125 %): the file it writes is byte for byte what DBeaver's own Copy gives. Work out every point like the one in step 5 on a screenshot while you write the script, from an element next to it (`boundsOf`) rather than fixed pixels, and run the script once before handing it over.

The person runs it with `node create-account.mjs --full-name "..." --country Peru`, and `node create-account.mjs --help` lists what it asks for.

## 8. Another language

Write the same few helpers the library has, in that language, and keep the rules of section 5. For example, in Python:

```python
import json, os, subprocess, sys, time
from pathlib import Path

AD = os.environ.get("AGENT_RECORD_BIN") or str(Path.home() / ".desktop-recorder-mcp" / "bin" / "agent-record.exe")

def run(*args):
    answer = json.loads(subprocess.run([AD, *map(str, args)], capture_output=True, text=True).stdout)
    if not answer["ok"]:
        error = answer["error"]
        raise RuntimeError(f"{args[0]}: {error['code']} {error['message']}")
    return answer["data"]

def find(window, timeout=15, **query):            # find(page, native_id="email") or role=, name=
    flags = [f"--{k.replace('_', '-')}" if v is True else f"--{k.replace('_', '-')}={v}" for k, v in query.items()]
    end = time.time() + timeout
    while time.time() < end:
        match = run("find", "--window-id", window["id"], "--first", *flags)["match"]
        if match and match.get("ref_id"):
            return match["ref_id"]
        time.sleep(0.4)
    raise RuntimeError(f"No {query} in {window['title']}")

def step(number, what, action):
    print(f"Step {number}. {what}")
    try:
        action()
    except Exception as error:
        sys.exit(f"Step {number} ({what}) failed: {error}")
```

Then each step reads like the plain-words list: `step(2, "Type the full name", lambda: run("set-value", find(page, native_id="fullName"), full_name))`. Add the parameters (`argparse`), the confirmation, `--stop-before`, and the browser opening and closing as in section 5.

## 9. Try it before handing it over

Run it once from a clean state (the app closed, the page not open), with the recorded values, and with `--yes` only if it has no irreversible step or you are on a test page. It must reach its final check. If a step fails, fix that step (see `script-repair`) and run it again from the start until it passes end to end. Do not run real public actions (send, post, pay) on a real site to test it: ask the person, or stop before that step.

Then give the person:

- the script and how to run it (with an example command),
- its parameters with their questions and examples (`--help`),
- the saved passwords it uses, by name, and that the first run will ask for them,
- what it does not cover, and why.
