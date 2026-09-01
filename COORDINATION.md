# Coordination — Termdeck

Status: active — #15/#16 merged; #8 PTY transport next

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
