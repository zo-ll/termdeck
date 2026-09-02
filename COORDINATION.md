# Coordination — Termdeck

Status: DONE — all 14 issues closed; full repo delivered

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
| [#7](https://github.com/zo-ll/termdeck/issues/7) | VT frame adapter | — | PR #15 merged (`4b4fec4`) |
| [#8](https://github.com/zo-ll/termdeck/issues/8) | Single-shell PTY transport | — | reviewed pass — pushed, [PR #17](https://github.com/zo-ll/termdeck/pull/17) |
| [#9](https://github.com/zo-ll/termdeck/issues/9) | One-terminal native engine | #7, #8 | blocked |
| [#10](https://github.com/zo-ll/termdeck/issues/10) | Native lifecycle | #9 | blocked |
| [#11](https://github.com/zo-ll/termdeck/issues/11) | Static master-stack renderer | — | PR #16 merged (`ddb45f4`) |
| [#12](https://github.com/zo-ll/termdeck/issues/12) | Promotion, zoom, narrow | — | reviewed pass — pushed, [PR #18](https://github.com/zo-ll/termdeck/pull/18) |
| [#13](https://github.com/zo-ll/termdeck/issues/13) | Status and scrollback chrome | #12 | done — `fe7922c` local (`coord/13-ui-chrome`), critic reviewing |
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

Rotated history: `.coordinator/journal/` (latest archives: check journal dir).
The last ~15 events — older ones live in the journal above; ask and the coordinator greps it.

- 2026-09-02: #44 critic PASS → pushed coord/44-min-stack → PR #45, awaiting user approval.
- 2026-09-02: #46 (quit modal 'y' doesn't quit) dispatched to claude (same session; coord/46-quit-modal).
- 2026-09-02: #46 done (claude `76c82c0`: root cause = shutdown sent only SIGTERM which shells ignore; now HUP+TERM, quit exits ~20ms, escalation/no-orphan unchanged; 158 tests) — in critic review.
- 2026-09-02: #46 critic PASS → pushed coord/46-quit-modal → PR #47, awaiting user approval.
- 2026-09-02: #42 PLANNING APPROVED — final spec on the issue (entry points:
  no-path folder browser / <folder> root+single-terminal / --config file;
  runtime add via '+' + ^g a from choice of roots; first selected = master;
  stable fe/be order). Slices opened: #48 A1 (codex, ready), #49 A2 (claude,
  blocked A1), #50 A3 (both; blocked A1), #51 A4 (codex; blocked A1+A2+A3).
  Waves: A1 → A2(+A3-prep) → A3 → A4.
- 2026-09-02: #48 A1 (CLI entries + discovery model) dispatched to codex (coord/48-cli-discovery). A2 (picker) design check with Claude Design deferred until A1 lands.
- 2026-09-02: #48 A1 done (codex `6dd1401`: fs resolution + discovery model; 163 tests) — in critic review.
- 2026-09-02: #48 A1 critic PASS (contracts empty; 2 non-blocking: redundant DEFAULT_SCROLLBACK, expect() invariants) → pushed coord/48-cli-discovery → PR #52. A2 (folder picker) now unblocks — user's Claude Design pass next.
- 2026-09-02: synced updated mockups (picker) into reference `63c5b4c`; dispatached claude_design MCP prompt to claude for the A2 picker design (import project, implement Termdeck TUI.dc.html).
- 2026-09-02: A2 picker DESIGN NOTE done (claude, coord/42-picker-spec `5b7a759` docs-only, off main): MCP import no-op — repo reference already matches live project (additive screens 06-08 + 5 boards); note fixes grid/colors/ordinal model/^g a sheet/click parity, answers 4 ambiguities, flags 1 export contradiction (/ root vs / filter+g root). Follow-up handed: same-path MULTI-INSTANCE support in picker + runtime-add (per-path counts, -2/-3 suffixes).
- 2026-09-02: A2 design accepted (user: / = filter, g = root); design note pushed+merged `2bdaf5f`. #49 A2 DISPATCHED to claude (coord/49-picker): implement the picker per the note incl. multi-instance same-path.
- 2026-09-02: #52/A1 MERGED (`7cab5ca`, gate 163) — #48 closed; A2 branch now needs to integrate main (claude builds on the seam; it read A1 from the branch meanwhile).
- 2026-09-02: #49 A2 done (claude `67358d3` after coordinator rebase onto main — its local A1 merge dropped; gate 180) — in critic review.
- 2026-09-02: #49 A2 critic BLOCK (read_dir errors flatten to false-empty; empty-folder branch unreachable). Correction to claude (error-vs-empty surfacing + tests; plus non-blockings: elsewhere stub, dead Listing fields, secondary-button minus parity).
- 2026-09-02: #49 correction done (claude `4ad98fc`: error-vs-empty surfaced, 3 tests, elsewhere partition, dead fields removed, secondary-minus SGR parity; 185 tests) — re-review handed to critic.
## Durable resumption
- This file, `docs/RESUME.md`, and `docs/WORKSTREAMS.md` are the tracked source
  of truth. tmux scrollback and local agent conversations are disposable.
- Before changing machines, commit worker changes and push `main` plus every
  active `coord/*` branch.
- On a fresh machine, follow `docs/RESUME.md`, read the relevant workstream, and
  give it to a new agent. Do not rely on an old conversation for context.
- Private remote: `https://github.com/zo-ll/termdeck`. `main` and the active
  `coord/*` branches were first pushed on 2026-08-31.
'
  heading from the earlier rotation run.)
