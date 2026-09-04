# Coordination journal — 2026-09

Automatically rotated from COORDINATION.md. Query with `rg`. Obsidian-compatible.

- 2026-09-01: Coordinator takeover. Verified main (`e78a5b2`) clean and in
  sync: fmt, clippy `-D warnings`, 26 tests pass. All four PRs merged on
  GitHub; the old `termdeck-agents` tmux session is gone (worker panes lost;
  recreate per RESUME.md). All `coord/*` branches, local and remote, are
  intentionally kept (user decision); their stale worktrees stay too.
  GitHub labels on #12/#13 flipped `blocked` → `ready-for-agent` (the #11
  merge opened both).
- 2026-09-01: Next wave dispatched in tmux session `personal` (control plane
  is tmux; Herdr not installed). Window `coordinator` hosts this session;
  `codex` (worktree `~/.worktrees/termdeck/08-pty-transport`, branch
  `coord/08-pty-transport`, Codex 0.152 interactive, gpt-5.6-terra/high,
  workspace-write sandbox) received issue #8; `claude` (worktree
  `~/.worktrees/termdeck/12-ui-master-stack`, branch `coord/12-ui-master-stack`,
  Claude Code 2.1.251, opus/effort high, bypass permissions) received issue
  #12 and must checkpoint after it — #13 waits for the coordinator's
  go-ahead. Task contracts live in `/tmp/shipwright/termdeck/<task>/`;
  prompts and logs retained there until integration. Workers commit on their
  branches but never merge/push/PR — the coordinator pushes branches, opens
  PRs, and merges only after review and user approval.
- 2026-09-01: Workflow fix (shipwright is WIP — tested, changed). Found via
  the pi critic session transcript: pi's TUI composer submits on every newline
  received, so tmux `paste-buffer` of a multi-line prompt becomes one USER
  message per line (~74 fragments for the contract). Codex/Claude tolerate
  multi-line paste; pi does not. Rule: never multi-line-paste into a pi
  interactive composer. The critic is a FULL interactive pi session (like
  codex/claude panes), launched with the complete role contract as its
  initial message argument (`launch-critic.sh` cats the contract into the
  launch command — one message, no composer). Later review assignments will
  be written to a full assignment file and handed to the critic as a short
  single-line read-this-file pointer via `prompt-target`, keeping every
  assignment complete and the transcript clean. Interim headless run removed;
  its log kept (`/tmp/shipwright/termdeck/critic/boot.log`) and the
  diagnostic transcript retained at
  `~/.pi/agent/sessions/--home-andrea-personal-termdeck--/...01a05bf3*.jsonl`.
- 2026-09-01: POLICY (user): workers never get full access; workers may
  commit locally when their sandbox allows, but must STOP before pushing and
  wait for review; ALL critic reviews run on LOCAL changes (never remote/PR);
  the coordinator is the only one who pushes, and only after the critic
  passes. Applied: codex Full-Access relaunch revoked (interrupted, resumed
  `workspace-write`), claude go-ahead for #13 given. Codex then re-verified
  the gate green (fmt, clippy `-D warnings`, 28 tests) and stopped without
  committing (gitdir outside sandbox — expected); deliverable is the local
  working tree on `coord/08-pty-transport` (Cargo.toml/Cargo.lock/
  src/engine/mod.rs + untracked src/engine/pty.rs). Critic received its first
  assignment — complete local review of #8 at
  `/tmp/shipwright/termdeck/critic/review-8.md` (single-line pointer, no
  paste fragmentation). Claude is implementing #13 with instructions to
  commit locally and stop for review.
