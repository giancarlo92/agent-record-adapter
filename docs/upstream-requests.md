# Small upstream changes that would simplify this project

None is needed for it to work; each would remove a workaround.

| Upstream | Change | Removes |
|---|---|---|
| agent-desktop | A public accessor for the UI Automation element of `tree::element::UIAElement` (`pub fn uia(&self) -> &uiautomation::UIElement`) | the layout mirror in `crates/windows-adapter/src/tree.rs` |
| agent-desktop | Radio buttons' checked state from `SelectionItem.IsSelected` (PR vercel-labs/agent-desktop#679, open) | the radio part of `background_evidence` in `hooks.rs` |
| agent-desktop | `LaunchOptions` field to start the first window without activation (`SW_SHOWNOACTIVATE`) | `system/separate_launch.rs` for ordinary launches |
| agent-desktop | `launch --no-attach` allowed when the arguments give the program its own profile | the `--no-attach` rewrite in `crates/cli/src/main.rs` and part of `separate_launch.rs` |
| agent-desktop | `list-displays` with a 1-based `number` | `run::list_displays` in the front |
| agent-desktop | The TCP listener table read on builds that report the state as 0 (`tcp_listener.rs`, fixed in the old fork) | — (DevTools port attribution of `launch --cdp`) |
| agent-browser | An option to keep `Target.activateTarget`/`Page.bringToFront` off and focus emulation on (a "background" mode) | the proxy's swallowing (it would still be needed for recording) |
