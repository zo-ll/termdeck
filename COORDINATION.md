# Coordination — Termdeck

Status: ACTIVE — tracker on GitHub.
Open: #112 (persistent + agent-aware deck — DECISION GATE, user), #114 (agent discovery), #113 (pin terminal), #115 (minimal scrollbar), #94 (agent API Phase 3: MCP — HELD on coord/94-ctl-mcp, user call), #33 (animations — parked/skip).
Recently closed: everything through #110 (shell integration) + #112 research brief committed (5ddf02c).
Freshness: 2026-09-04 (takeover) — main `5ddf02c`, gate green (324 tests: 320 lib + 4 integration).

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

- Worker protocol (earned, keep): fresh session per task by default (reuse only
  healthy mid-series); give work only to IDLE workers (verify prompt, no
  working/queued state); single Enter submits for codex (double-Enter only for
  claude); workers commit locally, never push/merge; finish = status marker +
  inbox ping; never steer a running worker — new specs only when the task is
  with the critic; codex restores keep reasoning effort high; NEVER merge a
  red-gated branch (#74/#80 lesson).
- Worker env: tmux session `personal`, one window per worker (user decision
  2026-09-04: no separate `termdeck-agents` session).

## Handoffs

Rotated history: `.coordinator/journal/` (latest archive: 2026-09).
- 2026-09-03 (end of day): main green 257 tests (`6c00bcc` + ledger). Shipped:
  #74 alt-screen wheel (incl. hotfix for a broken-merge lapse), #76 empty-stack,
  #75 app-width sizing, #77 declutter + scan, #66/#67 (#64/NB cleanup prior).
  Workers stopped; codex backend 404 / claude 529 — muse healthy. (Full note in journal.)
- 2026-09-03: #82 CI done (8f30398+20b0d87; gate green, --all-features;
  triggers every push/PR) — critic review; merge on user go (do-not-dispatch
  honored: not yet merged).
- 2026-09-03: PARKED #81 + #82 (user) — worktrees/branches removed locally, then
  fully DELETED local AND remote; user works them 2026-09-04. ONLY #33/#71/#72
  parked remain. All workers stopped; main green 257.
- 2026-09-04: Coordinator takeover (muse-spark). Baseline `cargo test --all-targets`
  green (257). Worker env restored in `personal` (codex-82, codex-81 windows).
  Worktrees `~/.worktrees/termdeck/{82-ci,81-split}` off origin/main `a0668a9`.
  #83 fix in progress; #82/#81 to codex next (parallel — no file overlap).
- 2026-09-04: #83 MERGED on main (`9fc38f6`, pushed). Dispatched #82 → codex in
  `personal:codex-82` (wt 82-ci) and #81 → codex in `personal:codex-81` (wt 81-split),
  both fresh sessions (gpt-5.6-sol/high), briefs + single-Enter submit, both Working.

## Durable resumption
- This file, `docs/RESUME.md`, and `docs/WORKSTREAMS.md` are the tracked source
  of truth. tmux scrollback and local agent conversations are disposable.
- Before changing machines, commit worker changes and push `main` plus every
  active `coord/*` branch.
- On a fresh machine, follow `docs/RESUME.md`, read the relevant workstream, and
  give it to a new agent. Do not rely on an old conversation for context.
- Private remote: `https://github.com/zo-ll/termdeck`. `main` and the active
  `coord/*` branches were first pushed on 2026-08-31.
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
- 2026-09-03: NB-cleanup batch dispatched to codex (idle): A1 DEFAULT_SCROLLBACK
  + expect() invariants; 64 feed-while-scrolled pin gap; #13 footer rule-row
  helper; #9 resize-scrollback NB (resolve or retire). coord/nb-cleanup.
- 2026-09-03: WORKER-SESSION POLICY changed (user, after codex/claude both
  died on limits): FRESH session per task by default; reuse only health-
  checked (mid-series + context >=~50% + not near limit); coordinator holds
  durable memory. Applied: nb-cleanup will use a FRESH codex session after
  the 2:56 PM reset.
- 2026-09-03: WORKER-SESSION POLICY changed (user): FRESH session per task
  by default; reuse only health-checked (mid-series + context >=~50% + not
  near limit); coordinator holds durable memory. Applied: nb-cleanup will
  use a FRESH codex session after the 2:56 PM reset (the old limited session
  is discarded).
- 2026-09-03: Claude's UI-DECLUTTER implemented as d25c97f on the STALE
  coord/50-runtime-add branch (needs re-root onto current main for review).
  Claude FLAGGED cross-ownership: touched src/session.rs (codex lane) to drop
  Deck::home/abbreviate + HOME lookup — pending the backend-impact scan.
  No ping due to missing finish protocol (fresh session) — protocol delivered
  to claude now; it must not push.
- 2026-09-03: USER: no user review of the declutter — sent to the CRITIC
  (with a mandatory authority-refresh honesty check: claude edited the
  reference/export itself). tmux-safety note: minimized window churn.
  Backend-impact scan (codex) still pending post-reset for the session.rs
  ownership part.
- 2026-09-03: Declutter critic PASS; authority refresh RATIFIED (implements
  the user's declutter order; lesson: implementers must get prior approval
  before editing the design authority). Integration: re-root d25c97f onto
  current main -> claude (coord/declutter). 3 NBs (incl. DESIGN.md
  sheet-background note) folded into the pending NB cleanup. Backend-impact
  scan (codex) still after reset.
