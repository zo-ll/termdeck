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

- 2026-09-02: A2 picker DESIGN NOTE done (claude, coord/42-picker-spec `5b7a759` docs-only, off main): MCP import no-op — repo reference already matches live project (additive screens 06-08 + 5 boards); note fixes grid/colors/ordinal model/^g a sheet/click parity, answers 4 ambiguities, flags 1 export contradiction (/ root vs / filter+g root). Follow-up handed: same-path MULTI-INSTANCE support in picker + runtime-add (per-path counts, -2/-3 suffixes).
- 2026-09-02: A2 design accepted (user: / = filter, g = root); design note pushed+merged `2bdaf5f`. #49 A2 DISPATCHED to claude (coord/49-picker): implement the picker per the note incl. multi-instance same-path.
- 2026-09-02: #52/A1 MERGED (`7cab5ca`, gate 163) — #48 closed; A2 branch now needs to integrate main (claude builds on the seam; it read A1 from the branch meanwhile).
- 2026-09-02: #49 A2 done (claude `67358d3` after coordinator rebase onto main — its local A1 merge dropped; gate 180) — in critic review.
- 2026-09-02: #49 A2 critic BLOCK (read_dir errors flatten to false-empty; empty-folder branch unreachable). Correction to claude (error-vs-empty surfacing + tests; plus non-blockings: elsewhere stub, dead Listing fields, secondary-button minus parity).
- 2026-09-02: #49 correction done (claude `4ad98fc`: error-vs-empty surfaced, 3 tests, elsewhere partition, dead fields removed, secondary-minus SGR parity; 185 tests) — re-review handed to critic.
- 2026-09-02: #49 re-review PASS → pushed coord/49-picker → PR #54. 1 new non-blocking: overlapping roots duplicate filter matches (cwd ⊂ HOME) — recorded; offer optional tiny fix. A3 unblocks after merge.
- 2026-09-02: #49 dedupe + rule-row re-review PASS (`e257242`, 186) — pushed, PR #54 ready for user merge approval; A3 next.
- 2026-09-02: #54 merged (`857babe`, 186 tests); #49 closed; binary rebuilt (picker live).
- 2026-09-02: USER: picker nav must be SIMPLE — Enter selects the path, right arrow descends (repos incl.), left arrow up. Correction-49c routed to claude (note §7 + impl + tests).
- 2026-09-02: NAV-AMENDMENT MERGED via PR #55 (coordinator mis-flow corrected: #54 had merged the original picker; nav-simple + key-row commits rebased onto main and shipped as their own PR — critic passed both). Binary rebuilt with the SIMPLE picker: Enter=select, →=descend (into repos too), ←=back, o=launch.
- 2026-09-02: picker shift+down/up range-select + mouse folder selection dispatched to claude (coord/49-range-select).
- 2026-09-02: range-select done (claude `5f82232`: shift+down/up additive incl folders; ESC[1;2A/B decoding + torn-sequence guard; folder click verified already working + 2 pins; 195 tests) — in critic review.
- 2026-09-02: USER — Shift+click = the range-select MOUSE TWIN (from highlighted row down to clicked row). Queued as a small follow-up after the keyboard-only range PR lands (followup-shift-click.md staged).
- 2026-09-02: range-select critic PASS → pushed coord/49-range-select → PR #56. Follow-up queued (Shift+click twin + decoder/torn-guard tests).
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
