# desktop-recorder-mcp

**Agent Record** for your AI agents, in one package: the official vercel-labs agent-desktop and agent-browser, built from their unmodified sources and shipped inside it, plus a background-only layer, record, saved passwords and Desktop Recorder. Nothing else is downloaded or installed, and it replaces an earlier desktop-recorder-mcp install in place (saved passwords, settings and recordings are kept).

One command to give your AI agents a desktop they can see, use and **record**: Claude Code, Codex, Gemini CLI, Cursor, VS Code, OpenCode and Claude Desktop.

```bash
npx desktop-recorder-mcp
```

It lists the agents on your computer; tick the ones you want and press Enter. For each one it adds the MCP server (named `desktop-recorder`) and, where the agent reads skills from a folder (Claude Code, Codex, OpenCode), these skills. The other agents read the same skills through the MCP server.

| Skill | What it teaches your agent |
|---|---|
| `agent-desktop`, `agent-desktop-windows`, `agent-browser` | The official skills: seeing and using desktop apps, and web pages through agent-browser |
| `agent-record-background` | Doing all of it in the background, the commands agent-record adds, and your own browser where you are already signed in |
| `desktop-recording` | Recording a task you do in a window, an app or a screen, with or without video |
| `recording-to-script` | Turning a recording into a script that repeats the task without AI, with the values that change as options and passwords filled by name. JavaScript by default; ask for Python, Go or any other language and it writes it in that one |
| `script-repair` | Fixing that script when the app or the page changed: it finds the step that no longer matches and changes only that one |

Some agents come in two forms that share one setup, and some do not:

| You use | Tick | Then |
|---|---|---|
| Claude Code in a terminal, or the **Code** tab of the Claude app | Claude Code | Start a new session |
| The **Chat** of the Claude app | Claude Desktop (a setup of its own) | Quit and reopen the app |
| The `codex` command, or the **Codex** app | Codex (both read `~/.codex/config.toml`) | Restart the Codex app |

The chat of the ChatGPT app cannot run a tool on your computer this way: it connects only to MCP servers on the internet.

Then open a new conversation and ask, in your own words:

- "What displays do I have?"
- "Record the Excel window with video and save it in D:\Videos\task.mp4", then "I'm done".
- "Turn that recording into a script; the customer and the amount change every time."
- Later, when the script stops with "Step 4 (...) failed": "The script failed at step 4, repair it."

A script runs on its own, without AI and in the background, while you keep working: `node invoice.mjs --customer "Lucia" --amount 150` (`--help` lists what it asks for).

While it records, a small red indicator at the bottom of the screen shows the time and a Stop button.

## Desktop Recorder: record a task and play it again, without AI

The installer also adds **Desktop Recorder**, a small floating bar (Start menu > Desktop Recorder, or `npx desktop-recorder-mcp app`):

- the round button records what you do in a screen or a window, with or without video, until you press it again;
- the gear chooses what to record, video or not, and the folder;
- the list shows your recordings: Play repeats one in the background while you keep working, the folder button opens it, and the repair button copies a ready prompt for your AI agent to repair the script;
- Settings shows the version: when a newer one is published, a click on **Update** installs it and opens the app again.

Stopping writes the script that repeats the task (`script.mjs`), without AI. `node script.mjs --help` lists the values you can change each time; with none it repeats the task as recorded.

The complete guide, step by step (the MCP tools, recording, demo videos made by an agent, checking a video, the app, known problems): [GUIDE.md](https://github.com/giancarlo92/agent-record-adapter/blob/main/packages/desktop-recorder-mcp/GUIDE.md).

## Passwords

The tool never records a password or writes one into a script. A script or an agent asks for a password by name (`secret fill <field> bank-login`). The first time, a small window asks you to type it once; it is saved in your system's own secret store (Windows Credential Manager, the macOS Keychain, the Secret Service on Linux), bound to that app and site, and the task goes on. From then on it is filled on its own. Agents never see it, and it is refused anywhere but a password field of that app and site.

To save one ahead of time, or change it:

```bash
agent-record secret set bank-login --app chrome.exe --site login.mybank.com
agent-record secret delete bank-login
```

## Other commands

```bash
npx desktop-recorder-mcp status                     # what is installed, and where
npx desktop-recorder-mcp --agents claude-code,codex # set up these agents without asking
npx desktop-recorder-mcp --dry-run                  # say what would change
npx desktop-recorder-mcp uninstall                  # remove it from the agents you pick
npx desktop-recorder-mcp app                        # open Desktop Recorder
```

The program goes in `%USERPROFILE%\.desktop-recorder-mcp\bin` and its private state (refs, recordings) in `%USERPROFILE%\.desktop-recorder-mcp\state`. Every configuration file it changes keeps a backup next to it (`.bak-desktop-recorder-mcp`).

**Windows for now. macOS and Linux are coming soon.**

## Responsible use

This tool can watch the mouse and keyboard and film the screen. It is built to be seen and to stay on your computer:

- It records only when you (or an agent you asked) start it, and a red indicator with a **Stop** button stays on screen the whole time.
- A recording stops by itself after 10 minutes (at most one hour). Nothing starts with Windows, and nothing records in the background without being asked.
- Passwords are never written to the recording. A video shows whatever is on the recorded window or display: record one window, not a whole screen.
- Recordings stay in the files you choose. Nothing is uploaded, and the tool has no telemetry.
- The source is open. Every file it changes in your agents keeps a backup.

Record only your own work, or with the consent of the people whose screen or data appear in it, and follow the privacy rules that apply to that data. Some sites forbid automation in their terms: check them before automating a site you do not own. The tool does not solve CAPTCHAs or get around bot detection.

## What this is

Agent Record Adapter is not a fork and not the official package. It uses [vercel-labs/agent-desktop](https://github.com/vercel-labs/agent-desktop) and [vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser) (Apache-2.0) exactly as published, and adds a separate layer: background mode first (an action never uses your mouse or keyboard nor brings a window to the front when there is a way to avoid it; when there is none, the official agent-desktop does it as before), `record` (a person's own actions to JSON Lines, with an MP4 video, `record start`/`stop`/`status` in the background and an on-screen indicator), `secret` (passwords asked for once and kept in the system's secret store), Desktop Recorder, and the skills `agent-record-background`, `desktop-recording`, `recording-to-script` and `script-repair`. Windows for now; macOS and Linux are coming soon. This package carries everything it needs: it does not need the official npm packages, and installs none.