- 2026-09-03: coord/declutter integrated (022c691, 238 tests; session.rs
  -7 dead-field removal carried; NB#1 fixed) — critic re-review queued.

## NEXT ACTIONS
TAKEOVER 2026-09-04 — new coordinator (pi). Baseline: main `5ddf02c`, 324 green
(320 lib + 4 integration). No workers in flight; no worktrees; relay alive;
inbox empty. Open set: #111 #112 #113 #114 #115 #94 #33.

1. #111 (BUG: explicit termctl notify not visible in the deck) — triage + fix.
   Root-cause pointers in the issue body: caller_pane() attribution in
   src/ctl/mod.rs vs Project::terminal identity normalization; render path in
   src/ui/deck.rs (NotifyKind::Message from non-active pane). One bounded slice;
   cross-lane ownership adjudicated at dispatch (ctl primary; ui render touch
   cross-approved via the #108/#84 precedent). No design-first gate — it is a bug
   in a shipped feature (#71/#97/#108) and currently blocks the fitness-agent
   notify workflow.
2. #113 (pin terminal at top of preview stack) — design-first: indicator +
   binding decision documented in a slice doc, then ONE bounded UI slice
   (Claude lane) with fixtures. Run AFTER #111 merges (shared ui/deck.rs).
3. #115 (minimal per-pane scrollbar, auto-hide on idle) — same shape as #113:
   design decision documented (master-first, display-only v1, injected-clock
   idle timer per the demoted()/TERMDECK_BLESS pattern) + ONE bounded UI slice
   (Claude lane). SERIALIZE with #113 (shared ui/deck.rs + ui/state.rs).
4. #94 (MCP adapter, held) — USER decision: live handshake test from the
   branch binary → merge, or drop. Until decided it stays on coord/94-ctl-mcp
   (REBASE onto main when picked up). Unblocks #114 layer 1.
5. #112 (persistent + agent-aware deck) — USER DECISION GATE. Brief §6 Q1–Q8;
   load-bearing Q1 (persistence desire A/B/both), Q2 (B's justification), Q3
   (constitution amendment: AGENTS.md/PLAN.md/DESIGN.md one-line clause for A;
   re-scope for B). Nothing dispatches from the brief until answered. If
   A+Pa+Pb approved → slices to codex (config/lifecycle resume; ctl event
   drain) + cross-lane Pb UI badge (claude).
6. #114 (agent discovery) — research-first: scope capabilities-verb shape /
   doc home / certification harness before impl. Layer 1 = #94 merge +
   handshake. Then slices: capabilities/schema verb (codex, ctl.v1 additive),
   termctl --help overhaul (codex), docs/agents/TERMDECK.md (docs).
7. #33 (animations) — parked; skip per user directive.
8. #116 (guaranteed peek — active screen main/alt; USER-approved design + queue):
   issue filed with acceptance criteria; worktree ~/.worktrees/termdeck/116-peek-active-screen
   (coord/116-peek-active-screen) + brief ready. Dispatch to the SAME codex session
   when #111 finishes+merges (rebases onto fresh origin/main at dispatch to avoid
   ctl/mod.rs overlap with #111). Then peek works on ANY pane incl. alt-screen TUIs
   (fixes this coordinator's blind spot at the root).
- 2026-09-04: TAKEOVER — previous coordinator closed (its slips: critic booted
  as plain shell/full-skill pi, codex workers off-recipe). This coordinator
  now runs window 0. Canonical env spec committed to the coordinator skill
  (0 coordinator, 1 critic muse-1.3 critic-skill-only, 2 claude-opus-high,
  3 codex-terra-high; relay + inbox drain + hard rules). Live layout:
  0 coordinator, 1 critic, 2 codex-81, 3 codex-82 (both workers in flight
  from the previous take-over's dispatch; monitored here).
- 2026-09-04: critic got fragmented boot-proto-fix (multi-line paste into pi
  composer -> per-line messages). Rule added to the coordinator skill:
  pi panes receive SINGLE-LINE pointers only; content in files. Critic
  recovered and is idle; #82 fully closed (re-review verified NB fixes).
- 2026-09-04: dispatched to codex lane: $N horizon-open investigation.
- 2026-09-04: #87 (horizon CLI commands fail in a termdeck terminal)
  dispatched to the codex lane (coord/87-horizon-open).
## LIVE STATE (pre-compaction snapshot)
- tmux personal: 0 coordinator (this), 1 critic (pi, muse-1.3, critic-skill
  only, idle), 2 claude lane (opus high, on #84), 3 codex lane (terra high/
  full access, on #87). Relay running (guarded real-message ping; inbox
  drained each turn). Main green 257, binary current.
- IN FLIGHT: #84 close terminals (claude) ; #87 horizon CLI commands in a
  termdeck terminal (codex). Critic idle → verdicts ping via relay.
- ALL ISSUES BY STATUS: closed: everything through #82. open/parked: #33
  animations, #71 notifications, #72 agent API (+ none else — #81/#82/#84/#87
  handled as above).
- PROCESS: pi panes single-line pointers only; only give work to idle
  workers; fresh session per task; coordinator commits/pushes/merges after
  critic passes + user says merge; NEVER merge a red gate.
- 2026-09-04: #84 done (claude 1987c9d, 272 tests, 15 new) — branch base
  stale (735b4ee pre-split); rebase onto origin/main conflicted in
  session.rs + ui/mod.rs (the #81 split targets). Integration routed back to
  claude (idle; owns the slice). Codex still on #87.
- 2026-09-04 (USER refinement, #84): closing the LAST pane should route
  through the existing QUIT-CONFIRMATION modal, not exit silently. Pending:
  deliver to claude's lane (idle) after its integration turn completes
  (never steer a working worker). If the user's composed line already
  reached claude, this is a no-op.
- 2026-09-04: #84 integration verified (claude 758dfaf on main: close_terminal
  -> session/lifecycle.rs, close_at+draw -> ui/deck.rs, close_column+HELP ->
  ui/chrome.rs, tests -> session/ui tests.rs; 272 green, snapshots honest, no
  orphans; NOT pushed). USER now steering claude directly (composer lines);
  REFINEMENT (last-pane close -> quit-confirm modal) still pending handoff
  when claude is clean-idle with no user-composed line.
- 2026-09-04: #84 MERGED (PR #89, 276 tests; binary rebuilt — ^g x +
  per-pane × live; last-pane close via quit-confirm). #87 merged earlier
  (PR #88). 2026-09-04 session: #81/#82/#84/#87 all shipped, critic-passed.
  All workers idle. Open/parked only: #33, #71, #72.
- 2026-09-04: RESEARCHER ROLE ADDED (user). New skill
  coordinator/references/research-protocol.md + researcher/SKILL.md + boot
  (muse-spark-1.3, researcher-skill ONLY, window 4 in the canonical env,
  single-line study pointers, brief at a deliverable path, finish ping).
  Research-first: parked/design items (#33/#71/#72) go to the researcher
  BEFORE implementation; user decides from the brief. My #72 hand-brief
  (docs/design/termdeck/agent-api-design-brief.md) may be pressure-tested by
  an independent researcher study.
- 2026-09-04 (USER): researcher is ON-DEMAND — NOT in the default env. The
  coordinator spawns window 4 only when the user asks to research (spawn
  recipe + research protocol in the coordinator skill), then kills the window
  after the brief lands. Env default = 0 coordinator, 1 critic, 2 claude,
  3 codex.
- 2026-09-04: #72 research delivered (researcher brief,
  docs/design/termdeck/agent-api-research.md, committed + pushed). Key:
  socket+JSON concurring with draft; PEEK BLOCKER (additive read-only
  frame/history trait method, default-body, needed for peek); input gated
  (TERMDECK_ALLOW_INPUT|--force); MCP = adapter; Q1-8 answered. Awaiting
  USER decision (adopt A? §4a allowed? input policy? close-last --force?)
  before Phase-1 dispatch (rendezvous + read verbs).
- 2026-09-04: agent-api IMPLEMENTATION SPEC written + pushed
  (docs/design/termdeck/agent-api-spec.md; #72 comment). Decisions fixed:
  socket+JSON, history_lines additive method, env trust + TERMDECK_PANE,
  input gated, close-last/close-self guards, nested override, MCP adapter.
  Phases 1/2/3 with acceptance. NEXT: user approval -> Phase-1 issue +
  dispatch to codex lane (engine/contracts/CLI ownership).
- 2026-09-04: USER APPROVED agent-api Phase 1. Issue #90 opened
  (rendezvous + read surface), coord/90-agent-ctl worktree, dispatched to a
  FRESH codex session (kill+relaunch, full access, terra high).
  Authority: docs/design/termdeck/agent-api-spec.md.
  Critic idle; claude idle. #90 ping -> gates -> critic -> merge.
- 2026-09-04 (LESSON): codex corrections can sit UNSUBMITTED — a send-keys
  Enter inside the bracketed-paste stream never submits. Verify pane state
  after every codex dispatch (Working vs pointer at prompt); one retry at
  most, never while Working (twin-ping guard). Baked into the coordinator
  skill. The #90 correction was delayed 1 turn this way; now running.
- 2026-09-04: Phase 2 (#92 control verbs) dispatched to FRESH codex
  (coord/92-ctl-verbs, on current main incl. Phase 1). Gates: input/close
  --force/close-self/sheet. NOTE: gh issue create output captures the URL
  ($N trap, 3rd time) — always use the literal issue number afterwards.
- 2026-09-04: Phase 3 (#94 MCP adapter) dispatched to FRESH codex
  (coord/94-ctl-mcp on current main). Pin MCP spec version at impl time;
  docs snippets pi/claude/codex. Agent API Phases 1+2 already MERGED
  (PR #91, #93; binary rebuilt; 288 tests main).
- 2026-09-04: #94 MCP PASS (protocol 2026-07-28 verified externally; 3 SHALL
  nits; no live claude/codex handshake test on this machine). #95 Ctrl+J
  PASS pending (queued). OPEN: user merge/drop call on MCP; #95 merge.
- 2026-09-04 (USER): MCP (#94) HELD on its branch — coord/94-ctl-mcp stays
  pushed/unmerged (critic-passed, protocol 2026-07-28 verified). Live
  handshake test deferred to the user from the branch binary. Issue #94 open
  + marked held. Rebase onto main when picked up.
- 2026-09-04: #72 CLOSED (umbrella done: Phases 1+2 merged #91/#93; Phase 3
  held on coord/94-ctl-mcp via #94). Open set now: #33, #71, #94 only.
- 2026-09-04: RESEARCH (#71 notifications + #33 animations) — researcher
  window spawned (on-demand); study-71 dispatched first (sequential; #33
  after its ping). Deliverables: notifications-research.md +
  animations-research.md.
- 2026-09-04: RESEARCH DONE for #71 + #33 (researcher window retired).
  Briefs committed: notifications-research.md, animations-research.md;
  #71/#33 comments posted. Awaiting USER decisions (directions, slices,
  sequencing — #33 pattern before #71 visual per the researcher note).
- 2026-09-04 (USER): SKIP animations (#33 stays parked); IMPLEMENT
  notifications (#71) — slices 1+2 (explicit ctl notify + BEL -> flash +
  non-modal toast; OSC-777 seam deferred). #97 opened, dispatched to FRESH
  claude (coord/97-notify; authority notifications-research.md §4; vt.rs
  BEL hook + ctl wiring cross-ownership approved by coordinator).
- 2026-09-04: #71 IMPLEMENTED (slices 1+2) + MERGED (PR #98, 307 lib; binary
  rebuilt — BEL/ctl-notify flash + toast + census live). #71 CLOSED; S3
  OSC-777 optional follow-up noted. Open set: #33 (parked) + #94 (held MCP)
  only.
- 2026-09-04: CI-EMAIL ROOT-CAUSED + FIXED (#99): the ONLY recurring CI
  failure was shutdown_terms_all_groups... (native.rs:588 zero-tolerance
  grace assert, flaked by ms under load -> 10 failed-run emails). Window
  assert (grace-1/4 .. 2x) merged (PR #99, test-only) — grace 5x at
  2.01-2.03s; sibling fast-path untouched. Emails stop. USER has a new
  task for claude (lane idle).
- 2026-09-04: #101 DESIGN IMPLEMENTED + MERGED (PR #102; 
  status row -> canvas row 1 all layouts; notify right-slot; 14 fixtures
  reblessed pure; critic pass; nit recorded). 4 DESIGN.md DOC-CONFLICTS
  FLAGGED by the worker (stale committed export / DesignSync-vs-MCP note /
  SPEC grid / split-divider row numbers) — PENDING USER: update DESIGN.md
  or leave.
- 2026-09-04: DESIGN SWAP — user updated the canvas on Windows desktop
  (Termdeck TUI mockups.zip, 17:51); swapped reference/ export into the repo
  (coord/102-design-swap, commit 0f18ff0: status bar now ROW 1 in the canvas
  = confirms #101 impl; 'idle 6m'->'job done' sample). Claude reviewing the
  updated design vs main (docs-sync check; job-done verdict; remaining
  DESIGN.md conflicts 2-4).
- 2026-09-04: #104-fold-footer (ISSUE #103 — GitHub shares one number space
  with PRs, so the footer issue = #103; branch coord/104-fold-footer)
  dispatched to FRESH claude: cut the fold-census footer per the updated
  canvas (audit behavior first; honest rebless; gate 309+3). #102-design-swap
  done (2 drifts, swap confirmed). Backend scan (#103-branch
  coord/103-backend-scan) CLEAN — no fixes.
- 2026-09-04: #103 footer FIX merged (PR #105, 9474754->main; binary rebuilt —
  fold-census footer gone per canvas; 312 tests). Docs-swap (PR #104) merged
  BEFORE it (correct order thanks to claude's flag). PENDING USER: the slice
  .md docs (collapse-stack.md/scrollable-stack.md/split-divider.md) still
  describe the footer + the 5 canvas doc-notes (DesignSync-vs-MCP text, SPEC
  grid, divider rows 0-39, zoom corner card, narrow double bar) + '1-4 vs N'.
- 2026-09-04: DOCS SWEEP (#106) dispatched to FRESH claude
  (coord/106-docs-sweep): slice docs to the canvas (footer deleted, row-1
  bar, promote-N, DESIGN.md 5 notes) + ZERO horizon mentions in docs/README
  (examples/*.yaml stay; code/horizontal untouched). Docs-only; grep-verify.
- 2026-09-04: #106 DOCS SWEEP MERGED (PR #106? — PR number captured; critic
  pass; docs-only; main 312 tests). Horizon = 0 in docs/README except the
  fidelity-pinned canvas reference export + a draft (20 hits) — USER CALL:
  re-export canvas or leave.
- 2026-09-04: SHELL INTEGRATION direction approved (auto-notify on command
  completion). Research dispatched (#107-branch study): when-to-notify rules,
  mechanism (BEL vs private sequence via the OSC seam), install via PTY env,
  coverage/security/determinism. Researcher window on-demand.
- 2026-09-04: SHELL-INTEGRATION issue #108 (dir 107-shell-integration,
  branch coord/109-shell-hook when implementing; no worktree yet).
  Cleaned: #97 (slice, merged) + #100 (record) closed now.
- 2026-09-04: #108 slice 1 MERGED (PR #110; shell hooks live in the binary
  — bash/zsh panes now auto-notify on exit!=0 or >=10s; private OSC 7777 via
  pre-scan decoder). Critic pass after the zmodload fix. Slice 2 pending
  (fixtures, identical-message refinement, fish?, nested-marker eyeball).
  NOTE: this machine has NO zsh binary — zsh path verified by construction.
- 2026-09-04: #108 slice 2 dispatched to FRESH codex (coord/111-shell-slice2:
  identical-message no-rearm (ui/state.rs, cross-ownership approved), rich
  keyframes, fish-or-defer, nested TERMDECK_SHELL_HOOK env fix, README para.
- 2026-09-04: #108 SHELL INTEGRATION COMPLETE (slices 1+2 merged; #108 closed;
  binary rebuilt — bash/zsh/fish panes auto-notify on exit!=0 or >=10s;
  TERMDECK_NOTIFY knobs; no-rearm; nested-safe). Open set: #94 (MCP held),
  #33 (animations parked). Main 320 lib.
- 2026-09-04 (TAKEOVER): new coordinator (pi/muse) — env detected termdeck
  (termctl live; master pane only; NO worker panes exist — recreate per the env
  runbook at dispatch). Baseline: main `5ddf02c`, 324 green; the EOD snapshot's
  75deb3d gained the research brief 5ddf02c. Relay (relay-termdeck.sh pid
  31177) alive; inbox empty — relay-log pings 21:58–22:53 are the
  FITNESS-AGENT project's tasks (delivered/consumed, not termdeck). tmux
  `default` holds termdeck/pi windows only. Open set refreshed (see Status).
  NO dispatch this turn — analysis + plan delivered; #111, then #113/#115
  (serialized on ui/deck.rs) ready on user go; #94/#112 are user decision
  gates. HYGIENE DEBT: COORDINATION.md rotation long overdue — handoff bullets
  live in THREE sections (## Handoffs, Durable resumption, LIVE STATE), not
  one; consolidation + eviction of the oldest >15 to the journal (most 09-03
  history is already archived) still owed.

- 2026-09-04 (WAVE 1+2 DISPATCH): user approved waves 1+2 in parallel. Worktrees
  built off origin/main `c0c6a4a`: `~/.worktrees/termdeck/111-notify-fix`
  (coord/111-notify-fix) + `~/.worktrees/termdeck/113-pin` (coord/113-pin).
  Briefs written: `.scratch/tasks/111-notify-fix.brief.md` +
  `113-pin.brief.md`. Panes via `termctl open`:
  - 111-notify-fix (codex FULL interactive session; user: no exec one-shots):
    astra attempt FAILED 400 — "'astra' model is not supported when using
    Codex with a ChatGPT account" (account-tier, NOT CLI version). USER: switch
    back to terra. On relaunch codex SELF-UPDATED 0.147.0→0.153.4 (standalone
    installer ran at launch, "Please restart Codex") — the `az` window became
    moot. Relaunched codex -s danger-full-access -m gpt-5.6-terra on 0.153.4
    (trust prompt skipped — project already trusted). #111 BRIEF DISPATCHED
    and VERIFIED WORKING ("I'll read the task brief... then implement...").
  - 113-pin (claude FULL session; user: no exec one-shots): launched
    `claude --dangerously-skip-permissions --model opus --effort high`;
    folder-trust prompt CONFIRMED. Pane STILL renders BLANK to termctl peek
    (process alive, ~4% CPU, 5min) — claude's alt-screen TUI is not captured
    by peek (codex's is). canNOT verify idle ⇒ #113 NOT dispatched (hard
    rule). USER eyeball pending: is 113-pin at claude's composer in the deck?
  - Pending: #115 (scrollbar) queued on claude lane AFTER #113 merges.
  - LESSON (user-caught, both lanes): I killed the claude pane on a misread —
    flat CPU + blank peek ≠ idle; claude is network-bound (waits on the API,
    low CPU mid-turn) and its TUI alt-screen is invisible to termctl peek.
    Transcript evidence later confirmed the killed session (e6f58c5a, 583KB)
    was WORKING (brief received; last-prompt mid-cycle). Old claude had NOT
    written to disk (worktree clean) — nothing lost. New claude re-dispatched,
    verified working via transcript. RULE (baked in): never kill a worker on
    ambiguity; when peek is blind, verify via the claude session transcript
    (~/.claude/projects/<munged-dir>/*.jsonl — user msgs + last-entry types
    show idle-vs-working) or ask the user (deck is visible to them); finish
    markers/pings are the durable source of truth.
- 2026-09-05: #111 DONE (codex commit 0142ee9 "Fix notify pane path attribution",
  marker RESULT=pass: "Normalized explicit notify pane paths and added coverage;
  full gate green", 1 commit, worktree clean). RELAY WEDGE discovered: relay
  process alive but loop frozen since 22:53 (fitness-agent era) — no ARRIVE for
  the 02:23 ping; restarted detached (pid 110637) → ARRIVE+DELIVER ok, inbox
  consumed. #111 routed to CRITIC (spawned pane "termdeck": pi
  muse-spark-1.3-contributor, critic-skill only, assignment
  .scratch/review/111.critic.md, single-line pointer; critic Working — reading
  diff at source). Verdict pings via inbox 111-notify-fix.critic.ping.
- 2026-09-05: #116 MERGED (user approval) — --no-ff merge (f57ab6a), gate green
  324 lib + 4 int, pushed; issue #116 CLOSED with smoke comment; binary
  rebuilt → ~/.local/bin/termdeck (restart loads #111+#116 together).
  Result: termctl peek now returns the ACTIVE screen (main/alt, "screen"
  field) — full-screen TUIs visible — this coordinator's peek blind spot is
  fixed at the root (per-user design approved at dispatch).
- 2026-09-05: #113 DONE (claude, 2 commits: 50f2578 feat(ui) + 9b2ea67 docs;
  marker RESULT=pass: "^g p pins the master to the stack top (held across
  promotion, one pin, accent mark + status-row unpin key); doc + 14 tests"
  gate 338 lib + 4 int — base 324 + 14 new). Design doc committed:
  docs/design/termdeck/pin-terminal.md. REBASED onto current origin/main
  (clean — no ui overlap with #111/#116); gate re-run: 1 flaky fail =
  known sandbox env flake (shutdown_terms…), re-run GREEN. Routed to CRITIC
  (assignment .scratch/review/113.critic.md — includes design-doc-vs-code
  agreement check, one-pin/promotion-demotion semantics, boundary check for
  contracts/engine, honest-fixtures check, mode-interplay matrix).
  Verdict ping inbox 113-pin.critic.ping. Claude lane IDLE; #115 STILL queued
  until #113 merges (shared ui/deck.rs + ui/state.rs — serialization per plan).
- 2026-09-05: #113 CRITIC VERDICT = PASS (inbox 113-pin.critic.ping). Design
  doc settles ALL open points (pin-held-not-spent, one pin, ^g p on master,
  unpin cost, ACCENT mark + conditional status key, mode matrix, runtime-only);
  implementation matches point-by-point (demotion-of-pinned-master → pin slot
  asserted in tests; unprefixed p passes to shell). Boundary honored: zero
  contracts/engine/ctl/config changes; runtime-only (pinned nowhere outside
  src/ui/); fixtures honest + unpinned-byte-identical guard test; PLAN.md
  binding table untouched (doc §6 discloses). 14 tests full matrix, fail
  pre-fix; gate green FIRST run 338 lib + 4 int (flake absent this time).
  NITS: (1) toggle_pin "returns whether anything changed" doc overpromises —
  always changes with a master present; (2) pinned().is_some() && …==active()
  double-call style nit in deck.rs. WAITING: user merge approval for #113;
  then merge + close + rebuild + dispatch #115 to claude.
- 2026-09-05: #113 MERGED (user approval, "merge and dispatch") — --no-ff
  merge (8a9be03), gate green 338 lib + 4 int, pushed; issue #113 CLOSED with
  smoke comment (^g p; restart loads #111+#113+#116). Binary rebuilt.
- 2026-09-05: #115 DISPATCHED to the claude lane — worktree
  ~/.worktrees/termdeck/115-scrollbar (coord/115-scrollbar) created on
  POST-#113 main (zero overlap by construction); brief
  .scratch/tasks/115-scrollbar.brief.md (design-first: slice doc
  docs/design/termdeck/scrollbar.md to settle panes/position/idle-timer/
  scrollback-mode/live-tail/display-only + injected-clock determinism +
  honest fixtures + full mode coverage). Claude verified idle (transcript: 1
  prompt, no new work) BEFORE dispatch; pointer landed (one lost-Enter
  retry; transcript shows the #115 prompt + 29 new entries) — WORKING.
  In flight: claude→#115. Open set: #112 (user gate), #114, #94 (held), #33.
- 2026-09-05: #116 CRITIC VERDICT = PASS (inbox 116-peek-active-screen.critic.ping):
  all 6 acceptance criteria met; active-suffix with history fallback; screen
  field derived from same adapter metadata (can't disagree with grid); error
  codes intact; tests discriminating (fail pre-fix, ctl test diverges history
  vs active content, asserts the ACTIVE tail). Gate green. NITS:
  (1) FakeEngine::active_screen_lines unset → Some([]) shadows the history
  fallback (behaviorally identical today; None-when-unset would be more
  faithful); (2) base pre-#111 merge note — critic PREDICTED clean rebase.
  REBASE DONE (clean, no conflicts — peek hunk untouched by #111's dispatch
  lines, exactly as the critic predicted): coord/116-peek-active-screen now
  = commit d4a7622 on post-#111 main; gate re-run green 324 lib + 4 int.
  WAITING: user merge approval for #116.
- 2026-09-05: #111 CRITIC VERDICT = PASS (inbox 111-notify-fix.critic.ping).
  Attribution-only fix: path normalization (symlink-spelling + tilde) + 2 tests
  (fail pre-fix / pass post-fix), gate green 322 lib + 4 int, deck.rs NOT
  touched ⇒ zero overlap with #113. Root cause confirmed: render path already
  handled non-active-pane NotifyKind::Message (flashing strip, census mark,
  non-modal toast for hidden panes, #71 non-modal principle intact — existing
  snapshots prove it); the bug was the caller_pane() identity mismatch only.
  NITS (recorded, non-blocking): (1) worker's .done/ping lacked user smoke
  steps (critic supplied them); (2) temp-dir cleanup skipped on assert-failure
  in one test; (3) no render-level test — justified by pre-existing snapshots.
  ENV FLAKE noted: engine::native::tests::shutdown_terms_… fails identically
  on pristine origin/main in the sandbox (process-group restriction), passes
  on re-run — pre-existing, unrelated to the 1-file ctl diff.
  RESOLVED: merged + closed below.
- 2026-09-05: #116 DONE (codex commit 34fe3d9 "Make termctl peek read active
  screens", marker RESULT=pass: "Peek now reads active main or alternate grids
  with screen metadata; full gate green", worktree clean). Routed to CRITIC
  (same critic pane, assignment .scratch/review/116.critic.md, verdict ping
  inbox 116-peek-active-screen.critic.ping). Base note: #116 branched PRE-#111;
  main still awaits the #111 merge — integration rebase of #116 planned once
  both merges are approved (ctl/mod.rs both touch, small overlap).
- 2026-09-05: #116 (guaranteed peek, alt-aware active screen) FILED (issue
  created), worktree coord/116-peek-active-screen + brief ready; DISPATCHED to
  the idle codex session (same full session, per user preference) and verified
  Working. #116 runs parallel to the #111 critic pass; at merge, rebase #116
  onto origin/main before/with integration (ctl/mod.rs overlap with #111).
- In flight: critic→#111 verdict; codex→#116 Working; claude→#113 Working
  (transcript-verified mid-implementation). #115 queued on claude lane after
  #113 merges. RELAY lesson: verify relay alive (log tail) when a ping is late.

- 2026-09-05: #115 DONE (claude, 2 commits: 80e1412 feat(ui) + 34d49f0 docs;
  marker RESULT=pass: "master-only scrollbar drawn into the pane border
  (zero-cost), raised by a scroll for 4s or by ^g [ mode, overflow-only,
  display-only; doc + 10 tests; gate green 352 (348 lib + 4 integration)").
  Design doc committed: docs/design/termdeck/scrollbar.md. Base = post-#113
  main (current; no rebase needed). Routed to CRITIC (assignment
  .scratch/review/115.critic.md — doc-vs-code agreement, zero-content-cost
  border draw, injected-clock determinism, coexistence with #113 pin mark +
  stack gutter, boundary check, full-mode test matrix). Verdict ping inbox
  115-scrollbar.critic.ping. Claude lane idle; queue is near-empty after #115
  (open: #112 gate, #114, #94 held, #33 parked).

- 2026-09-05: #115 CRITIC VERDICT = PASS (inbox 115-scrollbar.critic.ping).
  Doc settles all five design points (master-only, border-track, 4s restartable
  window, mode-pins-no-countdown, output-is-not-a-scroll, overflow-only,
  display-only) with rejected alternatives; code matches every load-bearing
  claim: zero-cost draw overwrites rendered border cells (no reflow), zero
  wall-time in diff (injected clock), overflow-only even in mode, tail
  anchoring preserved corner, per-pane arming + master-only draw (preview
  wheels raise nothing). ONE justified boundary exception: src/session.rs
  (+27/-4) calls the new DeckState::{mark_scrolled,scrolling} API at the 3
  existing scroll dispatch sites (keyboard Reaction::Scroll, esc-to-live,
  wheel) — no contract/engine/ctl change (ScrollbackPosition reused).
  Coexistence with #113 pin mark + stack gutter proven; fixtures honest
  (scrollbar.txt new; scrollback.txt 1-cell rebless; byte-identical settled
  frames). 10 deterministic tests, full matrix, fail pre-fix. Gate: first run
  hit the documented sandbox flake; clean re-run green 348 lib + 4 int
  exactly as expected. NITS: (1) session.rs second UI-lane exception worth
  a lane-note line (first was #13's rule-row helper); (2) SCROLLBAR_WINDOW
  duplicates NOTIFY_WINDOW's 4000 with its own rationale.
  WAITING: user merge approval for #115 (then merge + close + rebuild).

- 2026-09-05: #115 MERGED (user approval) — --no-ff merge, gate green
  348 lib + 4 int, pushed; issue #115 CLOSED with smoke comment; binary
  rebuilt (one restart loads #111+#113+#115+#116). WAVE COMPLETE — this
  session shipped 4/4: #111 notify attribution, #113 ^g p pin, #115
  scrollbar, #116 alt-aware peek. ALL LANES IDLE (claude, codex, critic).
  Open set: #112 (persistent+agent-aware deck — USER DECISION GATE),
  #94 (MCP held on coord/94-ctl-mcp — user handshake/merge/drop),
  #114 (agent discovery — layer 1 = #94; non-MCP slices could start),
  #33 (animations — parked/skip). Next candidates: #112 verdict → Pa/Pb/A
  slices; or #114 non-MCP slices on the free codex lane; or #94 merge/drop.

- 2026-09-05: ASTRA REPO AUDIT (user-requested, tmux `default:1`, codex
  gpt-6-astra; hit its ChatGPT usage cap while self-filing — coordinator took
  over). Verdict: KEEP the project, prioritize HARDENING over new features;
  five high-priority boundaries (#117-#121) first. 15 findings filed as issues
  #117-#131 (label `astra-audit`; bug for 1-10, enhancement for 11-15), each
  with locations + reproductions + fix direction. Priorities: FIRST #117-#121
  (blocking socket/PTY UI freezes, job cleanup, bracketed paste, reusable pane
  IDs); SECOND #122-#126 (API lies, bash -l hook, scrollback config, timing
  metadata, ANSI/mode-aware input); THIRD #127-#131 (picker CPU/resize, Git
  metadata, CLI/docs/README + research-brief overstatements, tests/CI, resume
  #112/#114/#94 on a hardened baseline). LOAD-BEARING for #112: the audit
  flags agent-aware-deck-research.md overstatements (persistence blocked
  "only" by policy; tmux "cannot" support approval; false universals) —
  correct the brief BEFORE the decision gate leans on it. Also: "integration
  tests" label is inaccurate (4 termctl unit tests); termdeck IS a multiplexer
  (multiplexes PTYs; detach/reattach is separate). Transcript saved:
  /tmp/astra-audit-pane.txt (2003 lines) + tmux default:1 scrollback.
  SUGGESTED WAVE ORDER: audit findings first (#117-#121 → critic → merge
  each), then #122-#126, then #127-#131; #112/#114/#94 resume after.
  No dispatch made — awaiting user direction on the audit queue.

- 2026-09-05 EOD (ALL LANES IDLE, safe to close): wave complete (#111/#113/
  #115/#116 shipped + critic-passed + merged + closed; binary rebuilt — one
  deck restart loads all four + the notify/peek fixes). ASTRA AUDIT queue
  (#117-#131, label astra-audit) DEFERRED TO TOMORROW per user — no dispatch
  today. Tomorrow's plan: start with the FIRST-priority boundaries #117-#121
  (blocking socket/PTY freezes, job cleanup, paste, reusable pane IDs) → each
  slice → critic → user merge approval; research-brief corrections inform
  #112's gate before decisions lean on it. Resume via COORDINATION.md +
  docs/RESUME.md. Transcript pointer (ephemeral, non-durable): /tmp/
  astra-audit-pane.txt + tmux default:1 scrollback — ALL material content is
  in the issues themselves.

- 2026-09-05 RESUME (same day, user: "we resume now"): env check — inbox
  empty, no markers, relay DEAD (deck restart killed it) → restarted
  (relay-termdeck.sh, detached); only master pane survived → worker panes
  recreated. AUDIT WAVE 1 STARTED: #117 (nonblocking bounded control-socket
  handling) worktree coord/117-socket-handling off f42b274, brief written,
  codex pane relaunched (full session, gpt-5.6-terra, idle user-confirmed —
  peek blind on its boot screen this time), #117 DISPATCHED + verified
  Working. Critic pane recreated (pi muse-spark-1.3-contributor, skill-only,
  booted). WAVE PLAN (audit order): #117→#118→#119→#120→#121 sequentially on
  the codex lane (all engine/session/ctl — no UI overlap), each → critic →
  user merge approval → next. Claude lane: holds unless user opts into a
  parallel non-overlapping slice (candidates: #127 picker, #129 docs).
  ENV NOTE: the deck restart loaded the #116 alt-aware peek — claude's TUI
  is now VISIBLE via termctl peek (blind spot fixed in practice, verified
  this session); claude pane recreated at the 113-pin worktree (full
  session, opus, idle) so the canonical env (coord|critic|claude|codex)
  is whole.
- 2026-09-05 (PARALLEL + TOKEN CONTINGENCY): user: parallelize — #127
  (picker CPU cache + resize repaint) DISPATCHED to claude (WORKING;
  worktree coord/127-picker-cache off main; cross-lane minimal touch at
  src/session.rs:80-85 coordinator-approved; zero overlap with #117's
  ctl/termctl files). USER DECISION (token plan): codex weekly usage
  <10% — when terra runs out, the WORKER LANE becomes PI with
  muse-spark-1.3-contributor (critic-style launch: pi --provider
  opencode-go --model muse-spark-1.3-contributor), single-line pointers
  only, same finish protocol — NOT codex pointed at muse (codex 0.153
  has no opencode-go provider surface; user explicitly wants pi+muse).
  Switch timing: complete #117 on codex first (never kill mid-slice),
  then relaunch the lane as pi+muse for #118+ if tokens are gone.
  Note: muse is critic-grade — worker quality may drop; user owns this
  tradeoff. In flight now: codex→#117, claude→#127 (parallel).

- 2026-09-05: #117 DONE (codex commit b90828e "fix: bound control socket
  clients"; marker pass: "Bounded nonblocking round-robin ctl sockets plus
  termctl response deadline and cap; full gate green"; worktree clean).
  Routed to CRITIC (assignment .scratch/review/117.critic.md — checks both
  defect-classes-fixed + failing-pre-fix tests, blocking-mode removal,
  partial-response progress, round-robin fairness, termctl cap+deadline,
  ctl.v1 wire/error-code preservation, one-request-per-frame invariant,
  gate). Verdict ping inbox 117-socket-handling.critic.ping. Codex lane back
  at prompt, idle. #118 (nonblocking PTY input) NEXT — lane choice pending
  user (codex tokens <10% vs pi+muse switch).
- 2026-09-05: #117 CRITIC VERDICT = PASS (inbox 117-socket-handling.critic.ping).
  AC1a starve: fixed — VecDeque<Pending> (MAX_CLIENTS 16), round-robin,
  test asserts ordering (pre-fix single slot would fail). AC1b block:
  fixed — blocking write_all gone; begin_response ≤2MB cap; ≤16KiB
  nonblocking writes, WouldBlock→requeue; test discriminates pre-fix
  (2MB−1K payload, 1K RCVBUF, <500ms assert). AC2 nonblocking both
  directions + 2s request/response deadlines + bounded buffers (64K
  line/16K chunk/2M response/16 peers). AC3 termctl call_with_limits
  (2s deadline, 2M cap), bin/termctl.rs unchanged, exit codes + parse
  tests untouched. AC4 ctl.v1 envelope + codes 1/2/3 intact; oversize→
  code 1 'exceeds protocol limits'. AC5 gate: 352 lib + 4 + 0 green
  FIRST run (no flake). One-request-per-frame preserved. NB notes:
  zero-write-as-complete (harmless), one test's wall-clock margins
  (load-sensitive, acceptable), no binary-level deadline test (thin
  wrapper, justified). WAITING: user merge approval for #117.

- 2026-09-05: #117 MERGED (user approval) — --no-ff merge (64b28f6), gate
  green 352 lib + 4 int, pushed; issue #117 CLOSED; binary rebuilt (deck
  restart loads it). Audit wave progress: FIRST-priority 1/5 done.
- 2026-09-05 (LANE SWITCH, user): codex tokens <10% — worker lane is now PI +
  muse-spark-1.3-contributor (NOT codex-toward-muse; user decision). New
  worker skill committed at /home/az/.pi/agent/skills/worker/SKILL.md (role:
  implementation worker; brief-as-contract; worktree-only; commit-local;
  never push/merge/PR/tracker; checkpoints; marker+ping finish). Codex pane
  closed. #118 (nonblocking bounded PTY input) worktree coord/118-pty-input
  off post-#117 main + brief written; worker pane "118-pty-input" launched
  (pi muse-spark-1.3-contributor, worker skill) and DISPATCHED — verified
  Working (exploring engine contracts). In flight: pi-worker → #118; claude
  → #127 (parallel). Critic idle.

- 2026-09-05: #127 DONE (claude, 2 commits bd29b50 perf/ui cache +
  0c68a66 fix/session rebuild-only-when-drawn; marker pass: "listings/search/
  roots cached with mtime invalidation, loop rebuilds only for drawn frames,
  resize alone repaints; idle CPU 21.6%→0.2%; 353 lib + 4 bin tests green";
  worktree clean; claude idle at composer). Routed to CRITIC (assignment
  .scratch/review/127.critic.md — mtime invalidation correctness, session.rs
  surgical-check (approved 80-85 region), resize-only dirty, picker behavior
  regression, gate 353+4). Verdict ping inbox 127-picker-cache.critic.ping.
  NABE (=next after #127 merges): #128 picker Git metadata + silent run-add
  failures (claude lane; same files ⇒ after merge).

## EOD 2026-09-04 (pre-close snapshot)
- main 75deb3d · 320 lib + 4 integration · binary current ~/.local/bin/termdeck
- tmux personal: 0 coordinator | 1 critic (idle) | 2 claude (idle) | 3 codex
  (idle) | researcher on-demand; relay alive; inbox empty.
- OPEN: #94 MCP (held on coord/94-ctl-mcp, user call) · #33 animations
  (parked). NOTHING in flight; safe to close.
- Durable-resumption: read COORDINATION.md + docs/RESUME.md + journal
  (the skills repo has the coordinator/critic/researcher specs incl. the
  canonical env + protocols).