- 2026-09-01: Status correction + branch split. My prior report was wrong on
  two points (critic was still working on #12, not delivered-yet-facts; claude
  had FINISHED #13, not mid-work). Both critic verdicts are final and pass
  (transcript turns: #8 verdict complete before push; #12 verdict pass with 3
  non-blocking notes). #12/#13 had been committed on ONE shared branch —
  split: `coord/12-ui-master-stack` reset to the exact reviewed commit
  `9606976`, #13 preserved on new branch `coord/13-ui-chrome` (`fe7922c`) with
  its own worktree. Pushed `coord/12-ui-master-stack` (PR #18); #8 stayed PR
  #17. Claude was told to HOLD and not start #14 (unauthorized; #14 waits for
  #12/#13 passes → merges). #13 handed to the critic for local review
  (`/tmp/shipwright/termdeck/critic/review-13.md`).
- 2026-09-01: USER: once the critic passes the pushed PRs, they may be
  merged; and the `#[allow(dead_code)]` on `pub(crate) mod pty` must be
  removed. Correction forwarded to codex (same session,
  `correction-allow-dead-code.md`): remove the attribute, keep the gate green
  with the smallest real resolution, commit locally, no push. After codex
  hands back, the critic re-checks ONLY the delta (re-review mode), then the
  coordinator pushes the amended `coord/08-pty-transport` (PR #17).
- 2026-09-01: FINISH PROTOCOL adopted (part of the coordinator skill
  workflow under test, not Termdeck-specific — patched into the coordinator
  and shipwright skills). Workers/reviewers no longer just go idle: every
  finished turn writes a one-line task-tagged timestamped marker at
  `<checkout>/.scratch/status/<task-slug>.done` (format:
  `/tmp/shipwright/termdeck/FINISH-PROTOCOL.md`) and best-effort pings
  `personal:coordinator`. New `watcher` window polls marker dirs every 4s and
  turns markers into a bell + banner ping plus a timestamped task-tagged row
  in `/tmp/shipwright/termdeck/events.log` (smoke-tested). Clauide adopted
  it while idle; codex/critic adopt from their next turn (in-flight turns not
  interrupted). `.scratch/` already gitignored — no repo change needed.
- 2026-09-01: Critic verdicts: **#12 pass** (3 non-blocking) — already pushed,
  PR #18 mergeable per user authorization. **#13 BLOCK** — one blocking
  finding: the Starting lifecycle state is never rendered/asserted (only 4 of
  5 states covered). Correction dispatched to claude (same session,
  `correction-13.md`): add a focused Starting render+color test. Two
  non-blocking #13 notes: `scroll_marker` chrome is not traceable to the
  accepted supplement (user decides keep-vs-drop — it also edited the
  already-reviewed `backend-promoted.txt`), and footer rule-row duplication
  (cosmetic). #8 delta re-review in progress (critic, re-review mode on the
  allow-removal + public-exposure change). Finish protocol WORKING live:
  watcher logged claude's retroactive 13-ui-chrome marker (11:37:07, tagged
  + timestamped).
- 2026-09-01: FINISH PROTOCOL v3 (user-corrected): the watcher's auto-inject
  into the coordinator pane was REMOVED (it re-fired old markers on restart
  and typed TERMDECK-EVENT lines into the active coordinator session — broke
  the conversation; tmux panes themselves stayed healthy). Watcher is now
  RECORD-ONLY (events.log + banner/bell). PRIMARY ping is now direct: on
  finishing, the worker/critic PROMPTS the coordinator the same way the
  coordinator prompts them — one-line mini prompt
  `<role>: finished <task> <summary>` via `tmux-agent.sh prompt-target
  personal:coordinator.0 <ping.txt> pi` (lands in the coordinator's
  conversation = cannot be missed). Sandboxed workers without tmux degrade to
  marker-only; coordinator reads events.log first thing every turn (standing
  order, now in the coordinator skill Phase 5b). claude, codex, and the
  critic all onboarded. Stray TERMDECK-EVENT lines from the broken v2 watcher
  may appear in the coordinator session historically; they are artifacts.
- 2026-09-01: #8 re-review PASSED (critic, delta = allow-removal + public
  exposure; gate green 28 tests) — delta committed `61ecf96` and pushed;
  PR #17 now contains both commits, mergeable per user authorization once a
  critic pass covers them (both #8 commits reviewed pass). #13 correction
  (claude `678a4dc`, Starting-state test, 57 tests) re-review dispatched to
  the critic; merge of #13 PR follows its pass.
- 2026-09-01: DIRECT PING PROVEN live — the critic's mini-prompt
  (`critic: finished 13-ui-chrome-critic-rereview pass …`) arrived in the
  coordinator conversation and was acted on without polling. #13 re-review
  PASS (Starting asserted in 678a4dc; gate 57 tests, parent re-verified).
  Pushed `coord/13-ui-chrome` → PR #19. All three slices now review-passed:
  #8 (PR #17), #12 (PR #18), #13 (PR #19) — each mergeable on user approval.
- 2026-09-01: FINISH PROTOCOL v4 (ponytail, user-driven): watcher REMOVED
  entirely. The ping chain is now just: (1) worker/reviewer PROMPTS the
  coordinator pane directly with a one-line mini prompt (primary — landed in
  conversation, proven), (2) marker file `.scratch/status/<task>.done` always
  written as the durable record + sandbox fallback (codex cannot reach tmux),
  (3) coordinator's standing order: read markers/events.log first thing every
  turn. Coordinator skill Phase 5b + FINISH-PROTOCOL.md updated accordingly.
- 2026-09-01: FINISH PROTOCOL v4 cleanup: removed the obsolete "no watcher"
  item from coordinator skill Phase 5b and the watcher mention from the
  shipwright skill — no watcher is mentioned anywhere in the workflow now.
  Protocol stands: worker/critic prompts the coordinator directly (primary) +
  marker file (record/sandbox fallback) + coordinator reads markers first
  thing every turn.
- 2026-09-01: ALL MERGED (user authorization: merge when the critic passes —
  all three passed). #17 squash-merged → `27375f2` (#8), #18 → `f732714`
  (#12), #19 → `d81eaff` (#13; branch first rebased onto post-#18 main —
  git dropped the duplicated 9606976 automatically — gate re-verified:
  fmt/clippy clean, 59 tests). Post-merge main: fmt/clippy `-D warnings`
  clean, 59 tests pass. Worker branches kept on the remote (user preference).
  Issues #8/#12/#13 remain open for user-controlled closure; #9/#10 (native
  engine) unblocked now (#8 in main); #14 (modal/input) unblocked (#12/#13 in
  main); next wave: #9/#10 (codex) + #14 (claude).
- 2026-09-01: Issues #1, #8, #12, #13 CLOSED (user request; comments reference
  their merges). Next wave dispatched: #9 (one-terminal native engine) →
  codex in `~/.worktrees/termdeck/09-native-engine` (`coord/09-native-engine`),
  #14 (modal + input modes) → claude in `~/.worktrees/termdeck/14-modal-input`
  (`coord/14-modal-input`); both worktrees off `origin/main` (`30d6eb7`), both
  briefs carry the finish protocol v4 (direct ping + marker + commit-local
  no-push). Old worker windows replaced; critic pane retained for reviews.
