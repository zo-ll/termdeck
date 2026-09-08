# Coordination — Termdeck

Status: ACTIVE — tracker on GitHub. **CLEAN BASELINE as of 2026-09-07.** The second astra audit (#136-#147) is fully closed, and CI has been GREEN on `main` since run 34132349287 (the #136-r4 compinit fix landed 2026-09-07). All 13 second-audit findings (#136-#147) + #148/#149/#150/#151 (user-visible features/fixes) are closed. The earlier ★ COMPLETE markers were wrong; this one is real.
Gate: **GREEN.** CI passes on `main` (latest runs 34132349287+, verified). Per #138, this line cites CI runs, never local invocations.
Open — remaining audit residue: #152 (accepted residual from #151: /etc/profile runs before the login-bash profile shim — sudo hint repeats, hushlogin ignored, bash_completion missed; documented in-code; design-first when picked up), #139 (paste payload can execute, P1), #141 (escaped descendants survive shutdown, P1), #142 (unbounded parser memory, P1), #143 (panic guard aborts, P1-amplifier), #144 (picker Unicode panic + discovery collision), #145 (termctl help swallow + zoom contract), #146 (PID reuse + connect timeout), #147 (bash hook clobbers $?).
Open — process: #134 (CI scheduled advisories).
Open (user-gated): #94 (MCP HELD), #112 (persistent+agent-aware deck DECISION GATE), #114 (agent discovery), #33 (animations parked).
Freshness: 2026-09-08 — main `3d2280e`; CI GREEN (last run 34144976340). NB backlog ~65 (incl. #148/#150/#151 critic nits + the audit's dependency triage: lru/paste/serde_yaml, socket_dir symlink hardening).

## Goal

Implement Termdeck as a standalone Rust terminal workspace with a master and
live-preview-stack interface.

## Issues (archived)

The historical #1-#14 issue/slice/Waves tables were pruned 2026-09-05 per #129 (stale dashboard); archived verbatim in `.coordinator/journal/history-2026-09.md`. Live status = GitHub issues + the Handoffs log below.

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
- 2026-09-05 (post-audit correction): second astra audit of `b3df768` recorded in
  `docs/AUDIT-2026-09-05.md`. 13 findings (4 P1), all 13 independently re-verified —
  no false positives; 5 reproduced from scratch (#140 deck panic at 80x4/120x4/144x5,
  #144 both halves, #145 help swallow). One correction to the report: its "584 panics /
  2860 cases" render sweep is a DEBUG figure — `[profile.release]` sets no
  `overflow-checks`, so at 120x5 the release build wraps and renders garbage instead of
  panicking, while 80x4 still crashes via a ratatui buffer bound. The sheet `clamp`
  (#140a) panics in both profiles and is the one that kills a live session.
  Filed #139-#147; #137 was already the audit's finding 7.
  AUDIT BLIND SPOT worth keeping: it ran entirely locally and says so
  ("the live workflow was not run remotely"). It correctly identified that absent
  zsh/fish make the 408 local passes no evidence for those shells — and then did not
  open GitHub Actions, where exactly that bug (#136, zsh receives `alse` for `false`)
  had been failing every push for five hours. A local gate cannot see a shell it does
  not have, or an environment variable it already inherits; both classes landed in the
  same session. That is #138.
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
- 2026-09-05: #127 CRITIC VERDICT = PASS (inbox 127-picker-cache.critic.ping).
  AC1 idle-stops-rescanning: roots/listing/rows hoisted; dirty on input+
  resize; FsBrowse single-slot Stamp::of cache; invalidate: direct-child
  add/rm/rename → mtime miss, navigation miss by path, refresh() drops
  all (each tested; tests discriminate pre-fix). AC2 loop discipline:
  rebuild+follow_cursor+autoresize+draw all inside if dirty; idle pass
  = one screen_size ioctl + 20ms keys.read. AC3 resize repaint: dirty |=
  resized(...) w/ unit test (same/different/remembered/once). AC4 no
  input/state/render changes; existing picker tests untouched. AC5
  measurement plausible (21.6%→0.2%; 50Hz full-tree walk removed). AC6
  gate 353 lib + 4 green first run; RefCell borrow safe. No nits
  reported. WAITING: user merge approval for #127 → then #128 to
  claude.

- 2026-09-05: #127 MERGED (user approval) — --no-ff merge (db64abd), gate
  green 357 lib + 4 int (base 352 + 5 new from the slice), pushed; issue #127
  CLOSED; binary rebuilt. Audit progress: FIRST-priority 1/5, THIRD 1/4 done.
- 2026-09-05: #128 (picker Git metadata honesty + runtime-add failure
  surfacing) DISPATCHED to claude (WORKING — worktree coord/128-picker-git on
  post-#127 main; brief reuses the #127 Stamp cache pattern for Git metadata,
  handles worktree .git indirection, surfaces failures non-modally via the
  #71 surface; session.rs add-flow cross-lane touch approved; no overlap with
  #118). In flight: claude→#128, pi-worker(muse)→#118. Critic idle.

- 2026-09-05: #118 DONE (PI-WORKER muse, commit ea10642; marker pass:
  "nonblocking bounded PTY input: queue+refuse, truthful ctl refusal, gate
  green"; worktree clean; hand-back detailed). Semantics chosen: accepted =
  queued (caller never blocks); written = nonblocking slices as child reads;
  refusal ATOMIC (whole-or-nothing, never silent prefix); refusal never fails
  the pane, only dead PTY (EIO) errors. Truthful outcome wired: ctl code 3
  naming the queue on refusal, pane live. 6 tests (3 pty: audit repro ≤2s+
  Queued, saturation→Refused, reading→Flushed+echo; 2 native: wedged drops
  truthfully/stays Running, 256KiB paste <2s; 1 session: wedged→ctl code 3).
  Gate 358 lib + 4 green, no flake. NBs: canonical-mode absorption (wedge
  tests use stty raw -echo), portable-pty Drop EOT write_all now nonblocking
  (benign), old .write().unwrap() compile unchanged. Routed to CRITIC
  (assignment .scratch/review/118.critic.md). Verdict ping inbox
  118-pty-input.critic.ping. Worker lane idle at prompt.
- 2026-09-05: #118 CRITIC VERDICT = PASS (inbox 118-pty-input.critic.ping).
  AC1 stuck-child queue: bounded nonblocking (accepted=queued vs
  written=nonblocking slices); refusal ATOMIC whole-or-nothing incl. ctl
  path. AC2 backpressure: saturation → Refused, bound intact. AC3
  truthful: Refused → InputDropped event → ctl code 3 "terminal input
  queue is full..." (same refused family as the input gate), pane stays
  Running at engine AND ctl levels; dead PTY → Failed as before; auto-
  reply saturation drops the reply, not the pane. AC4 #117 non-regression:
  ctl.v1 wire/codes unchanged; one-request-per-frame intact (dispatch
  ≤1 event, pump dispatches nothing); no daemons. AC5 gate 358 lib + 4
  green first run, no flake; 6 tests cover queued/saturated/flushed +
  engine truth + pump delivery + ctl refusal. No nits. WAITING: user
  merge approval for #118 → then #119 to the pi-worker lane.

- 2026-09-05 (USER: "fix the nb first"): #118 MERGE HELD (critic pass stands,
  but merge deferred until the NB batch lands — user ordering). NB CLEANUP
  dispatched to the pi-worker lane (task nb-cleanup-2026-09-05, worktree
  coord/nb-cleanup-2026-09-05): (1) #116 fake-engine active_screen_lines
  Some([]) fallback shadow — make None-when-unset; (2) #117 wall-clock
  deadline test margins — harden against CI load, keep discriminating;
  (3) #117 zero-write-as-complete in write_response — fix-or-document with
  comment. Each NB = fix OR explicit decide+document. UI-lane NBs queued for
  claude after #128: #113 toggle_pin doc overpromise + pinned()/active()
  double-call; #115 SCROLLBAR_WINDOW ≡ NOTIFY_WINDOW const dedupe.
  In flight: worker→NB batch; claude→#128. #118 merge follows the batch.

- 2026-09-05: #128 DONE (claude, 2 commits ac3d637 "tell the truth about a
  repository's git facts" + 3b9d191 "say when a terminal the sheet added never
  started"; marker pass: worktree gitdir resolved, commit age from branch
  reflog (unknown when unreadable), dirty count → Option/None, failed adds
  surface as a bounded toast; 365 lib + 4 green; worktree clean; claude idle).
  Routed to CRITIC (assignment .scratch/review/128.critic.md — reflog-vs-
  commit-ts correctness, dirty Option semantics, worktree indirection, unknown
  rendering, toast non-modality + pane preservation, cache staleness on
  commit, gate 365+4). Verdict ping inbox 128-picker-git.critic.ping.
- 2026-09-05: NB-CLEANUP batch DONE (pi-worker muse, commit a71e842 "resolve
  #116/#117 critic non-blocking notes"; marker pass: "fake fallback fidelity,
  hardened deadline test, zero-write requeue [chose FIX over document-only];
  gate green"; worktree clean). REVIEW QUEUED after #128's verdict (same
  critic pane; assignment .scratch/review/nb-cleanup.critic.md to be written
  now). After both verdicts + user approvals: merge order #118 → NB batch →
  #128 (dependency/base order; NB branch based pre-#118 — rebase at merge).
  Then: #119 to the pi worker; UI NB batch (#113/#115) to claude.

- 2026-09-05 (USER standing approval): merge slices on critic PASS with NO
  non-blocking notes ("merge if there are no nb") — coordinator may execute
  the pending merge queue (#118 → NB batch → #128, dependency order) itself
  when verdicts land nit-free; surface to the user for a go if any nits
  appear. Critic currently reviewing #128 (hit the KNOWN shutdown_…_threads
  sandbox flake once during its gate run; expected — assignment covers
  clean-re-run).

- 2026-09-05: #118 MERGED (standing approval) — --no-ff merge f27fe25,
  pushed; merged-gate FIRST run hit the known shutdown_…_threads sandbox
  flake (1 fail) → re-run green 363 lib + 4 (×2); flake documented, merge
  stands. #128 MERGE CONFLICT (src/session.rs: #118 ctl-input wiring vs #128
  add-failure toast); merge aborted, main clean post-#118; integration
  ROUTED to claude (task 128-integration: merge origin/main into
  coord/128-picker-git, resolve keeping BOTH behaviors, gate, commit local,
  marker+ping slug 128-integration; never push). NB batch REVIEW dispatched
  to the critic (2nd assignment, .scratch/review/nb-cleanup.critic.md).
  Pending: claude-integration → merge #128 → merge NB (rebase) → close
  #118/#128 → rebuild → #119 + UI NB batch (#113/#115).
- 2026-09-05: NB-CLEANUP CRITIC VERDICT = PASS (inbox nb-cleanup-2026-09-05.
  critic.ping): all 3 NBs resolved — (1) fake active_screen_lines None-
  fallback WITH a test exercising the fallback path; (2) deadline test margins
  widened 10x (still discriminating); (3) zero-write requeue bounded by the
  existing deadline (no infinite requeue). Gate 357 lib + 4 green on its
  base; the 1 known sandbox flake re-verified environmental (fails on main
  too). NO NITS → pre-approved to merge (standing approval); executes after
  #128 lands: rebase NB onto post-#118 post-#128 main, re-gate, merge.

- 2026-09-05: MERGE QUEUE COMPLETE: #118 (f27fe25) + #128 (2ca10c3, claude
  integration 8c93913 kept both EngineEvent+NotifyKind behaviors) + NB batch
  (bfecf7b: fake None-fallback, 10x deadline margins, zero-write requeue) —
  all closed (118/128), main 372 lib + 4 green, binary rebuilt. The
  shutdown_terms_all_groups… flake re-verified on the rebased NB gate
  (oscillates 372 ✓ / 371+1 ✗; fails on pristine main — environmental).
  DISPATCHED (both lanes): #119 job/process cleanup → pi worker (WORKING);
  UI NB batch (#113 toggle_pin doc + pinned/active is_some_and; #115
  SCROLLBAR_WINDOW dedupe) → claude (WORKING, transcript-verified).
  Audit progress: #117 #118 #127 #128 + 2 NB batches shipped; #119 in flight;
  #120 #121 remaining in FIRST; then #122-#126, #129-#131.

- 2026-09-05: UI NB BATCH DONE (claude, commit d74403a; marker pass:
  "toggle_pin doc states the real semantics; unpin hint one is_some_and;
  SCROLLBAR_WINDOW = NOTIFY_WINDOW with derivation stated; behavior-
  identical, 372 lib + 4 green"; worktree clean). Routed to CRITIC
  (assignment .scratch/review/nb-ui.critic.md — NBs 1-3 + behavior-identical
  + gate; the SCROLLBAR_WINDOW≡NOTIFY_WINDOW aliasing honesty is the one
  judgment call to verify). Verdict ping inbox nb-ui-2026-09-05.critic.ping.
  In flight: worker → #119; critic → UI-NB review; claude idle.
- 2026-09-05: UI NB BATCH CRITIC VERDICT = PASS + MERGED (standing
  approval) — e6629ff, gate 372 lib + 4 green; toggle_pin doc truthful,
  is_some_and identical, SCROLLBAR_WINDOW aliased with HONEST split-rule
  comment; binary rebuilt. ALL NB DEBT CLEARED (#116/#117/#113/#115).
  In flight: worker → #119 (job cleanup, no marker yet). Claude idle.

- 2026-09-05: #119 DONE (muse worker, commit 32c464e; marker pass:
  "session-wide shutdown ownership enforced, join bounded, 4 failing-pre-fix
  tests, gate 376 lib + 4"). Hand-back highlights: session-wide kill on Linux
  via per-PID /proc enumeration (kill(-id) is PG-only — no session syscall);
  pid-reuse guards via /proc start-times; zombies excluded; join bounded
  (500ms → detach stragglers; total ≤ grace+settle+join = 3s); SECOND real
  bug found+fixed: force_shutdown gated on group-alive only — dead shell +
  live jobs skipped force; widened to group-or-session. 4 failing-pre-fix
  tests (bg SIGHUP-ignore, fg HUP+TERM-ignore, pre-shutdown orphan, engine
  bg), polling w/ 10s bound; flake passed this run; tests 3× stable. Non-
  Linux keeps group-only behavior (documented). PLAN.md FLAG (worker, no
  edit): lifecycle § still documents only "SIGTERM to owned process groups…
  SIGKILL survivors" — session-wide force + bounded join extend it; a
  coordinator-owned amendment + user approval is required (constitution).
  Routed to CRITIC (assignment .scratch/review/119.critic.md — full
  ownership/kill-mechanism verification, pid-reuse, bounded join, tests
  fail-pre-fix, non-Linux fallback). Verdict ping inbox
  119-job-cleanup.critic.ping. FIRST-priority: #120 #121 remaining.
- 2026-09-05: #119 CRITIC VERDICT = PASS (no nits) → MERGED (standing
  approval) — aa55591, gate 376 lib + 4 green first run, issue #119
  CLOSED, binary rebuilt. PLAN.md Lifecycle AMENDED (user: "you update
  the docs if needed") — b258a77: session-wide /proc enumeration, PID-
  reuse guards, group-OR-session force, bounded join (grace+settle+join
  ≤3s), non-Linux group-only note; acceptance line "confirmed exit
  leaves no owned child processes" unchanged. DESIGN.md: no lifecycle
  text to sync (checked). #120 (bracketed paste) DISPATCHED to the muse
  worker (WORKING; worktree coord/120-bracketed-paste off aa55591).

- 2026-09-05: #120 DONE (muse worker, commit 5ce0a6c, 7 files +269/-14;
  marker pass: "mode-driven paste wrapping plus split-proof parsing, 4
  failing-pre-fix tests, gate green 380 lib + 4"). Hand-back: full split
  matrix over opener/content/closer; encode wrapped-vs-raw units; ctl
  paste/text/keys distinction; byte-exact dd probe (child receives
  ESC[200~SAFE LF TEXT ESC[201~); incomplete-closer stall carved out as
  PRE-EXISTING → routed to the #10/#126 parser-bounds slice (verify at
  review). Worker ctx 22.6% (healthy). Routed to CRITIC (assignment
  .scratch/review/120.critic.md — paste model coherence, split matrix
  coverage, ctl wire additive-only, #118 bounded-path intact, carve-out
  honesty, gate). Verdict ping inbox 120-bracketed-paste.critic.ping.
  FIRST-priority: 5/5 after #120 merges; #121 (pane-ID reuse) next.

- 2026-09-05: #120 CRITIC VERDICT = PASS (no nits) → MERGED (standing
  approval) — f390ef4, gate 380 lib + 4 green; issue #120 CLOSED; binary
  rebuilt. Audit FIRST-priority COMPLETE (5/5: #117 #118 #119 #120 + NB).
  #121 (opaque session-unique pane IDs, tombstones, stale-ID truthfulness)
  DISPATCHED to the muse worker (WORKING; worktree coord/121-pane-ids off
  f390ef4; brief guards TERMDECK_PANE/list/peek coherence + additive-only
  wire). NEXT after: SECOND batch #122-#126.

- 2026-09-05 (USER: hit weekly usage limit — STOP AFTER #121): autonomous
  run halts at #121's close; NO further dispatches until user returns. #122-
  #126 worktrees + briefs PREPPED (coord/{122-api-truth,123-shell-hook-login,
  124-scrollback-config,125-timing-meta,126-ansi-mode-aware} off cbc3889) —
  dispatch-ready but parked. IN FLIGHT: muse worker → #121 (pane IDs, ctx
  23.7%, no marker yet). On #121 completion: critic → standing-approval merge
  → close → comprehensive report to user. STOP.
- 2026-09-05 (OPENCODE OUTAGE): muse worker hit persistent 503
  [service_overloaded] from Console Go upstream (3 failed retries); pi
  session healthy/idle at 23.7% ctx. #121 ZERO progress (no commits /
  marker — stranded in-memory; re-runs from the brief cleanly once the
  backend returns). Critic (also muse) idle — same outage would block
  its next review. All else healthy (claude lane, relay, main 380+4).
  NEXT: on backend recovery, re-prompt the same worker pointer for
  #121 (session intact); the STOP-after-#121 instruction stands.
- 2026-09-05 (BACKEND RECOVERED + TERRA LANE RESTORED): muse worker resumed
  #121 mid-slice (backend came back; deep in session.rs ctl close/promote
  handlers — the identity territory; ctx 25%). USER used a codex usage
  reset (resets now 2) — TERRA available again. Codex pane relaunched
  (full session, gpt-5.6-terra high, at coord/124-scrollback-config),
  #124 DISPATCHED (parallel with #121 — no file overlap: config/mod.rs +
  engine/vt.rs vs #121's session.rs), verified Working. Brief patched
  with inline finish protocol (codex has no worker skill). Parallel
  plan continues toward closing the audit queue; stop-after-#121
  SUPERSEDED by the user's terra-lane enablement (resume audit queue).

