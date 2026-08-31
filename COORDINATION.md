# Coordination — Termdeck

Status: active — #15/#16 independently reviewed; rebase round dispatched; awaiting user merge approval

## Goal

Implement Termdeck as a standalone Rust terminal workspace with a master and
live-preview-stack interface.

## Issues

| # | Title | Blocked by | Branch | Worker | Skills | PR | Status |
|---|-------|------------|--------|--------|--------|----|--------|
| [1](https://github.com/zo-ll/termdeck/issues/1) | Shared contracts and fake engine | — | `coord/01-contracts` | Codex (Terra/high) | ponytail | [#5](https://github.com/zo-ll/termdeck/pull/5) | merged (`b880b60`) |
| [2](https://github.com/zo-ll/termdeck/issues/2) | Native PTY and terminal engine epic | 1 | — | Codex | ponytail | — | split into #7–#10 |
| [3](https://github.com/zo-ll/termdeck/issues/3) | Master-stack interface epic | 1 | — | Claude Code via `claudep` (Opus/high) | ponytail | [#6](https://github.com/zo-ll/termdeck/pull/6) | design merged; #11 active |
| [4](https://github.com/zo-ll/termdeck/issues/4) | Production integration and WSL acceptance | 2, 3 | — | Codex | Rust | — | pending |

## Small tasks

Each slice targets one observable behavior and one focused review boundary. If
an agent turn exceeds roughly 25 minutes, it checkpoints instead of widening
scope.

| Issue | Slice | Blocked by | Status |
|---|---|---|---|
| [#7](https://github.com/zo-ll/termdeck/issues/7) | VT frame adapter | — | PR #15 open; rebase round dispatched |
| [#8](https://github.com/zo-ll/termdeck/issues/8) | Single-shell PTY transport | — | ready |
| [#9](https://github.com/zo-ll/termdeck/issues/9) | One-terminal native engine | #7, #8 | blocked |
| [#10](https://github.com/zo-ll/termdeck/issues/10) | Native lifecycle | #9 | blocked |
| [#11](https://github.com/zo-ll/termdeck/issues/11) | Static master-stack renderer | — | PR #16 open; fixture cleanup verified; rebase round dispatched |
| [#12](https://github.com/zo-ll/termdeck/issues/12) | Promotion, zoom, narrow | #11 | blocked |
| [#13](https://github.com/zo-ll/termdeck/issues/13) | Status and scrollback chrome | #11 | blocked |
| [#14](https://github.com/zo-ll/termdeck/issues/14) | Modal and input modes | #12, #13 | blocked |

## Waves

- Wave 1: issue 1 — merged
- Wave 2A: #7 and #8 in parallel; approve/merge design draft PR #6
- Wave 2B: #9 and #11
- Wave 2C: #10 plus #12 and #13
- Wave 2D: #14
- Wave 3: issue 4, decomposed after the engine/UI epics close

## Decisions

- External generic tool; Horizon integration is configuration-only.
- Rust 1.98.0, Ratatui/Crossterm, Alacritty terminal state, portable PTYs.
- Master-and-preview-stack interface; no grid.
- No tmux, OpenMux, daemon, persistence, or mouse forwarding in v1.
- Worker session: `termdeck-agents`, with `codex` and `claude` windows.
- The committed Design export is the visual review authority; Claude may also
  inspect the linked project through the `claude_design` MCP.
- With a one-hour user window, next-wave tasks are intentionally bounded to one
  behavior and a focused test/snapshot rather than whole subsystem branches.

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
- 2026-08-31: Issue 1 corrections committed as `52fa845` and pushed through
  branch tip `8d0b12a`. Independent format, Clippy, 17-test, `check`, and `list`
  verification passed with no remaining blocking findings.
- 2026-08-31: Issue 1 PR #5 opened for user review; no merge performed.
- 2026-08-31: Claude's missing-state supplement committed as `2a3cf24` and
  pushed through `9364643`. Source/structural review passed: three artboards,
  no external resources, accepted palette only, clean HTML structure, original
  export untouched. Human browser inspection remains available before UI work.
- 2026-08-31: Draft PR #6 opened for issue 3 so design and later UI work remain
  visible on the same branch and review thread.
- 2026-08-31: Pre-merge ponytail review found one simplification: consolidate
  FakeEngine's three parallel state maps. Normal seam tracing also found no
  native-output drain method. Both narrow corrections were returned to Codex;
  PR #5 remains unmerged.
- 2026-08-31: Ponytail re-review passed after `0691fd7` reduced FakeEngine by
  three net lines and added the neutral event-drain seam. PR #5 was squash-
  merged as `b880b60` under the user's conditional approval; 18 post-merge
  tests passed. Issue #1 remains open for user-controlled closure.
- 2026-08-31: Replaced broad next-wave implementation with GitHub slices #7–#14.
  #7 and #8 are ready; UI slice #11 waits only for draft PR #6 approval/merge.
- 2026-08-31: Reviewed design PR #6 squash-merged as `12bd431`. Fresh worktrees
  were created from that `main`; #7 and #11 were dispatched in new visible tmux
  windows `codex-vt` and `claude-ui`. Historical worker panes remain intact.

- 2026-08-31 (evening): Previous coordinator (Codex session in `visura:0`, now out of tokens) opened PR #15 and PR #16 and routed the fixture-cleanup correction to Claude. New coordinator session took over from its written handoff.
- 2026-08-31 (evening): Independent review from source. PR #15 (`coord/07-vt-adapter`): adapter code verified against alacritty 0.26 sources (`point_to_viewport` semantics match); fmt/clippy clean, 23 tests pass in the worktree. PR #16 (`coord/11-ui-master-stack`): fixture cleanup verified (`pub mod fixture` → `#[cfg(test)] mod fixture`, docs updated, `termdeck::ui` exports only `Deck`, release build clean); fmt/clippy clean, 21 tests pass with cleanup applied. Main checks: fmt/clippy clean, 18 tests pass. No blocking findings in either PR.
- 2026-08-31 (evening): Found both PR branches one docs commit behind `main` (merge-base `12bd431`, main has `ffe0b2a`), so both PR diffs show a stale revert of COORDINATION.md. Neither worker edited it — a rebase onto `main` resolves cleanly. Rebase + push rounds dispatched to the same workers (`codex-vt`, `claude-ui`). Merges remain blocked on explicit user approval.

## Durable resumption

- This file, `docs/RESUME.md`, and `docs/WORKSTREAMS.md` are the tracked source
  of truth. tmux scrollback and local agent conversations are disposable.
- Before changing machines, commit worker changes and push `main` plus every
  active `coord/*` branch.
- On a fresh machine, follow `docs/RESUME.md`, read the relevant workstream, and
  give it to a new agent. Do not rely on an old conversation for context.
- Private remote: `https://github.com/zo-ll/termdeck`. `main` and the active
  `coord/*` branches were first pushed on 2026-08-31.