- 2026-09-01: #9 critic review PASS (direct ping landed in conversation;
  1 non-blocking resize-scrollback note). Parent gate re-verified (fmt,
  clippy, 60 tests). Delta committed by coordinator `eecd958` (codex sandbox
  cannot commit/push/tmux) → pushed → PR #20. Codex ping channel: sandbox
  blocks tmux (probed from outside: Operation not permitted on the socket;
  `unix-socket:`/`ipc:any`/`network:host` permission keys also fail, keys not
  documented in the installed binary) — marker-only unless the user picks the
  minimal inbox relay. #14 still in flight (claude).
- 2026-09-01: #14 critic review PASS (direct ping landed; gate green 85
  tests). Pushed `coord/14-modal-input` (`c4f2b6c`) → PR #21. Both #9 and #14
  critic-passed and ready to merge (awaiting user approval in this wave).
  Remaining open question: codex ping channel (marker-only vs minimal inbox
  relay).
- 2026-09-01: CODE-X PING CHANNEL FIXED (user: codex must ping in conversation
  like claude/critic; scroll_marker stays as merged). Added a minimal INBOX
  RELAY (window `relay`, /tmp/shipwright/termdeck/inbox/): sandboxed workers
  write ONE line to a new <task>.ping file; the relay delivers it into the
  coordinator conversation and consumes it (single-line, new-files-only,
  guarded to the pi foreground command — unlike the removed watcher, no stale
  re-fires or multiline injection). Self-test verified (DELIVER logged);
  codex onboarded and confirmed. FINISH-PROTOCOL.md + coordinator skill Phase
  5b updated. PRs #20/#21 remain open awaiting user merge approval.
- 2026-09-01: WORKFLOW RELOCATION (user: the workflow is not termdeck-specific
  — the shared machinery must not live under the repo-scoped state dir).
  Moved into the coordinator skill (home: ~/.pi/agent/skills/coordinator →
  /home/andrea/personal/skills/coordinator, committed there):
  `references/finish-protocol.md` (authoritative protocol), `scripts/relay.sh`
  (inbox relay), global inbox `/tmp/shipwright/inbox/`, relay log
  `/tmp/shipwright/relay.log`. Repo-scoped copies deleted; codex re-onboarded
  with the global path (confirmed). Per-task state stays per shipwright
  convention under /tmp/shipwright/termdeck/<task>/.
- 2026-09-01: FINISH PROTOCOL v5 — UNIFIED, harness-agnostic (user request):
  the inbox relay is now the ONLY worker→coordinator ping channel for EVERY
  harness (pi critic, Claude Code, Codex, future) — one line to
  /tmp/shipwright/inbox/<task>.ping + the marker; no per-harness tmux
  prompt-target pings anymore. Proven live: claude's protocol-adoption ping
  arrived via the inbox, not tmux. Critic confirmed adoption. Protocol doc:
  coordinator skill references/finish-protocol.md; relay: scripts/relay.sh
  (committed in the skills repo). The reviewer for #14 reported pass (input
  router and modals verified, c4f2b6c, 85 tests) — PR #21 open; PR #20 open;
  both awaiting user merge approval.
- 2026-09-01: MERGED (user approval): #20 (one-terminal native engine) →
  `4aadb16`, #21 (modal + input modes) → `200e3cb`. Post-merge main:
  fmt/clippy `-D warnings` clean, 86 tests pass. Issues #9 and #14 CLOSED
  (comments reference merges). Remaining tracker: #10 (native lifecycle,
  blocked on #9 — NOW UNBLOCKED, codex lane), #2/#3 epics in progress, #4
  production integration (blocked on #2/#3). Worker branches kept (user
  preference).
