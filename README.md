# Agent Record Adapter

One product for AI agents and people on Windows: desktop apps, web pages and recording, everything in the background. It is made of the two vercel-labs projects, used **exactly as published**, plus a layer of our own:

- [agent-desktop](https://github.com/vercel-labs/agent-desktop) for desktop apps (`vendor/agent-desktop`, git submodule on `main`);
- [agent-browser](https://github.com/vercel-labs/agent-browser) for web pages (`vendor/agent-browser`, git submodule on `main`);
- our layer: background-first routes, the recorder, secrets, extra commands, one CLI and one MCP server, the Desktop Recorder app, the installer and the skills.

There is no fork. Updating to a new upstream version is `scripts/update-upstream.ps1`.

## What is in it

| Piece | Where |
|---|---|
| Background mode: clicks, values, keys, typing into canvases, launches and windows that never take the person's mouse, keyboard or foreground; upstream's own route as fallback; `AGENT_DESKTOP_BACKGROUND=0` turns it off | `crates/windows-adapter` |
| Twins told apart by their place (unnamed icons in rows, unlabelled fields) | `crates/windows-adapter/src/twins.rs` |
| Hidden work desktop (`AGENT_DESKTOP_WORK_DESKTOP=<name>`) | `crates/windows-adapter/src/system/work_desktop.rs` |
| Recorder (a person's input through hooks + UI Automation, an agent's actions, MP4 video, indicator) | `crates/windows-adapter/src/input/record`, `crates/record` |
| Secrets in the system's credential store, filled by name | `crates/record/src/commands/secret*.rs` |
| `menu`, `click-at`, `navigate`, `stroke`, `type w-<id>`, `press --window-id` in the background | `crates/record`, `crates/windows-adapter` |
| Web pages through agent-browser, in a dedicated Chrome kept behind the person's windows, through a DevTools proxy | `crates/browser` |
| The engine: upstream agent-desktop's own CLI and MCP server, compiled unmodified with our adapter | `crates/engine` |
| `agent-record`: one CLI and one MCP server for all of it | `crates/cli` |
| Desktop Recorder app (record, play, repair) | `apps/recorder` |
| Installer for Claude Code, Codex, Gemini, Claude Desktop, Cursor, VS Code, OpenCode | `packages/desktop-recorder-mcp` |
| Skills: background, recording, recording-to-script, script-repair | `skills/` |
| Live checks | `tests/background` |

How the pieces fit: [docs/architecture.md](docs/architecture.md).

## Use

```powershell
agent-record list-windows                     # any agent-desktop command
agent-record snapshot --app notepad.exe
agent-record click @s1abc:e4                  # background route first
agent-record record start --app notepad.exe   # record a person or an agent
agent-record browser open https://example.com # any agent-browser command
agent-record browser snapshot -i
agent-record mcp                              # one MCP server: desktop_* and agent_browser_* tools
```

## Build

```powershell
git submodule update --init
cargo build --release            # target\release\agent-record.exe and agent-desktop.exe (the engine)
cargo test -p agent-record-core -p agent-record-windows -p agent-record -p agent-record-browser
```

agent-browser is built from the unmodified submodule and shipped inside the installer package, next to `agent-record.exe`: `cargo build --release --manifest-path vendor/agent-browser/cli/Cargo.toml --target-dir target/agent-browser`. The package is `packages/desktop-recorder-mcp` (`npm run prepare-vendor` there copies the four programs in); it installs nothing from npm, and in every agent its MCP server is named `desktop-recorder`.

## License

Apache-2.0. Code from agent-desktop and agent-browser is credited in [NOTICE](NOTICE).
