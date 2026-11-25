# Architecture

Agent Record Adapter puts a layer around the unmodified vercel-labs agent-desktop and agent-browser. Nothing under `vendor/` is edited: both are git submodules on `main`, updated with `scripts/update-upstream.ps1`.

## The pieces

```
agent-record.exe (crates/cli)                 one CLI + one MCP server
 ├─ our commands, in process ────────────────► crates/record (commands) + crates/windows-adapter
 │   record, secret, stroke, menu, click-at,
 │   navigate, type w-<id>, list-displays,
 │   skills (ours + upstream's)
 ├─ browser ... ─────────────────────────────► crates/browser ─► agent-browser (crates/web-engine: unmodified
 │                                                     │            sources, carried inside agent-record.exe)
 │                                                     └─ DevTools proxy ─► dedicated Chrome (behind)
 ├─ every other command ─────────────────────► the engine (crates/engine, carried inside agent-record.exe)
 │                                               upstream's own src/main.rs, unmodified,
 │                                               built with our WindowsAdapter
 └─ mcp ── merges ── engine's `mcp` (desktop_*) + agent-browser's `mcp` (agent_browser_*) + our tools
```

### One package (`packages/desktop-recorder-mcp`)

Everything ships in one npm package, `desktop-recorder-mcp` (the name of this project's earlier releases; `agent-record` is taken on npm). It carries two programs, both built here: `agent-record.exe`, which carries inside the engine and agent-browser (see "One executable" below), and `desktop-recorder.exe`, the recorder app. The installer copies them into `%USERPROFILE%\.desktop-recorder-mcp\bin` and installs nothing from any other package. It removes the `agent-desktop.exe` and `agent-browser.exe` an earlier build installed there (one still running is renamed aside and deleted by a later install).

`bin` can be changed only by the person, SYSTEM and Administrators; everyone else (another account, a sandbox group such as Codex's, which gives itself Modify on `~/.desktop-recorder-mcp`) can read and run it but not replace `agent-record.exe`. The recorder app is built by `prepare.js` with the SHA-256 of the `agent-record.exe` it ships (`AGENT_RECORD_SHA256`) and refuses to run a different one (`apps/recorder/src/tool.rs`, `checked_agent_record`); `AGENT_RECORD_BIN` (development) is not checked.

### One executable (`crates/cli/src/parts.rs`, `crates/engine`, `crates/web-engine`)

agent-record.exe carries the engine and agent-browser as libraries built from their unmodified sources: each crate's build script writes upstream's own `main.rs` into OUT_DIR as a library root (`crates/embed_root.rs`): its `mod x;` lines get absolute `#[path]`s into the submodule, `fn main` is made public and its inner attributes are left out; nothing under vendor/ is changed. agent-browser's own build script runs from its folder (it generates the DevTools protocol types), and its dashboard folder is mirrored where its relative path points (`crates/web-engine/packages`, ignored by git). `crates/web-engine/cli/Cargo.toml` is upstream's manifest under our name, written by `scripts/sync_web_engine.py` (`--check` in CI, `--pin` sets our Cargo.lock to upstream's dependency versions where cargo allows).

Each still runs in a process of its own, as before: agent-record starts itself with `AGENT_RECORD_PART=<engine|browser>@<its pid>`. A process takes the part only when that pid is its parent (a program the engine launches inherits the variable, and agent-record started from it stays agent-record), and re-marks the variable with its own pid so the processes it starts of itself keep the part (the engine's MCP calls; agent-browser's daemon, which agent-browser starts with `AGENT_BROWSER_DAEMON`). `AGENT_RECORD_ENGINE` and `AGENT_RECORD_BROWSER_BIN` / `AGENT_BROWSER_BIN` (when the file exists) still run separate executables, for development. The release profile unwinds on panic (agent-browser's daemon relies on a panicking task not ending the process; upstream agent-desktop aborts) and keeps opt-level 3 for agent-browser, tokio and the image crates (upstream's speed); the rest is "z". The release agent-record.exe is about 30 MB, a little less than the three programs it replaces together. In every agent the MCP server is named `desktop-recorder`, so an install replaces an earlier one in place, keeps its saved passwords (`secrets.json`), the app's settings and the recordings, and removes the name an earlier build used (`agent-record`) and its npm folder (`browser`). The agents: Claude Code, Codex, Gemini CLI, Claude Desktop, Cursor, VS Code and OpenCode (`~/.config/opencode/opencode.json`, servers under `mcp.servers`, skills in `~/.config/opencode/skills`).

The agent-browser built here embeds a placeholder in place of its optional web dashboard (`agent-browser dashboard`): building the dashboard needs pnpm inside the submodule. Nothing in this project uses the dashboard.

### The engine (`crates/engine`)

Its binary is upstream's own `vendor/agent-desktop/src/main.rs`. The trick is one line of `Cargo.toml`:

```toml
agent-desktop-windows = { package = "agent-record-windows", path = "../windows-adapter" }
```

Upstream's code says `agent_desktop_windows::WindowsAdapter::new()`; that name now resolves to our crate, which re-exports everything upstream's Windows crate exports (`pub use upstream_windows::*`) except `WindowsAdapter`, ours. So upstream's whole CLI and MCP server (sessions, traces, leases, cursor overlay, refs, envelopes) run unchanged, with our adapter. Upstream's 224 CLI unit tests pass inside the engine with background mode on and off.

### Our adapter (`crates/windows-adapter`)

`WindowsAdapter { inner: upstream WindowsAdapter }` implements upstream's four traits. `src/forward.rs` is generated by `scripts/gen_forwarding.py` from the trait sources in the submodule: every method forwards to `inner`, except the hooked ones, which call `ours_*` in `src/hooks.rs`. Run it after each upstream update (`--check` fails when out of date): a new upstream method that was not forwarded would fall to its not-supported default.

Hooked methods (each tries our route, then upstream's):

| Method | Our route first |
|---|---|
| `execute_action` | the old fork's router (`actions/background.rs`): `BN_CLICKED` buttons, SWT tabs, PrimeNG dropdowns, Chromium pointer messages, posted clicks, hidden radio/checkbox boxes; `WM_SETTEXT`; list items by name; File Explorer rename; typing into documents and canvases; dropdown select; expand/collapse fallback; retype of the last character; focus guard around all of it; agent actions noted for a running recording |
| `press_key_for_window` | classic menu command, a button with that shortcut, posted keys with Ctrl/Alt/Shift |
| `launch_app` | a folder opens in Explorer; a program with its own profile (`--user-data-dir`, `-data`) or on the work desktop starts as a separate instance with `SW_SHOWNOACTIVATE`; Chromium gets the anti-occlusion switches; a guard watches from before the program starts (a program started from where the person just typed may activate its first window at once) and gives the foreground back |
| `window_op` | minimize/restore without activation |
| `observe_tree` | a minimized window is shown behind first |
| `get_live_element` | incomplete reads retried; evidence for upstream's gates (below) |
| `list_windows` | an empty work desktop is an empty list |

**Evidence instead of gate changes.** The old fork changed upstream's actionability gates. Here the same decisions reach upstream's unchanged gates as evidence from `get_live_element` in background mode: an element with real bounds offers `Click` (posted at its centre); lists, trees, grids and value fields offer `Select`; a tab, radio or checkbox with real bounds is not offscreen; a radio's states are complete from `SelectionItem.IsSelected` (what upstream PR vercel-labs/agent-desktop#679 fixes upstream).

**Reaching the UI Automation element.** Upstream keeps the `uiautomation::UIElement` inside its `UIAElement` crate-private. `src/tree.rs` reads it through a layout mirror of that type, checked once per process (same size and alignment, and the element read back equals the one put in through upstream's own constructor). If upstream changes the type, the check fails, the background routes answer "not supported" and upstream's route runs: an upstream change can cost background routes, never correctness. A public accessor upstream would remove this (docs/upstream-requests.md).

**Twins by place** (`src/twins.rs`). Upstream refuses a ref that was not unique at capture (`identity_not_unique`). The front then finds the element again by its place: in the snapshot's window, the one element with that role (and name) at the saved bounds, ties between nested elements broken by the saved child path. Nothing is guessed: no match or several keep the refusal.

### Our core (`crates/record`)

Platform-neutral: the background switch, the recording format, secrets (store, policy, prompt), and the commands `record`, `secret`, `stroke`, `menu`, `click-at`, `navigate`, `type w-<id>`. It re-exports all of `agent-desktop-core`, so the commands ported from the fork keep their `crate::` paths; `RecordOps` is the trait with the operations upstream's traits do not have (they were new trait methods in the fork).

### Web pages (`crates/browser`)

Any page with DevTools is agent-browser's. The MCP server starts no browser: agent-browser's server lists its tools without one, and the dedicated Chrome and the proxy start at the first web tool call that needs a page (`LazyBrowser`), so an agent that never opens a page never shows a Chrome, and a Chrome the person closes comes back only when a web tool is used again. The proxy keeps one port from start to start (saved in `proxies/<chrome port>.json`), because agent-browser's daemon keeps the port it was first given. The dedicated browser is the first of Chrome, Edge and Brave installed (`AGENT_RECORD_BROWSER=chrome|edge|brave` picks one, `AGENT_RECORD_CHROME` takes any Chromium program), each with a profile of its own (`chrome-profile`, `edge-profile`, `brave-profile`); Edge is started with `--disable-features=msImplicitSignin` in the same switch as the occlusion feature, since a new Edge profile otherwise signs into the person's Windows Microsoft account and syncs their extensions. The person's own browser without DevTools is any Chromium window (`Chrome_WidgetWin_1`, address bar `OmniboxViewViews`, `BraveOmniboxViewViews` in Brave), so the desktop route is the same in Chrome, Edge and Brave; `--daily` without a port finds DevTools in Chrome's, Edge's or Brave's `DevToolsActivePort`. `agent-record browser <args>` starts, when needed, a dedicated Chrome (own profile under `%LOCALAPPDATA%\DesktopRecorder\chrome-profile`, anti-occlusion switches, no startup window, its window sent behind) and a DevTools proxy, then runs agent-browser with `--cdp <proxy>`. The proxy swallows `Page.bringToFront` and `Target.activateTarget`, keeps focus emulation on so typing lands in a page that is behind, and, while a recording runs, writes agent actions (`driver: "browser"`, `by: "agent"`, secrets never written) to the recording's agent inbox. `--daily [port]` targets the person's own Chrome when it has DevTools; without DevTools, its pages are desktop windows (`navigate`, `click`, ... through window messages, `actions/chromium_input.rs`).

The old fork's own DevTools route (`cdp_page`, `devtools`, `cdp_socket`, `cdp_endpoint`, about 1,160 lines) was not brought over: agent-browser replaces it.

## Copies of upstream helpers

Private upstream helpers our code needs are small copies (Apache-2.0), to keep in step on updates:

- `crates/record/src/compat.rs`: state root, home dir, symlink check, unwritable output;
- `crates/record/src/commands/support.rs`: window target, `load_ref_entry`, operation response, pointer policy checks;
- `crates/windows-adapter/src/actions/{chain,disclosure,value_write,select_search}.rs`;
- `crates/windows-adapter/src/input/mouse.rs` (stroke input), `system/mod.rs` (window id, image name);
- `crates/cli/src/catalog.rs` (MCP tool schemas, adapted from upstream's `src/mcp/catalog.rs`).

## Fork changes to upstream's core, and where they live now

The old fork also changed upstream's core code. Each is reproduced outside it:

| Fork change in core | Here |
|---|---|
| Actionability gates (no press action, offscreen tabs/radios, select on lists and value fields) | evidence from `get_live_element` (`hooks.rs::background_evidence`) |
| Radio buttons' states (`SelectionItem.IsSelected`) | same evidence (upstream PR #679) |
| Twins by context and place, unnamed password fields by their secure state | `twins.rs`, used by the front for ref actions and by `secret fill` (`RecordOps::act_by_place`) when upstream refuses a ref as not unique |
| Read a value back after the command's time ran out | `hooks.rs::ours_get_live_element`: right after an action of this process, an expired deadline gets 1.5 s of its own |
| `find --first` returns the first match that has a ref (and an editable combo box's text box) | the front (`run.rs::find_first`) |
| `wait --event window-opened` counts a dialog a background click opened while finishing | the note written before each background click, applied to the wait's first baseline (`ours_capture_signal_baseline`, set by the front with `AGENT_RECORD_WAIT_OPENED`) |
| `wait --text` with `--window-id` | the front (`run.rs::wait_text_in_window`) |
| `list-displays` numbered | the front, also as the MCP tool `desktop_list_displays` |
| `launch` of a program with a profile of its own while another runs; Chromium anti-occlusion switches; folders open in Explorer | `separate_launch.rs` and `hooks.rs::ours_launch_app` |
| `launch --cdp` of a browser with a profile of its own while another runs | the front (`launch_cdp.rs`): port switches as plain arguments, endpoint verified and reported as `data.cdp` |
| Unnamed fields named by the text before them | the front (`labels.rs`): `find --name` in a window falls back to it; actions then go through twins-by-place |
| Value proven by the page through DevTools (date fields) | not needed: pages with DevTools are agent-browser's, which reads the page itself |

## Known limits

- A web control drawn by the page itself (a PrimeNG-like radio or list) reaches the recorder as UI Automation reports it, an unnamed group inside the document, as it did in the old fork, which also used UI Automation only. The recorder names the control the person meant: an actionable ancestor, else the block's own radio button, checkbox or switch, else the single one beside it in its wrapper (`record_promote.rs::is_web_block`, `record_element.rs::web_control`). A block with none of these stays a group.
- A document an app draws itself (WordCraft) is only read through UI Automation: a script writes it by typing what its text lacks at its end (`window.write` in the script library), and such an app takes typed text only while its window is active.

- `stroke` does not show the drawn cursor while it moves (upstream's cursor hold is private); the stroke itself is the same.
- `click-at` on a classic Win32 push button in a window behind does not press it (a native button needs the mouse capture, which a background window cannot take; the old fork had the same limit): `click` on its ref presses it with `BN_CLICKED`. `click-at` is for drawn content (a grid, a canvas), where it works.
- Two upstream issues seen while testing, not ours: `find` on GridCraft's tree can answer "RefEntry contains an invalid role"; on this machine upstream refuses the default state folder `~/.agent-desktop` ("private file DACL grants an untrusted principal"), so the checks use their own `AGENT_DESKTOP_HOME`.

## Any language

Nothing compares the words of an app's or a browser's interface, so every route works in any language of Windows, the app or the browser:

- A browser page's address: the page's own document, else the address bar found by its class (`OmniboxViewViews`, Brave's `BraveOmniboxViewViews`), never by its name ("Address and search bar", "Barra de direcciones y de búsqueda", "アドレス検索バー"). `secret fill` reads it through `RecordOps::browser_address`.
- A file's name in File Explorer: the cell's property id `System.ItemNameDisplay`, not the column's name.
- A shortcut a menu or a button shows ("Strg+S", "Mayús+Ctrl+S", "Save (Ctrl+S)"): each key known by the names Windows gives it in every keyboard layout installed (`GetKeyNameTextW`, `actions/key_names.rs`) and the English ones. A button is found by the keys its accelerator or its name shows, never by the words of its name.
- Which recorded click asks before it runs: the last click on a button by default, by its place and role, and whatever the person marks in the app (`ask-before.json`).

## Keys and text in a window behind

A key or typed text goes to a window behind as the messages a real key sends, never through the person's keyboard (`actions/posted_keys.rs`):

- The window is first told it is active and focused, with the messages Windows sends a window that comes to the front (`WM_NCACTIVATE` with -1 so its title bar is not repainted, `WM_ACTIVATE`, `WM_SETFOCUS`), and told it is inactive again afterwards (not that it lost the focus: WPF then forgets its focused field). Frameworks that take keys only while active and focused (winit, so WordCraft) take them; a WinForms or classic window gives the focus back to its field. A window the person is using is never told anything, and Windows' foreground never moves.
- A key goes as a real one does, down and up: the app's own message loop makes the character (`TranslateMessage`; winit, WPF, WinForms and classic apps all do). Typed text into a field that can be read is measured on its first character (sent with its character too): typed twice, one is erased and the rest go as keys only; typed once (an editor whose input method does not translate posted keys), the rest go with their characters.
- `type w-<id>` in a window that lost the activation finds the field Windows kept for it (its thread's focus), as UI Automation reports none there.
- `type <ref>` into a text field or document that takes the focus (also one an app draws itself: egui, DeckCraft's notes) is offered to upstream's gate as `TypeText`: the field gets the focus through accessibility with its window awake, the window is woken again after it (the guard that gives the person their window back deactivated it, and a WinUI field then dropped the keys), and the text goes as keys. When the field shows none of it, the command fails instead of reporting success.
- Every key (`press --window-id`) runs guarded: a key that opens a window of its own (Ctrl+S, a Save dialog) gives the person their window back.
- A button's keys are read from its name even when it shows several ("Save (Ctrl+S, Alt+2)").
- A click on a toggle button or switch that leaves its state unchanged (WinUI: Paint's tools ignore Invoke) toggles it through its toggle pattern, and the new state is checked.
- `type <ref>` into a control with a window of its own (a classic or WinForms field) that does not hold its window's focus (a dialog that keeps it on another control: Character Map on its grid) posts the keys to that control's window.
- A busy app (just opened) handles a key late: typed text is read for its first character only once the app has taken its queue (`WaitForInputIdle`), so a late character is not taken for none and typed again.
- A window that hosts another process's window (a packaged UWP app: its frame belongs to ApplicationFrameHost, its content to the app) gets its keys on the hosted window, whose thread reads them (`focused_in` → `hosted_window`).
- A symbol typed with Shift ("=" is Shift+0 on a Spanish keyboard) goes as a key only when Shift can be put in the app's key state; an app in a sandbox (packaged apps) refuses to share it, and the symbol goes as its character, never as the unshifted key ("0"). The same for `press` of such a symbol.
- `type w-<id>` in a window whose accessibility tree has no field and no document (Java Swing, a game, a canvas) sends the text as keys where the app's own focus is; nothing can read it back.
- In a Chromium window (a browser, an Electron app) the text goes through its own route (`chromium_input`); when the page does not show its text to accessibility (VS Code's editor), the text is reported sent, never typed again or followed by a click into the page. A document's text is read with its text pattern too: a web page's value is its address.
- A launch is guarded on what it shows, not only on its process: a window that was not open before the launch counts as the launched program's (a packaged app is started by the system's broker; a single-instance app hands over to the running one) until the person clicks or presses a key. A launch that found no window yet is watched for one up to 10 s. On the work desktop it is watched on the desktop on screen: a packaged app without an app alias (Calculator) opens there, behind the person's windows, never on the work desktop.
- The focus guard also counts a window that hosts one of the target's windows (a UWP frame) as the target's: a click on Calculator's buttons brought its frame forward.
- `query_app_inventory` hook: the app list has only the UWP frame's host, while the window list names the app; an exact query for an app the window list names gets it from its windows, so `press --window-id` resolves a UWP window.

Limits: a click or a stroke on Paint's canvas takes only real pointer input (`WM_POINTER`), so placing a text box or drawing there needs `--headed`. Java AWT (Swing) moves every mouse message to the window under the real cursor, so a `click-at` in a Swing window needs `--headed`; its buttons work by keyboard (Tab, Space). A packaged app's own shortcuts that it handles only while active (Calculator's Escape) do nothing behind; its digits and operators do. An app that builds its editor or a new window only while it is really active (WordCraft opened behind and never active: its document gets no focus by accessibility or by a background click, and a new window stays empty) needs the window in front once (`--headed`); once it has been active, it takes keys and text behind other windows.

### Checking a change: one app per UI technology

A change to these routes is checked on every Windows UI technology at once, never on one app: `tests/background/scripts/tech-suite.sh` runs, on the hidden work desktop and behind the person's windows, `tech-matrix-test.sh` (Win32 classic: Character Map; WinForms and WPF: test programs built by `apps/build-test-apps.ps1`; Java Swing, with no accessibility tree; Electron: VS Code with a test profile; UWP: Calculator), WordCraft and DeckCraft (winit/egui), Paint (WinUI 3), and the Chromium page tests. No decision in the code looks at an app's name: each one keys on how a technology behaves (a frame hosting another process, a field with its own window, a sandboxed key state, a page that hides its text).


## Foreground, interaction leases and DevTools navigation

- **When a program may take the foreground.** Windows lets any program take it once the person has not used the keyboard or mouse for the foreground lock timeout (200 s by default), and Chromium then brings its window forward about 30 ms after a click or a value arrives (it injects a key to get the right, which also lifts `LockSetForegroundWindow`). The guard gives the window back at once; while input is still being sent it waits for it to arrive (deactivating the browser halfway loses it), so a flash of a few tens of milliseconds remains possible on an unattended PC. A guard watches from before a launched program starts, and the foreground is locked (`behind::foreground_lock`) while a guard watches and while a window is minimized, restored or shown behind from minimized: a window restored without a guard stayed in front for seconds.
- **One interaction lease per work desktop.** Upstream serializes every command with one lock for the user (`%ProgramData%\agent-desktop\<SID>\interaction.lock`). Commands on a work desktop take that desktop's own lock (`system/desktop_lease.rs`, `%LOCALAPPDATA%\DesktopRecorder\locks`), so a bot there does not wait for the agents on the person's desktop, nor they for it; on the person's desktop the lock is upstream's.
- **navigate through DevTools.** A browser window whose browser has a DevTools endpoint (found among its process's loopback listeners) is navigated through it (`actions/devtools_navigate.rs`, from the old fork): `--in-app` changes the address as the app's own links do, and a full load the app sends home is followed by one in-app try. Without an endpoint the address bar is used; `--in-app` then needs one.
- **The drawn cursor over MCP.** The cursor belongs to a session: an MCP server started without `--session` starts one the first time `desktop_cursor_overlay_enable` (or `_disable`) is called, and runs every later call in it.

## Web form controls and toolkits, by how they behave

- **A web date field** (`<input type="date">`, `"month"`) in Chromium is spin-button segments that ignore a value written through accessibility. `set-value` with the field's own format (`YYYY-MM-DD`) clicks the first segment and types the digits behind the person's windows; each segment is known by its range (day 1..31, month 1..12, year beyond), so any language and order works (`actions/date_segments.rs`). Chromium may leave the field's own value unreported (on a work desktop it never reports it), so a read of such a field (`get`, the gates' live read) is the date its segments show.
- **A web page's own list** (`<select>`) in Chromium is a combo box with no options until its list opens, and that list is drawn over the person's windows. `select` focuses it and types the option's text, as a person does in a closed list; Chromium picks it and tells the page (`change`). An option whose text only holds the value ("+34" in "ES +34") is reached with Home and Down. A value written instead was ignored by the page and reported as done (`actions/web_select.rs`).
- **winit apps** (Rust: egui, WordCraft, GridCraft), known by the `Winit Thread Event Target` window on their thread, make no character from a posted key: they read the character message that follows a key as its text. `press` and `type` send a printable key with its character there (winit joins them into one event, so nothing is typed twice); a grid that starts editing on typed text now starts. That event window is also never the window `launch` answers with: a titled window of the program is waited for.
- **A read of a ref with a twin** (`get` on an unnamed field that has another one just like it) is made on the element found by its place, as the actions already were.

## Checking scripts without running them (`check-scripts`)

`agent-record check-scripts <files or folders>` tells, after an update, which scripts no longer fit the programs installed here, and where. It runs nothing in any app:

- **Calls.** It finds every call a script makes (JavaScript/TypeScript, Python, sh, PowerShell, cmd): `run(...)` of a desktop-script.mjs copy, a program run directly (`execFileSync(bin, [...])`, `subprocess.run([exe, ...])`), a function of the script that passes its arguments on to one of those (`native(window, ...args)`), and argument lists built in parts (`['find', ...]` and their `push`es).
- **The programs' own parsers.** Each call is parsed by the installed program, never by a copy of its rules: agent-desktop's commands by the engine with `--help` added (its parser refuses an unknown command, option or value first, and with `--help` nothing runs; the usage line then gives the required arguments, and a `[VALUE]` that may come from `--stdin` is required without it); our own commands by their clap definition; agent-browser's by its help (it ignores unknown options, so those are warnings). A value known only at run time gets placeholders of several kinds; one still refused is reported as not checked, never as fine.
- **Programs and copies.** A program path written in a script must exist and be the right program (`click-at` sent to agent-desktop.exe is an error). Each desktop-script.mjs copy runs only its own lookup (`agentDesktop()`, which asks a program for its help) to see which program it finds here. A problem inside a copy is an error for a script only when the script uses that function or a method calling it; otherwise a warning.
- **Passwords.** Names filled by a script and not saved are warnings (the first run asks for them); values are never read.

It cannot know whether the app or page still has what a script uses, or what the script does once it runs: running the script answers that.

Measured (24 Nov 2026, `tests/background/scripts/check-scripts-battery.mjs`): 41 runs of real recorded scripts (test pages and WordCraft, on the hidden desktop) as found, with the current helper, and with breaks an update could bring (a command or option renamed or gone, an invalid value, a required argument dropped, the program moved, an old helper copy, a page changed). With the check frozen before any run: 34 of 37 conclusive cases right (91.9%), no false alarm, the 3 misses all a value that may come from `--stdin`; the rule for those, added after, gets 37 of 37 on the same cases (4 cases were inconclusive: the run stopped before the broken call). "Compatible" does not mean "works": 7 of 7 scripts with the current helper failed later for other reasons, which only running shows.

## A script's steps as JSON (`script-steps`)

`agent-record script-steps <file>` gives the steps of a script made from a recording as JSON, so a bot runner (Aprende) repeats them without AI. It runs nothing. It reuses the check-scripts tokens, with a small JavaScript parser (`crates/cli/src/script_steps/parse.rs`) for statements, calls, objects, arrow functions and `await`.

- A JavaScript script that uses desktop-script.mjs is read as the helper's calls: `task`'s title and values (`params`, with `secret: true` for a value passed to `fillSecret`), the windows it opens or finds, and each `step(n, title, ...)` with its actions (`open_browser`, `open_app`, `find_window`, `click`, `set_value`, `clear`, `select`, `check`, `uncheck`, `paste`, `fill_secret`, `menu`, `shortcut`, `choose_menu_item`, `write`, `click_at`, `click_at_offset`, `read`, `wait_text`, `wait_new`, `wait`, `screenshot`, `confirm`, `close`, `command`). The generated `windows["w-..."] = await openBrowser(...)` and `win(id, parts, app)` name each window by its recorded id; a window the script only finds gets a `find_window` action where it is first used.
- Every text an action or a window gives is a value: `{"text"}`, `{"param"}`, `{"template": "Create the account {username}?"}` (literal braces doubled) or `{"env"}`; numbers and yes/no are written as they are. Descriptions are plain strings: the task's `title`, a step's `title` (`{name}` where a value goes) and a param's `ask`, `example`, `options` and `default`. Element queries and options keep the helper's keys in snake_case (`native_id`, `in_document`, `timeout_ms`).
- Any other script (Python, PowerShell, a shell, JavaScript without the helper) gives each agent-record call written in it as a step with one `command` action.
- What cannot be known without running the script (a condition, a loop, error handling, a value computed at run time, a call into the script's own code) is listed under `unsupported` with its line, code, reason and step, never guessed; a value set by such code makes what uses it unsupported too.

## Recording details

- An agent's own actions while a recording runs are recorded too (`by: "agent"`, `input/record/record_agent.rs`), so a task an agent did becomes a script like a person's: element actions, `click-at` (with the point it chose), `press --window-id`, a tree item opened (`expand: true`, replayed with `expand`, since a click only selects it), a menu command (`menu_path`, replayed with `window.menu`) and `close-window` (a `close` event). In the steps, a click with no element of its own is measured from the nearest panel around it that has a name and waits for that panel (a results grid's cell inside its results tab), a click on a container that gave the focus is kept when command keys follow it in that window (Ctrl+A, Ctrl+C in a grid), and Ctrl+A outside a field is a command. A step's id goes with its role (a dialog's Save button and the first file of its list are both 1). An app installed as a Windows package (Notepad) is opened by its name, and `openApp` takes the window the launch opened, or the one window of an app that keeps one; it never writes into another window that holds a document, or one with unsaved changes. Each recording's buttons in the bar are named after it (`Play: Recording 2026-11-25 18.17.28`), so an agent or a screen reader tells them apart.
- A paste into a field is followed like typing: the text it left is recorded (a `type` event) at the next flush, so a script writes that text instead of pasting whatever the clipboard holds then. Keys that only move the caret or select in a field (Ctrl+End, Shift+Home) are left out of the script: the value that follows holds their effect.
- The Desktop Recorder bar is topmost and never takes the focus (`WS_EX_NOACTIVATE`); the launch guard's sweep to the bottom skips topmost windows, so the bar keeps its place. A recording started from the bar leaves the bar's own clicks out (`AGENT_RECORD_IGNORE_PID`, checked before any UI Automation read). `AGENT_RECORD_INCLUDE_INJECTED=1` makes the bar record synthesized input too (accessibility tools, tests).
- egui drops a press whose position it was never told: a pointer that jumps onto the bar and presses at once, or comes back to the very pixel it left (winit reports no move then). The bar's window procedure sends moves ending on the press's point before each press, and a press and release that arrive in one frame are spread over two (`main.rs::steady_press`, `view.rs::steady_clicks`).

## Results (24 Nov 2026)

- Unit tests: core 40, Windows layer 45 (and 1 ignored), front 14, browser 35, recorder app 14, installer 19: all pass. Clippy and rustfmt clean on our crates and the app.
- Upstream's CLI unit tests in the engine: 224 pass with background on and off (3 path-dependent contract tests and the shell-surface tests that press real Win+A skipped).
- Live, with the person's foreground unchanged throughout: see tests/background/README.md.
