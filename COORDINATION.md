# Coordination — Termdeck

Status: MERGED — workflow-fixes (relay round/singleton/atomic + protocol docs + COORDINATION rotation) merged 99f2cb1; product delivery through #155 complete at baseline dc6cfcf (2026-09-08).
- Closed delivery queue: audit findings and #134/#152, sessions (#112/#156), #153/#154/#157, and #155; latest recorded product merge `a55c421`.
- Parked decisions: #94 (MCP), #114 (discovery), #33 (animations) have no explicit closure in this checkout's journal. The last handoff's “Open: none” is not a refreshed tracker census; verify these decisions before dispatch.
- Gate evidence: last recorded CI run `34144976340` was green; CI has not been refreshed for this dashboard edit. Latest recorded #155 local gate: 488 library + 10 termctl + 1 main tests.
- Residuals: NB backlog last recorded at ~65; #155 raw input containing whitespace is tokenized (surfaced to user). Do not equate delivery closure with residual closure.
- Scope here: external workflow repairs and local rotation only; no tracker changes, push, merge, or PR. This worktree snapshot does not supersede later coordinator activity elsewhere.

## Goal

Implement Termdeck as a standalone Rust terminal workspace with a master and
live-preview-stack interface.

## Issues (archived)

The historical #1-#14 issue/slice/Waves tables were pruned 2026-09-05 per #129 (stale dashboard); archived verbatim in `.coordinator/journal/history-2026-09.md`. Live status = GitHub issues + the Handoffs log below.

## Decisions

- External generic tool; Horizon integration is configuration-only.
- Rust 1.98.0, Ratatui, native ANSI backend/input decoder, Alacritty terminal state, portable PTYs (Crossterm superseded; see `docs/PLAN.md`).
- Master-and-preview-stack interface; no grid.
- No tmux, OpenMux, daemon, or live-process resume in the product. Persistence is on-disk layout/text snapshots restored into fresh shells; mouse forwarding is supported (see `AGENTS.md` and `docs/PLAN.md`).
- Worker session: tmux `personal`, with coordinator, critic, Claude, and Codex lanes; researcher on demand.
- The committed Design export is the visual review authority; Claude may also
  inspect the linked project through the `claude_design` MCP.
- With a one-hour user window, next-wave tasks are intentionally bounded to one
  behavior and a focused test/snapshot rather than whole subsystem branches.