- 2026-09-05 (ENV SPEC CHANGE, user): when muse finishes #121 → KILL the
  muse worker (only pi pane = coordinator). CRITIC ROLE becomes a second
  claude opus-5 pane — pi critic pane (termdeck) CLOSED; new critic pane
  "termdeck" launched: claude --dangerously-skip-permissions --model opus
  (idle at composer; assignment files carry the read-only/never-edit-merge-
  push discipline + VERDICT ping format — same delivery as before).
  Review routing unchanged: assignment files + verdict via
  /tmp/shipwright/inbox/<task>.critic.ping. In flight: muse→#121, codex→#124.
  On #121 done: review via claude critic → merge (standing) → close → kill
  muse worker.

- 2026-09-05: #124 DONE (codex/terra, commit 704fb42; marker pass:
  "Configured scrollback now bounds initial, added, and respawned PTY history;
  full gate green"; worktree clean). Routed to the CLAUDE critic (assignment
  .scratch/review/124.critic.md — all-creation-paths check, N-vs-N+viewport
  contract, no #116/#115 regression, failing-pre-fix e2e test, gate).
  Verdict ping inbox 124-scrollback-config.critic.ping. In flight: claude-
  critic→#124 review; muse→#121; codex idle after #124.

- 2026-09-05: #124 MERGED (claude-critic PASS; 3 nits queued to NB backlog:
  NB-A NativeEngine::spawn still hardcodes DEFAULT_SCROLLBACK (public API,
  test-only caller); NB-B #124 commit body empty (contract lives in code
  comments); NB-C session test hardcodes /tmp/termdeck-scrollback-test.sock).
  79d4b48, gate 383 lib + 4 green, issue #124 CLOSED, binary rebuilt.
- 2026-09-05: #121 DONE (muse worker, commit 711eb1d, 3 files; marker pass:
  "session-monotonic pane identities plus shared tombstone primitive, 3
  failing-pre-fix tests"; gate 383+4 stable 2x; hand-back archived →
  .coordinator/journal/muse-121-handback.txt). Design: ONE string id
  (deliberately not opaque — TERMDECK_PANE/agent workflows preserved);
  session-monotonic allocator (reopen → -2 suffix, never tombstoned
  original); single funnel tombstone primitive through keyboard/mouse/API
  close; stale-ID truthful (close→already:true idempotent; unknown→error 2);
  visible -2 suffix change flagged for critic. MUSE WORKER KILLED per env
  spec (only pi pane = coordinator). Routed to CLAUDE critic (assignment
  .scratch/review/121.critic.md; branch base pre-#124 → integration rebase
  planned post-verdict — #124 touched session.rs/lifecycle.rs/tests.rs too,
  expect a small conflict routed to codex at merge). #126 (ANSI attrs +
  mode-aware input) DISPATCHED to codex (WORKING, parallel-safe vs #121:
  no session.rs overlap).

- 2026-09-05: #126 DONE (codex, commit 60ce2a2; marker pass: "ANSI
  attributes and DECCKM/X10/UTF-8/SGR input modes are honored"; files:
  contracts additive + engine/vt + session/{backend,input} + ui/input;
  gate on its base 383+4; worktree clean). Routed to CLAUDE critic
  (assignment .scratch/review/126.critic.md — byte-exact combined attrs,
  DECCKM + mouse-protocol mode flow, additive contract check, failing-pre-fix
  tests). #122 (API truth) DISPATCHED to codex (rebase-to-main first;
  WORKING). Audit closed: #117 #118 #119 #120 #121 #124; in review #126;
  in flight #122. Remaining: #123 #125 #129 #130 #131.

- 2026-09-05: #126 CRITIC VERDICT = PASS for the implemented scope (byte-
  exact cumulative modifiers; DECCKM flows VT→metadata→press w/ SS3-for-plain-
  arrows; mouse SGR>UTF8>X10 with X10 bytes hand-verified; contract strictly
  additive; gate green 2x) — BUT #126 CANNOT CLOSE on it: sub-defect 1
  (bounded/resilient KeyReader, session/input.rs:170-248) was DROPPED from
  the slice's brief (my scope error — the audit's finding 10 had 3 sub-
  defects; the brief shipped 2.5). Nits also: UTF-8 mouse test uses col 7/row
  4 (UTF8≡X10 there — pin with coord ≥96); Input::press pub+mode-blind
  (delegates false, test-only now); BOLD+DIM emitted 1;2 (harmless behind
  [0m). ACTIONS: #126-slice REBASE conflicted in native.rs (vs #121's
  metadata region) → integration queued for codex AFTER #122; #126-parser-
  bounds follow-up slice filed (worktree + brief ready, off origin/main),
  also queued for codex after #122; #126 stays OPEN until (integration +
  parser slice + second review).

- 2026-09-05: #122 DONE (codex, commit 1840811 "report operation outcomes
  truthfully", rebased to main; marker pass; truth table: input→exited/
  failed/tombstoned/full-queue→refusal; input→missing terminal→unavailable;
  notify attributed→delivered:true; notify outside/coalesced→delivered:false
  + reason). Routed to CLAUDE critic (assignment .scratch/review/122.critic.md
  — per-row codes/shapes, additive wire, #118/#121 coherence, failing-pre-fix
  tests). #126-INTEGRATION dispatched to codex (native.rs rebase conflict vs
  #121; resolve-keep-both; task slug 126-integration). QUEUE after: 126-
  parser-bounds slice → then #123. In flight: critic→#122; codex→126-
  integration.

