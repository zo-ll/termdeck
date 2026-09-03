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

- 2026-09-02: range-select critic PASS → pushed coord/49-range-select → PR #56. Follow-up queued (Shift+click twin + decoder/torn-guard tests).
- 2026-09-02: #56 merged (`88bec35`, 195 tests); binary rebuilt (keyboard range live). Shift+click twin + decoder tests dispatched to claude (coord/49-shift-click).
- 2026-09-02: shift+click twin done (claude `1bddfb2`: shared select_between, shift-on-release SGR bit 2, 4 decoder tests incl. torn-guard; note 7 pairs it; no fixture moved; 202 tests) — critic review.
- 2026-09-02: Shift+click twin MERGED via #57 (`910c31d`, 203 tests) — binary rebuilt (full picker parity: Enter/arrows/o, shift+down/up + shift+click, mouse select incl folders, / filter, multi-instance). #42 remains: A3 (runtime add) + A4 (integration).
- 2026-09-02: #50 A3 (runtime add: + / ^g a chooser, dynamic spawn + re-layout, same-path instances) DISPATCHED to claude (coord/50-runtime-add).
- 2026-09-02: #50 A3 done (claude `25fc0b0`: sheet ^g a/+ / enter/+ /o/esc, [.] lock relaxed, NativeEngine::add (only engine touch), push_terminal folded-append, -2/-3 vs running set; 214 tests) — critic full review + codex quick engine-seam check in parallel.
- 2026-09-02: A3 MERGED via #58 (`9be6a0c`, 214 tests; codex engine approve; #50 closed; binary rebuilt — runtime add live). NB open: add-sheet mouse parity partial (+, -, filter, root-cycle, xN badge keyboard-only) — user decision pending. Next: A4 (integration) closes #42.
- 2026-09-02: PARALLEL: claude → add-sheet mouse parity (coord/50-sheet-parity); codex → A4 integration/acceptance (coord/51-integration) — both working.
- 2026-09-02: USER testing feedback: shift-range must TOGGLE (unselect too) + per-row checkbox square (click/Tab toggles). Dispatched to claude (coord/49-range-toggle).
- 2026-09-02: 49-range-toggle done (codex `4945b56`: toggle ranges + checkboxes; 222 tests) — critic review.
- 2026-09-02: range-toggle/checkbox MERGED via #61 (222 tests); binary rebuilt. NBs recorded: filter-hint copy, deliberate-instance add-path test.
- 2026-09-02: #63 rescope merged (176d132; sheet keys pinned, over-claim gone; 224 tests). Picker test backlog ZERO. Remaining: only #33 (animations).
- 2026-09-03: USER test findings: master scroll + prompt return gap (new OUTPUT doesn't snap to live tail; #31 is input-only). #64 dispatched to codex (coord/64-live-tail): reproduce + output-snap rule + wheel-down-to-live.
- 2026-09-03: #64 done (codex `d02daa5`: live output follows tail, history stays deliberate; 226 tests) — critic review.
- 2026-09-03: #64 MERGED via PR #65 (`97c63ab`, 228 tests; coordinator number-mixup: issue 64 / PR 65). #64 closed. Binary rebuilt with the live-tail fix.
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
- 2026-09-02: USER directive — monitor claude every 2 min; at 99% usage FORK
  the active slice (49-range-toggle) to codex. Monitor running detached
  (claude-monitor.sh, 30s interval): pings the inbox at >=99% or if the pane dies;
  log /tmp/shipwright/claude-monitor.log.
- 2026-09-02: USER stopped claude (was 93%+ usage) — 49-range-toggle routed to
  CODEX on a fresh clean branch (claude's uncommitted partial picker.rs
  discarded per precedent); claude window closed; usage monitor stopped.
  codex working (same session, absolute worktree path).
- 2026-09-02: TMUX ACCIDENTALLY CLOSED — restored: coordinator/critic/codex
  windows recreated in `personal`; relay survived (setsid) and reaches
  personal:coordinator.0 again; /tmp survived (task assets, critic boot);
  codex window resumed with --last (may be a fresh context — the uncommitted
  M src/ui/picker.rs in the 49-range-toggle worktree is the real continuity;
  continuation.md staged). USER checking resume manually before any nudge.
- 2026-09-02: RESPAWN RECIPE FIXED (user catch): codex restores must keep
  `-c model_reasoning_effort=high` (a restore dropped it to medium). The
  49-nb2 test was produced by the medium session (accepted, test-only).
- 2026-09-02: #63 BLOCK (critic): the NB test over-claimed sheet range
  (no input route exists) — re-scope to a state-level parity pin routed to
  codex (correction). Critic finish-protocol slipped (no ping/marker) —
  reminder re-sent. Target: re-review then merge #63.
- 2026-09-03: REBOOT/RESTORE — environment was reset again (tmux + /tmp
  wiped). Restored: coordinator (this), relay (from skill script), critic
  (fresh boot incl. reinforced finish protocol), codex (terra HIGH/full
  access), claude (standby). Durable state (main 5e2efda 224 tests, journal,
  worktrees, issues) intact. User testing on the current binary.
- 2026-09-03: DISPATCH LESSON — the double-Enter after prompt-target is ONLY
  for claude (stubborn composer); for CODEX a single Enter submits, and a
  second Enter re-submits the brief -> duplicate turn -> duplicate finish
  ping (this caused the 64-live-tail twin pings). Use ONE Enter for codex.
- 2026-09-03: #64 BLOCK (critic): snap inert (revert-pass), runtime-add raw
  size hides prompt, metadata refresh loses preview marker/row at tail.
  Correction-64 routed to codex (single-Enter deliver, no finish re-run).
- 2026-09-03: USER — Ctrl+g N promotes by terminal NUMBER (not 1-4 only;
  deck has 16). Dispatched to codex (coord/66-promote-n): digit-sequence
  capture (~600ms), clamp, hint, tests.
- 2026-09-03: USER — ^g a sheet must allow RE-OPENING an already-open path
  (locked-row Enter = add another instance; the [.] refusal is revised) +
  picker-like sheet navigation (→/←). Dispatched to codex
  (coord/67-sheet-reopen).
- 2026-09-03: USER principle — file explorers must have NO restrictions:
  the add-sheet allows any folder/repo (incl. already-open → new instance);
  [.] lock removed; picker-like navigation. Amended 67-sheet-reopen brief
  (interrupted the old-spec turn).
- 2026-09-03: PROCESS RULE (user): never steer a running worker — new/changed
  specs are sent only when the current task is WITH THE CRITIC. Patched into
  the coordinator skill. (Applied going forward; the #67 amendment already
  in-flight runs as the current task.)
- 2026-09-03: HARD RULE (user) for the coordinator: never submit anything
  to a worker while it is working; only give work when the worker is idle.
  Verify idle (prompt, no working/queued state) before every dispatch.
  Current: codex STOPPED at prompt, idle; #66 (dirty tree, red gate) and #67
  (not started) both pending the user's direction.
- 2026-09-03: #66 REDO (from scratch, fresh branch; partial discarded) —
  dispatched to codex (idle-verified per the hard rule): Ctrl+g N digits
  (~600ms), clamping, DELIBERATE fixture re-bless (hint '1-4'->'N' affects
  many status rows). #67 scheduled AFTER #66 merges (user sequence).
- 2026-09-03: #66 done fresh (codex `914aa80`, 234 tests; hint fixtures
  re-blessed) — critic review.
