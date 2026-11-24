<!-- Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork. -->
# Background-mode and recording checks (Windows)

Live checks of the background layer and the recorder, on local pages, in test Chrome windows with profiles of their own. They never use the person's mouse or keyboard and never bring a window forward.

## Run

From Git Bash, with a build of the workspace (`cargo build` or `cargo build --release`):

```bash
bash tests/background/setup.sh
AR="$PWD/target/debug/agent-record.exe"
bash tests/background/scripts/sgh-like-test.sh "$AR" native
bash tests/background/scripts/agent-record-test.sh "$AR"
bash tests/background/scripts/nav-test.sh "$AR" native
bash tests/background/scripts/file-pick-test.sh "$AR"
bash tests/background/scripts/studio-picker-test.sh "$AR" plain
bash tests/background/scripts/secret-unnamed-test.sh "$AR"
bash tests/background/scripts/browser-record-test.sh "$AR" <agent-browser.exe> "<Aprende Replica>/app/testdata/forms/signup-v1.html"
```

Desktop apps on a hidden work desktop (nothing shows on the screen): set `AGENT_DESKTOP_WORK_DESKTOP=artest` and drive WordCraft, GridCraft or `apps/classic-menu.ps1` (`launch powershell.exe --arg=-File --arg=<path>`) with `agent-record`.

The `cdp` modes of the old fork drove pages through its own DevTools route, which this project replaced with agent-browser (`agent-record browser ...`, see docs/architecture.md); they are kept for reference and are not expected to pass.

The scripts work in `%USERPROFILE%\AprendeReplica-e2e\enter-test` (pages, snapshots, recordings, Chrome profiles `profile-*`). Each closes only the Chrome it started. State goes to `%LOCALAPPDATA%\AgentRecordAdapter\tmp\js-state`, never the agents' own state folder.

## Results (24 Nov 2026, debug build)

| Check | Result |
|---|---|
| `sgh-like-test.sh native` | 12 / 12 PASS (menu group, twins by place in rows, unlabelled fields, PrimeNG radios) |
| `agent-record-test.sh` | 4 agent events with `by: "agent"` (click, type, radio click, `press return --window-id`) |
| `nav-test.sh native` | PASS: the address bar's inline completion did not replace the asked address |
| `file-pick-test.sh` | PASS: Chrome's file picker opened by a background click, path written, dialog's Open clicked, page shows `picked: sample.csv`; all three recorded |
| `studio-picker-test.sh plain` | PASS: picker opened by a background click, foreground unchanged |
| `secret-unnamed-test.sh` | PASS: `secret fill` on a password field with no name, label or id (found by place), page got all 12 characters, value never printed |
| `browser-record-test.sh` | PASS: agent-browser's fill, radio click and select recorded with `driver: "browser"`, `by: "agent"` |
| File picker by hand: `wait --event window-opened` after a background click | finds the dialog (upstream alone times out) |
| File picker by hand: `find --name "File name" --first` | the text box with a ref (upstream alone: a label without one) |
| Work desktop (`AGENT_DESKTOP_WORK_DESKTOP=artest`), WordCraft | launch, new document, `type w-<id>`, `press ctrl+a` (posted), `press ctrl+b` (its Bold button): text in the document |
| Work desktop, GridCraft | Name Box `set-value`, F2, `type w-<id>` and posted digits: cell B2 = `42424242`; `click-at` on the drawn grid moves the selection |
| Work desktop, `apps/classic-menu.ps1` (WinForms with a Win32 menu) | `menu File Save`, `menu --find "As CSV"`, `press ctrl+s` as the menu command, `WM_SETTEXT`, `BN_CLICKED`: all PASS |
| agent-browser through the proxy (headless and headed behind) | open, snapshot, fill, select, check, type: values in the page, person's window kept in front |

| Minimize and restore (Chrome behind) | no activation: the window minimizes and comes back, the person's window stays in front (upstream alone brought it forward) |
| `launch <folder>` | opens in File Explorer behind the person's window; on the work desktop with Explorer's own process (`/separate`) |
| `record --video` | H.264 MP4 of the window (1296x988, 14.7 s, 177 frames) beside the events |
| Indicator | shown while recording, gone after, absent with `--no-indicator`; its Stop button ends the recording (`stop_reason: indicator`) |
| Recording to script | `desktop-recorder --make-script` on an agent recording, then `node script.mjs --apellido-materno Rojas`: page opened, clicked, field written with the new value, radio chosen, in the background |
| Desktop Recorder app (work desktop) | Record starts a recording, agent actions captured, Stop writes `script.mjs`; Play replays it (test app shows the result); Settings and Recordings open; Repair copies the repair prompt (clipboard restored) |
| Installer (`prepare.js`, `npm pack`, then `npx ./desktop-recorder-mcp-1.0.0.tgz install --home <test home>`) | one package of 31 files carrying four programs (agent-record, the engine, agent-browser 0.44.0 built from the submodule, Desktop Recorder) and seven skills; nothing installed from npm; MCP server `desktop-recorder` set up in Claude Code, Codex and OpenCode (`mcp.servers`, `type: local`); started from OpenCode's entry it serves 117 tools (84 `desktop_*`, 33 `agent_browser_*`) and `agent_browser_open` read a local test page |
| Real install over the earlier desktop-recorder-mcp 0.2.0 (Claude Code, Codex, Claude Desktop), after `npm uninstall -g agent-desktop` | each agent has one `desktop-recorder` server running `agent-record.exe mcp` (Claude Code: Connected); the 8 saved password names, the app's settings and the recordings kept; the earlier skills replaced; the running old engine set aside for the open agents |
| `browser --daily <port>` | attaches to a running Chrome by its DevTools port and reads its page; without a port, says the desktop route is used |
| A real agent (Claude Code `-p`, only our MCP server) | listed the displays, read our recording skill, reported the recording status |