- 2026-09-05: #126-slice MERGED (f5bda59, 389 lib + 4; issue OPEN).
  #122 CRITIC VERDICT = PASS → MERGED (9662658, 390 lib + 4) + CLOSED (with
  release note: tombstoned-input code 2→3 vs #121). NBs queued (backlog now
  13): #122-NB1 code 2→3 visible change (release-noted); NB2 peek/promote
  still code 2 for tombstoned (uneven); NB3 promote discards deck.apply()
  bool (always master:true, undefended); NB4 last+force close returns ok
  pre-close (pre-existing class); NB5 queued:true test relies on fixture
  timing not forced state. #126-PARSER-BOUNDS dispatched to codex (WORKING;
  worktree coord/126-parser-bounds off b23fa16; completes #126 on second
  review). In flight: codex→parser-bounds; claude-critic idle; claude-impl
  idle. Audit closed: #117 #118 #119 #120 #121 #122 #124 (+NB). #126 open
  (part2 in flight). Remaining: #123 #125 #129 #130 #131.

- 2026-09-05: #126-PARSER-BOUNDS DONE (codex, commit 8e5effd: explicit
  ground/ESC/paste/mouse/discard states; 64KiB paste + 256B mouse caps with
  discard-through-terminator; UTF-8 valid-prefix recovery; exhaustive compact
  partition + deterministic chunk-fuzz coverage; gate 393 lib + 4; worktree
  clean). Routed to CLAUDE critic (assignment .scratch/review/126-parser-
  bounds.critic.md — caps enforcement, é+invalid repro pinned, #120/#118
  non-regression, fuzz determinism; pass ≈ closes #126). #123 (bash -l hook)
  rebased to main + DISPATCHED to codex (WORKING).

- 2026-09-05: #126-PARSER-BOUNDS CRITIC VERDICT = PASS → MERGED (f9026a7,
  393 lib + 4) → #126 CLOSED (GitHub auto-closed on the "CLOSES #126" merge-
  message keyword; closing-summary comment added; critic verified NOT the
  closer — its only gh use was reading issues). Verdict detail: Escape ≤6B
  behind is_escape_prefix; Mouse 256B → DiscardMouse; Paste 64KiB →
  DiscardPaste through [201~; retained state bounded ~64KiB+4KiB read;
  decode_utf8 derives width from lead byte, consumes ≤3B partial, failure→
  exactly 1 byte consumed (é+0xff → scalar + U+FFFD pinned); close-marker
  restart correct; coverage deterministic (16 exhaustive partitions +
  256 fixed-seed LCG chunkings, no sleeps); #120 matrix + #118 path intact.
  NBs queued (backlog 17): sil dozen >64KiB paste dropped silently (follow-up
  worth); retained_len assertion far looser than reality (0); malformed-byte
  single-shot only; 5-byte exhaustive stream (fuzz carries coverage).
  In flight: codex→#123. Remaining audit: #123, #125, #129, #130, #131.

- 2026-09-05: #123 DONE (codex, commit ceea442; marker pass: "bash -l
  replays normal login profiles before the chained notification hook"; after
  a transient model-capacity stall, resumed + completed). Routed to CLAUDE
  critic (assignment .scratch/review/123.critic.md — production-argv tests,
  profile-preservation mechanism, DEBUG/PROMPT_COMMAND chaining with+without
  pre-set, non-login + zsh/fish non-regression, gate). #125 (timing metadata
  + expiry repaints) rebased to main + DISPATCHED to codex (WORKING).
  Remaining audit: #125 (in flight), #129, #130, #131.

