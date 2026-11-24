# Desktop Recorder (Agent Record Adapter)

A small floating bar, like Windows voice typing, that records a task and plays it again, without AI. The complete guide to the app, the MCP server and recording (install, every step, demo videos, known problems) is [GUIDE.md](../../packages/agent-record-adapter/GUIDE.md).

- **Record** (the round button in the middle): records what you do in one screen or window, with or without video, until you press Stop. While it records, the bar shows the time and is left out of every capture, so it is not in the video.
- **Settings** (the gear): what to record, with video or not, the folder, and the version. The app asks npm for a newer version at start and every six hours (a click on the version asks again); when there is one, the gear shows a dot and the version row becomes **Update 0.2.0 → 0.2.1**: it installs the new version for the agents already set up, opens the new app and closes. If it fails, the app stays open and says why.
- **Recordings** (the list): the latest recordings. **Play** runs the script in the background, so you can keep working; the folder button opens the recording's folder; the repair button copies a prompt to paste into your AI agent (any of them) so it repairs the script with the `script-repair` skill.

Stopping writes, next to the recording (`recording.jsonl`, `video.mp4`), the script that repeats it: `script.mjs` with `desktop-script.mjs`, made without AI by the same rules as the `recording-to-script` skill. Every typed or chosen value is a parameter whose default is the recorded value: `node script.mjs` repeats the task as it was, and `node script.mjs --help` lists the values you can change. What has no background command (a key with no button, a drag on a canvas) is listed as not repeated instead of being guessed.

Each play writes its whole output to `last-run.log` in the recording's folder; the repair prompt points the agent there. Once a script has been changed by hand or by an agent, Play keeps it as it is (it is only written again from the recording while it is still the one the app wrote).

## Build

```powershell
cd apps\recorder
cargo build --release      # target\release\desktop-recorder.exe
cargo test
```

It runs `agent-record` (Agent Record Adapter) from its own folder, else from `~/.agent-record-adapter/bin` (`AGENT_RECORD_BIN` overrides both), and `node` from PATH. `desktop-recorder --make-script <recording.jsonl> <folder>` writes a script without opening the window.

For tests, `DESKTOP_RECORDER_HOME` moves its settings and default folder, and `DESKTOP_RECORDER_LANG=es` or `en` sets its language (otherwise the language of Windows).
