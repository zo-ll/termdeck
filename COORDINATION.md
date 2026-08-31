# Coordination — Termdeck

Status: active — issue 1 dispatched; issue 3 design preflight running

## Goal

Implement Termdeck as a standalone Rust terminal workspace with a master and
live-preview-stack interface.

## Issues

| # | Title | Blocked by | Branch | Worker | Skills | PR | Status |
|---|-------|------------|--------|--------|--------|----|--------|
| 1 | Shared contracts and fake engine | — | `coord/01-contracts` | Codex (Terra/high) | ponytail | — | in progress |
| 2 | Native PTY and terminal engine | 1 | — | Codex | Rust | — | pending |
| 3 | Master-stack interface | 1 | `coord/03-ui` | Claude Code via `claudep` (Opus/high) | ponytail | — | blocked; design preflight running |
| 4 | Production integration and WSL acceptance | 2, 3 | — | Codex | Rust | — | pending |

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

## Durable resumption

- This file, `docs/RESUME.md`, and `docs/WORKSTREAMS.md` are the tracked source
  of truth. tmux scrollback and local agent conversations are disposable.
- Before changing machines, commit worker changes and push `main` plus every
  active `coord/*` branch.
- On a fresh machine, follow `docs/RESUME.md`, read the relevant workstream, and
  give it to a new agent. Do not rely on an old conversation for context.
- No Git remote is configured yet. Cross-machine synchronization remains
  blocked until a remote is selected and the branches are pushed.
