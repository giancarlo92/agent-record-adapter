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
| Installer (`prepare.js`, then install, status, uninstall in a test home) | three programs and seven skills installed; MCP set up in Claude Code, Codex, Gemini, Claude Desktop, Cursor, VS Code, Windsurf; the installed server answers as an agent starts it (84 tools, displays, our and official skills); uninstall removes all but recordings |
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
| agent-browser 0.44.0 from npm (`npm install --prefix <home>/.agent-record-adapter/browser --no-save agent-browser@0.44.0`, as the installer does) | npm's script policy skipped its postinstall, the executable came with its platform package; the front found it there (`browser --version`), and `browser-record-test.sh` with it: PASS |
| `stroke --headed` in Paint | the zigzag drawn on the canvas with the real pointer; without `--headed`, refused (`POLICY_DENIED`) |
| First-use password window (`secret fill` with no secret saved) | the window asked, the test password typed with the real keyboard was saved (bound to chrome.exe), written into an unnamed field (by place) and the page got all 12 characters; Cancel writes nothing; the value never appears in any output |
| Upstream's physical typing (`--headed type`) | works in Notepad (`SendInput.type_text`); WordCraft refuses it ("lost focus before physical input"), while our background route types into WordCraft |


## Scripts and pages

See the old fork's FORK.md for what each page imitates. `sgh-like.html`: menu groups, row icons, unlabelled fields, PrimeNG-like radios and dropdowns; `spa.html`: a single-page app; `file-pick*.html`: file inputs; `studio.html`: an upload dialog opened from JavaScript; `tab-a/b.html`: two tabs with the same title; `toggle-test.html`: checkboxes that ignore Space; `slow-select.html`: a list that shows the previous option for a moment; `print.html`: print preview.
