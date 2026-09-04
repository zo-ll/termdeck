# Termdeck workstreams

These task contracts are designed to survive new machines and completely new
agent conversations. Current status and exact branch ownership live in
`COORDINATION.md`.

## 1. Shared contracts and fake engine

Build the smallest compiling foundation that allows UI and native engine work
to proceed independently: application-owned shared types, YAML configuration
loading and validation, `check` and `list` CLI commands, and a deterministic
in-memory fake terminal engine.

Acceptance:

- Shared types expose no Ratatui, Alacritty, Crossterm, or PTY types.
- The shipped example configuration validates and lists projects with
  actionable errors.
- The fake engine produces deterministic frames and status transitions.
- Focused config and state tests pass.
- No real PTY, terminal emulator, production UI, daemon, or speculative
  abstraction is added.

## 2. Native terminal engine

Behind the frozen issue 1 contract, implement usable native terminal sessions:
PTY I/O, terminal parsing, resize, process status, respawn, and clean shutdown.
Generalize only enough for the configured maximum.

Acceptance:

- Bash, color, Unicode, paste, resize, Ctrl+C, exit detection, and respawn work.
- Owned process groups terminate on confirmed shutdown.
- Terminal state crosses the boundary only through shared contracts.
- Focused integration tests and all repository checks pass.

Blocked by workstream 1.

## 3. Master-stack interface

Claude Code owns this workstream. Implement the accepted design against the
frozen contracts and fake engine. The committed HTML and `support.js` under
`docs/design/termdeck/reference/` are the visual review authority. The linked
Claude Design project in `DESIGN.md` is an optional supplementary source.

Acceptance:

- Frontend-active, backend-promoted, zoomed, exited, and narrow states match the
  accepted master-and-vertical-preview-stack design.
- Promotion preserves stable terminal identity and puts the old master in the
  selected preview slot.
- Key hints, status text, truncation, colors, borders, and responsive behavior
  follow the accepted export.
- Rendering snapshots and all repository checks pass.
- No PTY, lifecycle, configuration, or shared-contract code changes.

Blocked by workstream 1.

## 4. Production integration

Connect the reviewed native engine and reviewed interface at the composition
root and make the shipped example configuration usable in WSL.

Acceptance:

- `termdeck idp` opens configured host shells with a master and live previews.
- Promotion, zoom, scrollback, resize, Vim, Codex, and Ctrl+C work end to end.
- Invalid configuration fails before partial startup.
- Normal exit and handled signals restore the outer terminal and leave no owned
  child processes.
- Automated checks and documented WSL manual acceptance pass.

Blocked by workstreams 2 and 3.

## Checks for every implementation handoff

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

The worker reports changed files, contract decisions, exact command results,
risks, and deviations; it commits but does not merge or push.
