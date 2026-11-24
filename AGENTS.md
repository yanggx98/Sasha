# AGENTS.md

Guidance for AI coding agents working in this repository. Human contributors should
also read [CONTRIBUTING.md](CONTRIBUTING.md).

## Project

Sasha is a desktop serial port debugging tool written in Rust (edition 2024) with
GPUI (`gpui-kit`). Features: multiple sessions, text/HEX views, quick and timed send,
Lua 5.4 scripting (`mlua`, vendored), live log streaming, TOML themes.
License: GPL-3.0-only. Windows is the only validated platform.

## Commands

```powershell
cargo run --locked      # run the app
cargo check --locked
cargo test --locked     # must pass before finishing a change
cargo fmt               # formatting is enforced in history ("style: apply cargo fmt")
```

## Layout

- `src/main.rs` — entry point.
- `src/model/` — data and state (config, settings, session, log, quick commands,
  timed send, stream output). No UI code here.
- `src/serial.rs`, `src/session_service.rs` — serial port I/O and session lifecycle.
- `src/scripting/` — Lua runtime and API exposed to scripts (tests in `tests.rs`).
- `src/ui/` — GPUI views; themes in `src/ui/theme*.rs` and `src/ui/theme_assets/`.
- `src/i18n.rs`, `locales/` — internationalization (`en-US.toml`, `zh-CN.toml`).
- `docs/` — user docs (`usage*.md`, `themes*.md`, `lua-scripting-api.md`; English and
  `.zh-CN` variants), screenshots, example theme.
- `scripts/` — example Lua scripts.

## Conventions

- **i18n**: every user-visible string goes through the locale files. Update **both**
  `locales/en-US.toml` and `locales/zh-CN.toml`; never hardcode UI text.
- **Docs**: when behavior documented in `README.md`, `README.zh-CN.md` or `docs/`
  changes, update them (both READMEs stay in sync).
- **Lua API**: changes to the scripting API must be reflected in
  `docs/lua-scripting-api.md` and covered in `src/scripting/tests.rs`.
- **Scope**: keep changes focused; no unrelated refactors.
- **Tests**: add or update tests for behavior changes where practical. Hardware and
  desktop interaction can't be fully tested — say how you verified manually.
- **Dependencies**: keep `Cargo.lock` committed and use `--locked`; don't add or bump
  dependencies without a reason.
- **Serial defaults**: DTR and RTS default to low.

## Commits

Short, imperative, Conventional Commits style, e.g. `feat(ui): add log search`,
`fix(serial): default DTR and RTS to low`. Don't commit or push unless asked.

Any commit an AI agent authored or substantially contributed to must end with a
`Co-Authored-By` trailer naming the agent and model, after a blank line, e.g.:

```
Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>
```

Use the agent's own real name and noreply address; don't impersonate another tool.
Pull request descriptions should also state that they were AI-generated.

## Versioning

SemVer in the `0.x` phase with a `-b` (beta) suffix, e.g. `0.1.9-b`: new features or
incompatible changes bump MINOR, fixes/docs/CI only bump PATCH. Bump `Cargo.toml`
only when releasing, in its own `chore: bump version to X` commit, and tag `vX`
(must match `Cargo.toml`) on it. Never bump without tagging, and don't tag or push
tags unless asked. Details in [CONTRIBUTING.md](CONTRIBUTING.md#versioning).

## Don't

- Don't commit internal workflow/tooling files (they are git-ignored on purpose).
- Don't claim macOS/Linux support; those platforms are unverified.