### With the person's real mouse and keyboard (24 Nov 2026, with the user's permission)

Simulated input (computer-use, `--headed`) is marked as injected by Windows, so these recordings use `--include-injected`.

| Check | Result |
|---|---|
| A person's recording in WordCraft (`record --app wordcraft.exe --include-injected`, with video and the indicator) | every real action recorded as the person's: clicks, Ctrl+V, Ctrl+A, Ctrl+B and the ribbon's **Italic** button by name; MP4 of the screen |
| A person's physical clicks on a web page (`mouse-click --headed` in Chrome, no DevTools) | recorded with the page address and the control meant, not the unnamed block UI Automation reports: textfield "Apellido Paterno", option "Personal…", radiobutton "Pago directo" |
| Desktop Recorder app by hand (real clicks on Record and Stop) | recording started and stopped, folder and `script.mjs` written. The app leaves injected input out, as designed (`AGENT_RECORD_INCLUDE_INJECTED=1` keeps it, for these tests and for accessibility tools) |
| Desktop Recorder app with content (`AGENT_RECORD_INCLUDE_INJECTED=1`): Record, then in WordCraft a click, Ctrl+End, Enter, typed words and a pasted name, then Stop | recorded: the click, Ctrl+End, Enter, the typed text and the text the paste left (`type` events with the document's value); the bar's own clicks left out; `script.mjs` of 2 steps (open WordCraft, write the document); the bar kept its top place and never took the focus |
| Play from the bar after deleting that line by hand | the script typed the missing line back at the document's end (`window.write`, WordCraft in front); "Done" on the bar, focus left on WordCraft. One run of seven doubled one letter ("Roojas"); the other six, and five `type w-<id>` runs in a row, were exact |
| Clicks on the bar that jump onto it and press at once (simulated input, touch, a pen), right after typing in WordCraft | Settings opened, closed and opened again (3 of 3); Stop on its first click. Before the fix such a click was lost whenever the pointer came back to the pixel it had left |
| Repair's double click | Claude came to the front with `claude://code/new` (nothing sent); its window shows no accessibility text, so the draft itself was not read |
| A dedicated Chrome already running on another port (a headless test one on 9433) | before: a new start on 9333 handed over to it and never answered, so the web tools were missing; now its saved port is used and the 33 `agent_browser_*` tools are there |
| No Chrome until a web tool is used (`agent-record mcp`, three agents starting it in a row, then `agent_browser_open` on sgh-like.html, then the Chrome closed by hand and used again) | 0 dedicated Chrome processes after starts, tool lists, a desktop tool and `agent_browser_flow_list`; Chrome started by the first `agent_browser_open` (page read, foreground unchanged); after a close by hand, still 0 while agents start, and back on the same proxy port at the next web tool; `claude mcp get` on the installed server: Connected, no Chrome |
| Chrome, Edge and Brave, the person's own browser without DevTools (`BROWSER_EXE=<program>` for sgh-like-test.sh, nav-test.sh and secret-unnamed-test.sh) | page checks 12/12 in each; `navigate` by the address bar in each (Brave's is `BraveOmniboxViewViews`, now found); `secret fill` of an unnamed password field in each; every test browser closed normally (exit type Normal), foreground unchanged. Edge's side pane holds an unnamed text field of its own, so the field check looks only inside the page. A new Edge profile signs into the Windows Microsoft account by itself: the tests start Edge with `--disable-features=msImplicitSignin` |
| Dedicated browser on Edge and Brave (`AGENT_RECORD_BROWSER=edge|brave`, own state folder and port) | `browser open`, `get title`, `snapshot -i` (30 refs) and a click on each, foreground unchanged; profiles `edge-profile` and `brave-profile`; the dedicated Edge stays signed out (0 accounts) |
| Any language: `secret-site-test.sh` (a secret bound to 127.0.0.1 and one bound to another site, page served on 127.0.0.1) with Chrome in Spanish, German and Japanese (`--lang=`), Edge and Brave in Spanish | the right secret filled in each (the address bar named "Barra de direcciones y de búsqueda", "Adress- und Suchleiste", "アドレス検索バー", "Dirección y barra de búsqueda"), the other one refused. The earlier build, in Spanish Chrome, could not read the address and refused both |
| WordCraft on the work desktop: `press ctrl+s` and `press ctrl+b` after typing | Ctrl+S ran its "Save (Ctrl+S)" button (found by the keys in its name, `Invoke.ShortcutButton`) and the document was saved; Ctrl+B was posted as keys and did not make the text bold (`wordcraft-cli inspect`: bold false), since WordCraft names its Bold button only in English |
| `stroke --headed` in Paint | the zigzag drawn on the canvas with the real pointer; without `--headed`, refused (`POLICY_DENIED`) |
| First-use password window (`secret fill` with no secret saved) | the window asked, the test password typed with the real keyboard was saved (bound to chrome.exe), written into an unnamed field (by place) and the page got all 12 characters; Cancel writes nothing; the value never appears in any output |
| Upstream's physical typing (`--headed type`) | works in Notepad (`SendInput.type_text`); WordCraft refuses it ("lost focus before physical input"), while our background route types into WordCraft |


## Scripts and pages

See the old fork's FORK.md for what each page imitates. `sgh-like.html`: menu groups, row icons, unlabelled fields, PrimeNG-like radios and dropdowns; `spa.html`: a single-page app; `file-pick*.html`: file inputs; `studio.html`: an upload dialog opened from JavaScript; `tab-a/b.html`: two tabs with the same title; `toggle-test.html`: checkboxes that ignore Space; `slow-select.html`: a list that shows the previous option for a moment; `print.html`: print preview.
