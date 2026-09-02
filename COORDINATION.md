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
- 2026-08-31 (evening): Claude committed the fixture cleanup (`1e84402` → rebased `d70e853`) and pushed; codex-vt's pane froze with the rebase prompt unsent in its composer, so the coordinator performed the docs-only rebase directly (worker deliverables untouched): `coord/07-vt-adapter` → `4996827`, `coord/11-ui-master-stack` → `d70e853`, both on top of `f57555f`. Both PR diffs are now code-only (no COORDINATION.md). Final verification: PR #15 — fmt/clippy clean, 23 tests; PR #16 — fmt/clippy clean, 21 tests, release build clean, `termdeck::ui` exports only `Deck`. Both PRs ready for user review; merge order #15 then #16 (both touch Cargo.lock).

- 2026-08-31 (evening): User approved both merges. #15 squash-merged as `4b4fec4` under user approval. #16 then reported a Cargo.lock/Cargo.toml conflict; rebased onto `4b4fec4` keeping both `alacritty_terminal` and `ratatui` deps (lock regenerated by cargo, verified both present), full checks passed (26 combined tests), and #16 squash-merged as `ddb45f4`. Post-merge main: fmt/clippy `-D warnings` clean, 26 tests pass, release build clean. Worktrees for the two merged branches are clean and can be removed; tmux worker panes left intact.
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

## Durable resumption

- This file, `docs/RESUME.md`, and `docs/WORKSTREAMS.md` are the tracked source
  of truth. tmux scrollback and local agent conversations are disposable.
- Before changing machines, commit worker changes and push `main` plus every
  active `coord/*` branch.
- On a fresh machine, follow `docs/RESUME.md`, read the relevant workstream, and
  give it to a new agent. Do not rely on an old conversation for context.
- Private remote: `https://github.com/zo-ll/termdeck`. `main` and the active
  `coord/*` branches were first pushed on 2026-08-31.
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
- 2026-09-01: #32 critic PASS → pushed → PR #36 R awaiting approval. #34a
  (lift 1-4 cap, config+engine) DISPATCHED to codex (coord/34a-lift-cap).
  #34b (scrollable stack list, UI) next — awaiting user choice (design-first
  vs direct) but queued to claude.
- 2026-09-01: #34b (scrollable stack list) DISPATCHED to claude (DIRECT per
  user — no design mockups): coord/34b-scrollable-stack. Paging gesture to be
  defined carefully vs #25 wheel; hit-testing by list offset; scroll
  indicators; N>4 via synthetic fixtures until #34a merges. Both lanes
  working (#34a codex, #34b claude).
- 2026-09-01: #34a done (codex; lifted cap, 8-PTY lifecycle, gate 119) — in
  critic review.
- 2026-09-01: #34a critic PASS → committed → pushed → PR (#34a), merge
  before #34b. #34b still with claude.
- 2026-09-01: #34b done (claude, `3a9175a`: scrolled window over preview
  list; ^g pgup/pgdn + wheel-over-chrome paging; gutter + "N more"; hit-test
  follows window; gate 129; screens 01-05 byte-identical, help.txt moved) —
  in critic review; branch base predates #36/#37, integration onto main after
  review.
- 2026-09-01: #34b review PASS but branch integration onto main CONFLICTED in
  src/ui/testdata/help.txt (both #32 and #34b edits) — conflict resolution
  routed to claude (same worker; left in conflicted state, not hand-fixed).
  FOLLOW-UP noted (claude's own idea): the "^g 1-4" hint labels hardcode 4 —
  should use the deck's count after the cap lift.
- 2026-09-01: #34b integration done (claude; merge 307ca80, help.txt resolved
  from merged code, gate 135) — pushed → PR opened. Awaiting user approval to
  close #34.
- 2026-09-01: #38 merged (`001c64f`, gate 135) — #34 CLOSED (all projects at
  once: cap lift + scrollable stack live). Release binary rebuilt +
  reinstalled. Open tracker: only #33 (animations, parked).
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
- 2026-09-02 (POST-REBOOT RESTORE): environment restored — tmux session
  personal with coordinator/critic/codex/claude windows; relay restarted
  detached (inbox live, self-test delivered); critic booted fresh (ready,
  idle, boot ping landed); codex standby; #39 re-dispatched to claude on a
  FRESH branch coord/39-collapsed-default (stale 35-collapsed-default left
  untouched per user). /tmp was wiped by the reboot — task briefs recreated
  from issue bodies/COORDINATION records.
- 2026-09-02: WORKFLOW (user-driven): reuse LONG-LIVED worker sessions per
  LANE (preserve context) — fresh sessions only on quota exhaustion or
  sandbox-scoped worktrees (codex). Patched into the coordinator skill
  (Phase 4, "reuse, don't relaunch"). Consequence: claude/critic keep one
  session each; codex engine slices serialize in one worktree where possible.