- Worker protocol (earned, keep): fresh session per task by default (reuse only
  healthy mid-series); give work only to IDLE workers (verify prompt, no
  working/queued state); submit once and capture actual turn state; retry Enter
  once only for a visibly unsubmitted pointer; workers commit locally, never push/merge; finish = status marker +
  inbox ping; never steer a running worker — new specs only when the task is
  with the critic; codex restores keep reasoning effort high; NEVER merge a
  red-gated branch (#74/#80 lesson).
- Worker env: tmux session `personal`, one window per worker (user decision
  2026-09-04: no separate `termdeck-agents` session).

## Handoffs

Rotated history: [219 older handoffs](docs/coordination-archive.md); earlier
archives in `.coordinator/journal/` (2026-09). Entries below are historical
events in order; only the top dashboard describes current status.

- 2026-09-08: ack-supervision done (codex gpt-6-astra/low, all external — no
  repo commit). Relay now CONFIRMS tmux sends (DELIVER+consume only after text
  AND Enter succeed; RETRY on transient failure, retained event + FAILED after
  3, Enter-phase isolation, no interleave); new read-only `check-aborted.sh`
  + provider-recovery.md (bounded 10/20/40s+jitter, one nudge after verified
  idle abort, restart/model switch needs coordinator auth); finish-protocol +
  SKILL.md require reviewed-HEAD in verdict (post-PASS behavioral change needs
  re-review). Tests: test_relay.py (extended), test-check-aborted.py,
  fmt/clippy/499. Relay PID 265383 (single). Live detector: critic idle-ok,
  claude idle-ok, codex working, researcher retry-wait (heuristic).
- 2026-09-08: COMPLEMENT research done (researcher; untouched by coordinator
  dispatch — arrived via relay). Termdeck as complementary pane watch/control
  layer over external multiplexer sessions (tmux/Zellij/Herdr). Recommend one
  local tmux READ-ONLY text-watch connector first (tmux 3.7b present, locally
  testable; ~7-15 person-days MVP; watcher exits close only watcher resources,
  foreign panes/processes survive; input/creation/destructive ops disabled for
  external panes). CORRECTION: Herdr is a separate terminal server with its own
  socket API — herd-lite is PHP/Laravel tooling, no local Herdr executable;
  assess via Herdr's own API, never assume tmux exposes Herdr panes. GATE: this
  is PRODUCT tmux integration — AGENTS.md:15 (no tmux/daemon) needs a USER
  DECISION + minimal candidate revision before any implementation (connectors
  may observe/control user-managed sessions; Termdeck never starts their
  servers or destroys panes; no daemon/background service). OPEN FOR USER:
- 2026-09-08: BRANCH CLEANUP (user-authorized): deleted all 62 stale
  origin/coord/* remote branches + removed 3 merged local branches and
  worktrees (coord/relay-fix, coord/sessions-engine, coord/complement-research).
  Remote now has only main. Local: only main.
- 2026-09-08: MISSED RESEARCH RECOVERED (user: “I told you to research some
  other topics didn't I?” — correct). Three studies written 22:33-34 were never
  run (researcher window gone; complement delivered, these did not):
  lua-scripting, performance, theming (each a full study in
  /tmp/shipwright/termdeck/<topic>/study.md). NOW RUNNING: re-dispatched to
  the codex gpt-6-astra/low worker as researcher (read-only, no worktrees per
  the studies), SEQUENTIALLY: lua-scripting (Working) → performance → theming,
  each a separate deliverable + marker + ping. Complement slice (tmux read-only
  proof on separate branch) queued after the research.
- 2026-09-08: lua-scripting research DONE (codex astra/low as researcher;
  deliverable lua-scripting-research.md 23KB; worker initially skipped the
  finish artifacts — nudged to write marker + ping; now complete). RECS: mlua
  0.12.1 vendored Lua 5.4; min = notify/pane-lifecycle hooks + copied state +
  bounded action API (retain YAML); interpreter in a session-owned helper
  process (crash can NEVER kill the deck); ~12-21 person-days minimum.
  performance research DISPATCHED next (same worker; marker+ping path pinned).
- 2026-09-08: USER: PURGE THE SESSIONS FEATURE (“we did some work regarding
  sessions lately — that needs to go”). Purge slice DISPATCHED to CLAUDE
  (idle; codex busy on performance research — engine-task ruling)
  coord/purge-sessions off origin/main ffa8d81; brief
  .scratch/tasks/purge-sessions.brief.md: revert BOTH sessions merges (228e50f
  engine + 0846cd7 picker) with -m 1 in REVERSE order, resolving conflicts vs
  #155/#157 (keep those); remove snapshot.rs/restore_lines/SnapshotBrowse/
  attach/sessions/save/restore context picker + fixtures; revert AGENTS.md
  persistence clause to original one-liner; acceptance = zero grep hits +
  help shows no verbs + gate green. #156 REOPENED (tracks the purge); #112
  commented SUPERSEDED. Claude Working.
  LANES: claude → purge-sessions; codex → performance research;
  theming queued (after performance).
- 2026-09-09: PURGE MERGED + CLOSED. purge-sessions merged (no-ff e3a7c8b;
  critic PASS no nits: greps clean, verbs/help purged, #155 named-keys + #157
  border intact and passing, no ghost variants/modules, AGENTS.md one-liner
  restored; gate 467+10+1). #156 CLOSED (purge complete). Binaries rebuilt —
  termctl help shows input + named-keys only; termdeck help has no
  attach/sessions/save/restore. ~2.6k lines of sessions code removed.
  Also fixed the RELAY stuck-pings: it had been targeting a stale coordinator
  pane (personal:coordinator.0); restarted pointed at %19; all retained
  claims recovered and delivered.
- 2026-09-09: performance research DONE (codex astra/low; deliverable
  performance-research.md 27KB, 142 lines, with REAL measure-idle.py runs:
  /bin/sleep 300 panes, CPU ticks + context switches). FINDINGS: dirty-only
  draw + Ratatui diff already exist; idle wakes = main loop 20ms (~50/s) +
  PTY readers 100ms (10/s/pane) — blocking polls not busy-spin; every chunk
  parsed→full owned frame→cloned even for hidden panes. STAGED FIX ~10-19
  person-days: P0 cancellable reader timeouts + cached geometry + baseline
  protocol; P0 shared main readiness/deadline wait (4-7d, biggest idle win at
  few panes); P1 parse/frame split + visible-only projections (best at many/
  hidden panes). Honest: no battery % claimed; strict before/after protocol +
  <0.5% core target at 16 idle panes; smallest high-confidence change =
  cancellable reader waits. Routed to user. theming research in flight
  (codex). LANES: codex → theming research; critic + claude idle.
- 2026-09-09: theming research DONE (codex astra/low; deliverable
  theming-research.md 165 lines complete; finish marker+ping NOT written —
  codex hit its 5h limit mid-finish (resets 03:30); deliverable verified
  complete + routed). RECS: typed 18-token RGB palette, one versioned YAML
  theme file, built-in default = current palette exactly; colors-only v1
  (~7-12 person-days), startup loading + recoverable file errors, defer
  live reload + terminal-theme detection; theming does NOT recolor child
  ANSI palettes (separate engine-facing extension if wanted); border-state
  contract = distinguishable RENDERED states (not 6 unique colors — notify
  shares warning, master/target share accent, #157 quiet rule kept);
  contrast: HINT 2.5:1 (low), MUTED 4.1 — high-contrast preset + diagnostics
  rather than recoloring default. ALL THREE research topics delivered:
  lua-scripting, performance, theming.
- 2026-09-09: CODEX 5H LIMIT EXHAUSTED (user hit limits; 5h window 0%,
  resets 03:30; weekly 62%). No codex dispatches until reset. Theming
  finish (marker+ping) pending reset. Complement proof (tmux read-only)
  still queued — codex after reset or another lane. LANES: critic + claude
  idle; codex capped (resets 03:30).
- 2026-09-08: STRATEGIC REFRAME (user): termdeck should COMPLEMENT
  multiplexers/agent-harness (tmux/zellij/herdr) as the watch/control layer,
  not compete by building a pane server / live-resume. User endorsed the
  PANE-SOURCE ABSTRACTION hypothesis (consume panes from any backend; peek/
  notify/master-preview/ctl consume that abstraction). Researcher study
  dispatched — pi CANNOT serve gpt-6-astra (opencode-go 401), so the research
  runs as CODEX (gpt-6-astra/low; user: “use codex not pi”), researcher role,
  read-only. Worktree coord/complement-research; brief
  .scratch/tasks/complement-research.brief.md (validate position per target,
  ground tmux control-mode/zellij/HERDR specifically, sketch the pane-source
  seam, MVP external-session-watch slice, consume-vs-expose, governance flag,
  fidelity). Deliverable
  /tmp/shipwright/termdeck/complement/termdeck-complement-research.md.
  Codex Working (loaded .codex/skills/researcher).
- 2026-09-08: THREE NEW RESEARCH-FIRST TOPICS queued (user) — performance/
  battery+resource management, Lua scripting, theming. Studies written at
  /tmp/shipwright/termdeck/{performance,lua-scripting,theming}/study.md,
  deliverables matching -research.md. RESEARCH QUEUE on the codex
  gpt-6-astra researcher (sequential): 1) complement-research (in flight) →
  2) performance-research → 3) lua-scripting-research → 4) theming-research.
  Dequeue as the lane frees; each: deliverable + marker + ping, then route
  the brief to the user.
- 2026-09-08: USER SCOPE DECISION — add BOTH termctl verbs, FULL
  functionality (not the cut MVP). Surface: `termdeck attach <ws>`/
  `sessions` = launch-time resume; `termctl save [name]` = runtime checkpoint
  of the live session (default=workspace name, named for backups);
  `termctl restore <name>` = runtime in-place restore (replace running
  session's workspace from snapshot, fresh shells + replay, destructive-explicit).
  Scope: save-on-clean-quit + termctl save/restore + attach/sessions +
  context picker + 2000-line text replay + banner. Still daemon-less,
  #112-gated, spike-first on restore_lines. Awaiting user GO to dispatch.
- 2026-09-08: USER GO — sessions feature DISPATCHED (full scope, both termctl
  verbs). Two parallel lanes off origin/main 0e061de:
  - coord/sessions-engine (codex): step-0 restore_lines spike (hostile
    transcript: shell unaffected, pending_input_len()==0, no Notify),
    session.v1 DTO + save/load (0600 atomic, XDG state dir, 2000-line cap),
    save-on-clean-quit, termctl save [name]/restore <name>, termdeck
    attach/sessions; must NOT touch shutdown model/daemon.
  - coord/sessions-picker (claude): SnapshotBrowse over sessions dir
    (recent-first, age+pane-count), context picker (Resume… + Open a folder…,
    empty→fall-through, no auto-attach), restore banner via set_notice;
    UI-only, interface = session.v1 schema + restore invocation.
  Both read /tmp/shipwright/termdeck/sessions/sessions-research-{2,2b}.md.
  Interface contract pinned (session.v1 schema fixed in §3). Both Working.
  COORDINATOR: apply AGENTS.md narrowing + #112 update (governance edit, per
  design §6) as part of this feature.
- 2026-09-08: AGENTS.md narrowed (session persistence = on-disk snapshots into
  fresh shells; no daemon/live-resume) at 4fee436; #112 CLOSED (decision:
  snapshot-restore adopted, live-resume out of scope).
- 2026-09-08: sessions-engine DONE (codex b45d577 replay + b57ca2a snapshots;
  marker RESULT=pass: engine-safe replay plus daemon-less session snapshots,
  CLI, ctl save/restore; 11 files +937 — snapshot.rs +433, native.rs +139,
  session.rs +142, ui/state.rs +47 [lane-adjacent], cli +49, termctl +39, ctl
  +32, main.rs +19, fake +24, contracts/engine.rs +9 [additive trait],
  tests ±36; not pushed). ROUTED to CRITIC (assignment
  .scratch/review/sessions-engine.critic.md — security-relevant replay
  hardening + non-execution proof; critic Working). Claude still on
  sessions-picker (13m+). IN FLIGHT: critic → sessions-engine; claude →
  sessions-picker.
- 2026-09-08: sessions-picker DONE (claude 2d2ba91 + 7b4eb6e; marker
  RESULT=pass: context picker header-only reader, 2 sections, no auto-attach,
  zero-sessions fall-through + restore banner; gate lib 475 / termctl 7 /
  main 1, fmt+clippy clean; 16 files +1479 — snapshot.rs +270, render.rs
  +312, state.rs +134, tests +386, session.rs +113, main.rs ±25; not pushed;
  resume routes to session::resume, engine restore_lines seam still open →
  merge sequenced engine-first). QUEUED for critic (assignment
  .scratch/review/sessions-picker.critic.md; reviewed after sessions-engine
  verdict). Critic Working on sessions-engine after a transient 429 nudge.
  Overlap watch: picker touches session.rs + main.rs (engine branch also) —
  merge engine first, then reconcile. IN FLIGHT: critic → sessions-engine
  (then sessions-picker); codex + claude idle.
- 2026-09-08: sessions-engine MERGED (no-ff 228e50f; critic PASS no nits:
  replay triple-hardening proven live with non-execution proof, schema
  pinned, save/restore verbs work end-to-end live-probed, shutdown untouched,
  no daemon; gate 466+7+1). #156 created (sessions feature tracking).
  sessions-picker ADVANCED to critic (critic Working). IN FLIGHT: critic →
  sessions-picker. Merge watch: picker touches session.rs/main.rs — will
  reconcile onto engine-merged main at picker merge. #156 stays open until
  both land.
- 2026-09-08: sessions-picker at CRITIC = HAND-BACK (verdict: resume() opens
  a fresh workspace under the restore banner instead of routing to the engine
  restore — no transcripts/layout/skips; rewire to run_restored after
  engine-first merge). Engine IS now merged (228e50f) so run_restored exists
  on main. Queued for claude (after #157). Claude currently on #157
  (hidden-stack-border). Critic idle. #156 stays open.
- 2026-09-08: #157 (UI: no accent border when stack hidden) DISPATCHED to
  claude — worktree ~/.worktrees/termdeck/hidden-stack-border
  (coord/hidden-stack-border off origin/main 39105e9); brief
  .scratch/tasks/hidden-stack-border.brief.md (when stack hidden —
  Layout::Zoom|Narrow at deck.rs:156-159 — master border must be quiet not
  ACCENT; stack-visible keeps ACCENT; don't regress drag/demoted/notify/
  preview; fixtures both ways). Claude Working.
  IN FLIGHT: claude → #157; then sessions-picker handback (rewire resume→
  run_restored) after rebase onto engine-merged main; critic idle.
- 2026-09-08: #157 DONE (claude d598cf7; marker RESULT=pass: zoom/narrow
  master border now IDLE_BORDER not ACCENT, stack-visible unchanged, gate
  467/7/1; deck.rs +7 / tests.rs +51; not pushed). ROUTED to CRITIC
  (assignment .scratch/review/hidden-stack-border.critic.md; critic Working).
  sessions-picker HANDBACK dispatched to claude (relaunched at sessions-picker
  worktree; brief .scratch/tasks/sessions-picker.handback.md: rebase onto
  engine-merged main + rewire resume()→run_restored so transcripts/layout/
  cwd-skips actually load, prove with a test; claude Working).
  IN FLIGHT: critic → #157; claude → sessions-picker handback; codex idle.
- 2026-09-08: #157 MERGED + CLOSED (no-ff 01999a7; critic PASS no nits:
  Zoomed|Compact arm maps exactly to the hidden-stack layouts with all other
  border states precedent; test fails pre-fix with ACCENT teal; gate 467+7+1).
  Worktree+branch pruned. (Earlier push to 68f5ef4 had a transient SSH timeout
  — retried OK.) IN FLIGHT: claude → sessions-picker handback; critic idle
  (next: picker re-review); codex idle. #156 open until picker re-passes.
- 2026-09-08: sessions-picker HANDBACK DONE (claude 0a5d5fa on top of
  rebased 2b31871+61579a1; base d4a9990; marker RESULT=pass: resume() routes
  to engine run_restored — layout+transcripts+cwd skips; gate 483/7/1 green;
  ahead 3, deduped against engine's snapshot.rs; not pushed). ROUTED to CRITIC
  for RE-REVIEW (assignment updated with handback clause: confirm resume→
  run_restored via engine-state test not banner; critic Working).
  IN FLIGHT: critic → sessions-picker re-review; claude + codex idle.
- 2026-09-08: SESSIONS FEATURE COMPLETE. sessions-picker MERGED (no-ff
  0846cd7; critic PASS re-review: resume() routes through run_restored via
  load_file with engine-state tests killing the old fresh-open behavior;
  dedup complete; gate 483+7+1). #156 CLOSED. Worktree+branch pruned.
  BINARIES REBUILT (cargo install --force): termdeck 1.99MB + termctl 514KB,
  `termdeck sessions` + `termdeck attach NAME` live in help.
  Remaining open: #155 (named send-keys) — queued, not picked up.
  LANES: critic/codex/claude/researcher idle.
- 2026-09-08: #155 (named send-keys grammar) DISPATCHED to the codex lane —
  worktree ~/.worktrees/termdeck/155-send-keys (coord/155-send-keys off
  origin/main d736cca); brief .scratch/tasks/155-send-keys.brief.md (tmux-like
  key-name grammar C-/M-/S- + named keys + literal text, encoded to bytes at
  termctl BEFORE the ctl request so the wire/schema stays bytes; gate
  --force/TERMDECK_ALLOW_INPUT + bounded path unchanged; BACKWARD COMPAT for
  raw-byte senders is a required explicit design decision; unknown name → clear
  error; align encoder with key_sequence()/KeyReader in src/session/input.rs;
  help + tests). One lost-Enter retry; verified Working.
  LANES: codex → #155; critic/claude/researcher idle.
- 2026-09-08: #155 DONE (codex d755812 "feat(termctl): encode named key
  input"; marker RESULT=pass: named termctl key encoder committed, full gate
  green; files input.rs +239 / termctl.rs +65 / session.rs ±2; not pushed).
  ROUTED to CRITIC (assignment .scratch/review/155-send-keys.critic.md —
  named-grammar correctness vs key_sequence()/KeyReader, wire/gate unchanged,
  and the REQUIRED backward-compat decision for raw-byte senders as the key
  focus; critic Working). IN FLIGHT: critic → #155; codex + claude idle.
- 2026-09-08: #155 MERGED + CLOSED (no-ff a55c421; critic PASS: named keys
  byte-exact with the outer reader, encode pre-request, ctl.v1 schema
  untouched, single-token raw bytes + Dollar-Ctrl pass through, Raw:
  documented; gate 488+10+1; one NON-BLOCKING residual recorded — raw input
  containing whitespace is split by the tokenizer; surfaced to user).
  Worktree+branch pruned. All audit + sessions + #157 + #155 DONE. LANES all
  idle. Open: none.
- 2026-09-08: RELAY DOUBLE-PING STILL OCCURRING (my content-md5 dedupe missed
  the same-event-different-text case — the #155 critic verdict re-emitted with
  fuller wording and both delivered). USER tasked a fresh codex worker (model
  astra → auto-resolved gpt-6-astra, effort LOW) to (1) fix relay.sh to dedupe
  by EVENT/task-slug not content, restart it, verify exactly one relay;
  (2) after the fix, write a COMPREHENSIVE review of the coordinator workflow
  to /home/az/Desktop/coordinator-workflow-review.md. Worktree
  coord/relay-fix; brief .scratch/tasks/relay-fix.brief.md. Worker Working.
  LANES: codex(astra/low) → relay-fix+review; critic/claude/researcher idle.
- 2026-09-08: RELAY FIX COMPLETE + VERIFIED (codex gpt-6-astra/low; marker
  RESULT=pass). Relay dedupes by EVENT/TASK-SLUG (not content): verified live
  — same-slug re-write → DUP/dropped, distinct slug → delivered. Relay
  restarted, single instance PID 256628. WORKFLOW REVIEW written to
  /home/az/Desktop/coordinator-workflow-review.md (23KB, FACT/INFERENCE/
  PREFERENCE, covers what worked/brittle + prioritized recommendations).
  fmt/clippy/499 tests pass; NO repo commits (infra+doc task, nothing to
  critic/merge). Worktree coord/relay-fix left for now (no repo change).
  LANES all idle. Open: none.
- 2026-09-08: WORKFLOW FIXES dispatched to the SAME codex worker
  (gpt-6-astra/low), time-boxed ~45 min, fixing the issues it identified in its
  review: (P0) relay.sh singleton lock + atomic inbox claim + task/role/round
  event identity, verified live; finish-protocol.md de-drifted (drop md5
  claim, document round identity); research-protocol.md artifact-completeness
  check; (P1) env-tmux.md dispatch-state (foreground ≠ Working); COORDINATION.md
  reconcile + rotate oldest handoffs to tracked archive (repo commit, local
  only). Brief workflow-fixes.brief.md. Worker Working; reports completed vs
  deferred. COORDINATION rotation commit will be reviewed by coordinator
  (governance) before merge.

## EOD 2026-09-04 (pre-close snapshot)
- main 75deb3d · 320 lib + 4 integration · binary current ~/.local/bin/termdeck
- tmux personal: 0 coordinator | 1 critic (idle) | 2 claude (idle) | 3 codex
  (idle) | researcher on-demand; relay alive; inbox empty.
- OPEN: #94 MCP (held on coord/94-ctl-mcp, user call) · #33 animations
  (parked). NOTHING in flight; safe to close.
- Durable-resumption: read COORDINATION.md + docs/RESUME.md + journal
  (the skills repo has the coordinator/critic/researcher specs incl. the
  canonical env + protocols).

- 2026-09-09: COORDINATOR SKILL de-termdecked (user request, project wrap-up) — actual
  skill changes committed to the SKILLS repo (/home/az/projects/skills, master,
  pushed) together with the session's relay/protocol hardening; this COORDINATION
  entry is only the project journal. Details of the cleanup: deleted
  references/env-termdeck.md + scripts/relay-termdeck.sh; SKILL.md env detection now
  generic (tmux runbook only, no termdeck/tmux bifurcation); relay env override renamed
  TERMDECK_COORD_PANE -> RELAY_COORD_PANE (relay.sh + env-tmux.md). Next relay restart
  must use RELAY_COORD_PANE=current coordinator pane id. Historical ack-supervision.notes.md
  left as a dated record (3 mentions) — delete on request.