- 2026-09-03: #64's 5 non-blocking findings dispatched to codex (idle;
  coord/64-nbs) BEFORE #67 per user: inert snap couplet, terminal_size test
  gaps, WIDTH-only chrome (short-wide window hides prompt), double FrameReady
  nit, tail-redraw contract comment.
- 2026-09-03: #66 critic BLOCK (escape-abort test weakened — passes both
  ways; needs non-active pending-number scenario). Correction-66 staged;
  deliver to codex ONLY when its 64-nbs turn completes (hard rule: no
  submit while working).
- 2026-09-03: 64-nbs done (codex `996a379`: lean tail redraw + terminal
  sizing; gate 228) → critic review (incl. contracts/engine.rs +2 and a
  suspicious COORDINATION.md -8 in its diff). #66 escape-abort correction
  delivered to codex (idle-verified).
- 2026-09-03: 64-nbs MERGED (PR #66 -> 03066d7, 228; residual NB
  feed-while-scrolled pin gap recorded). #66 promote-n re-review queued.
  #67 after #66 merges.
- 2026-09-03: #66 (promote-n) needs main-integration — merge of origin/main
  conflicted in session.rs (vs 64-nbs); integration routed to codex (idle).
  Plan: resolve+gate+merge commit -> I merge PR #67 -> then #67 (sheet) to
  codex.
- 2026-09-03: #66 merged (PR #67 -> 0fd1800, 234; binary rebuilt — Ctrl+g N
  live). #67-sheet (unrestricted + nav) dispatched to codex (idle-verified;
  note: codex <25% of 5h limit). Claude: UI-declutter DESIGN task via the
  claude_design MCP — nudged to submit; running separately.
- PENDING (user): AFTER claude's UI-declutter design lands, CHECK whether the
  removals/simplifications in the design affect BACKEND code (session.rs /
  engine / contracts / input) — a UI declutter can orphan or contradict
  backend seams. Route the scan to codex (engine lane) before/with any
  implementation of the declutter.
- 2026-09-03: #67-sheet done (codex ec62ae2: unrestricted sheet + folder
  targets, reuses PickerState) — critic review; branch base is old — main
  integration after review (conflicts possible vs 64-nbs/#66).