- 2026-09-01: #10 (native multi-terminal lifecycle) DISPATCHED to codex —
  worktree ~/.worktrees/termdeck/10-native-lifecycle, branch
  `coord/10-native-lifecycle` off main `e926a5e`; brief includes the FINISH
  PROTOCOL (inbox ping `/tmp/shipwright/inbox/10-native-lifecycle.ping` +
  marker). Coverage: 1–4 configured terminals w/ stable identity, exited
  frame/scrollback preservation + respawn, SIGTERM→2s→SIGKILL shutdown, no
  owned processes/threads after shutdown. Codex window replaced (was #9) and
  working.
- 2026-09-01: Relay moved to DETACHED background process (user: no visible
  window needed as long as logs are inspectable). pid recorded at
  /tmp/shipwright/relay.pid; activity logged at /tmp/shipwright/relay.log;
  script + detached-mode docs in the coordinator skill (scripts/relay.sh,
  pushed). Detached delivery verified. Tmux layout: coordinator, codex,
  claude, critic only.
- 2026-09-01: #10 critic review PASS (ping landed; gate 87 tests). Delta
  committed `3355b12` (codex sandbox cannot commit) → pushed
  `coord/10-native-lifecycle` → PR #22, awaiting user approval. #4 (production
  integration) is the only remaining slice — both epics' slices now merged
  (#1/#7/#8/#9/#10 engine; #11/#12/#13/#14 UI); once #22 merges, #4 unblocks.
- 2026-09-01: #22 merged (`d0e4a0c`, gate 87 tests) — #10 CLOSED. #4
  (production integration, FINAL slice) dispatched to codex: worktree
  ~/.worktrees/termdeck/04-production-integration, branch
  `coord/04-production-integration` off main `d0e4a0c`; brief covers
  composition-root wiring (real session: master + live previews), e2e
  promotion/zoom/scrollback/resize/Ctrl+C/respawn, invalid-config-fails-
  before-partial-startup, terminal restoration on exit/signals/panics,
  documented WSL manual acceptance. Codex window replaced and working.
- 2026-09-01: PENDING (user): after the current codex #4 turn finishes, give
  codex permissions to reach crates.io (fetch new deps) WITHOUT full access —
  relay stays as the ping channel. Probes so far: `sandbox_permissions`
  network keys and `network.allowed_domains`/`network.enabled` config keys all
  failed to open network in the workspace-write sandbox; codex binary strings
  hint at a newer `permissions`/`PermissionProfile` model — re-investigate
  when codex is idle (or fallback: pre-warm new deps into the shared cargo
  cache from the coordinator shell so sandboxed builds resolve offline).
- 2026-09-01: Critic verdict on #4: BLOCK (handback) — composition and
  signal-safe shutdown sound, gate 88 tests, but WSL_ACCEPTANCE.md lacks the
  required idle-CPU (<2%) acceptance step from PLAN.md. Correction routed to
  codex (same session, correction-4.md): add ONLY the idle-CPU step, keep gate
  green, real marker timestamp this time. Final slice; on re-review pass →
  commit → push → PR → then #2/#3 epics close.
- 2026-09-01: #4 re-review PASS (idle-CPU step added, gate 88 tests).
  Committed `cd26bfa` → pushed → PR #23 — the FINAL slice. Pending: user
  merge approval → then close epics #2/#3 and mark the tracker done.
  Also pending: codex crates.io permission change (codex is now idle,
  per user: investigate after its last turn). Flaky observation recorded:
  one engine real-shell test flaked once under load (87/1) — follow-up
  robustness candidate.
- 2026-09-01: TRACKER COMPLETE. #23 merged (`60546a3`, final main gate:
  fmt/clippy clean, 88 tests). Closed #4 (production integration) and epics
  #2/#3 — zero open issues remain on zo-ll/termdeck. The deliverable:
  termdeck CLI/config/contracts (#1/#5), VT adapter (#7/#15), PTY transport
  (#8/#17), native engine (#9/#20), lifecycle (#10/#22), master-stack UI
  (#11/#16, #12/#18, #13/#19, #14/#21), production integration (#4/#23).
  Loose ends (not blocking): codex crates.io permission for future sessions;
  one flaky real-shell engine test (green on reruns); scroll_marker chrome
  kept per user (decision open).
- 2026-09-01: LOOSE ENDS: scroll_marker DECIDED — keep (user choice; no
  longer open). codex crates.io permission: exhausted — 8 config shapes probed
  (sandbox_permissions network keys, network.enabled, network.allowed_domains,
  network.domains, permissions.network.domains) all `Operation not
  permitted`/FAIL in workspace-write; the installed codex 0.152 exposes no
  reachable network grant. Adopted fallback (documented for future sessions):
  pre-warm new deps into the shared cargo cache from the coordinator shell
  (throwaway `cargo add` + `cargo fetch`), so sandboxed codex builds resolve
  offline — same mechanism that worked for #8. Flaky engine test: hardening
  dispatched to codex (branch `coord/flaky-test`, worktree
  ~/.worktrees/termdeck/flaky-test): audit fixed sleeps → deadline-polls,
  comfortable real-shell deadlines, no assertion weakening, gate ×3.
- 2026-09-01: Flaky-test hardening PASS (critic re-review; gate green 88 ×3,
  parent re-verified). Committed `700b38d` → pushed `coord/flaky-test` → PR
  #24 — ALL LOOSE ENDS CLOSED (scroll_marker kept by decision; deps
  pre-warm fallback documented; flake hardened). Tracker DONE + loose ends
  closed; only remaining actions are the user's PR approvals/merges (#23
  awaiting? no — #23 merged; #24 new).
- 2026-09-01: FINAL. #24 merged (`f456508`) — final main: fmt/clippy clean,
  88 tests. Zero open issues, zero loose ends. Coordinated run complete.
- 2026-09-01: Post-delivery finding ("screens not scrollable") investigated
  live (smoke window). VERIFIED: scrolling WORKS on the active/master
  terminal via `Ctrl+g [` then j/k/arrows/PgUp/PgDn/g/G — content pans and
  the `line N/M` position label tracks correctly (122→119). Stacked PREVIEWS
  are read-only (only an "↑ N lines above" tag) — by design. Mouse wheel is
  intentionally NOT forwarded (v1 scope). Live-mode PgUp does nothing outside
  scrollback mode. Clean quit (`Ctrl+g q` → y) closed the app with NO orphaned
  shells. No code bug confirmed; awaiting user's expected behavior to decide
  if a UX/enhancement slice is wanted.
- 2026-09-01: Finding "screens not scrollable" opened as issue #25 and
  DISPATCHED to codex (coord/25-scrollback) — coordinator stopped doing
  hands-on work (user correction) and delegated. Brief hands codex the
  coordinator's probe as evidence-to-verify + asks it to choose the real
  root cause and fix in scope (or route UI-owned fix to Claude). Finish
  protocol applies (inbox 25-scrollback.ping + marker).
- 2026-09-01: USER requirements (sanctioned deviations from v1 "no mouse
  forwarding"): (1) panes scrollable with the MOUSE WHEEL, (2) drag-and-drop
  panes between stack and master to SWAP them. Both open as issues (#25 wheel,
  #26 drag-drop); both need crossterm mouse capture enabled in the session
  loop (codex's lane). #25 already dispatched to codex; #26 queued to codex
  after #25 (same worker; codex will flag any UI-rendering piece for Claude).
  Notes left on #25 (wheel behavior decided: all panes).
- 2026-09-01: #25 investigation done (codex: no bug — keyboard scrollback works
  as designed; the gap is the absent mouse). User DECIDED wheel-scroll on all
  panes → dispatched to codex as a feature (followup-wheel.md): crossterm
  mouse capture (torn down on exit), session dispatches engine Scroll to the
  hit-tested pane, renderer hit-testing, keyboard scrollback unchanged. #26
  (drag-drop) queues after. codex working.
- 2026-09-01: #26 retitled/expanded (user): "Mouse pane actions — drag-drop
  swap + double-click promote". Folds double-click-to-promote into the same
  mouse slice (shared capture + hit-testing with #25); gestures dispatch the
  existing frozen SelectPosition/Promote actions. Queues to codex after #25.
- 2026-09-01: DIRECTIVE (user): DO NOT dispatch any new work to claude — keep
  the critic as the reviewer only. #27 (collapse stack HEIGHT — clarified to
  mean reducing the stack's height, master taller, not hiding previews) is
  PARKED (not dispatched to anyone). #25 (codex wheel, 89 tests green) handed
  to the critic for local review.
- 2026-09-01: #25 critic PASS (wheel + mouse teardown, 89 tests). Committed
  `39935bc` → pushed `coord/25-scrollback` → PR #28, awaiting user approval.
  #26 (mouse gestures: drag-drop + double-click) still queued to codex;
  #27 (collapse stack height) parked (no claude dispatch).
- 2026-09-01: #26 (drag-drop swap + double-click promote) DISPATCHED to codex
  (coord/26-mouse-actions off main 43f2f48). Builds on #25's mouse capture +
  hit-testing; dispatches existing frozen actions; no contracts changes.
  #27 collapse remains parked (no claude dispatch). Codex window replaced.
- 2026-09-01: #26 done (codex; drag-drop swap + double-click promote via frozen
  actions; gate 92 tests). Handed to the critic for local review.
- 2026-09-01: #26 critic PASS (gate 92 tests). Committed `112b1a5` → pushed
  `coord/26-mouse-actions` → PR #29, awaiting user approval. #27 collapse
  parked pending user's Claude design step.
- 2026-09-01: User replaced the design mockups (Windows Desktop zip →
  repo reference) with collapsed-stack states; reference synced into
  docs/design/termdeck/reference/ (commit 438bff8) — the updated visual
  authority. #27 DESIGN phase dispatched to CLAUDE (supersedes the earlier
  "no claude dispatch" hold for this task): study the new reference, produce
  docs/design/termdeck/collapse-stack.md spec (geometry/control/states/
  interplay/edges/what-to-implement), design-only. Claude working in
  coord/27-collapse-stack. #29 (mouse actions) pending merge approval.
- 2026-09-01: QUEUED for delivery to claude (after its #27 design turn settles —
  no mid-turn interruption): the claude_design MCP prompt (import project
  8aa66d51-…, focus Termdeck TUI.dc.html, read support.js, "Implement:
  Termdeck TUI.dc.html"). Staged at
  /tmp/shipwright/termdeck/27-collapse-stack/mcp-prompt.md.
- 2026-09-01: #27 DESIGN SPEC committed by claude `4af49d0`
  (docs/design/termdeck/collapse-stack.md, 568 lines: geometry, `^g c` +
  chevron control, states, interplay, edge cases, coder list, 11 ambiguities
  flagged). QUEUED claude_design MCP prompt then DELIVERED — claude is now
  importing/implementing `Termdeck TUI.dc.html` via the MCP. Design spec
  ready for user review before implementation.
- 2026-09-01: USER authorized #27 IMPLEMENTATION in claude's lane (supersedes
  the earlier "no claude dispatch" directive for this issue; design spec
  accepted). claude's design work is done (spec 4af49d0; MCP artboard work
  complete — screen 05 collapsed-preview is the only unimplemented part).
  Claude selected option 1: implement per collapse-stack.md in
  coord/27-collapse-stack (DeckState.collapsed, stack_layout(), strip
  renderer, ^g c + marker click, status/footer, collapsed-stack.txt fixture;
  screens 01-04 byte-identical; commit local, no push). R3 responsive
  follow-on explicitly NOT included.
- 2026-09-01: #27 implementation committed by claude `eb28a42` (gate 106
  tests, clean tree). NOTED: marker/chevron-click deferred (pending #26 which
  has merged) — critic asked to rule blocking vs non-blocking against the
  accepted spec. In critic review.
- 2026-09-01: #27 critic verdict: BLOCK — geometry+snapshots land; marker/
  chevron-click (spec H2 control) deferred despite #26 merged. Correction
  routed to claude: integrate merged #26 into coord/27-collapse-stack (merge
  origin/main, resolve conflicts), implement the marker-click per spec §2 on
  the #26 mouse infra, gate green, commit local. Claude working.
