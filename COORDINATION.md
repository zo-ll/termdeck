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

- 2026-09-01: #34b review PASS but branch integration onto main CONFLICTED in
  src/ui/testdata/help.txt (both #32 and #34b edits) — conflict resolution
  routed to claude (same worker; left in conflicted state, not hand-fixed).
  FOLLOW-UP noted (claude's own idea): the "^g 1-4" hint labels hardcode 4 —
  should use the deck's count after the cap lift.
- 2026-09-01: #38 merged (`001c64f`, gate 135) — #34 CLOSED (all projects at
  once: cap lift + scrollable stack live). Release binary rebuilt +
  reinstalled. Open tracker: only #33 (animations, parked).
- 2026-09-02 (POST-REBOOT RESTORE): environment restored — tmux session
  personal with coordinator/critic/codex/claude windows; relay restarted
  detached (inbox live, self-test delivered); critic booted fresh (ready,
  idle, boot ping landed); codex standby; #39 re-dispatched to claude on a
  FRESH branch coord/39-collapsed-default (stale 35-collapsed-default left
  untouched per user). /tmp was wiped by the reboot — task briefs recreated
  from issue bodies/COORDINATION records.
- 2026-09-02: #39 critic PASS → pushed → PR opened. Awaiting user approval.
- 2026-09-02: #39 done (claude, `d80183a`: previews start folded, ^g c
  expand-all-first, 5 snapshots re-blessed stack-column-only, gate 138) — in
  critic review.
