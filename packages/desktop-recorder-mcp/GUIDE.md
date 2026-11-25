# Agent Record Adapter and Desktop Recorder: the complete guide

This guide covers everything Agent Record Adapter adds to the official agent-desktop and agent-browser, in one place, for a person and for an AI agent that has never seen the project:

1. [What there is](#1-what-there-is)
2. [Install, update, uninstall](#2-install-update-uninstall)
3. [The MCP server and the CLI](#3-the-mcp-server-and-the-cli)
4. [Operating an app: the observe-act loop](#4-operating-an-app-the-observe-act-loop)
5. [Recording, with or without video](#5-recording-with-or-without-video)
6. [Filming your own work: a demo video made by the agent](#6-filming-your-own-work-a-demo-video-made-by-the-agent)
7. [Checking and repairing a video](#7-checking-and-repairing-a-video)
8. [From a recording to a script, and repairing a script](#8-from-a-recording-to-a-script-and-repairing-a-script)
9. [The Desktop Recorder app](#9-the-desktop-recorder-app)
10. [Passwords](#10-passwords)
11. [Rules](#11-rules)
12. [Known limits and problems](#12-known-limits-and-problems)
13. [Troubleshooting](#13-troubleshooting)

Windows only for now. **Background mode** is on by default: every action first takes a way that never moves the person's mouse, never types on their keyboard and never brings a window to the front, so the person can keep working while an agent or a script runs. When an action has no such way, the official agent-desktop does it, as the backup (and that may use the keyboard or the pointer, or bring the window forward). `AGENT_DESKTOP_BACKGROUND=0` turns background mode off: then agent-record behaves exactly as the official agent-desktop.

Agent Record Adapter is not a fork. The official vercel-labs agent-desktop and agent-browser are used exactly as published (git submodules in the repository, the npm package for agent-browser); everything of ours is a separate layer around them, so each new upstream version is taken as it is. Working on the project itself (the layer, building, testing, updating the upstream projects) is documented in the repository.

---

## 1. What there is

| Part | What it is | Where |
|---|---|---|
| `agent-record` | The CLI agents and scripts run: every agent-desktop command with the same syntax and answers, plus `record`, `secret`, `stroke`, `menu`, `click-at`, `navigate`, `press --window-id`, `type w-<id>` and `browser` (agent-browser) | `%USERPROFILE%\.desktop-recorder-mcp\bin\agent-record.exe` once installed |
| Engine | The official agent-desktop, built from its unmodified sources with our background adapter; `agent-record` runs agent-desktop's own commands through it | Inside `agent-record.exe` |
| agent-browser | The official vercel-labs agent-browser, built from its unmodified sources, for web pages in a browser with DevTools (`agent-record browser ...`) | Inside `agent-record.exe` |
| MCP server | `agent-record mcp`: every command as an MCP tool for an AI agent | Registered in each agent by the installer under the name `desktop-recorder` |
| Skills | The official `agent-desktop`, `agent-desktop-windows` and `agent-browser`, unmodified, and ours: `agent-record-background`, `desktop-recording`, `recording-to-script`, `script-repair` | Copied into Claude Code, Codex and OpenCode; served to every agent by `desktop_skills_get` |
| Desktop Recorder | A small floating bar that records a task and plays it again, without AI | `%USERPROFILE%\.desktop-recorder-mcp\bin\desktop-recorder.exe`, Start menu > Desktop Recorder |
| `desktop-recorder-mcp` | The one package that carries and installs all of the above: nothing else is downloaded or installed | `packages/desktop-recorder-mcp` in the repository |

Source code: `crates/` (the background adapter, the recorder, secrets and the `agent-record` front), `apps/recorder/` (the app), `skills/` (our skills), `packages/desktop-recorder-mcp/` (installer), `vendor/` (the official projects, unmodified).

---

## 2. Install, update, uninstall

### Install

From npm (once a version is published):

```powershell
npx desktop-recorder-mcp
```

From a package file built here (`packages/desktop-recorder-mcp/desktop-recorder-mcp-<version>.tgz`). npx takes a package file only as `./name`, from the file's own folder:

```powershell
cd "C:\path\to\agent-record-adapter\packages\desktop-recorder-mcp"
```

```powershell
npx ./desktop-recorder-mcp-1.0.0.tgz
```

It lists the agents found on the computer; tick the ones to set up and press Enter. Then:

| The person uses | Tick | Then |
|---|---|---|
| Claude Code in a terminal, or the Code tab of the Claude app | Claude Code | Start a new session |
| The Chat of the Claude app | Claude Desktop | Quit and reopen the app |
| `codex`, or the Codex app | Codex | Restart the Codex app |

Agents that are open while it installs keep the previous version until they restart: their MCP servers hold the old program, which the installer sets aside (renames) instead of overwriting. Close Desktop Recorder before installing.

What it writes:

- `%USERPROFILE%\.desktop-recorder-mcp\bin`: `agent-record.exe` (with the engine and agent-browser inside), `desktop-recorder.exe`, `version.txt` (only you, SYSTEM and Administrators can change this folder); `%USERPROFILE%\.desktop-recorder-mcp\secrets.json` (the names of saved passwords, never their values) and `recorder.json` (the app's settings), kept by every install;
- `%USERPROFILE%\.desktop-recorder-mcp\state`: private state of the MCP server (refs, recordings). It stays private: do not share it;
- each agent's configuration, with a backup next to it (`.bak-desktop-recorder-mcp`);
- a Start menu entry "Desktop Recorder".

Uninstalling removes all of it, after showing the list and asking: it stops its programs that still run (an agent's MCP server, the dedicated browser, Desktop Recorder), removes the MCP server and the skills from every agent and the backups of their configuration, deletes the saved passwords from Windows' Credential Manager, the folders above, the dedicated browsers' profiles (`%LOCALAPPDATA%\DesktopRecorder`), the recordings in Desktop Recorder's own folder (`Documents\Desktop Recorder`), agent-desktop's and agent-browser's state (`~\.agent-desktop`, `~\.agent-browser`, kept when the official programs are installed too), the Start menu entry and npx's copy of the package. Recordings in a folder you picked in the app, and scripts you saved anywhere else, stay. `uninstall --agents claude-code` only removes it from those agents; `--yes` does not ask; `--dry-run` only lists.

### Other installer commands

```powershell
npx desktop-recorder-mcp status
```

```powershell
npx desktop-recorder-mcp --agents claude-code,codex
```

```powershell
npx desktop-recorder-mcp --dry-run
```

```powershell
npx desktop-recorder-mcp uninstall
```

```powershell
npx desktop-recorder-mcp app
```

`npx desktop-recorder-mcp@<version> update` installs that version again for the agents already set up, without asking (this is what the app's Update button runs). `--home <folder>` installs into another home folder, for tests only.

### Update

Desktop Recorder asks npm for the latest version at start and every six hours. Settings always shows a version row: "Version 0.2.0 · up to date", "· no connection", or, when npm has a newer one, "Update 0.2.0 → 0.2.1" with a dot on the gear. Pressing Update installs the new version, opens the new app (without taking the focus) and closes the old one; if it fails, the app stays open and shows why. A version only reaches people once it is published on npm.

### Publish (the maintainer)

In `packages/desktop-recorder-mcp`: build agent-record first (`cargo build --release -p agent-record` at the repository root), then `node scripts/prepare.js` (copies `agent-record.exe`, builds `desktop-recorder.exe` with its SHA-256, copies our skills and the official ones into the package, and records the agent-browser version inside), then `npm run release` (publishes and waits until the version can be downloaded). npm asks for 2FA in the browser. Run `npx desktop-recorder-mcp@<version>` from another folder, not from the package's own.

---

## 3. The MCP server and the CLI

### MCP tools

Every CLI command is a tool named `desktop_` plus the command, with `-` and spaces as `_`: `snapshot` is `desktop_snapshot`, `set-value` is `desktop_set_value`, `record start` is `desktop_record_start`, `skills get` is `desktop_skills_get`. A tool's fields are the command's options without `--` and with `-` as `_`; a positional argument is named after itself (`click REF` is `{"ref_id": "@s8f3k2p9:e1"}`, `set-value REF VALUE` is `ref_id` and `value`). Only `secret set` and `secret delete` are not tools: the person stores passwords.

Each tool returns the CLI's JSON envelope: `{"ok": true, "data": ...}` or `{"ok": false, "error": {"code", "message", "suggestion", "disposition"}}`. Repeat an action only when `error.disposition.retry` is `"safe"`.

First read the skills, through the MCP server:

- `desktop_skills_get {"name": "desktop"}`: the observe-act loop;
- `desktop_skills_get {"name": "platform"}`: what differs on Windows;
- `desktop_skills_get {"name": "recording"}`, `"recording-to-script"`, `"script-repair"`.

### The CLI

An agent with a shell can run the same program directly:

```powershell
& "$env:USERPROFILE\.desktop-recorder-mcp\bin\agent-record.exe" list-windows
```

`--help` on any command lists its options. Use the CLI instead of the MCP server when you need something the server cannot give per call:

- a **session** of your own (`session start`). The MCP server runs every call with the global flags it was started with; the one exception is the drawn cursor, which the server switches on in a session of its own (section 6);
- long sequences inside a script (`node`, PowerShell), which run without a round trip per action.

In Codex, the CLI runs through Codex's own shell, which may run in its sandbox; if a CLI call fails there with an access error while the same MCP tool works, approve running that command outside the sandbox, or use the MCP tools, which have the cursor too (section 6).

Refs (`@s8f3k2p9:e1`) belong to the namespace of the snapshot that printed them: a ref from an MCP snapshot works in MCP calls; a ref from a snapshot inside a CLI session works in that session. Do not mix them. To share the MCP server's state from the CLI, set `AGENT_DESKTOP_HOME=%USERPROFILE%\.desktop-recorder-mcp\state`.

### Command families

| Need | Commands |
|---|---|
| See | `list-windows`, `list-apps`, `list-displays`, `snapshot` (`-i` for interactive elements only), `find`, `get`, `is`, `screenshot`, `list-surfaces` |
| Act | `click`, `set-value` (to write text), `clear`, `select`, `check`, `uncheck`, `toggle`, `expand`, `collapse`, `scroll`, `scroll-to` |
| Menus | `menu --window-id W "File" "Save as..."`, `menu --window-id W --shortcut ctrl+s`, `menu --window-id W --find "Export"` (classic menu bars); `click` for ribbons and modern menus |
| Content with no element (grids, canvases, charts) | `click-at --window-id W X Y` (pixels of the window's screenshot; `--double` for a double click) |
| Apps and windows | `launch` (`--arg`, `--no-attach`, `--cwd`, `--cdp`), `close-app`, `minimize`, `restore`, `move-window`, `resize-window` |
| One key in one window | `press <key> --window-id W`: return, escape, tab, space, backspace, forwarddelete, home, end, pageup, pagedown, the arrows, f1-f12 (no Ctrl, Alt or Shift) |
| A browser tab's address | `navigate --window-id W <url>` (`--in-app` for a path of the same single-page app, keeping what it holds in memory) |
| Wait | `wait` (time, element, text, window, `--menu`, `--event window-opened`) |
| Clipboard | `clipboard-get`, `clipboard-set`, `clipboard-clear` |
| Several steps in one call | `batch` |
| Recording | `record start`, `record stop`, `record status`, `record` (foreground) |
| Passwords | `secret set`, `secret list`, `secret delete`, `secret fill` |
| Presentation | `session start --cursor`, `cursor-overlay enable --label "..."`, `cursor-overlay disable` |

**Background first, then everything from the original.** These have a background way and do not disturb the person: `click`, `set-value`, `clear`, `select`, `check`, `uncheck`, `expand`, `collapse`, `scroll`, `press <key> --window-id W` (Enter, Escape, Tab, arrows), `navigate`, `menu`, `click-at`, `launch`, `restore`, `minimize`, `move-window`, `resize-window`, `secret fill`. If an action did not work (refused, `delivered_unverified` and nothing changed when you look again, or the element cannot be reached by ref), use the original agent-desktop's options, all of them available as in the official tool: `--headed` (a real click or key on the window brought forward), `type`, `press` (with Ctrl, Alt or Shift too), `double-click`, `right-click`, `hover`, `drag`, `mouse-click --xy X,Y` for an element you can only reach by its position, `focus-window`, `maximize`, `launch --activate`. They may move the person's pointer or bring the window forward for that action; that is expected. Do not give up on a step while one of them can do it.

---

## 4. Operating an app: the observe-act loop

**Web apps work in the person's own browser, as it is.** Use the Chrome, Edge or Brave window the person already has open (any Chromium browser), with their profile and their session: nothing needs to be started in a special way. In it, without coming forward and without the person's mouse or keyboard:

- `click` on a button, link, icon or menu group is a real pointer click at that point of the page (as mouse messages to the browser window), so components that listen to the pointer answer (a menu group's header, a date picker's button). A group or list item with no name of its own is clicked at its header (its first visible text), so a second click closes it again. Checkboxes, radio buttons, fields and lists keep their accessibility actions first; a component library's radio or checkbox whose real input is hidden (PrimeNG's `p-radiobutton`) is then clicked at its drawn box;
- a minimized browser window is restored behind the person's windows (never brought forward) before a pointer click, since a minimized page has nowhere to click;
- `set-value` on a web field also types its last character again as background keys, so a field that only takes typed input (PrimeNG's date field) keeps the value when it loses focus; dates are written as the field shows them (`15/03/1990`);
- unnamed twins get refs of their own: the icons of a table row (told apart by the row's text and their order in the cell) and unlabelled fields beside their labels (each field takes its own label, even when the label comes after it);
- `press return --window-id W` presses Enter (or Escape, Tab, an arrow) on the page's focused element: the field last written into;
- a page's **file picker** (an upload button, YouTube Studio's "Select files") opens with `click` on the button, or `click-at` on its spot, while the browser stays behind the person's windows: the click is a real pointer click inside the page, so the page may open its file dialog. Do not try to bring the window forward (`focus-window` fails: Windows refuses it). Wait for the dialog with `wait --window Open` (its title in the system's language: "Abrir"), or `wait --event window-opened --window Open` right after the click; write the path with `set-value` in its "File name" field and click its own Open button (a `button` or a split `menubutton` directly in the dialog, not the "Open" arrow inside the file name or file type list). A browser extension's click may not open it while the window is not in front; Desktop Recorder's does;
- `press space --window-id W` on a focused radio button or checkbox checks that it changed. When the page ignores the key (a component library's hidden input), the option is clicked at its box; when it still does not change, the press fails and names the option, so a script stops there instead of going on with the previous choice;
- `set-value` whose read-back runs out of time (a file dialog's name field that answers slowly) reads the field once more, for up to 1.5 s: a value that is there is reported as applied, not as `TIMEOUT`;
- `navigate --window-id W <url>` writes the address in the address bar and presses Enter: the same tab, the same session, also right after `launch`. With DevTools, an app whose router sends a full load to its home is taken to the asked route the way its own links do.

**Open the browser with `launch`, so it works while covered.** Chrome pauses a page whose window other windows cover completely (it stops drawing it and updating its accessibility tree). Every Chromium browser that `launch` starts (`launch chrome.exe`, `msedge.exe`, `brave.exe`, `vivaldi.exe`) is started with Chrome's own switches that turn that pause off for that run, with the person's normal profile and session: nothing in the browser's settings changes. So when the browser is not open yet, open it with `launch`; when the person's browser is already open, it keeps the switches it was started with, and the person may close it so the agent opens it again with `launch` (Chrome restores the session). After each action on a page, read its effect with `wait` or a new `snapshot`.

A browser started with `launch <browser> --cdp` (DevTools) works too: then the same actions go through DevTools, and `navigate --in-app` is available. It is optional.

1. Find the window: `list-windows --app chrome.exe` (or `launch` it: the result has `pid` and `window.id`).
2. `snapshot --window-id w-123 -i`: the interactive elements with refs.
3. Act on a ref: `click`, `set-value`, `select`, `check`, `expand`...
4. Check the effect: a new `snapshot`, `get --property value`, `wait` for the element or window you expect. Refs go stale when the UI changes: on `STALE_REF` or `AMBIGUOUS_TARGET`, snapshot again.

Useful patterns:

- **Start an app:** `launch <absolute path or System32 name>`. It opens **behind** the person's windows and does not take the focus; only the person can bring it to the front. A browser for tests: `launch "C:\Program Files\Google\Chrome\Application\chrome.exe" --no-attach --arg=--user-data-dir=<own profile folder> --arg=--new-window --arg=<url>` (its own profile, never the person's).
- **Dialogs** (Save as, Export, Open): after the click that opens one, `wait --event window-opened` or `list-windows`, then snapshot the dialog's window and fill its fields (`set-value` on the file name, `click` on Save). A pending dialog also shows in `data.surfaces` of an action result.
- **Dropdowns** (web selects, PrimeNG, combo boxes): `select` on the combobox with the option's text (`select @ref "@gmail.com"`) opens it, picks the option and checks it in one call. To show the list in a demo, `click` its "dropdown trigger" or `expand` the combobox; then `click` the option and `snapshot` to confirm the new value.
- **Writing text:** `set-value` on the field, with the whole text (`type` would send keystrokes through the person's keyboard). Then check it with `get --property value`. There is no Tab key: act on each field by its ref.
- **Enter, Escape, Tab, arrows:** `press return --window-id W` presses the key in that window only, on the element that has the window's focus (the field last written into), never through the person's keyboard; a browser is told it is active for that moment only, and Windows' foreground does not move. Check the effect afterwards (`snapshot`, `get`, `wait`): the result says only that the key was delivered.
- **Shortcuts:** keys with Ctrl, Alt or Shift go through the original `press` (the window comes forward for the key). Without that, to run what Ctrl+E (or any shortcut) does, use `menu --shortcut ctrl+e`; to run an item by name in any submenu, `menu --find "Export to PDF"`; in apps with ribbons and in web pages, find the button and `click` it.
- **Going to a page that has no link:** `navigate --window-id W http://127.0.0.1:4200/ruta` opens it in the same tab through the address bar, keeping cookies and storage (a page reload: what the app keeps only in memory is lost, so prefer the app's own links when there is one). With a DevTools browser, `--in-app` changes the route the way the app's own links do. The result gives the address reached and `reached: true` when it is the one asked for.
- **Repeated names** ("Ver datos" on every chart, "Editar" on every row, unnamed fields beside their labels, a day number in a calendar): a ref is told apart from its twins by its context (the nearest ancestors' names, or the text before its branch: the chart's title, the row's first cell, the field's label). Act on it as on any ref. When two of them share the context too, the action is refused with `AMBIGUOUS_TARGET` (`identity_not_unique`): narrow it down with `snapshot` of a smaller part, or act on something next to it that is unique.
- **Results grids, canvases, charts:** `screenshot --window-id w-123`, then `click-at` on the pixel of that screenshot.
- **Anything that cannot be undone** (send, delete, publish, pay): ask the person first, unless they said to do it.

---

## 5. Recording, with or without video

`record` watches what the **person** does in one window, app or screen and writes each action as a JSON line. It can also film it as an MP4.

### Start

```powershell
agent-record record start --window-id w-123
agent-record record start --app EXCEL.EXE
agent-record record start --display 2 --video --video-out D:\Videos\task.mp4
```

MCP: `desktop_record_start` with the same fields (`window_id`, `app`, `display`, `video`, `video_out`, `max_duration_ms`, `out`). It returns once the recording is ready, with `id`, `out` (the JSON Lines file) and, with video, the video path.

| Option | Meaning |
|---|---|
| `--window-id`, `--app`, `--display` | What is recorded. One window, one app or one screen; never every screen by default. `--display` takes `primary`, the number from `list-displays` (1, 2, 3...) or its id |
| `--video`, `--video-out X.mp4` | Also film it (H.264 MP4, 12 frames per second). Without `--video-out`, beside the events |
| `--max-duration-ms` | Stops by itself after this time: default 600000 (10 minutes), at most 3600000 (1 hour) |
| `--out` | Where the JSON Lines go (default: the state folder, `recordings/<id>.jsonl`) |
| `--include-injected` | Also keep input that Windows marks as synthetic (a remote session, another automation tool) |
| `--no-indicator` | No red indicator (for an app that shows its own Stop, or a test) |

While it records, a small red indicator at the bottom of the screen shows the time and a Stop button. The indicator is left out of the video.

### Status and stop

```powershell
agent-record record status
agent-record record stop            # --id <id> when more than one runs
```

MCP: `desktop_record_status`, `desktop_record_stop`. Stop waits until the video is finished and returns the number of actions, the duration and the video path. Each session is described in `<state folder>\recordings\<id>.json`. In `record status`, `elapsed_ms` of an `ended` recording is the length it had when it stopped; it keeps growing only while it records.

The recording runs in a detached process: it survives the end of an agent's turn and the call that started it. Start and stop it in the same conversation anyway.

### What is in the recording

`recording_started`, `recording_ready`, one line per action, `recording_stopped`. Each action has `kind` (`click`, `type`, `select`, `press`, `scroll`, `drag`), `element` (`role`, `name`, `native_id`, `help_text`, `label`, `ancestors`, `match_count`), `window` (`title`, `app_name`, `path`, `url` for a web page with its scheme, `owner_id` for a dialog another window opened, such as Chrome's file picker: a recording of that window keeps it), `value` for `type` and `select` (`secure: true` on a password: no text is kept), and `t_ms`. An agent's actions carry `"by": "agent"`: a `select` keeps the option the list showed once it settled (Chrome's print preview shows the previous destination for a moment), a `click-at` keeps the element under the point and `window_point` (the point as given), and a `press --window-id` keeps the page's focused element. A web action's `url` is the address of the tab that holds the element, even when the window's tabs share a title. The last line has `video.offset_ms`: an action at `t_ms` is at `t_ms - offset_ms` in the video. Full field list: `agent-desktop skills get desktop commands-system`, section `record`.

### What the video shows

The video copies the recorded area **from the screen** (GDI), and draws the real mouse pointer on it:

- the area is the window's rectangle (`--window-id`), the display (`--display`), the display of the app's first window (`--app`) or the primary display. It is **fixed when the recording starts**: a window moved or resized afterwards leaves the frame;
- whatever is on top in that area is what is filmed. A window covering the recorded one is filmed instead, and so are notifications and anything else visible there;
- a minimized window has no area: restore it first;
- the MP4 is fragmented: if the recording process is killed, the video still plays up to that moment.

---

## 6. Filming your own work: a demo video made by the agent

The `desktop-recording` skill tells an agent not to act while it records the person. A demo is the opposite case: the person asks the agent to operate an app and to film it. Then the agent records the screen with video and acts; the JSON Lines list the agent's own actions too (`"by": "agent"`: its clicks, values, choices and keys, described like a person's), so the same recording can also become a script. What matters for the demo is the MP4.

### Prepare (with the person)

1. **Choose the display.** `list-displays`. The best place is a display the person is not using, with the app alone on it. Record that display (`--display 2`), not the window (see [known problems](#12-known-limits-and-problems)).
2. **Put the app on it, filling it.** `launch` the app, then `move-window` it to the display's top-left corner and `resize-window` it to the display's size (both from `list-displays`; `maximize` would bring it forward). The app opens behind the person's windows, so **ask the person to bring it to the front once** (its taskbar button) and to leave that display alone: no other windows on it and no mouse over it (their real pointer is drawn in the video). Then check a first frame (section 7) before the long work.
3. **Choose where the videos go**, an empty folder, for example `%USERPROFILE%\Videos\demo-<name>`.
4. **Plan segments.** One recording lasts at most one hour. Split a long demo into segments of a few minutes each, one per area of the product (for example 01-login, 02-customers, 03-reports-export). A short segment that fails is cheap to film again.

### Make the agent's actions visible: the cursor overlay

Accessibility actions do not move the mouse, so without help the person (or a video) sees the screen change with no visible cause. The cursor overlay draws a presentation-only cursor that travels to each element the agent acts on, with an optional caption. It never moves the real mouse, never takes the focus, and clicks pass through it. **It is off by default.**

**From an MCP agent** (Claude, Codex, ChatGPT):

| Tool | What it does |
|---|---|
| `desktop_cursor_overlay_enable` (`label` optional, plus the style options of its schema) | Shows the cursor. The first time, the server starts a session of its own and runs every later call in it, so **take a new snapshot** right after: refs from before it was turned on answer `SNAPSHOT_NOT_FOUND`. |
| `desktop_cursor_overlay_disable` | Hides it. The session stays, so refs keep working; turning it on again needs no new snapshot. |

The cursor appears in the centre of the main display and moves to each element on the next action. It also goes away when the agent closes (the server ends its session). Use it when the person wants to watch what the agent does, or when filming a demo; leave it off otherwise (it adds about 355 ms per action). A server started with `--session` (or `AGENT_DESKTOP_SESSION`) uses that session instead, as the original agent-desktop does.

**From the CLI**, in a session of your own:

```powershell
$ad = "$env:USERPROFILE\.desktop-recorder-mcp\bin\agent-record.exe"
$start = & $ad session start --name demo --cursor | ConvertFrom-Json
$env:AGENT_DESKTOP_SESSION = $start.data.session_id
& $ad cursor-overlay enable --label "Exporting the report to PDF"
& $ad snapshot --window-id w-123 -i      # take snapshots inside the session
& $ad click "@s1a2b3c4:e17"
& $ad cursor-overlay disable
& $ad session end
```

- Start the session before the first snapshot; refs from outside the session are not seen inside it.
- Each `enable --label` replaces the caption; use it as the subtitle of each step (up to about six words). It fades after six seconds without a command.
- It costs about 355 ms per action.
- It starts in the centre of the primary display, not over the app.
- Not checked yet: that the overlay always shows in the recorded video. Check it in the first segment (section 7) before filming the rest.

Without the overlay the demo still works; captions can be added later when editing.

### Film a segment

```powershell
& $ad record start --display 2 --video-out "$env:USERPROFILE\Videos\demo-acme\01-login.mp4" --max-duration-ms 1200000
# ... the agent operates the app: snapshot, act, check each step ...
& $ad record stop --id <id from record start>
```

MCP equivalent: `desktop_record_start {"display": "2", "video_out": "...\\01-login.mp4", "max_duration_ms": 1200000}`, then the actions, then `desktop_record_stop {"id": "..."}`.

During a segment:

- wait a second after `record start` and before `record stop`, so the video does not cut the first and last action;
- pause briefly (about one second) after each visible change so a viewer can follow it; `wait` for the screen to settle after loads, exports and dialogs;
- check every step (snapshot, `get`, `wait`) before the next one. A failed step should be repaired, not filmed in a loop;
- keep the app's window where it was when the segment started;
- never type real passwords (use `secret fill`, section 10) and never film private data the person did not agree to show;
- before an action that cannot be undone on real data (delete, send, publish), ask the person, unless the demo uses test data and they said so.

### Exports in a demo

An export usually opens a Save dialog: `wait --event window-opened`, snapshot the dialog, `set-value` the file name to a path inside the demo folder, click Save, then `wait` for the dialog to close and check the file exists (from the shell). To show the exported file in the video, open it on the recorded display (`launch` the viewer with the file as `--arg`) and `move-window` it there.

### Join and show the result

When every segment is checked, join them with ffmpeg (if installed: `ffmpeg -version`). In the demo folder, write a list in order and concatenate without re-encoding:

```powershell
Get-ChildItem *.mp4 | Where-Object Name -ne 'demo.mp4' | Sort-Object Name | ForEach-Object { "file '$($_.Name)'" } | Set-Content -Encoding ascii list.txt
ffmpeg -v error -f concat -safe 0 -i list.txt -c copy demo.mp4
```

If the segments have different sizes (recorded on different displays), re-encode instead: `ffmpeg -f concat -safe 0 -i list.txt -vf "scale=1920:-2" -c:v libx264 -crf 23 demo.mp4`. Without ffmpeg, deliver the segments in order.

To show the video to the person at the end, give them its full path and open it in a player with `launch` and the file as an argument, for example `launch "C:\Program Files\Google\Chrome\Application\chrome.exe" --no-attach --arg=--user-data-dir=<a profile folder of the demo> --arg=--new-window --arg=file:///C:/Users/.../demo.mp4`. It opens behind their windows: tell them it is open in the taskbar (`--activate` brings it forward, only if they asked to see it now). Not tested yet; if it fails, give the person the path to open it themselves.

---

## 7. Checking and repairing a video

Check every segment as soon as it is stopped, before filming the next one.

1. **The stop result.** `record stop` must return `ok: true` with a video path and a duration close to the time the segment took. An error, or a stopped session you did not stop, means the segment is incomplete.
2. **The file plays and is long enough** (ffprobe comes with ffmpeg):

   ```powershell
   ffprobe -v error -show_entries format=duration:stream=width,height,nb_frames -of default=nw=1 01-login.mp4
   ```

   A duration of 0, "moov atom not found" or "Invalid data" means no video was written: film the segment again.
3. **The frames show the app.** Extract a frame every few seconds and look at them (an agent can read PNG files):

   ```powershell
   New-Item -ItemType Directory -Force frames | Out-Null
   ffmpeg -v error -i 01-login.mp4 -vf fps=1/5 frames\01-%03d.png
   ```

   Look for: the app filling the frame (not another window, not a black frame), each step's change visible, the overlay cursor and captions when used, no private data. Delete the `frames` folder afterwards.
4. **If it failed**, find why (section 13), fix it, and film **only that segment** again with the same file name (delete the broken file first). Go back to the state the segment expects (the same screen, the same data) before starting it again. Keep a short log of each segment: name, start and stop time, result, and what was repaired.

Repeat until every segment passes; only then join them.

---

## 8. From a recording to a script, and repairing a script

**recording-to-script** (`desktop_skills_get {"name": "recording-to-script"}`): turns a recording (or a task the agent did, or one the person describes) into a script that repeats the same steps without AI, in the background. JavaScript by default (`script.mjs` with the helper `desktop-script.mjs`), or any language the person asks for. Typed and chosen values become parameters (`node script.mjs --customer "Lucia" --amount 150`; `--help` lists them); passwords are filled by name with `secret fill`.

**script-repair** (`desktop_skills_get {"name": "script-repair"}`): when a script stops with `Step 4 (...) failed`, it finds the first step that no longer matches the app, changes only that step, keeps the original as `script.original.mjs`, tests with `--stop-before` around steps that cannot be undone, and runs the whole script again.

---

## 9. The Desktop Recorder app

A small floating bar, like Windows voice typing. Open it from the Start menu (Desktop Recorder) or with `npx desktop-recorder-mcp app`. Its language follows Windows (Spanish or English).

### The bar

- **Record** (the round button in the middle): starts recording what is chosen in Settings, with or without video. Press it again to stop. While recording, the bar shows the time and is left out of the video.
- **Settings** (the gear).
- **Recordings** (the list icon).
- **Close** (the X): if a recording is being saved, it waits for the save.

### Settings

| Row | What it does |
|---|---|
| What to record | A screen (`Screen 1`, `Screen 2`...) or an open window |
| With video | Also film it as `video.mp4` |
| Save in | The folder for recordings; by default `%USERPROFILE%\Documents\Desktop Recorder` |
| Version / Update | The installed version and whether it is up to date; a click checks npm again. With a newer version: "Update x → y", see [Update](#update) |

### Recordings

Each recording is a folder named `Recording YYYY-MM-DD HH.MM.SS` inside the chosen folder, with:

| File | What it is |
|---|---|
| `recording.jsonl` | The recorded actions |
| `video.mp4` | The video, when asked for |
| `script.mjs`, `desktop-script.mjs` | The script that repeats the task, written without AI when recording stops |
| `last-run.log` | Everything the last play wrote, and its exit code |
| `repair-prompt.md` | The repair instructions, when they were asked for |
| `script.original.mjs` | The script before a repair |

For each recording, the list shows its name, the number of steps and the result of the last play, with three buttons:

- **Play**: runs `script.mjs` in the background, so the person can keep working. Before a step marked to ask it asks: by default the last click on a button (the one that usually sends, saves or pays), whatever language its name is in. Click a recording's line to see its steps and tick or untick the clicks that ask (kept in `ask-before.json` next to the recording). The result shows on the row; a failed play colours the repair button.
- **Repair with AI**: copies to the clipboard a ready prompt for any AI agent: use the `script-repair` skill, the recording's folder, the step that failed and the rules (background only, no invented data, keep `script.original.mjs`, test with `--stop-before`, ask before steps that cannot be undone, say what changed). The person pastes it into their agent.
- **Folder**: opens the recording's folder.

Once a script has been changed (by hand or by an agent), Play keeps it as it is; it is only written again from the recording while it is still the one the app wrote.

### From the command line

```powershell
node script.mjs --help
node script.mjs
node script.mjs --customer "Lucia" --amount 150
desktop-recorder --make-script <recording.jsonl> <folder>
```

`--make-script` writes the script for a recording without opening the window.

---

## 10. Passwords

Nothing in this tool records a password or writes one into a script or a log. A script or an agent asks for one by name:

```powershell
agent-record secret fill @s1a2b3c4:e5 bank-login
```

It works on an unnamed password field too (most web logins), as long as the window has only one; with two unnamed password fields it is refused (`AMBIGUOUS_TARGET`) rather than guessed. The first time, a small window asks the person to type it once; it is saved in Windows Credential Manager, bound to that app and site, and the task goes on. After that it is filled on its own. Agents never see it, and it is refused anywhere but a password field of that app and site. Ahead of time, or to change it:

```powershell
agent-record secret set bank-login --app chrome.exe --site login.mybank.com
agent-record secret delete bank-login
```

Never ask the person to type a password into the conversation.

---

## 11. Rules

- **Background first.** Use the background way when it works; when it does not, use the original options (`--headed` and the rest) so the task gets done. They may move the pointer or bring the window forward for that action.
- **Consent.** Record and film only what the person asked for, only the screen or window agreed, and stop when they say so. Never film someone else's screen or data without consent.
- **No invented data.** Use the values the person gave or the demo's test data; when something is missing, ask.
- **Actions that cannot be undone** (send, delete, publish, buy, pay): ask first, unless the person said otherwise for test data.
- **No CAPTCHAs, no bot-detection workarounds,** and check a site's terms before automating it.
- **Private state stays private:** `%USERPROFILE%\.desktop-recorder-mcp\state`, browser profiles, recordings with personal data.
- **Report what happened:** what worked, what failed, what was repaired and what was not checked.

---

## 12. Known limits and problems

| Limit or problem | What to do |
|---|---|
| **Video of a window fails** on some windows: `record start --window-id ... --video` answered `Video: could not accept screen frames: The data specified for the media type is invalid... (0xC00D36B4)` with a Chrome window on 19 November 2026, and no video was written | Record the display the window is on (`--display N`) with the window maximized. Not yet fixed |
| The video copies the screen: whatever covers the area is filmed | Keep the app alone and in front on its display; check the frames (section 7) |
| The video area is fixed at start | Do not move or resize the window during a segment |
| A recording lasts at most one hour (10 minutes by default) | `--max-duration-ms`, and segments |
| The cursor overlay needs a session, which the MCP server does not have | Use the CLI for a demo with overlay (section 6) |
| `type`, `press` without a window, `maximize`, `launch --activate` and the mouse commands run the original code: they may take the keyboard, pointer or foreground for that action | Expected: use them whenever the background way did not work |
| An app started with `launch` opens behind the person's windows | The person brings it to the front once; check the first frames |
| Keys and drags with no background equivalent | Not repeated by scripts; listed as "not repeated" |
| Items of a right-click (popup) menu are read when recorded, but this was not tested live | A script that skips them can be repaired with `script-repair` |
| `find --app X` can fail with `AMBIGUOUS_TARGET` when the app has a popup window | Search per window: `find --window-id` |
| `launch notepad.exe` restores the person's Notepad tabs | Use your own windows (a browser with its own profile) for tests |
| macOS and Linux | `record` answers `PLATFORM_NOT_SUPPORTED` |

---

## 13. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `record start`: "The recording ended before it was ready" with a Video error | The video could not be set up (see the window problem above) | Record a display instead of a window |
| `record start`: "the recorded area has no size on screen" | The window is minimized or hidden | `restore` it, or record a display |
| `record stop`: "No recording '...' is running" | It already stopped: the time limit, Stop on the indicator, or its process ended | `record status`; the video plays up to the moment it stopped. Film the rest as a new segment |
| ffprobe: "moov atom not found" / duration 0 | No video was written | Film the segment again; check the window or display was valid |
| The frames show another window, or black | Something covered the area, or the window was moved or minimized | Ask the person to bring the app to the front on its display; film again |
| 0 actions although the person acted | Their input was synthetic (remote session, other automation tool) | Record again with `--include-injected` |
| 0 actions although an agent acted | The recording was started with the foreground `record` (only `record start` sessions take the agent's actions), or the agent used an installed version older than this one | Use `record start` / `desktop_record_start`, and the current version |
| "The file picker opens only with the browser in front" / `wait --event window-opened` times out after clicking an upload button | The click came from another tool (a browser extension), or the picker opened while the click was still finishing and, before the 21 November 2026 fix, the wait counted it as already open | Click the button with `desktop_click` (or `click-at`); wait with `wait --window Open`; install the current version. If the page shows nothing to click (the person's own Chrome, completely covered, pauses its page), take its spot from a screenshot and use `click-at`, or open the browser with `launch` |
| `set-value` in a file dialog answers `TIMEOUT` although the path is written | Before the 21 November 2026 fix: the read-back ran out of time while the dialog was slow | Install the current version (it reads the field once more and reports the value as applied) |
| `press space` comes back done but the option did not change | Before the 21 November 2026 fix | Install the current version: it clicks the option when the key is ignored, or fails naming it; or click the option's ref |
| A recording keeps the previous option of a list, misses a `click-at`, or has another tab's address | Before the 21 November 2026 fix | Install the current version and record again |
| A recording of a browser window has nothing of its file picker, or misses its Open confirmation | Before the 20 November 2026 fix: the picker is another window, of another Chrome process | Install the current version (the picker's actions are kept in the recording of the window that opened it) |
| A script opens `https://` for a local server (`127.0.0.1:4200`) | A recording made before the 20 November 2026 fix kept the address without its scheme | Record again, or write `http://` in the script |
| `SNAPSHOT_NOT_FOUND` right after `desktop_cursor_overlay_enable` | The ref came from a snapshot taken before the cursor was turned on (the server now runs in its own session) | Take a new snapshot |
| `desktop_cursor_overlay_enable` answers "require an active session" | An MCP server older than the 20 November 2026 switch | Install the current version and restart the agent |
| An action did nothing (`delivered_unverified`, the page unchanged) | The background way did not reach the element's handler | Do it with the original options: `--headed` click, `double-click`, `mouse-click --xy`, `type`, `press` (section 3) |
| `secret fill` or `set-value` on a password field: `AMBIGUOUS_TARGET`, `identity_not_unique` | Before the 19 November 2026 fix: the field has no name. Now only when the window has two unnamed password fields | Install the current version; with two fields, ask the person which one, or fill it another way the app offers |
| A dropdown does not open (`click` comes back `delivered_unverified`, `expand` "All chain steps exhausted") | Before the 19 November 2026 fix, for dropdowns opened by their container | Install the current version, or use `select` on the combobox |
| Web actions seem to do nothing, or reads show the old page | The browser was not started by `launch` and its window is completely covered, so Chrome paused the page | Open the browser with `launch` (the person closes it first if it is open; its session is kept) |
| A menu group or a date picker does not open on `click` (`delivered_unverified`) | Before the 19 November 2026 native pointer click; or the page is paused (row above) | Install the current version; keep the window visible |
| `AMBIGUOUS_TARGET` on a button, icon or field that repeats | Two elements share the name and the context | Take a snapshot of a smaller part, or act on a unique element next to it |
| A tool returns `STALE_REF` or `AMBIGUOUS_TARGET` | The UI changed since the snapshot | Snapshot again and use the new refs |
| `SNAPSHOT_NOT_FOUND` with a session | The snapshot was taken outside the session | Snapshot inside the session (`AGENT_DESKTOP_SESSION` set) |
| A step returns `delivered_unverified` and `data.surfaces` lists a dialog | The app is waiting on a dialog | Snapshot the dialog and answer it |
| `menu` fails with `details.items` | The item has another name | Use a name from `details.items`, or `menu --find` |
| An agent says the installed agent-record is older than the script | The agent still runs the previous version (its MCP server started before the install) | Restart the agent or open a new conversation |
| Desktop Recorder: "agent-record is missing" | Not installed in `%USERPROFILE%\.desktop-recorder-mcp\bin` | `npx desktop-recorder-mcp` |
| Desktop Recorder: a play failed | The app or page changed | Repair with AI (section 9), then Play again |
| The update failed | npm or the network | The app shows the error; try again later, or `npx desktop-recorder-mcp@<version> update` |