- 2026-09-01: #27 correction done (claude merged origin/main/#26 + marker click
  `71233cc`, gate 114 tests, screens 01-04 byte-identical) — re-review handed
  to the critic.
- 2026-09-01: #27 re-review PASS (marker click landed; gate 114). Pushed
  coord/27-collapse-stack → PR #30, awaiting user approval. Close of the
  mouse/collapse feature set once merged.
- 2026-09-01: User test findings — 4 issues opened: #31 wheel-scroll no live
  prompt (→ codex, coord/31-scroll-live, dispatched); #32 collapse control
  undiscoverable (→ claude, coord/32-collapse-discoverability, dispatched,
  always-visible markers + help hint); #33 animations (feature/design-first,
  needs user decision); #34 open ALL projects at once (config scope + the v1
  1-4 terminal cap — needs user decision on how many). #31/#32 workers
  working.
- 2026-09-01: #34 DECIDED (user): N previews — the stack becomes a
  SCROLLABLE LIST. Decomposed: #34a lift cap (config+engine, codex),
  #34b scrollable stack list (UI, claude, design-first). Cap located:
  config/mod.rs:161 + native.rs MAX_TERMINALS=4; engine internals Vec-based.
  Queues after #31/#32. #33 (animations) still awaiting user's choice.
- 2026-09-01: #33 (animations) parked by user (library answer given: tweening +
  render tick, no full ratatui animation framework). Focus: #31/#32 in flight;
  #34a/#34b queued.
