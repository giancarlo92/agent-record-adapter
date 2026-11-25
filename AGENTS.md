# Guide for coding agents

Agent Record Adapter wraps the unmodified vercel-labs agent-desktop and agent-browser (submodules under `vendor/`). Read [README.md](README.md) and [docs/architecture.md](docs/architecture.md) first.

## Rules

- **Never edit anything under `vendor/`.** A behaviour that needs upstream changed goes in our layer (a hook in `crates/windows-adapter/src/hooks.rs`, evidence for the gates, the front) or in [docs/upstream-requests.md](docs/upstream-requests.md).
- **Ours first, upstream as fallback.** Every background route falls back to upstream's own route; `AGENT_DESKTOP_BACKGROUND=0` must give upstream's behaviour exactly.
- **Everything in the background**: no use of the person's mouse, keyboard or foreground unless `--headed`.
- After an upstream update run `scripts/update-upstream.ps1` (it regenerates `forward.rs`; never edit that file by hand).
- Secrets: never print, log or commit them; the agent can list and fill secrets, never read or set them.
- Tests never use the person's mouse or keyboard: test pages in `tests/background/pages` and the Aprende Replica test forms, headless or behind.
- Do not run real public actions on real sites while testing.
- Code comments and saved content in English. Every file of ours starts with `Part of Agent Record Adapter (see NOTICE).`
- Commit only when the user asks.

## Commands

```powershell
cargo build --workspace
cargo test -p agent-record-core -p agent-record-windows -p agent-record -p agent-record-browser
cargo test -p agent-record-engine --bins -- --skip shell_surface --skip surfaces --skip contract_command_surface
python scripts/gen_forwarding.py --check
python scripts/sync_web_engine.py --check
```

The shell-surface tests press real Win+A (Quick Settings): never run them.