- 2026-09-02: #41 S1 (mouse-first: draggable stack-width divider, keyboard
  parity) DISPATCHED to claude (SAME session — lane reuse; coord/41-mouse-ratio
  off main). Brief: visible divider + drag => live split (bounds decision),
  keyboard nudge bindings as equivalence, no regression of wheel/drag/
  promote/markers, snapshot + tests. FOLLOW-UP noted (claude's own): collapse
  hint/help label should read "expand" while folded.
- 2026-09-02: RECONCILE (coordinator session truncated by a tmux bug; relay
  log recovered the missed turns): #41 S1 done by claude `bb29ff6` (draggable
  divider + ^g -/^g = parity, bounds 0.55-0.85, per-session persistence,
  ceil-fix, 151 tests) and critic PASS (region discrimination + ceil-fix
  regression tested, 3 non-blocking). Pushed coord/41-mouse-ratio → PR #43,
  awaiting user approval. (Also fixed a duplicated '
- 2026-09-02: USER — give codex permissions too (parity with claude): codex
  now runs danger-full-access (safety = worker contract + critic + user
  approval; supersedes the earlier no-full-access stance for worker parity).
  Codex upgraded to 0.152.1 (auto during relaunch) — running flat, sessions
  reusable across worktrees; tmux pings + crates.io work directly. No
  sandbox exception remains in the reuse policy.
- 2026-09-02: WORKFLOW (user-driven): reuse LONG-LIVED worker sessions per
  LANE (preserve context) — fresh sessions only on quota exhaustion or
  sandbox-scoped worktrees (codex). Patched into the coordinator skill
  (Phase 4, "reuse, don't relaunch"). Consequence: claude/critic keep one
  session each; codex engine slices serialize in one worktree where possible.
- 2026-09-02: correction for #41 S1's 3 non-blocking findings routed to
  claude (same session, coord/41-mouse-ratio): drop dup resizing, wheel
  releases the divider (doc+code aligned), cross-check test for
  master-ratio constants. Working.

Rotated history: `.coordinator/journal/` (latest archive: 2026-09, 56 events).
The last ~15 events — older ones live in the journal above; ask and the coordinator greps it.
  (lift 1-4 cap, config+engine) DISPATCHED to codex (coord/34a-lift-cap).
  #34b (scrollable stack list, UI) next — awaiting user choice (design-first
  vs direct) but queued to claude.
  user — no design mockups): coord/34b-scrollable-stack. Paging gesture to be
  defined carefully vs #25 wheel; hit-testing by list offset; scroll
  indicators; N>4 via synthetic fixtures until #34a merges. Both lanes
  working (#34a codex, #34b claude).
  critic review.
  before #34b. #34b still with claude.
  list; ^g pgup/pgdn + wheel-over-chrome paging; gutter + "N more"; hit-test
  follows window; gate 129; screens 01-05 byte-identical, help.txt moved) —
  in critic review; branch base predates #36/#37, integration onto main after
  review.
  src/ui/testdata/help.txt (both #32 and #34b edits) — conflict resolution
  routed to claude (same worker; left in conflicted state, not hand-fixed).
  FOLLOW-UP noted (claude's own idea): the "^g 1-4" hint labels hardcode 4 —
  should use the deck's count after the cap lift.
  from merged code, gate 135) — pushed → PR opened. Awaiting user approval to
  close #34.
  once: cap lift + scrollable stack live). Release binary rebuilt +
  reinstalled. Open tracker: only #33 (animations, parked).
  expand via marker click (inverts #27/#32). Issue #39 opened; dispatched to
  claude (coord/35-collapsed-default): default folded strips, markers show ▸
  from frame one, ^g c = toggle-all/expand-all, deliberate snapshot
  re-blessing per fixture (honesty enforced by the critic).
  by default) remains OPEN, ready-for-agent, fully specced on GitHub. claude
  exhausted its session tokens mid-task (~91% then cut) — its partial work in
  ~/.worktrees/termdeck/35-collapsed-default (M src/ui/state.rs only, no
  commit) is intentionally NOT saved (user decision); do not touch it. Resume
  tonight: claude session resets 20:10 (Europe/Malta) — reroute/redo #39 from
  a clean fresh branch off main (the existing coord/35-collapsed-default
  branch is stale with partial state; prefer a new branch), or ask the user.
  Task brief lives at /tmp/shipwright/termdeck/35-collapsed-default/task.md.
  personal with coordinator/critic/codex/claude windows; relay restarted
  detached (inbox live, self-test delivered); critic booted fresh (ready,
  idle, boot ping landed); codex standby; #39 re-dispatched to claude on a
  FRESH branch coord/39-collapsed-default (stale 35-collapsed-default left
  untouched per user). /tmp was wiped by the reboot — task briefs recreated
  from issue bodies/COORDINATION records.
  LANE (preserve context) — fresh sessions only on quota exhaustion or
  sandbox-scoped worktrees (codex). Patched into the coordinator skill
  (Phase 4, "reuse, don't relaunch"). Consequence: claude/critic keep one
  session each; codex engine slices serialize in one worktree where possible.
  now runs danger-full-access (safety = worker contract + critic + user
  approval; supersedes the earlier no-full-access stance for worker parity).
  Codex upgraded to 0.152.1 (auto during relaunch) — running flat, sessions
  reusable across worktrees; tmux pings + crates.io work directly. No
  sandbox exception remains in the reuse policy.
  expand-all-first, 5 snapshots re-blessed stack-column-only, gate 138) — in
  critic review.
  parity) DISPATCHED to claude (SAME session — lane reuse; coord/41-mouse-ratio
  off main). Brief: visible divider + drag => live split (bounds decision),
  keyboard nudge bindings as equivalence, no regression of wheel/drag/
  promote/markers, snapshot + tests. FOLLOW-UP noted (claude's own): collapse
  hint/help label should read "expand" while folded.
- 2026-09-02: #41 S1 correction done (claude `1f47d6c`: dup resizing
  deleted, paste-release added — key/wheel already released per doc, cross-
  check test for ratio constants; 152 tests). Re-review handed to the critic.
  (Skills: lean rewrite committed+pushed.)
- 2026-09-02: #44 (stacked previews start at minimal width) dispatched to claude (same session; coord/44-min-stack).
- 2026-09-02: #44 done (claude `c1d614f`: min-width start 0.85, examples stop pinning 0.70 so override works, strip degrades at 15% instead of truncating, zero fixtures re-blessed, fresh-start.txt; 157 tests) — in critic review.
- 2026-09-02: #44 critic PASS → pushed coord/44-min-stack → PR #45, awaiting user approval.
- 2026-09-02: #46 (quit modal 'y' doesn't quit) dispatched to claude (same session; coord/46-quit-modal).
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