- 2026-09-01: #31 done (codex; wheel input returns live viewport to tail;
  gate 115) — in critic review. #32 still in flight (claude).
- 2026-09-01: #31 critic PASS → committed → pushed → PR #31 (…) awaiting
  approval. #32 in flight (claude).
- 2026-09-01: #32 done (claude, `48ab5d3`: always-visible markers + help ^g c;
  gate 116) — in critic review.
- 2026-09-02: #41 S1 done (claude, `bb29ff6`: draggable divider col 98 + ^g -/= parity, per-session persistence, ceil-rounding fix found live, gate 151) — in critic review.
- 2026-09-01: #32 critic PASS → pushed → PR #36 R awaiting approval. #34a
  (lift 1-4 cap, config+engine) DISPATCHED to codex (coord/34a-lift-cap).
  #34b (scrollable stack list, UI) next — awaiting user choice (design-first
  vs direct) but queued to claude.
- 2026-09-01: #34b (scrollable stack list) DISPATCHED to claude (DIRECT per
  user — no design mockups): coord/34b-scrollable-stack. Paging gesture to be
  defined carefully vs #25 wheel; hit-testing by list offset; scroll
  indicators; N>4 via synthetic fixtures until #34a merges. Both lanes
  working (#34a codex, #34b claude).
- 2026-09-01 (end of day): USER change — stack previews COLLAPSED BY DEFAULT,
  expand via marker click (inverts #27/#32). Issue #39 opened; dispatched to
  claude (coord/35-collapsed-default): default folded strips, markers show ▸
  from frame one, ^g c = toggle-all/expand-all, deliberate snapshot
  re-blessing per fixture (honesty enforced by the critic).
- 2026-09-01 (end-of-day, saved for tonight): #39 (stack previews collapsed
  by default) remains OPEN, ready-for-agent, fully specced on GitHub. claude
  exhausted its session tokens mid-task (~91% then cut) — its partial work in
  ~/.worktrees/termdeck/35-collapsed-default (M src/ui/state.rs only, no
  commit) is intentionally NOT saved (user decision); do not touch it. Resume
  tonight: claude session resets 20:10 (Europe/Malta) — reroute/redo #39 from
  a clean fresh branch off main (the existing coord/35-collapsed-default
  branch is stale with partial state; prefer a new branch), or ask the user.
  Task brief lives at /tmp/shipwright/termdeck/35-collapsed-default/task.md.
- 2026-09-01: #32 critic PASS → pushed → PR #36 R awaiting approval. #34a
  (lift 1-4 cap, config+engine) DISPATCHED to codex (coord/34a-lift-cap).
  #34b (scrollable stack list, UI) next — awaiting user choice (design-first
  vs direct) but queued to claude.
- 2026-09-01: #34a critic PASS → committed → pushed → PR (#34a), merge
  before #34b. #34b still with claude.
- 2026-09-01: #34a done (codex; lifted cap, 8-PTY lifecycle, gate 119) — in
  critic review.
- 2026-09-01: #34b (scrollable stack list) DISPATCHED to claude (DIRECT per
  user — no design mockups): coord/34b-scrollable-stack. Paging gesture to be
  defined carefully vs #25 wheel; hit-testing by list offset; scroll
  indicators; N>4 via synthetic fixtures until #34a merges. Both lanes
  working (#34a codex, #34b claude).
- 2026-09-01: #34b done (claude, `3a9175a`: scrolled window over preview
  list; ^g pgup/pgdn + wheel-over-chrome paging; gutter + "N more"; hit-test
  follows window; gate 129; screens 01-05 byte-identical, help.txt moved) —
  in critic review; branch base predates #36/#37, integration onto main after
  review.
