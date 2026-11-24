---
name: desktop-recording
tags: screen-recording, record, task-recording, desktop-automation, ai-agent
requirements:
  - agent-record
description: >
  Record what a person does on their desktop, with agent-record record: their clicks, typing,
  choices and keys as one JSON line per action, and optionally an MP4 video of the window or
  screen. Use when the person asks to record a task, record a window, an app or a screen, film
  what they do, capture their steps so a script can repeat them later, or stop a recording.
  Triggers on: "record this", "record what I do", "grabar", "graba la ventana", "record the
  Excel window with video", "start recording", "stop recording", "ya terminé", "I'm done",
  "film my screen", "capture my steps". Then, to turn the recording into a script, use the
  recording-to-script skill.
---
<!-- Part of Agent Record Adapter (see NOTICE). -->

# Recording a person's task

`record` watches what a person does in one window, app or screen and writes each action as a JSON line: what they clicked, what they typed, what they chose. It can also film it as an MP4. It records the person; while a recording started with `record start` runs, your own element actions (`click`, `set-value`, `select`, `check`, `secret fill`, `press --window-id`...) are recorded too, marked `"by": "agent"`, so a task you did while it recorded becomes steps as well (a `secret fill` is noted by the secret's name, never its value). It is the first half of automating a task; the second half is the `recording-to-script` skill.

Windows only for now; elsewhere it answers `PLATFORM_NOT_SUPPORTED`.

**Desktop apps and web pages.** Desktop apps are driven with agent-record's desktop commands (`click`, `set-value`, `press --window-id`, `menu`, ...). A web page in a browser started with DevTools goes through the official agent-browser, run as `agent-record browser <agent-browser command>` (MCP: the browser tools of the same server); it also works in the background. The person's own browser without DevTools (where they are already signed in) is driven with the desktop commands (`navigate --window-id`, `click`), which reach its pages as window messages.

## 1. Agree what to record

- **One window, one app or one screen.** Ask unless the person said. A narrow scope keeps this conversation and anything private on other windows out of the recording.
- With more than one screen, run `agent-record list-displays` (or `record status`) and let them pick by number (`--display 2`, `--display primary`). Never record every screen by default.
- **Video or not.** A video helps later to understand an action, but it shows everything visible in the recorded window or screen. If they want one, ask where to save it (an `.mp4` path) or use the default.
- A window: find its id with `agent-record list-windows` and record it with `--window-id`.

## 2. Start

```bash
agent-record record start --window-id w-123 --video --video-out D:\Videos\task.mp4
agent-record record start --app EXCEL.EXE
agent-record record start --display 2 --video
```

MCP: `desktop_record_start` with the same fields. It returns once the recording is ready, with the `out` path (the JSON Lines file) and, with video, the video path.

Then tell the person, in plain words:

- that it is recording, what (the window, app or screen) and where the files go;
- that a small red indicator at the bottom of the screen shows the time, with a **Stop** button;
- to do the task normally, at their own pace, and to tell you when they are done (or press Stop);
- that typed passwords are never written to the recording (a video still shows the screen, but a password field only shows dots).

Do not act on their desktop while it records: your actions are left out of the events but would appear in the video.

A recording stops by itself after 10 minutes (`--max-duration-ms`, at most one hour). For a long task, say so before starting and set a longer limit.

## 3. Stop

When they say they are done, or pressed Stop (`record status` shows the session `ended`):

```bash
agent-record record stop            # add --id <id> if you started more than one
```

MCP: `desktop_record_stop`. It returns how many actions were recorded, how long it lasted and the video. Give the person the `out` path and the video path.

If it reports 0 actions although the person acted (the video shows it), their input was synthetic (a remote session, another automation tool) and was ignored: record again with `--include-injected`. If it says the session ended before it was stopped, your client closed it when a turn ended: the video plays up to that moment; record again and keep start and stop in one conversation.

## 4. What is in the recording

The `out` file has `recording_started`, `recording_ready`, one line per action and `recording_stopped`. Each action line has:

- `kind`: `click`, `type`, `select`, `press`, `scroll`, `drag`;
- `element`: what was acted on (`role`, `name`, `native_id`, `help_text`, `label`), with `ancestors` and `match_count` when several look alike;
- `window`: `title`, `app_name`, `path` (the program), `url` for a web page (scheme included: `http://127.0.0.1:4200/...`; the tab that holds the element), and `owner_id` for a dialog another window opened (Chrome's file picker): the recording of that window keeps it;
- `value` on `type` and `select`: what the field ended up holding. `secure: true` marks a password: no text was recorded;
- `t_ms`, and in the last line `video.offset_ms`: an action at `t_ms` is at `t_ms - offset_ms` in the video.

The full field list is in `agent-record skills get desktop commands-system` (section `record`).

## 5. Next

When the person wants the task to run again by itself, follow the `recording-to-script` skill (`agent-record skills get recording-to-script`). Ask them which typed values change every time: those become the script's parameters.

## Rules

- Record only when the person asks, only what they agreed to, and stop when they say so.
- Never record someone else's screen or data without their consent.
- Never ask the person to type a password into the conversation; the recording never contains one.