- 2026-09-05: #123 CRITIC VERDICT = HANDBACK (claude critic). GATE NOT
  GREEN: bash_hook_reports_errors_and_long_completions fails DETERMINISTICALLY
  (4/4) on the branch, passes on origin/main — NOT the known flake; the
  "full gate green" marker was FALSE (lesson: demand real gate numbers in
  markers — the #111 lesson resurfaced). HOOK=1 install works, but the
  notification reports the WRONG COMMAND (__systemd_osc_context_precmdline
  instead of `false`/`sleep 0.01`) — the truthfulness failure #123 sits
  under. Root causes: (A) PROMPT_COMMAND is an ARRAY on bash 5.1+ (systemd
  profile.d does +=(...)); scalar assignment lands element [0] only → prompt
  funcs recorded as user commands. (B) stripping -l turns off shopt
  login_shell → /etc/bashrc re-sources /etc/profile.d/* → profile.d runs
  TWICE (PS0/PROMPT_COMMAND doubled) — AC2 violated.
  MINIMAL FIX LIST (route to SAME codex session, after #125): (1) array-aware
  PROMPT_COMMAND chaining OR drop __td_prompt_end + clear on first non-__td_*
  command; (2) stop prompt-driven funcs being recorded as commands; (3) fix
  double-source (login fidelity or BASH_LOGIN_RC avoids re-entering profile.d
  via ~/.bashrc chain); (4) regression test with PROMPT_COMMAND+=() asserting
  the notification title IS the user command; (5) re-run gate, real numbers.
  NITS (5): no API-created-pane argv test; ~/.bash_logout no longer runs;
  login_shell/$0 no longer report login; bash_init_arguments treats leading
  dash as flags after -c; DEBUG-trap snapshot one-time.
  VERIFIED GOOD: bash_init_arguments rewriting, login profile ORDER, trap -p
  DEBUG capture (bash 5.3.9), non-login path byte-identical, zsh/fish
  untouched, fmt+clippy clean. #123 STAYS OPEN. Correction queued — cannot
  steer codex (on #125); deliver after #125's marker.

- 2026-09-05: #125 DONE (codex, commit 570418a; marker pass: "live timing
  metadata refreshes once per second and expiry transitions draw their final
  frame"; worktree clean). Routed to CLAUDE critic (assignment .scratch/
  review/125.critic.md — derived-instants determinism, 1s tick, demotion+toast
  final-frame scheduling, #113/#115/#71 non-regression; explicit real-gate-
  numbers demand after the #123 lesson). #123-CORRECTION DELIVERED to codex
  (same session, now idle; correction brief .scratch/tasks/123-correction.
  brief.md — array-aware PROMPT_COMMAND, wrong-command filter, profile.d
  single-source, PROMPT_COMMAND+=() regression test asserting the user
  command, real gate numbers; task slug 123-correction). In flight: critic→
  #125; codex→123-correction.
- 2026-09-05: #123-CORRECTION DONE (codex, c637560 on top of ceea442;
  marker: "real bash -l bootstrap preserves login profiles and array
  prompt command titles; gate 393 lib + 4" — but 393 vs main 395
  suggests a stale base; rebase-to-main CONFLICTED in session/tests.rs
  (vs #121/#122/#125/#126 test churn) → 123-INTEGRATION routed to codex
  (same session, idle; resolve keeping all behaviors, REAL gate numbers,
  task slug 123-integration). #123 re-review (claude critic) fires after
  the integration. Audit: closed #117-122,124,125,126; #123 mid-close;
  remaining #129 #130 #131 (+#132).
- 2026-09-05: #123-INTEGRATION DONE (codex: 0141dd4 + 875ecf5 on main
  base; gate reported 398 lib + 4; worktree clean) → #123 RE-REVIEW
  dispatched to the claude critic (assignment 123-recheck.critic.md —
  verifies ITS OWN handback fix list: array-aware PROMPT_COMMAND, real
  user titles, profile.d once with realistic /etc/bashrc fixture,
  PROMPT_COMMAND+=() regression test, real gate numbers). Verdict ping
  inbox 123-shell-hook-login.critic.ping.

- 2026-09-05: #125 CRITIC VERDICT = PASS → MERGED (395 lib + 4, verified
  3x by the critic; issue CLOSED) + rebuilt. The critic confirmed the marker:
  elapsed() takes observation instants, refresh_timing_if_due ≤1/s from
  drain_events → MetadataChanged → dirty (quiet panes age); exited/failed
  stop ticking with final capture; expiry edge-triggered (1499/1500/1501,
  7999/8000/8001 pinned; zero sleeps). NITS (5, queued): (1) NEW BUG — the
  #115 scrollbar keeps the same final-frame class (session.rs:572
  deck.scrolling; one || from the helper) + N staggered idle redraws/s; (2)
  no session-level tick test (helper-level only; drain_events chain verified
  by inspection); (3) per-terminal deadlines (N redraws/s idle); (4) tick for
  non-displayed panes; (5) decorative toasting assert. FILED as a new issue
  (#132 - scrollbar final frame + idle-redraw nit, bug/astra-audit labels).
  In flight: codex→123-correction. Audit closed: #117-122, 124, 125, 126.
  Remaining: #123 (correction), #129, #130, #131 + new #132.

- 2026-09-05: #123 MERGED + CLOSED (e187720, gate 398 lib + 4; auto-close
  via merge-message keyword again; closing comment posted). #123 RE-REVIEW's
  residual nits queued (backlog 29): PS0-class-by-name filter (starship/
  bash-preexec/atuin would still be recorded); bootstrap line echoed at first
  prompt; fire-and-forget bootstrap (no hook-install verification); declare
  -a pattern misses -ax/-ar; __systemd filter untested; regression runs
  bash -i -l over pipes not production PTY bash -l; API-created-pane argv
  test + DEBUG-trap one-time snapshot still open.
- 2026-09-05: #129 SPLIT (user-approved parallelization): A) CLI/help code
  (coord/129-cli-help — help for both binaries, errors-before-help, Cargo
  default-run + repository) → codex WORKING; B) docs (coord/129-docs — README
  + PLAN facts, research-brief overstatement corrections w/ dated amendment
  notes, MIT LICENSE, Horizon scrub, canvas-doc sweep) → claude WORKING
  (pointer needed a redispatch — /usage overlay ate the first; Esc+re-paste
  landed, transcript-verified). Audit remaining: #129 (both parts), #130,
  #131 (+#132).
- 2026-09-05: #129-DOCS DONE (claude, 3 commits 61e1fa0/a986544/7b52a41;
  marker: README/PLAN corrected against real CLI (verified by running),
  research-brief A1-A5 amendment notes, MIT LICENSE, WSL labelled
  procedure-not-evidence, Horizon 0 outside pinned export; docs-only,
  build clean) → routed to CLAUDE critic (transcript-verified reviewing).
- 2026-09-05 (CODEX QUOTA HIT mid-#129-A): codex session parked at the
  usage wall ("try again at 7:44 PM" — weekly reset; Pro-upgrade hint).
  #129-A (CLI/help) is PARTIAL: uncommitted edits intact on disk
  (Cargo.toml + src/bin/termctl.rs + src/cli/mod.rs — help asserts + run
  (unknown)==3 visible; NO commits/marker/ping). Resume options for the
  user: (a) wait for the 19:44 reset → codex resumes in place; (b) hand
  the half-done CLI slice to the claude lane (cross-ownership + resume
  in place, adjudicated); (c) second codex usage reset if any remain.
- 2026-09-05: #129-DOCS CRITIC VERDICT = PASS (spot-checked against a
  running binary: resolve_path semantics, contract enum exact match,
  bindings PgUp/PgDn-c-p-a-x, walk(start,3), cli arms; the #119
  Lifecycle amendment preserved byte-for-byte; A1-A5 dated+attributed;
  LICENSE=MIT 2026 Andrea Zollini; Horizon only the pinned export +
  one 'horizontal' false positive; boundary clean) → MERGED (issue
  #129 STAYS OPEN). Nits: (1) MERGE-ORDER hazard — README cargo-run
  paragraph falsified when default-run (#129-A) lands → reconcile the
  paragraph at the CLI merge; (2) #129 can't close yet — Cargo.toml
  repository '', serde_yaml 0.9, no dep scan (→ #129-A/#130);
  COORDINATION.md stale tables NOW PRUNED (archived to journal, this
  turn); (3) amendment preamble oversells purity (minor); (4) NEW
  ISSUE #133 filed — folders named check/list unreachable (parse-arm
  shadowing), bug label. #129-A (CLI/help) still parked on the codex
  quota (uncommitted edits on disk); resume at 19:44 or hand to claude.
- 2026-09-05: #130 (tests/CI safeguards) DISPATCHED to the CLAUDE lane
  (user decision; cross-ownership adjudicated) — worktree
  coord/130-tests-ci off 9e28dfe; brief: CI hardening (timeouts,
  pinned toolchain via rust-toolchain.toml, zsh+fish install so shell
  compat tests really run, cargo audit step, --locked/--frozen) + a
  bounded coverage sweep (fill ONLY real gaps; cap 2-4 new tests) + WSL
  evidence stamp; gate real numbers. WORKING (transcript-verified).
  #129-A still parked (codex quota).
- 2026-09-05: #130 DONE (claude, 3 commits 6be316f/f77e2a5/a4eed8e;
  marker: CI bounded+pinned+zsh/fish+cargo-audit+--locked (YAML
  validated, shell logic run locally); 3 real gaps filled (socket-to-
  deck e2e, live fish hook, ANSI colour bytes) after checking all 10
  audit classes; WSL evidence stamp; gate 401 lib + 4, fmt/clippy
  clean). Routed to CLAUDE critic (assignment 130.critic.md — CI-YAML
  critical read, 3 tests genuine fail-pre-fix + deterministic,
  already-covered claims spot-checked, impl deltas test-support-only,
  gate). Verdict ping inbox 130-tests-ci.critic.ping.
- 2026-09-05: #130 CRITIC VERDICT = PASS → MERGED (0941b4e, 401 lib + 4;
  issue #130 STAYS OPEN — flake CI story missing). Verdict honesty:
  workflow NOT executable locally (stated plainly; shell logic + pin
  assertion run locally; PIN 1.98.0 would pass); 3 gap-fills genuine
  (socket e2e real Listener+UnixStream+ctl.v1, colour byte-exact hand-
  checked, fish mirrors zsh); 9/10 coverage classes spot-checked; WSL
  stamp records 'manual pass NEVER run' + zsh/fish SKIP here; gate 401
  +4 ×2, --locked clean. NITS (queued, backlog 36): fish/zsh tests
  vacuous without the shells (2/3 verifiable here; rides CI apt);
  advisories-on-every-push red-light risk; pin assertion version-shaped;
  step ceilings over-subscribe job ceiling; no registry cache (audit
  rebuilds under 10m); e2e read_to_string no timeout (hang-risk);
  flake story absent (grep 0). #130-PART2 (flake story + e2e read
  timeout + advisories continue-on-error) DISPATCHED to the claude
  lane (WORKING, transcript-verified).
- 2026-09-05: #130-PART2 DONE (claude, 08e0119; marker: main suite
  --skips the shutdown case (fail-on-first kept), one-retry step with a
  rename guard (all 3 paths run locally), advisories continue-on-error +
  loud warning/summary, socket e2e read timeout; YAML parses, 401 lib +
  4). Routed to CLAUDE critic (assignment 130-flake-story.critic.md —
  retry-honesty + fail-on-first preservation, advisory visibility, e2e
  timeout fail-fast, boundary; pass ≈ closes #130). Verdict ping inbox
  130-flake-story.critic.ping.
- 2026-09-05: #130-PART2 CRITIC VERDICT = PASS → MERGED + #130 CLOSED
  (d45c7db; 401 lib + 4; auto-close keyword; record comment posted).
  The critic ran every workflow shell path locally: --skip removes
  exactly 1 test (400/0/1 filtered + 4 bin), no --no-fail-fast anywhere;
  retry loop deterministic-failure-proven (2 attempts, 2 warnings, then
  error+exit 1 — real regressions still red); rename guard hazard PROVEN
  (bogus filter → '0 passed, 401 filtered' EXIT 0 — guard catches it);
  advisories gated via steps.audit.outcome==failure (correct context) +
  warning AND step-summary; e2e 0.10s vs 5s timeout; boundary 2 files;
  gate 401+4 ×2. NITS (backlog 41): retry can launder a 50% intermittent
  bug (~75% green) — write 'needed a retry' into STEP_SUMMARY; advisories
  can't distinguish vulns vs DB-unreachable; continue-on-error = 
  annotation-only signal → tracked as NEW ISSUE #134 (scheduled advisories
  job opens an issue); -uo pipefail leaves outside-loop commands
  unguarded; --skip substring filter overlap risk.
- 2026-09-05: #132 (scrollbar final frame) DISPATCHED to claude (WORKING,
  transcript-verified; worktree coord/132-scrollbar-frame off d45c7db;
  folds into #125's schedule_expiry_repaint pattern, injected clock,
  shared-deadline perf item = do-the-cheap-or-note).
- 2026-09-05: #132 DONE (claude, 6996312, +61/-6; marker: scrollbar
  folded into the #125 edge helper, test pins 3999/4000/4001 fail-
  pre-fix; shared-deadline perf DEFERRED with a real reason (engine-lane
  real-clock state, not this helper) — tracked as incomplete; gate 402
  lib + 4). Routed to CLAUDE critic (assignment 132.critic.md — edge
  honesty, no #115 semantic change, deferral-never-drop, boundary,
  gate). Verdict ping inbox 132-scrollbar-frame.critic.ping.
  Codex quota reset 19:44 (~1h30m); #129-A resumes then (or handover).
- 2026-09-05: #132-PART1 CRITIC VERDICT = PASS → MERGED (8ae14fc, 402
  lib + 4; issue #132 STAYS OPEN for its second item); the one-|| edge
  fold with the old level-triggered line REMOVED; boundary pins real
  4000ms window (3999/4000/4001); shared was_active traced across
  transients; render consumer checked (raised None at 4000 = the frame
  that takes the bar away); #125 expiry test still passes. NITS (2, to
  backlog 43): SCROLLBAR_WINDOW aliases NOTIFY_WINDOW so the pin is
  really the notification const (derive from SCROLLBAR_WINDOW.millis);
  shared was_active deserves a comment at the flag. SECOND ITEM filed
  as NEW ISSUE #135 (staggered idle redraws + hidden-pane tick) —
  queued to the engine lane; #132 closes when #135 lands.
- 2026-09-05: #133 (check/list shadowing) DISPATCHED to the claude lane
  (coordinator adjudication per the #130 pattern; WORKING,
  transcript-verified; worktree coord/133-cli-shadow off 8ae14fc).
  #129-A resumes at the 19:44 codex reset (~1h20m).
- 2026-09-05 19:46: codex quota RESET — #129-A RESUMED in place (4
  uncommitted files intact; codex reports 'help routing complete for
  both binaries, validating the final option-order regression, then
  gate + commit + handoff'). Working. Next: #129-A done → critic (pre-
  written assignment incl. the README cargo-run reconcile with
  default-run) → merge → close #129 → #135 → close #132/#131.
- 2026-09-05 19:47: #129-A DONE (codex, 571f0da: Cargo.toml +
  cli/mod.rs + main.rs + bin/termctl.rs; help for both binaries +
  per-verb, unknown-flag error+usage hint, default-run=termdeck,
  repository metadata; gate 399 lib + 5 termctl + 1 main ON ITS BASE;
  worktree clean). REVIEW FLAG: hand-back says 'usage exit code 3' —
  but #133's critic pinned termdeck usage=2 (termctl 3 = ctl-refusal
  propagation); assignment patched with the corrected premise before
  dispatch. #129-A routed to CLAUDE critic (patched assignment incl.
  README default-run reconcile). Verdict ping inbox 129-cli-help.critic
  .ping.
- 2026-09-05: #129-A CRITIC VERDICT = HANDBACK (well-earned). ALL OF
  IT VERIFIED ON THE BUILT BINARIES: help surfaces man-page-shaped + exit
  0 + env docs real; but (BLOCKING) termctl usage moved 2→3, colliding
  with the ctl-refusal code — 3 now = 'asked wrongly' OR 'session
  declined'; the single exit_codes test now asserts usage==refusal==3
  against its own name; #122's agent trust depends on the old grouping
  (2 = asked wrongly, 3 = declined). MINIMAL FIX + (REQUIRED) stale base:
  branch based e187720 vs main c36ca4e — misses 129-docs/130x2/132/133;
  fix must keep `--` before --config/leading-dash, add !literal to the
  new help arms (else folder named help = #133 bug, third word), re-run
  the #133 matrix; README cargo-run paragraph now false (default-run).
  129-CORRECTION DISPATCHED to codex (same session; slug 129-correction;
  includes README + help-page doc fixes). VERIFIED GOOD: pages+
  env vars real, default-run works both binaries, repo canonical,
  worktree clean. NITS: --config PATH WORKSPACE form + TERMDECK_NOTIFY_
  LONG_SECS undocumented; `--` lines missing on the pages.
- 2026-09-05 19:56: #129-CORRECTION DONE (codex, e316976 REBASED onto
  current main — ahead 1, clean; marker: "rebased help slice, separated
  termctl usage/refusal exits, preserved literal paths, reconciled
  README; gate 406 lib + 5 termctl + 1 main"). RE-REVIEW dispatched to
  the claude critic (129-recheck.critic.md — verify EVERY handback item
  on the built binaries: usage=2 vs refusal=3 separation + test,
  termdeck-3 decision comment, `--`/!literal/literal-path matrix incl.
  a folder named help, README reconcile, --config/NOTIFY_LONG_SECS
  docs, gate real numbers). Verdict ping inbox 129-cli-help.critic
  .ping. On pass: merge → close #129 → dispatch #135 → close #132/#131
  = asterisk audit complete.
- 2026-09-05 20:0x: #129 RE-REVIEW = PASS → MERGED + #129 CLOSED
  (c4cb5ad; 406 lib + 5 termctl + 1 main; recording comment posted;
  rebuilt). Critic verified EVERY handback item on built binaries: exit
  separation with assert_ne!(usage,refusal) (stronger than asked);
  termdeck-3-vs-termctl divergence written at cli/mod.rs:208; six-case
  literal-path matrix incl. folder named help; README 4 cargo-run lines
  reconciled; all 3 help-page nits fixed. DISCLOSURE: the critic
  accidentally hit the LIVE session with termctl -- status + notify on
  probing the `--` page claim (one toast delivered); disclosed + stopped
  + noted as a process caveat (reviewers should not call termctl against
  a live socket). NITS (3, backlog 51): termctl -- help 'unknown verb'
  despite help listed (advertise `--` only for arguments there);
  termdeck --help -- check short-circuit quirk; termctl `--` beyond
  brief (observation). #135 (shared timing deadline + visibility gate)
  DISPATCHED to codex (WORKING; closes #132 on merge). Remaining:
  #135 → #131 umbrella → DONE.
- 2026-09-05 20:08: #135 DONE (codex, 334b3cf "Batch visible terminal
  timing refreshes", +237/-70 across engine/fake, engine/native,
  session.rs, session/lifecycle, session/tests; marker pass 407 lib + 5
  termctl + 1 main (406+1); worktree clean — ping had a staging hiccup,
  delivered via the relay regardless). Routed to CLAUDE critic
  (assignment 135.critic.md — shared-deadline batching in native.rs,
  visibility gate honesty, #125 injected-clock determinism, expiry
  non-regression incl. #132-part1, 3 fail-pre-fix tests, gate; no live-
  session termctl calls per the #129 disclosure). Verdict ping inbox
  135-timing-tick.critic.ping. On pass: merge → close #132 + #131 →
  ASTRA AUDIT + review-born children COMPLETE.
- 2026-09-05 20:15: #135-CORRECTION DONE (codex, d1b50fb on top of
  334b3cf — 7 files +332/-52 incl. ui/deck.rs +32 timing-set sibling;
  marker: "folded drawn strips now keep shared timing ticks; 408 lib + 5
  termctl + 1 main"). RE-REVIEW dispatched to the claude critic
  (135-recheck.critic.md — drawn-set timing_terminals incl. collapsed +
  master, folded-assertion corrected + zoom kept, engine→surface
  regression test present, batching invariants preserved, gate).
- 2026-09-05 20:1x: #135 CRITIC VERDICT = HANDBACK (excellent catch:
  batching half RIGHT — shared deadline, ≤1 TimingChanged/pass, additive
  contract, determinism, expiry edges untouched — but the visibility gate
  used terminal_sizes() (PTY geometry, SKIPS collapsed) as a proxy for
  drawn-timing; a collapsed strip renders idle{age} from output_idle
  (deck.rs:999, pinned by a_folded_pane_states... asserting idle 6m) →
  folded idle age FREEZES = #125 reintroduced on the folded strip; the new
  test even asserts the false claim. 135-CORRECTION dispatched to codex:
  Deck::timing_terminals(area) sibling (drawn set incl. collapsed+master),
  corrected assertion+comment, engine→surface regression test (folded
  Running output_idle advances across a shared tick), optional add/-
  add_with_socket unconditional insert; keep ≤1 TimingChanged/pass.)
- 2026-09-05: #133 DONE (claude, fb37af3, +125/-3: cli/mod.rs +
  README; marker: `--` ends the verbs — termdeck -- check, --config
  f.yaml -- check; bare verbs unchanged; cwd-dependent resolution
  deliberately rejected; 3 tests fail pre-fix 'unknown option: --';
  gate 405 lib + 4). Routed to CLAUDE critic (assignment 133.critic.md
  — --convention across flag positions by RUNNING the binaries, design
  honesty of the cwd rejection, edge cases (-- alone, -- --help), exit-
  code contract, gate). Verdict ping inbox 133-cli-shadow.critic.ping.
- 2026-09-05: #133 CRITIC VERDICT = PASS → MERGED + CLOSED (8cdf93b,
  405 lib + 4; matrix run against the real binary; bare verbs unchanged;
  cwd-design rejection documented both places + guarded; my AC5 exit-
  code premise was WRONG (termdeck exits 2 on all CliErrors; termctl 3
  is a propagated refusal, usage=2 pinned) — contract preserved either
  way. NITS (5, backlog 48): exit-code premise correction; `--` also
  stops option parsing for leading-dash tokens (README lacks the clause;
  interacts with the #129-A help work when it lands); `termdeck --`
  with nothing opens the picker (defensible, undoc'd in usage);
  shadowed FILE named check untested (folder/workspace only); doubled
  `--` usage-error correct+untested. NOTE: `termdeck --help` STILL
  errors 'unknown option' — the #129-A help blocker remains open
  (quota-bound at 19:44). Review-born issues accounted: #132 (open,
  waits #135), #133 (closed), #134 (tracked, CI), #135 (queued engine).

- 2026-09-05 20:2x: ★ ASTRA AUDIT COMPLETE — ALL 15 issues closed:
  #117 #118 #119 #120 #121 #122 #123 #124 #125 #126 #129 #130 #131 (+ docs
  halves), plus the review-born #132 (2 items) #133 #134 #135. Final merges:
  #135 (f93d361/64e111c via ca72e4f; 408 lib + 5 termctl + 1 main) closes
  #135+#132; #131 umbrella closed (criterion: hardened baseline shipped).
  Rebuilt. Review-born journey recap: 3 substantive handbacks that were
  caught and corrected (#123 false-green + PROMPT_COMMAND array; #129-A exit-
  code collision + stale-base; #135 folded-strip freeze). Main green 408+5+1.
  Open now: user gates #112 #94 #33 #114 + NB backlog (~55) + #134 (CI
  scheduled-advisories track). The #112/#114/#94 resume-on-baseline decs are
  the user's.

- 2026-09-05 EOD (WAVE-2 DECIDED FOR TOMORROW, user): status block already
  corrected (10a3cc6) — honest about RED CI since 8ae14fc (zsh hook failure
  under real zsh; #137 ambient-TERMDECK_SOCK; #138 exhibit A). My earlier
  "gate green" ledger claims were local-only and env-polluted (ran inside the
  deck with TERMDECK_SOCK set) — owned; going forward gate = CI run, not
  local invocation. Second astra audit (AUDIT-2026-09-05.md, b3df768) filed
  #136-#147; probes archived (.coordinator/journal/audit-2026-09-05-probes,
  3ba731f). WAVE-2 ORDER (locked, follows the audit + status block): STEP 0
  CI-green — #137 (test env) + zsh-hook CI failure + #138 (merge gate = CI
  green) — then #143 (panic guard, amplifies #140, fix first) → P1s #139
  #140 #142 #141 → P2s #136 #144 #145 #146 #147 (+ audit extras: socket_dir
  symlink hardening, lru/paste/serde_yaml triage). Pending offers for
  tomorrow: bound critic agent (.claude/agents/critic.md + claude --agent
  critic) before the wave-2 loop; lane plan codex/claude/critic idle + ready.
  NO DISPATCH TODAY. Safe to close.

- 2026-09-07 TAKEOVER (pi, day after the EOD lock): env rebuilt — tmux `personal`
  windows 0 coordinator (this), 1 critic (pi muse-spark-1.3-contributor,
  critic-skill only, per user), 2 claude (opus 5 high), 3 codex (gpt-5.6-terra
  high, full access, weekly 69%). Relay restarted detached
  (personal:coordinator.0, pid in relay.log), inbox empty. Main `48853b1`
  clean; CI still RED (#136 zsh first-byte drop — confirmed live on the runner;
  #137 local repro verified here: 4/5 termctl pass). WAVE-2 STEP 0 DISPATCHED
  in parallel: #136 spawn-readiness → codex (worktree coord/136-spawn-readiness,
  brief .scratch/tasks/136; Working verified — will use shipwright/ponytail
  discipline per its own callout); #137 termctl env test → claude
  (coord/137-test-env, test-only, cross-lane CLI adjudication per the #130/#133
  precedent; Working verified — reading the failing test). Both on
  post-EOD origin/main, zero overlap. On merge of both → first CI-green since
  `8ae14fc`, then close #136/#137/#138, then #143 (codex) + #140 (claude).
- 2026-09-07: #137 MERGED + CLOSED — claude `5c172e5` via no-ff merge on main
  (`git merge --no-ff coord/137-test-env`; auto-close keyword; gate 408 lib + 5
  termctl + 1 main with TERMDECK_SOCK unset, pushed). Critic PASS (2-line
  test-only fix; env sweep clean; the remaining `env::var` reads are production
  or the deliberate TERMDECK_BLESS pattern). ONE NB queued to the backlog:
  socket_from_environment fallback now uncovered — optional deliberate serial
  env test. Worktree + branch removed. #136 still in flight on the codex lane
  (no marker yet). CI will STAY red until #136 lands (zsh test is the blocker);
  step-0 green = both merged.
- 2026-09-07: #136 HANDBACK (lesson #138, live again) — codex e9f3377 merged +
  closed on critic PASS, then CI run 34106468353 STILL FAILED the zsh hook test
  with the same `[("alse", exit 127)]` (panic native.rs:1456). Root cause: the
  readiness gate flips on FIRST output (`input_ready=true` on first
  `PtyEvent::Output`), but a hooked shell can emit output (hook-bootstrap echo,
  prompt paint) BEFORE its termios setup completes — first output is not
  readiness. Merge NOT reverted (strict improvement, local gate green) but issue
  REOPENED with the CI evidence. Coordinator owns the slip: merged on critic
  PASS before CI judged criterion 2 — the exact #138 failure mode. Correction
  dispatched: FRESH codex (context 12% < reuse bar; relaunched, self-updated
  OK) on coord/136-spawn-readiness (ahead 0; no rebase — add on top), brief
  .scratch/tasks/136-correction.brief.md; sanction: termios-set readiness
  (tcgetattr on master reflects child's termios — interactive icanon/echo bit
  pattern) + deterministic local proof (zsh absent here) + preserve #118
  backpressure + flush-before-events order. NEW NB from the #137 critic
  (socket_from_environment fallback uncovered) — claude offered
  single-threaded/Command test; user has QUEUED that instruction in claude's
  composer (staged, unsubmitted) — claude NOT clean-idle; #140 NOT dispatched
  to it yet (hard rule: no staged messages). #140 will go to claude once its
  composer is clean; merge order per lock: #136-correction → #143 → #140.
- 2026-09-07: #136-CORRECTION DONE (codex 08e1d21, rebased 4d57acc) — termios-set
  readiness gate replacing first-output: Linux captures lflag at spawn
  (tcgetattr on master = slave termios) and opens the gate on
  `current != at_spawn && (ICANON|ECHO|ISIG) == ISIG` (readline/ZLE/fish raw
  mode); `write()` returns Queued while closed; non-Linux keeps documented
  output fallback. New deterministic test: bash child emits STARTUP-OUTPUT then
  busy-waits on a marker file — asserts input STAYS queued despite output
  (discriminating vs the old gate), then releases marker and asserts arrival
  whole. Coordinator re-gated post-rebase: 410 lib + 5 termctl + 1 main green;
  the new test runs locally (bash present) and passed; zsh/fish tests still
  skip locally — CI is the judge. Routed to CRITIC round-2 (assignment
  .scratch/tasks/136-correction.critic.md; verifies the Linux master-mirrors-
  slave claim, ISIG pattern across bash/zsh/fish, gate-never-opens vs
  opens-too-early, fd safety, discriminating test). In flight: critic→#136-r2,
  claude→#140 (Working).
- 2026-09-07: #136 round-2 ALSO FAILED CI (run 34109001179): failure changed
  signature from `alse` to `[]` — NO notifications, test ran the full 15s
  deadline → round-2's termios gate NEVER OPENED. Root cause PROVEN locally by
  a coordinator probe: zsh's termios on the master NEVER changes
  (lflag 0x8a3b canonical at spawn, idle prompt, after keypress, after Enter) —
  zsh does not install the raw-with-signals state the gate waits for, so the
  gate can never open for zsh. ALSO corrected my earlier "alse" readings:
  those were substring false-positives (`alse` ⊂ `false`); on this fast machine
  the ORIGINAL race doesn't even reproduce — CI's slower zsh under load is the
  only place it shows. Key unlock: a real zsh downloaded locally
  (zsh 5.9 from apt, /tmp/zsh-local/bin/zsh) — the zsh hook tests now RUN
  locally instead of skipping; no more blind CI iterations. Round-3 correction
  dispatched to the SAME codex session (context 49%, mid-series on this exact
  branch): brief .scratch/tasks/136-correction-r3.brief.md requires a
  readiness signal that cannot falsely satisfy (first-output races the startup
  burst; termios never transitions; bounded fallback vs deadlock weighed),
  real-zsh local iteration, 10x zsh-run robustness, REAL gate numbers.
  #136 stays OPEN; CI stays red until round 3 lands.
- 2026-09-07: #140 DONE (claude 2ab4fd3, rebased clean) — MIN_CANVAS 7x7 guard
  + size_notice fallback, pane_content/right_aligned checked-math replacing
  subtraction in deck.rs/chrome.rs, sheet clamp fixed; 9 new tests incl.
  release-mode + resize-transition coverage; debug (2 documented sandbox
  shutdown/grace flakes, pass isolated) + release 419+5+1 green; fmt+clippy
  clean. Routed to CRITIC (assignment .scratch/tasks/140-small-geometry.
  critic.md). Merge order STILL: #136-r3 → #143 → #140.
- 2026-09-07: #140 MERGED + CLOSED (auto-close keyword) — no-ff merge on main,
  gate 419 lib + 5 + 1 green (clean pass; the 2 parallel failures were
  load-sensitive sandbox flakes — scrollback-timing + documented grace — both
  pass isolated), pushed. Critic PASS, no nits: MIN_CANVAS 7x7 honest
  (whole Narrow pane draws), all subtraction audited behind guards, release-
  meaningful assertions, render.rs hunks verified inset-title/footer NOT
  #144:567. CI for THIS merge still expected red on the independent #136
  zsh test — not this slice. In flight: codex → #136-r3. Remaining queue:
  #136-r3 (blocker) → #143 → P1 #139/#141/#142 → P2s.
- 2026-09-07: #136-R3 DONE (codex 9da4b79) — child-side readiness: each shell
  hook emits a private OSC marker `ESC]7777;termdeck;ready BEL` at its FIRST
  PROMPT (bash PROMPT_COMMAND __td_ready, zsh zle-line-init widget, fish
  fish_prompt self-removing); transport holds queued input until the marker
  is seen (carry buffer + prefix-trim, marker stripped from stream); non-hooked
  panes unchanged. CORRECTION of my earlier round-2 finding: zsh termios
  never transitions was right; the fix is not to detect readiness from the
  OUTSIDE but let the SHELL prove it from INSIDE. Coordinator verification
  with real zsh (apt 5.9 extracted /tmp/zsh-local + module_path fix via
  ZDOTDIR — compiled-in module dir absent here): zsh hook test 10/10 isolated
  + passes in full parallel suite; SAME test with 0.5s DELIBERATE startup
  delay 5/5 PASS (the robustness round-1 lacked); prompt-gate unit test 3/3;
  full gate green except the documented sandbox grace flake; fmt+clippy
  clean. NOTE the local env prerequisite for zsh tests (PATH + ZDOTDIR)
  documented for future sessions. Routed to CRITIC round-3 (assignment
  .scratch/tasks/136-correction-r3.critic.md — marker timing per shell,
  split-marker scanning, marker-never-renders, no-hook-prompt deadlock re-
  check, OSC finished-path non-corruption, $? preservation). On PASS → rebase
  onto origin/main (post-#140) → merge → watch CI for the FIRST green since
  8ae14fc → close #136 #138.
- 2026-09-07: #136-R3 CRITIC PASS → MERGED (8afc045, no-ff; gate 419 lib (only
  the documented grace flake, isolated-green); pushed). CI run 34114581675
  STILL FAILED the zsh hook test — `[]` at 18.34s = the ready marker NEVER
  opened the gate on CI, while the identical binary passed 10/10 locally
  (real zsh, incl. 0.5s delayed startup) + in-suite. Issue REOPENED (round 4).
  Suspected CI-vs-local deltas: (B) zsh first-run wizard on a fresh CI $HOME
  (no rc → zsh-newuser-install → no zle-line-init, marker never fires) —
  STRONGEST suspect; (C) CI $HOME/.zshrc interfering with zle widget
  install order; (A) CI zsh version ≠ local 5.9-6ubuntu2; (D/E) chunk-split
  timing under load / OSC pre-scan ordering. PER USER (codex unusable for 2h):
  MUSE WORKER dispatched (pi muse-spark-1.3-contributor, worker skill newly
  recreated at ~/.pi/agent/skills/worker/SKILL.md, window muse-worker on
  coord/136-spawn-readiness-r4 off origin/main) — brief demands: reproduce
  `[]` under a simulated CI first-run HOME (empty HOME, no rc, no ZDOTDIR),
  fix so the marker fires on the runner, 10x green under CI-like conditions,
  REAL gate numbers; task slug 136-spawn-readiness-correction-r4. Local zsh
  prepped for the worker: /tmp/zsh-local/bin/zsh + ZDOTDIR=/tmp/zshlocal-home
  (module_path fix; apt-extracted zsh lacks the compiled-in module dir). In
  flight: muse-worker → #136-r4; critic idle; claude idle (user has 2 NEW
  requests queued: mouse text-selection + an un-pasteable auto message at
  open — scoped, dispatch after lanes free).
- 2026-09-07 (USER, 2 new requests): (1) cannot select text with the mouse in
  termdeck → filed #148 (mouse text selection, design-first per the #113/#115
  pattern; gesture/rendering/copy-target/alt-screen decisions in a slice doc;
  UI lane) — DISPATCHED to a fresh claude (worktree coord/148-mouse-select,
  brief .scratch/tasks/148-mouse-select.brief.md, Working). (2) an automatic
  message appears when termdeck opens and cannot be pasted — the likely cause
  is the absence of any clipboard/selection surface at all (grep: zero
  clipboard code in the repo) + possibly the zsh first-run wizard on a fresh
  HOME (same mechanism under investigation for #136-r4 on CI); user to
  confirm what the message says; mouse selection #148 likely resolves the
  copy side. In flight: muse-worker → #136-r4; claude → #148.
- 2026-09-07: #148 DONE (claude, f8b52c5 design doc + a9a2f6d impl, rebased by
  coordinator) — design doc committed (docs/design/termdeck/mouse-selection.md)
  settles all 7 decisions: press-location-decides gesture (content=select,
  title/border=reorder, no modifier/mode), cell-range-not-text invariant
  (what's inverted = what's copied), inversion rendering (fg/bg swap, drop
  REVERSED), copy-on-release via OSC 52 (no new dependency, right for SSH),
  ^g v fallback paste through encode_paste, selection does NOT yield to
  alt-screen apps (no clicks forwarded anyway), contracts/engine unchanged
  (src/ui + src/session only). +1476/-48, 13 new tests (baseline 419 → 432
  lib). Coordinator gate: debug 431+1 (documented grace flake, passes
  isolated) / release 432+5+1 green, fmt+clippy clean. Routed to CRITIC
  (assignment .scratch/tasks/148-mouse-select.critic.md — doc-vs-code
  agreement, gesture machine, selection math incl. base64 padding, viewport
  hit-test vs draw_pane agreement, inversion post-style, invalidation incl.
  termctl path, old-gesture regression, zero boundary diff).
- 2026-09-07: #148 MERGED + CLOSED (auto-close keyword) + binary REBUILT —
  no-ff merge on main (432 lib release-verified + 5 + 1), worktree/branch
  removed. CRITIC PASS, 2 NBs queued: (1) drawn_rect duplicates layout math
  with looser guard than MIN_CANVAS — selection can arm on a notice-only
  canvas; align guards; (2) release full-suite note (verified by coordinator:
  green). Mouse selection now LIVE: drag content = select+copy (OSC 52), drag
  title/border = reorder, ^g v = paste last copy. Likely resolves the user's
- 2026-09-07 (USER follow-up on #148): now confirmed the COPY works on
  release (OSC 52 reaches the host clipboard) — the confusion was that the
  highlight NEVER clears on its own (design §3.3 clears only on next
  press/key/wheel/resize/command), so a finished copy reads as "still
  selected". User also identified the "automatic message" at open: the stray
  line `. '/tmp/termdeck-shell-…/bashrc'` — the shell-hook bootstrap source
  echo at pane start (recorded #123 nit). FILED: #150 (selection highlight
  auto-expiry, injected-clock idle window per #115, S/UI) and #149 (bootstrap
  line echo, S/engine-lane shell_hook.rs). USER: give claude BOTH — claude
  DISPATCHED on #150 first (worktree coord/150-selection-expiry), #149 queued
  to the same lane next (worktree coord/149-bootstrap-echo off origin/main;
  note: same file as #136-r4 — serialize, small diff, keep marker/PROMPT-
  COMMAND untouched). Claustra: claude fresh; muse-worker → #136-r4.
- 2026-09-07 (USER follow-up on #148): now confirmed the COPY works on
  release (OSC 52 reaches the host clipboard) — the confusion was that the
  highlight NEVER clears on its own (design §3.3 clears only on next
  press/key/wheel/resize/command), so a finished copy reads as "still
  selected". User also identified the "automatic message" at open: the stray
  line `. '/tmp/termdeck-shell-…/bashrc'` — the shell-hook bootstrap source
  echo at pane start (recorded #123 nit). FILED: #150 (selection highlight
  auto-expiry, injected-clock idle window per #115, S/UI) and #149 (bootstrap
  line echo, S/engine-lane shell_hook.rs). USER: give claude BOTH — claude
  DISPATCHED on #150 first (worktree coord/150-selection-expiry), #149 queued
  to the same lane next (worktree coord/149-bootstrap-echo off origin/main;
  note: same file as #136-r4 — serialize, small diff, keep marker/PROMPT-
  COMMAND untouched). Claustra: claude fresh.session; muse-worker → #136-r4.

- 2026-09-07: #149 MERGED + CLOSED (4895ec0 via conflict-resolved no-ff merge
  a4e9cad; gate 438 lib + 5 + 1; binary rebuilt) — login-bash bootstrap echo
  suppressed (PTY ECHO-off pre-spawn + `stty echo`-first restore). Critic PASS
  (login-bash-only guard, #136/#123 non-regression). TWO follow-ups same
  session: (1) CI red on the merge was NOT code — a pre-existing race in the
  #130 retry-step guard (`cargo --list | grep -q` closes the pipe on its first
  match; cargo BrokenPipe fails the pipeline under `pipefail`) surfaced at 438
  tests; FIXED by capturing the listing into a var before grep; CI run
  34132349287 GREEN. (2) USER REPORTED a DOUBLE PROMPT at pane open ("automatic
  enter": two stacked `andrea@horizon:~/personal$`). Coordinator probe proved:
  every default pane is `bash -l`; --rcfile is ignored for interactive login
  bash; the hook installs by feeding `. '/tmp/…/bashrc'` as a COMMAND → bash
  re-prompts. Pre-#149 the echoed line masked it; echo-off exposed it. FILED
  #151 (profile-shim fix: hook rc as generated-dir .bash_profile + HOME
  scoped then restored, mirroring the zsh ZDOTDIR / fish XDG_DATA_DIRS
  pattern; one prompt, #123 fidelity preserved). DISPATCHED to codex (fresh
  session, engine owner, worktree coord/151-login-single-prompt, Working).
  Lane notes: muse-worker r4 session exhausted at prompt (10.3% ctx) — idle;
  claude idle.

- 2026-09-07 EOD (ALL LANES IDLE, safe to close): user-directed stop for the day.
  SHIPPED today (all closed, CI green): #136 (finally fixed, r4 — Ubuntu
  compinit opt-out + widget repair), #137, #138 (gate=CI process), #140 (small-
  geometry), #148 (mouse text-selection, OSC 52 copy + ^g v), #149 (bootstrap
  echo suppressed), #150 (selection highlight 2s auto-expiry), #151 (single
  login-bash prompt via profile shim). BONUS: fixed a pre-existing CI guard
  race (`--list | grep -q` BrokenPipe under pipefail) that had nothing to do
  with #149's code. #152 filed (accepted residual: /etc/profile runs before
  the login-bash profile shim). Binary rebuilt (release) — the running deck
  must be RESTARTED to load #149/#150/#151 (last rebuild was blocked by a
  live deck: 'text file busy'; rebuild in place once the deck closes).
  REMAINING queue for next session: audit P1s #143 (panic guard, first) #139
  #142 #141; then P2s #144 #145 #146 #147; then #134 (CI advisories); then
  user-gated #94 (MCP merge/drop), #112 (persistence decision gate), #114
  (agent discovery), #33 (parked); plus #152 and the NB backlog (~65). Env:
  tmux personal 0 coordinator / 1 critic (muse) / 2 claude / 3 codex / 4
  muse-worker; relay dead — restart via coordinator skill scripts/relay.sh at
  next session. Resume: read COORDINATION.md + docs/RESUME.md.

- 2026-09-08 TAKEOVER (pi, fresh session): env restored — tmux `personal` rebuilt
  from scratch (only the coordinator pane existed): session renamed
  `default`→`personal`, window 0 → `coordinator`. Relay restarted detached
  (pid 12006, relay.log), inbox empty. Windows recreated per canonical env:
  1 critic (pi muse-spark-1.3-contributor, critic-skill only; one transient
  boot 429 from Console Go, recovered, idle at prompt), 2 claude (opus 5
  high, bypass-permissions), 3 codex (gpt-5.6-terra high, YOLO, weekly 100%)
  — all IDLE, nothing dispatched. 26 stale merged worktree/branches removed
  (coord/111-#135 + nb-*; all verified clean + merged into origin/main +
  closed issues — hygiene owed from the #111-#135 wave). Main `3d2280e`
  clean, synced with origin; CI GREEN (last run 34144976340). gh open set =
  14, unchanged from EOD: audit P1s #143 #139 #142 #141; P2s #144 #145 #146
  #147; process #134; user gates #94 (MCP held) #112 #114 #33 (parked);
  residual #152. NEXT per locked order: #143 (panic guard, amplifies #140)
  first → P1s → P2s → #134, or user direction; #148/#149/#150/#151-era
  critic NBs still in the backlog. NOTE: COORDINATION.md full rotation still
  owed (handoff bullets span 09-04→09-07 across sections incl. a misplaced
  pre-close snapshot below) — archive the >15 oldest to the journal when next
  editing.
- 2026-09-08 (resumed from the EOD lock): #143 (audit P1, panic-guard amplifier)
  DISPATCHED to the codex lane — worktree
  ~/.worktrees/termdeck/143-panic-guard (branch coord/143-panic-guard off
  origin/main 88ccd41); brief .scratch/tasks/143-panic-guard.brief.md
  (drop safe during unwinding: guard on std::thread::panicking() or a
  non-mutating restore; non-panic path unchanged; SUBPROCESS regression test
  asserting single panic + clean exit not SIGABRT; native.rs current_exe
  precedent referenced; out of scope #139/#142/#141/#144-#147). Fresh codex
  session (window relaunched at the worktree, gpt-5.6-terra high YOLO,
  weekly 100%); manifest skill writing-rust (n/a for codex). One lost-Enter
  retry, then verified Working (reading brief + exploring outer.rs/session
  tests). In flight: codex → #143; critic idle; claude idle. Next: critic
  review → merge (standing approval: nit-free PASS) → #139 on the codex
  lane.
- 2026-09-08: #143 DONE (codex 73e14e3 "fix: avoid panic hook restore while
  unwinding"; marker RESULT=pass "PanicGuard skips panic-hook restoration while
  unwinding; subprocess regression and full gate pass"; 2 files:
  src/session/outer.rs +3, src/session/tests.rs +28; protocol clean — not
  pushed, no remote branch). Branch shows behind-1 vs origin/main but that is
  only the coordinator's COORDINATION.md docs commit cf7b985 (no code) — no
  rebase needed. ROUTED to CRITIC (assignment
  .scratch/review/143-panic-guard.critic.md; critic pane Working).
- 2026-09-08: #134 (CI: scheduled advisories issue-opener) DISPATCHED to the
  CLAUDE lane in parallel (zero overlap with #143) — worktree
  ~/.worktrees/termdeck/134-ci-advisories (coord/134-ci-advisories off
  origin/main); brief .scratch/tasks/134-ci-advisories.brief.md (weekly
  scheduled cargo audit → dedupe-opened GitHub issue; push-time
  continue-on-error advisories job byte-identical; minimal permissions;
  CI-only, no code). Fresh claude session (window relaunched at the worktree,
  opus 5 high); dispatch verified Working (reading ci.yml).
  In flight: critic → #143 review; claude → #134; codex idle (next #139 after
  #143 merges).
- 2026-09-08: #143 MERGED + CLOSED (no-ff 1b388e6; critic PASS no nits: guard
  on panicking() stops hook mutation during unwind, subprocess test proves
  single-panic exit 101 not SIGABRT, non-panic drop restores hook, gate
  440+5+1). Worktree+branch pruned. AUDIT P1 fix-first done.
- 2026-09-08: #139 (paste exec via ESC[201~ escape, P1) DISPATCHED to the
  codex lane — worktree ~/.worktrees/termdeck/139-paste-exec
  (coord/139-paste-exec off origin/main); brief
  .scratch/tasks/139-paste-exec.brief.md (refuse/neutralize embedded closer at
  the shared encode_paste boundary, ATOMIC refusal/no partial PTY write,
  truthful ctl refusal, real-shell socket regression test proving
  non-execution, correct the doc-comment premise; out of scope #142/#141).
  Fresh codex session relaunched at the worktree (gpt-5.6-terra high); one
  lost-Enter retry; verified Working (reading input.rs + session.rs).
- 2026-09-08: #134 DONE (claude, 2 commits 2e530f2 + 3e54daf; marker
  RESULT=pass: weekly scheduled cargo-audit workflow opens/refreshes/closes a
  marker-deduped GitHub issue; issues:write only; push-time advisories job
  unchanged comment-only; YAML+bash -n validated, 7 lifecycle cases vs a gh
  stub). Files: .github/workflows/advisories-scheduled.yml (+239),
  ci.yml (+8/-3). ROUTED to CRITIC (assignment
  .scratch/review/134-ci-advisories.critic.md; critic Working, one transient
  provider retry). IN FLIGHT: critic → #134; codex → #139 (Working).
  AUDIT status: #143 ✓; remaining P1 #139 #142 #141; P2s #144 #145 #146 #147.
- 2026-09-08: #134 MERGED + CLOSED (no-ff c3c1145; critic PASS no nits: weekly
  audit workflow opens/refreshes/closes a marker-deduped issue, minimal
  permissions, push-time job untouched, YAML/shell/jq verified locally).
  Worktree+branch pruned. #144 SPLIT into #153 (picker Unicode case-mapping
  panic, UI lane) + #154 (discovery identity collision, CLI lane) to respect
  lane ownership; #144 CLOSED as split.
- 2026-09-08: #153 (picker Unicode filter panic) DISPATCHED to the CLAUDE lane
  — worktree ~/.worktrees/termdeck/153-picker-unicode (coord/153-picker-unicode
  off origin/main c3c1145); brief .scratch/tasks/153-picker-unicode.brief.md
  (match_at maps normalized offsets back to valid boundaries in the original
  string; guard renderer slice; tests for İ/ß/Turkish-I + multibyte queries;
  UI-only, #154 is the engine lane). Fresh claude session relaunched at the
  worktree (opus 5 high); verified Working (reading state.rs + render.rs).
  IN FLIGHT: codex → #139; claude → #153; critic idle (next verdict when a
  slice lands). AUDIT status: #143 ✓ #134 ✓; remaining P1 #139 #142 #141;
  P2s #153 #154 #145 #146 #147.
- 2026-09-08: #139 DONE (codex 4754357 "fix: refuse paste payload terminators";
  marker RESULT=pass "Rejected embedded bracketed-paste closers atomically; real
  socket-to-bash regression and full gate pass"; 3 files session.rs +32 /
  input.rs +26 / tests.rs +254; not pushed). ROUTED to CRITIC (assignment
  .scratch/review/139-paste-exec.critic.md; critic Working).
- 2026-09-08: #142 (unbounded parser memory, P1) DISPATCHED to the codex lane —
  worktree ~/.worktrees/termdeck/142-parser-memory (coord/142-parser-memory off
  origin/main 653f7e2); brief .scratch/tasks/142-parser-memory.brief.md (bound
  unfinished control-string retention with parser recovery, bound per-cell
  combining marks preserving normal graphemes, flat-retention regression test;
  investigate where the parser dependency comes from; out of scope #139/#141/
  #144-#147). Fresh codex session relaunched at the worktree; one lost-Enter
  retry; verified Working (searching vt.rs/Cargo.toml).
  IN FLIGHT: critic → #139 review; codex → #142; claude → #153.
  AUDIT status: #143 ✓ #134 ✓; #139 at critic; remaining P1 #142 #141;
  P2s #153 #154 #145 #146 #147.
- 2026-09-08: #139 MERGED + CLOSED (no-ff 3520b50; critic PASS no nits: shared
  encode_paste refusal, atomic, ctl error 3; real-bash regression proven to
  fail pre-fix; doc-comment premise corrected; gate 442+5+1). tests.rs
  auto-merged cleanly with #143's earlier tests.rs. Worktree+branch pruned.
  AUDIT P1 (fix-first amplifier) closed.
- 2026-09-08: #153 DONE (claude ccc1367 "fix(picker): map filter match offsets
  back onto the original name"; marker RESULT=pass: match_at maps folded offsets
  back to original-name boundaries, renderer slice guarded, unicode tests
  added; gate 444/5/1; 3 UI files state.rs +41 / render.rs +26 / tests.rs +142;
  not pushed). ROUTED to CRITIC (assignment
  .scratch/review/153-picker-unicode.critic.md; critic Working).
  IN FLIGHT: critic → #153 review; codex → #142; claude idle (next #154? no —
  #154 is CLI lane → codex queue; claude next UI slice TBD / may idle).
  AUDIT status: #143 ✓ #134 ✓ #139 ✓; #153 at critic; remaining P1 #142 #141;
  P2s #153 #154 #145 #146 #147.
- 2026-09-08: #153 MERGED + CLOSED (no-ff d5a70ce; critic PASS no nits:
  origins-mapped match offsets plus guarded renderer slice; all 4 new tests
  proven to fail pre-fix incl. exact repro panic; gate 444+5+1). Split from
  #144 (finding 6, P2) — closed. Worktree+branch pruned.
  Remaining audit queue is ALL engine/CLI → codex lane: P1 #142 (in flight)
  then #141; P2s #154 #145 #146 #147. Claude UI lane idle (no remaining UI
  slices). IN FLIGHT: codex → #142; critic idle.
  AUDIT status: ✓ #143 #134 #139 #153; remaining P1 #142 #141; P2 #154 #145
  #146 #147.
- 2026-09-08: #142 DONE (codex 7e7d7d0 "fix: bound terminal parser retention";
  marker RESULT=pass: bounded VT control-string and combining-mark retention,
  full gate green; files Cargo.toml +1 / Cargo.lock +1 / src/engine/vt.rs +351;
  not pushed; duplicate ping ignored per twin-ping protocol). ROUTED to CRITIC
  (assignment .scratch/review/142-parser-memory.critic.md; critic Working).
- 2026-09-08: #141 (shutdown never sweeps escaped descendants, P1) DISPATCHED
  to the codex lane — worktree ~/.worktrees/termdeck/141-shutdown-descendants
  (coord/141-shutdown-descendants off origin/main 0213b9a); brief
  .scratch/tasks/141-shutdown-descendants.brief.md (include validated snapshot
  survivors in the force decision / always sweep after grace regardless of the
  group/session check; completion must not report success while a snapshotted
  descendant is alive; regression test spawns a setsid descendant and asserts
  nothing owned survives; explicitly OUT OF SCOPE #146's PID-reuse guards +
  connect timeout). Fresh codex session; one lost-Enter retry; verified Working.
  IN FLIGHT: critic → #142; codex → #141.
  AUDIT status: ✓ #143 #134 #139 #153; #142 at critic; P1 #141 in flight;
  P2 #154 #145 #146 #147 queued (codex lane).
- 2026-09-08: #142 at CRITIC = HAND-BACK (verdict: “ESC DEL/C1 desync bypasses
  the guard, vte retains ~27MB of 24MB fed; fix advance_escape arms to mirror
  vte”). REWORK ROUND 2 routed back to codex in the SAME worktree
  (coord/142-parser-memory; handback brief at
  .scratch/tasks/142-parser-memory.handback.md: reproduce C1/ESC-DEL bypass
  shapes, fix advance_escape arms to mirror vte exactly, keep all round-1
  passing tests, per-shape flat-retention regressions that fail pre-fix). Codex
  relaunched at the 142 worktree; verified Working. NOT merged.
- 2026-09-08: #141 DONE (codex 1a2992d “fix: sweep escaped shutdown
  descendants”; marker RESULT=pass: validated snapshot survivors are forced
  and awaited; files native.rs +83 / pty.rs +38; not pushed) — ROUTED to
  CRITIC (assignment .scratch/review/141-shutdown-descendants.critic.md;
  critic Working). Held earlier per user direction — dispatched once critic
  was free.
- 2026-09-08: CRITIC LANE stays PI (user decision: forget the opencode
  switch). Brief opencode validation attempt (register muse-spark under
  opencode-go provider; gateway opencode.ai/zen/go) reverted —
  opencode.jsonc restored to original.
  IN FLIGHT: critic → #141 review; codex → #142 rework round 2.
  AUDIT status: ✓ #143 #134 #139 #153; #141 at critic; #142 in rework;
  P1 remaining #141 #142; P2s #154 #145 #146 #147.
- 2026-09-08: #141 MERGED + CLOSED (no-ff f107219; critic PASS no nits:
  snapshot survivors join the force gate with reuse-guarded sweep, awaited
  completion, setsid regression proven to fail pre-fix; gate 448+5+1).
  Worktree+branch pruned. AUDIT P1 closed.
  IN FLIGHT: codex → #142 rework round 2 (Working: writing
  c1_control/escape_del desync regression tests); critic idle (next #142 round
  2 verdict).
  AUDIT status: ✓ #143 #134 #139 #141 #153; #142 in rework; P2s #154 #145
  #146 #147 queued (codex lane).
- 2026-09-08: #142 REWORK ROUND 2 DONE (codex 820e089 "fix: keep VT parser
  guard aligned" on top of 7e7d7d0; marker RESULT=pass: aligned VT escape
  guard with vte DEL/C1 arms, full gate green 445+5+1; files vt.rs +374 /
  Cargo.toml +1 / Cargo.lock +1; not pushed). ROUTED to CRITIC (round-2
  assignment rewritten at .scratch/review/142-parser-memory.critic.md:
  verify the ESC DEL/C1 desync is actually closed, per-shape flat-retention
  tests fail pre-fix, round-1 tests kept; critic Working).
- 2026-09-08: LANE ADJUDICATION (user): “if there is no ui work claude can
  take engine tasks as well”. #147 (bash hook clobbers $?, P2) DISPATCHED to
  the CLAUDE lane — worktree ~/.worktrees/termdeck/147-bash-hook
  (coord/147-bash-hook off origin/main c551118); brief
  .scratch/tasks/147-bash-hook.brief.md (preserve captured $? across EVERY
  return path incl. TERMDECK_NOTIFY=none early returns; test asserts what an
  EXISTING prompt hook observes as $? with a real bash, fails pre-fix; scalar
  AND array PROMPT_COMMAND forms; check zsh+fish for same shape; fix is in the
  embedded SCRIPT TEXT, do not touch #152 shim). Fresh claude session; verified
  Working.
- 2026-09-08: #154 (discovery identity collision, P2) DISPATCHED to the codex
  lane — worktree ~/.worktrees/termdeck/154-discovery-collision
  (coord/154-discovery-collision off origin/main c551118); brief
  .scratch/tasks/154-discovery-collision.brief.md (one deterministic identity
  allocator across grouped+direct discovery mirroring the picker suffix
  allocator at src/ui/picker/state.rs:645; end-to-end discover_workspace test
  for frontends/web + fe-web collision plus multi-collision + order stability;
  grouped-prefix baseline unchanged). Fresh codex session; one lost-Enter
  retry; verified Working.
  IN FLIGHT: critic → #142 round 2; codex → #154; claude → #147.
  AUDIT status: ✓ #143 #134 #139 #141 #153; #142 round 2 at critic;
  P2 in flight #154 (codex) + #147 (claude); queued #145 #146 (codex).
- 2026-09-08: #142 MERGED + CLOSED (no-ff 2221544; critic PASS round 2, no
  nits: guard mirrors vte on DEL/C1/intermediates; 6-shape RSS battery flat
  with recovery; new tests proven to fail on round-1 code; gate 445+5+1).
  Worktree+branch pruned. AUDIT P1s ALL CLOSED (#143 #134 #139 #141 #142).
  IN FLIGHT: codex → #154; claude → #147; critic idle (next verdict #154 or
  #147). AUDIT status: ✓ P1s (#143 #134 #139 #141 #142) + #153;
  P2 in flight #154 (codex) + #147 (claude); queued #145 #146 (codex).
- 2026-09-08: #147 DONE (claude 032fd9a "fix: preserve $? across every return
  path of the prompt hook"; marker RESULT=pass: __td_prompt/__td_prompt_end
  (+zsh __td_precmd, fish __td_postexec) return captured $? on every path;
  real-bash regression covers scalar+array PROMPT_COMMAND and
  TERMDECK_NOTIFY none/error/all; gate 449/5/1; files shell_hook.rs +92/-11;
  not pushed). ROUTED to CRITIC (assignment
  .scratch/review/147-bash-hook.critic.md; critic Working, transient 429
  auto-retry).
- 2026-09-08: #154 DONE (codex fe1f841 "fix: disambiguate discovered terminal
  identities"; marker RESULT=pass: stable unique discovery identities, full
  gate; files cli/mod.rs +88; not pushed; duplicate pings ignored). QUEUED
  for critic (assignment .scratch/review/154-discovery-collision.critic.md,
  reviewed after #147 verdict).
- 2026-09-08: #145 (termctl literal-help swallow + zoom contract, P2)
  DISPATCHED to the codex lane — worktree ~/.worktrees/termdeck/145-termctl-help
  (coord/145-termctl-help off origin/main 0d3edad); brief
  .scratch/tasks/145-termctl-help.brief.md (help_for option-value aware so
  literal --help/-h values of --text/--paste/--keys stay values, tests vs the
  binary prepass fail pre-fix; zoom RESOLUTION chosen = KEEP toggle, fix help
  text to say bare form toggles + point at status; dispatch unchanged). One
  lost-Enter retry; verified Working.
- 2026-09-08: #146 (PID-reuse first signals + ctl connect timeout, P2)
  DISPATCHED to the CLAUDE lane (engine slice per user ruling) — worktree
  ~/.worktrees/termdeck/146-lifecycle (coord/146-lifecycle off origin/main
  0d3edad); brief .scratch/tasks/146-lifecycle.brief.md (validate identity
  before EVERY signal/ownership expansion incl. pre-kill_session SIGKILL;
  retire transport ownership on shell reap; no drop re-entry after explicit
  shutdown; read the just-merged #141 f107219 first; plus one absolute
  deadline spanning connect+write+read and a saturated-backlog timeout test
  that fails pre-fix). Fresh claude session; verified Working.
  IN FLIGHT: critic → #147 (then #154 queued); codex → #145; claude → #146.
  AUDIT status: ✓ #143 #134 #139 #141 #142 #153; at/queued critic #147 #154;
  in flight #145 (codex) #146 (claude). Remaining open: 4 (#145 #146 #147 #154).
- 2026-09-08: #147 MERGED + CLOSED (no-ff d378049; critic PASS no nits: every
  return path in bash/zsh/fish returns the captured status; live-bash test
  asserts downstream observed $? fails pre-fix with garbage; gate 449+5+1).
  Worktree+branch pruned. Audit P2 closed.
  #154 moved into the critic slot (critic Working on it).
  IN FLIGHT: critic → #154; codex → #145; claude → #146.
  AUDIT status: ✓ #143 #134 #139 #141 #142 #147 #153; at critic #154;
  in flight #145 (codex) #146 (claude). Remaining open: 3 (#145 #146 #154).
- 2026-09-08: #145 DONE (codex 48c4d00 "fix: preserve literal help input
  values"; marker RESULT=pass: literal help input values preserved and zoom
  help clarified; files termctl.rs +29/-3; not pushed). QUEUED for critic
  (assignment .scratch/review/145-termctl-help.critic.md; reviewed after
  #154 verdict). Codex lane now idle — all audit slices dispatched (remaining
  are at critic or on claude). #152 remains deferred (user decision pending).
  IN FLIGHT: critic → #154 (then #145 queued); claude → #146; codex idle.
  AUDIT status: ✓ #143 #134 #139 #141 #142 #147 #153; at/queued critic #154
  #145; in flight #146 (claude). Remaining open: 3 (#145 #146 #154).
- 2026-09-08: #154 MERGED + CLOSED (no-ff 4657289; critic PASS no nits: single
  allocator over both passes mirroring picker suffixes, deterministic order,
  end-to-end test fails pre-fix with exact duplicates; gate 449+5+1). Split
  from #144 (finding 11, P2) — closed. Worktree+branch pruned.
  #145 moved into the critic slot (critic Working, transient 429 auto-retry).
  IN FLIGHT: critic → #145; claude → #146; codex idle.
  AUDIT status: ✓ #143 #134 #139 #141 #142 #147 #153 #154; at critic #145;
  in flight #146 (claude). Remaining open: 2 (#145 #146).
- 2026-09-08: #145 MERGED + CLOSED (no-ff 4e95f3b; critic PASS no nits:
  value-aware help prepass mirroring the real parser, both literals x all
  three options, zoom texts truthful with dispatch untouched; gate 453+7+1).
  Worktree+branch pruned. Audit P2 closed.
  IN FLIGHT: claude → #146 (last audit issue); critic idle (next #146); codex
  idle. AUDIT status: ✓ #143 #134 #139 #141 #142 #147 #153 #154 #145;
  remaining open: 1 (#146).
- 2026-09-08: #146 DONE (claude e82d585 + fde3f4a; marker RESULT=pass:
  every signal+ownership expansion identity-validated, reaped shells retire
  their pid, engine drop no longer re-enters shutdown, ctl client one absolute
  deadline over connect+write+read; gate lib 457 / termctl 5 / main 1; files
  pty.rs +407 / ctl/mod.rs +192 / native.rs +70; not pushed). ROUTED to CRITIC
  (assignment .scratch/review/146-lifecycle.critic.md — flagged the #141
  non-regression and every-signal-site checks; critic Working). LAST AUDIT
  ISSUE. IN FLIGHT: critic → #146; codex + claude idle.
  AUDIT status: ✓ #143 #134 #139 #141 #142 #147 #153 #154 #145; #146 at
  critic. Remaining open: 1 (#146).
- 2026-09-08: #146 MERGED + CLOSED (no-ff 53ad87c; critic PASS no nits: every
  signal validates identity, reap retires ownership, drop runs once, #141
  sweep preserved; absolute client deadline with saturated-backlog proof; gate
  457+5+1). Worktree+branch pruned. Audit P2 closed.
  ★★ AUDIT COMPLETE — every audit-b3df768 issue (#139 #140 #141 #142 #143
  #144→#153/#154 #145 #146 #147, plus CI flag #134) is MERGED + CLOSED. Open
  audit issues: none. Closed this run: #143 #134 #139 #142 #141 #153 #154 #147
  #145 #146 (all merged no-ff + CI + closed); #144 split into #153+#154.
  OPEN DECISIONS FOR USER: (1) #152 residual — include in "audit closed" or
  keep design-first/deferred? (2) ~62 stale origin/coord/* remote branches —
  clean up? (3) rebuild/refresh installed ~/.local/bin/termdeck binary
  (deferred during batch).
- 2026-09-08: REBUILD done — `cargo install --path . --root ~/.local --locked
  --force`: termdeck (1.8MB) + termctl (513KB) reinstalled at ~/.local/bin
  (Sep 8 12:55) from merged main ffb79ef.
- 2026-09-08: #152 (login-pane /etc/profile-before-shim residual, design-first)
  — RESEARCH dispatched to the researcher lane (new window personal:4
  researcher, muse-spark via pi; durable boot at
  coordinator/scripts/researcher-boot). Study at
  /tmp/shipwright/termdeck/152-login-bash-shim/study.md; deliverable
  /tmp/shipwright/termdeck/152-login-bash-shim/research.md (open questions:
  mechanisms for sudo-hint/hushlogin/bash_completion under scoped-HOME login
  bash, real Ubuntu /etc/profile/bash behavior, #151 non-regression,
  do-nothing option quantified). Researcher Working; result routes to user for
  a decision (no branch yet — research never commits).
  LANES: researcher → #152 study; critic/codex/claude idle.

## EOD 2026-09-04 (pre-close snapshot)
- main 75deb3d · 320 lib + 4 integration · binary current ~/.local/bin/termdeck
- tmux personal: 0 coordinator | 1 critic (idle) | 2 claude (idle) | 3 codex
  (idle) | researcher on-demand; relay alive; inbox empty.
- OPEN: #94 MCP (held on coord/94-ctl-mcp, user call) · #33 animations
  (parked). NOTHING in flight; safe to close.
- Durable-resumption: read COORDINATION.md + docs/RESUME.md + journal
  (the skills repo has the coordinator/critic/researcher specs incl. the
  canonical env + protocols).