- 2026-09-01: #34b integration done (claude; merge 307ca80, help.txt resolved
  from merged code, gate 135) — pushed → PR opened. Awaiting user approval to
  close #34.
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
- 2026-09-02: #49 re-review PASS → pushed coord/49-picker → PR #54. 1 new non-blocking: overlapping roots duplicate filter matches (cwd ⊂ HOME) — recorded; offer optional tiny fix. A3 unblocks after merge.
- 2026-09-02: #49 dedupe + rule-row re-review PASS (`e257242`, 186) — pushed, PR #54 ready for user merge approval; A3 next.
- 2026-09-02: #54 merged (`857babe`, 186 tests); #49 closed; binary rebuilt (picker live).
- 2026-09-02: USER: picker nav must be SIMPLE — Enter selects the path, right arrow descends (repos incl.), left arrow up. Correction-49c routed to claude (note §7 + impl + tests).
- 2026-09-02: NAV-AMENDMENT MERGED via PR #55 (coordinator mis-flow corrected: #54 had merged the original picker; nav-simple + key-row commits rebased onto main and shipped as their own PR — critic passed both). Binary rebuilt with the SIMPLE picker: Enter=select, →=descend (into repos too), ←=back, o=launch.
- 2026-09-02: picker shift+down/up range-select + mouse folder selection dispatched to claude (coord/49-range-select).
- 2026-09-02: range-select done (claude `5f82232`: shift+down/up additive incl folders; ESC[1;2A/B decoding + torn-sequence guard; folder click verified already working + 2 pins; 195 tests) — in critic review.
- 2026-09-02: USER — Shift+click = the range-select MOUSE TWIN (from highlighted row down to clicked row). Queued as a small follow-up after the keyboard-only range PR lands (followup-shift-click.md staged).
- 2026-09-02: range-select critic PASS → pushed coord/49-range-select → PR #56. Follow-up queued (Shift+click twin + decoder/torn-guard tests).
- 2026-09-02: #56 merged (`88bec35`, 195 tests); binary rebuilt (keyboard range live). Shift+click twin + decoder tests dispatched to claude (coord/49-shift-click).
- 2026-09-02: shift+click twin done (claude `1bddfb2`: shared select_between, shift-on-release SGR bit 2, 4 decoder tests incl. torn-guard; note 7 pairs it; no fixture moved; 202 tests) — critic review.
- 2026-09-02: Shift+click twin MERGED via #57 (`910c31d`, 203 tests) — binary rebuilt (full picker parity: Enter/arrows/o, shift+down/up + shift+click, mouse select incl folders, / filter, multi-instance). #42 remains: A3 (runtime add) + A4 (integration).
- 2026-09-02: #50 A3 (runtime add: + / ^g a chooser, dynamic spawn + re-layout, same-path instances) DISPATCHED to claude (coord/50-runtime-add).

## Rotated from COORDINATION.md 2026-09-04 (#83: header was stale, tail trimmed)
### Handoff bullets (pre-2026-09-03-EOD)
Rotated history: `.coordinator/journal/` (latest archives: check journal dir).
The last ~15 events — older ones live in the journal above; ask and the coordinator greps it.

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
- 2026-09-03: #67-sheet MERGED (PR #68 -> dbfb253, 237 tests; binary rebuilt — sheet unrestricted + folder targets + nav). Remaining: claude UI-declutter design (running) -> backend-impact scan (codex) after.
- 2026-09-03: DECLUTTER MERGED (PR #69 -> a048d76, 238 tests; binary rebuilt). Remaining: codex backend-impact scan (session.rs Deck::home sign-off + NB cleanup batch) after its ~2:56 PM reset.
- 2026-09-03: declutter backend scan merged (PR #70 -> d63047d, 239; binary rebuilt). ONLY remaining: NB-cleanup batch to codex at ~14:56 reset (fresh session).
- 2026-09-03: #75 merged (PR #76 -> 5a898eb, 243; #75 closed; binary rebuilt). #74 (scroll-stream) still open/parked — possibly resolved by the sizing fix; awaiting user decision to retry or close.
- 2026-09-03: #74 + hotfix merged (7c2e469; main green 257; #74 closed). Alt-screen wheel forwarding live (#74), empty-stack (#76/#78), sizing (#75). Lesson: NEVER merge a red-gated branch (my sequence lapse caused the hotfix).
### Stale NEXT ACTIONS section
## NEXT ACTIONS (resume here if coordinator context resets)
1. codex RESET ~14:56 — dispatch TWO fresh-session tasks (fresh-per-task
   policy): (a) backend-impact scan of the declutter incl. session.rs
   Deck::home/abbreviate removal sign-off (coord/backend-scan); (b) the NB
   cleanup batch (coord/nb-cleanup: A1 DEFAULT_SCROLLBACK + expect(),
   feed-while-scrolled pin, #13 footer helper, declutter DESIGN.md residual
   NBs, #9 resize-scrollback resolve/retire).
2. Then merge those (critic pass each, coordinator pushes/merges).
3. User is TESTING the decluttered binary (238 tests, main a048d76).
4. Worker protocol in effect: fresh session per task; only give work to
   IDLE workers; single-write finish protocol (inbox ping + marker).
- 2026-09-03: REASSIGNED — backend-impact scan of the declutter goes to
  CLAUDE (its own session.rs removal; coord/declutter-scan), not codex.
  codex post-reset keeps only the NB cleanup batch.
  NEXT ACTION stays: dispatch NB cleanup to codex at ~14:56 (fresh session).
- 2026-09-03: declutter backend scan (claude) APPROVED + one fix 35ac12f
  (trailing '·' at 33-35 char names, gated pair; 239 tests) — critic
  review queued.
- 2026-09-03: NB-cleanup dispatched to a FRESH codex session (coord/nb-cleanup
  reset onto main a68abf5; brief incl. the declutter 40+-name clamp NB).
  Reset confirmed by user.
- 2026-09-03 15:07: NB-cleanup done (codex d9bd013, 242 tests; ping logged
  DELIVER) — critic review. NOTIFICATION feature (user): assume a GENERIC
  termdeck user WITHOUT the coordinator skills — design = `termdeck notify`
  (documented) + implicit BEL/OSC-777 decode so any agent works; design-
  first-vs-direct still open.
- 2026-09-03: Notification feature PARKED for research (user unsure) — issue
  opened on GH (generic-user spec: termdeck notify + BEL/OSC-777 decode;
  design questions listed). No dispatch until the user decides.
- 2026-09-03: AGENT-API issue opened (AI-first: termdeck notify/list/status/
  open/promote with --json, generic-user constraint, research/parked like
  #71). No dispatch yet.
- 2026-09-03: RELAY IS NOW NOTIFY-ONLY (research-verified + applied by the
  muse-spark-1.3 researcher, commit 5e7454d): zero keystroke injection —
  inbox .ping files badge (✉ N) + ARRIVE log; the coordinator DRAINS
  inbox/*.ping at turn start (read+rm) and surfaces messages in replies.
  Root cause found: send-keys shared the PTY input queue (mid-draft gluing,
  stray Enter, copy-mode swallow + view yank).
- 2026-09-03: #74 dispatched to codex (coord/73-scroll-stream): cannot
  scroll a streaming/long session — suspected #64 output-follow re-pinning
  (disengage on intentional scroll) + scrollback cap sanity. Researcher's
  relay ARRIVE tests noted (delivered as designed).
- 2026-09-03: USER STOPPED codex — #74 (scroll-stream) interrupted; no
  work done (was ~7s in). Issue #74 remains OPEN, undispatchded; revisit
  when the user decides.
- 2026-09-03: #75 dispatched to codex (coord/75-app-width): PTY size must
  equal the VISIBLE master area (chrome excluded) so full-screen apps aren't
  width-cut; folds in #74's scroll report (likely same sizing root); #74
  itself remains open, parked (user stopped it earlier).
- 2026-09-03: #75 done (codex df00104: size terminals to visible panes; 243
  tests) — critic review queued.
- 2026-09-03: #74 re-scoped (wheel into alt-screen apps, not tail-pinning)
  and dispatched to codex (coord/74-app-scroll) after user retest.
- 2026-09-03: codex BACKEND DOWN (persistent 404 on
  chatgpt.com/backend-api/codex/responses — server/account side; probe
  failed too). #74 (alt-screen wheel) blocked until it recovers (or reassign).
  Claude pane stuck in queued-messages state -> relaunched FRESH for #76
  (hide stack when empty).
- 2026-09-03: Providers struggling (OpenAI codex 404; Anthropic 529). Muse-
  spark (opencode-go) took over: #74 engine-lane muse worker + #76 now on a
  second muse worker (claude lane). Claude window closed (overload).
- 2026-09-03: #76 done (muse 7ab023f: empty stack hides column/divider,
  master full width, runtime-add restores; 247 tests) — critic review.
- 2026-09-03: #74 done (muse 89950ae: wheel into alt-screen apps; 252 tests)
  — critic review; branch predates #76, integration after.
- 2026-09-03: USER: do not dispatch — #81 (split god-files) and #82 (CI)
  NOT dispatched (interrupted). Both remain OPEN, undispatched.
### END OF DAY 2026-09-03 (kept condensed in dashboard)
## END OF DAY 2026-09-03
- main green 257 tests (`6c00bcc` + ledger). Shipped today: #74 alt-screen
  wheel (hotfix for a broken-merge lápse), #76 empty-stack, #75 app-width
  sizing, #77 declutter + scan, #66/#67 (#64/NB cleanup prior), fresh-per-
  task policy, guarded real-message relay (notify-only rejected by user;
  unguarded injection LOADED with copy-mode guard). WORKERS: codex (OpenAI)
  backend 404 — paused; claude (Anthropic) 529 — paused; muse (opencode-go)
  healthy — #81/#82 NOT dispatched (user halt; both OPEN).
- NEXT (user's call): #81 split god-files, #82 CI, #74 leftover retest in
  app, plus parked #33/#71/#72.
- 2026-09-03: #82 CI done (8f30398+20b0d87; gate green, --all-features;
  triggers every push/PR) — critic review; merge on user go (do-not-dispatch
  honored: not yet merged).
- 2026-09-03: PARKED also #81 + #82 (user) — both worktrees/branches removed
  locally (remote branches kept same as others). ONLY #33/#71/#72 parked
  remain. All workers stopped; main green 257.
- 2026-09-03: #81/#82 fully DELETED (worktrees, local AND remote branches) —
  user will work on them tomorrow. All other coord/* remote branches kept.
