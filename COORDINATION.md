# Coordination — Termdeck

Status: active — review corrections dispatched for issues 1 and 3 preflight

## Goal

Implement Termdeck as a standalone Rust terminal workspace with a master and
live-preview-stack interface.

## Issues

| # | Title | Blocked by | Branch | Worker | Skills | PR | Status |
|---|-------|------------|--------|--------|--------|----|--------|
| [1](https://github.com/zo-ll/termdeck/issues/1) | Shared contracts and fake engine | — | `coord/01-contracts` | Codex (Terra/high) | ponytail | — | corrections required |
| [2](https://github.com/zo-ll/termdeck/issues/2) | Native PTY and terminal engine | 1 | — | Codex | Rust | — | pending |
| [3](https://github.com/zo-ll/termdeck/issues/3) | Master-stack interface | 1 | `coord/03-ui` | Claude Code via `claudep` (Opus/high) | ponytail | — | blocked; design gaps being drafted |
| [4](https://github.com/zo-ll/termdeck/issues/4) | Production integration and WSL acceptance | 2, 3 | — | Codex | Rust | — | pending |

## Waves

- Wave 1: issue 1
- Wave 2: issues 2 and 3 in parallel
- Wave 3: issue 4

## Decisions

- External generic tool; Horizon integration is configuration-only.
- Rust 1.98.0, Ratatui/Crossterm, Alacritty terminal state, portable PTYs.
- Master-and-preview-stack interface; no grid.
- No tmux, OpenMux, daemon, persistence, or mouse forwarding in v1.
- Worker session: `termdeck-agents`, with `codex` and `claude` windows.
- The committed Design export is the visual review authority; Claude may also
  inspect the linked project through the `claude_design` MCP.

## Handoffs

- 2026-08-31: Codex received issue 1 with its acceptance commands and commit-only
  handoff requirement.
- 2026-08-31: Claude received a read-only design preflight. Implementation stays
  blocked until issue 1 freezes the shared contract.
- 2026-08-31: Issue 1 checkpoint `8fc4b96` was independently verified and
  pushed through branch tip `3e1b78b`. Format, all-feature tests (5 passed),
  Clippy with warnings denied, `check`, and `list` passed. Issue 1 remains in
  progress pending the UI contract gaps found by Claude.
- 2026-08-31: Independent review blocked issue 1 on CLI/plan mismatch, combining
  character loss, and duplicate exit-code authority. Claude preflight was
  blocked on three missing reference states and contradictory access wording.
  Corrections were routed back to the same workers.
- 2026-08-31: Migrated the four original local workstream issues to GitHub after
  the private remote became available; issues #1–#4 are now the visible tracker.

## Durable resumption

- This file, `docs/RESUME.md`, and `docs/WORKSTREAMS.md` are the tracked source
  of truth. tmux scrollback and local agent conversations are disposable.
- Before changing machines, commit worker changes and push `main` plus every
  active `coord/*` branch.
- On a fresh machine, follow `docs/RESUME.md`, read the relevant workstream, and
  give it to a new agent. Do not rely on an old conversation for context.
- Private remote: `https://github.com/zo-ll/termdeck`. `main` and the active
  `coord/*` branches were first pushed on 2026-08-31.
