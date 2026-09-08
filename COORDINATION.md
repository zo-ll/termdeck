# Coordination — Termdeck

Status: REVIEW READY — workflow-fixes implemented on `coord/relay-fix`; product delivery through #155 is complete at baseline `dc6cfcf` (2026-09-08).
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

## EOD 2026-09-04 (pre-close snapshot)
- main 75deb3d · 320 lib + 4 integration · binary current ~/.local/bin/termdeck
- tmux personal: 0 coordinator | 1 critic (idle) | 2 claude (idle) | 3 codex
  (idle) | researcher on-demand; relay alive; inbox empty.
- OPEN: #94 MCP (held on coord/94-ctl-mcp, user call) · #33 animations
  (parked). NOTHING in flight; safe to close.
- Durable-resumption: read COORDINATION.md + docs/RESUME.md + journal
  (the skills repo has the coordinator/critic/researcher specs incl. the
  canonical env + protocols).
