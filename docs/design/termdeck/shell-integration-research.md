# Shell integration: auto-notify on command completion (#107) — research brief

Researcher (muse-spark) · 2026-09-04 · study: `/tmp/shipwright/termdeck/107-shell-integration/study.md`
Related: notifications-research.md §4–5 (v1), issues #71/#97 (v1 notify surface, shipped).

## 1. Executive summary (the decision in 5 lines)

1. **WHEN = non-zero exit OR long-running (default long ≥ 10 s), tunable via `TERMDECK_NOTIFY`/`TERMDECK_NOTIFY_LONG_SECS`.** Every-exit spams; error-only misses the canonical slow-success case. (Q1)
2. **MECHANISM = private OSC emitted by the hook, decoded by the deferred pre-scan seam from #71 research.** BEL cannot carry code/command; termctl-from-the-hook forks per prompt. The OSC path is also *spoof-proof by construction* (bytes arrive down the pane's own PTY). (Q2, Q6)
3. **INSTALL = VS Code-style per-shell injection (generated rc / `ZDOTDIR` / fish data-dir), guarded to interactive shells only, idempotent via marker var.** No user edits, no daemon, nothing when the pane isn't a supported interactive shell. (Q3)
4. **RICHNESS = yes: command (truncated, sanitized) + exit code/duration in a `Message`.** The whole v1 render path (`notify_text`, toast, right-slot) already takes `Message`; cost is a `clip()` width that already exists. (Q4)
5. **NOISE = keep v1 windows, plus one argued refinement: identical body inside the toast window re-arms nothing.** Master-drop already answers the "own failing command" case; typing-while-flashing needs no new suppression. (Q5)

## 2. What exists today (grounding, from evidence)

- **The engine→session→UI path for a rich message is already end to end.** `session.rs:452–453` forwards *any* `EngineEvent::Notify { terminal, kind }` into `notifies.record(...)` with no kind-specific logic; `ctl notify` already delivers `NotifyKind::Message` through it (`src/ctl/mod.rs:280`); `notify_text` (`src/ui/chrome.rs:246–253`), the toast (`src/ui/deck.rs:1086–1168`), the strip flash (`deck.rs:811–868`), and the right-slot `clip(..., NOTIFY_SLOT)` (`deck.rs:1440–1448`) all render `Message` today. So this slice is **shell-side + one decoder site**, with zero session/UI plumbing changes required. [fact, verified]
- **BEL decode is the working zero-onboarding precedent.** `PtyReplyListener` counts BEL (`src/engine/vt.rs:33–43`), `take_bells()` takes (not reads) the count (`vt.rs:92–94`), `native.rs:118–126` emits one `NotifyKind::Attention` per drain, and a scripted-PTY test proves a real child bell arrives as a notification (`native.rs:793–802`). BEL carries no payload — that is the entire gap this study fills. [fact, verified]
- **The OSC pre-scan seam is designed but unbuilt.** v1 research §4 verified alacritty 0.26 *drops* unknown OSC, so a private sequence needs a raw-byte scan *before* `adapter.feed()`, in `NativeTerminal::handle_pty_event` (`native.rs:106–145`), consuming the sequence so alacritty never sees it. The recommended seam signature stands: pure `fn scan_notify(&[u8]) -> (Vec<Notify>, Vec<u8>)`. [fact, v1 brief + re-verified call sites]
- **Env channel exists; rc generation does not.** Every pane inherits `TERMDECK_SOCK` + `TERMDECK_PANE` (`src/engine/pty.rs:62–63`), but nothing today generates shell rcs or wraps `project.command` (config `command` is an arbitrary argv, `src/config/mod.rs:92,225–236`). "Env-injected rc" is therefore *new machinery to build*, not an existing loop to reuse — the study's phrasing is optimistic and §5 corrects it. [fact, verified]
- **V1 constants (unchanged unless argued):** flash 4 s (`NOTIFY_WINDOW`), toast 8 s (`TOAST_WINDOW`), coalesce 2 s (`COALESCE_WINDOW`), master-drop + precedence rules (`src/ui/state.rs:60–110`); exit glyphs already distinguish pane-death (`status_label` "exit N", `chrome.rs:270+`). [fact, verified]

## 3. Possibilities

### Q1 — WHEN

| # | Rule | Pros | Cons | Fit |
|---|------|------|------|-----|
| a | Every exit | Simple; never misses | Spam (`ls`, `cd`, prompt-command noise); violates no-heuristic spirit by crying wolf | Rejected |
| b | Non-zero only | Catches failures; near-zero noise for successes | Misses the canonical case: `cargo build` succeeding after 6 min while user reads another pane | Partial |
| c | Long-running only (≥ threshold) | Catches slow successes; threshold is one number | Misses fast failures in background panes (typo'd command, failing test loop) | Partial |
| d | **Non-zero OR long-running** ✅ | Covers both canonical cases; each half is explicit-or-decoded, no heuristic | Two knobs instead of one | Highest |
| e | Env knob `TERMDECK_NOTIFY=none\|error\|long\|all` (default = d) | Escape hatch for noisy workflows; generic users never touch it | Document + parse; trivial | Include with (d) |

Precedents [mixed — see §7]: tmux has **no** exit-code/duration notify at all — `bell-action any|none|current|other` + `monitor-bell` react only to BEL [fact, verified `man tmux` locally]; kitty shell integration marks prompts (OSC 133) and notifies only via *explicit* OSC 99 desktop notifications [fact, fetched kitty docs]; foot's OSC 777 is likewise explicit-only [fact, fetched foot-ctlseqs]; the long-threshold convention's best-known instance is the `undistract-me` bash plugin (default 10 s) [prior-knowledge precedent, not re-fetched — §7]. Nobody in the surveyed set auto-notifies on *every* exit: (a) has no precedent and good reason.

### Q2 — MECHANISM (what the hook emits)

| # | Emit | Pros | Cons | Fit |
|---|------|------|------|-----|
| A | Plain BEL | ~0 engine work; reuses `take_bells` path verbatim | No code/command → toast says "attention", useless for triage; shares the channel with readline/vim/fzf beeps | Fallback only |
| B | **Private termdeck OSC** ✅ | Rich (code + command + duration); single write, no fork; PTY-riding = zero transport | Needs the pre-scan seam + split-read carry buffer; sequence design must avoid collisions | Recommended |
| C | `termctl notify` from the hook | Reuses ctl auth + rich path; no parser work | Fork+exec+socket dial on *every prompt* (latency on the hot path); needs `termctl` on pane `PATH`; strictly heavier than one `printf` | Rejected as primary |

Sequence shape: do **not** reuse foot's `777;notify;title;msg` — a termdeck pane nested in real foot would double-notify at the outer emulator. Use a private number with an exact-match prefix, e.g. `ESC ] 7777 ; termdeck ; finished ; code=N ; secs=S ; cmd=<sanitized> (BEL|ST)`. The pre-scan consumes only bytes matching the exact `7777;termdeck` prefix and passes everything else through untouched (fail-open). Cap payload (~512 B) and strip controls at *both* ends (hook sanitizes; parser enforces).

### Q3 — SHELL COVERAGE + INSTALL

Shell hooks [prior-knowledge mechanics, standard and stable — §7]: bash = `PROMPT_COMMAND` (post) + `DEBUG`-trap or `$PROMPT_COMMAND`-adjacent pre-hook for start-time + `$?`; zsh = `preexec`/`precmd` (+ `add-zsh-hook`); fish = `--on-event fish_preexec` / `fish_postexec` with `$status`. Timing via `$SECONDS`/`EPOCHSECONDS` (bash 5+) — builtins only, never fork `date`.

Install vectors:

| # | Vector | Pros | Cons | Fit |
|---|--------|------|------|-----|
| i | **Generated-rc injection à la VS Code** ✅ | Zero onboarding; per-shell correct: bash `--rcfile <gen>`, zsh `ZDOTDIR=<gen-dir>`, fish data-dir shim; generated file sources the user's own rc first, then appends the hook | New machinery (tempdir + per-shell templates); must not fight user `ZDOTDIR`/`--rcfile` (only applies when termdeck chose the command — i.e. default shell panes, never overrides an explicit custom `command`) | Recommended |
| ii | `BASH_ENV`/`ENV`/`ZDOTDIR` env-only | No argv changes | `BASH_ENV` affects non-interactive shells too (must self-guard); `ZDOTDIR` hijacks the user's config location; fish needs a different trick anyway — env-only cannot cover all three cleanly | Partial (use as fallback inside (i), not the mechanism) |
| iii | Ship a file users source | Trivial; explicit opt-in | Violates the user-approved zero-onboarding direction | Rejected as primary; keep as documented fallback for exotic shells |
| iv | `/etc/profile.d`, system-wide edits | — | Touches the user's system outside termdeck; never acceptable | Rejected |

Guards (all in the snippet, all cheap): skip when non-interactive (`case $- in *i*)` / fish `status is-interactive`); skip when already installed (`TERMDECK_SHELL_HOOK` marker → also handles nested sessions, matching the nested-`TERMDECK_SOCK`-override precedent, agent-api-spec §4); skip when `TERMDECK_SOCK`/`TERMDECK_PANE` absent (hook degrades to silent no-op outside termdeck panes — the same snippet sourced elsewhere is harmless).

### Q4–Q5, Q8 — see §6 (answered inline with the recommendation).

## 4. Recommendation (not a decision)

Ship in **two slices**, reusing the v1 surface untouched:

1. **Slice 1 — hook + decoder + default rule.** Generated-rc injection for bash+zsh (fish if the template stays under ~30 lines, else slice 2); hook emits the private OSC iff `code != 0 OR secs >= TERMDECK_NOTIFY_LONG_SECS` (default 10) unless `TERMDECK_NOTIFY` narrows it (`none|error|long|all`, default unset = error+long); pre-scan + carry buffer in `native.rs` `handle_pty_event` *before* `feed`; `FakeEngine` trigger helper for `Message` kind; hook skeletons unit-tested by sourcing in `bash --norc`-style harnesses with faked vars.
2. **Slice 2 — polish + fixtures.** Snapshot fixtures for toast/flash with rich `cmd → exit N` rows (copy the `TERMDECK_BLESS` keyframe pattern: `notify-flash-t*.txt`); identical-message refinement (§6 Q5); docs (`termctl notify --help` cross-ref + AGENTS.md convention snippet stays the *explicit* path; the hook is the *automatic* one).

No session.rs changes (the `Notify` arm is kind-generic), no contracts changes (`Message` exists), no new transport, no daemon.

## 5. Risks & unknowns

- **Hook latency on the hot path.** The post-hook runs before every prompt render; keep it to builtins + one `printf`. A `termctl` fallback would fork — do not add one. Measure prompt latency in review (`time` 1000 prompt renders). [inference]
- **Split-read reassembly.** PTY chunks split OSC mid-sequence; the carry buffer (cap: hold at most N trailing bytes containing `ESC`, flush on overflow = fail-open) is the subtlest code in the slice. Property-test: random splits of valid + hostile streams never lose or corrupt passthrough bytes. [inference]
- **Noisy neighbors in shared channels.** Long `ssh`/`tail -f` sessions emit no hook (only prompt boundaries emit) — strictly quieter than BEL monitoring. The remaining noise is *correct* signal (a failing loop fails repeatedly) tamed by the Q5 refinement. [inference]
- **Shell-version skew.** `$EPOCHSECONDS` needs bash 5+; fall back to `$SECONDS` (resets per shell, monotonic — sufficient for durations). Old fish without event flags: degrade to uninstalled (guard, don't polyfill). [inference]
- **Could not verify (§7):** exact long-notify precedent semantics in WezTerm/VS Code; fish `XDG_DATA_DIRS`-style injection details for our shim; which shells panes actually run in the field (affects bash/zsh/fish priority — default `command` values in shipped configs would settle it).

## 6. Open questions answered (Q1–Q8)

- **Q1 WHEN: (d) non-zero OR long-running, with (e) knob, default = (d), long ≥ 10 s.** (a) has no precedent and spams; (b)/(c) each miss a canonical case; 10 s default follows the best-known long-command convention. `TERMDECK_NOTIFY=none|error|long|all` + `TERMDECK_NOTIFY_LONG_SECS` (default 10).
- **Q2 MECHANISM: (B) private OSC; reuse the deferred pre-scan seam.** (A) BEL stays as the untaught-tool fallback but cannot triage; (C) termctl-per-prompt is fork-heavy on the hot path. Suggested form `ESC ] 7777 ; termdeck ; finished ; code=N ; secs=S ; cmd=… (BEL|ST)`; exact-prefix match, fail-open, dual-side sanitization. The seam: pure `scan_notify` + per-terminal carry buffer in `native.rs` before `feed`; `FakeEngine` helper mirrors the existing `ring` precedent (`fake.rs:93–102`).
- **Q3 SHELLS + INSTALL: bash + zsh + fish via (i) VS Code-style generated-rc injection; guards for non-interactive / already-installed / missing env.** bash `PROMPT_COMMAND`+`DEBUG`/`$?`, zsh `preexec`/`precmd`, fish `fish_preexec`/`fish_postexec`; builtins only (`$SECONDS`/`EPOCHSECONDS`, `printf`); sources user rc first; applies only to default shell panes, never wraps an explicit custom `command`; `TERMDECK_SHELL_HOOK=1` marker for idempotence/nesting.
- **Q4 RICHNESS: yes — command (truncated ~48 cols, sanitized) + exit code + duration as `Message{title, body}`**, e.g. title `cargo build`, body `exit 1 · 84s` (success-long: body `done · 6m12s`). Cost is bounded: payload cap ~512 B, existing `clip(..., NOTIFY_SLOT)` + `notify_text` title·body join do the rendering with no layout changes.
- **Q5 NOISE: keep 2 s coalesce / 4 s flash / 8 s toast; one refinement — identical title+body inside the toast window re-arms nothing** (stops a failing loop from pinning the flash forever; any *different* message re-arms normally). Master-drop already handles "own command failed while active" (`state.rs:76–86` — the active pane's record is dropped, and that is correct: you're looking at it). Typing-while-flashing needs no suppression (non-modal by design, v1 §5 Q2). No per-pane mute in this slice (v1 deferred it; revisit on field reports).
- **Q6 SECURITY: (1) no `eval`, ever** — static snippet, single-quoted heredoc, no interpolation of user content; (2) builtins only — no bare-name binaries (PATH attack needs no hook binary to exist); (3) dual-side sanitization — hook strips `ESC/BEL/controls` from `cmd`, parser caps length + strips controls + fails open, so a hostile binary name or crafted command line cannot smuggle sequences; (4) spoofed `TERMDECK_*` buys nothing — pane identity for OSC/BEL comes from *which PTY the bytes arrived on*, not from env (strictly stronger than the ctl `/proc`-environ path, `ctl/mod.rs:caller_pane`); (5) `TERMDECK_NOTIFY=*` values are allow-list matched, never executed.
- **Q7 TESTS: (1) scripted-PTY tests** — spawn bash/zsh with the generated rc, run `false` / `sleep`-with-low-threshold, assert `EngineEvent::Notify{Message}` (copy the `native.rs:793` bell-test shape; skip shells whose binary is absent); **(2) pure-function units + property tests** for `scan_notify` incl. random-split reassembly and hostile passthrough; **(3) hook-logic tests** by sourcing the snippet with faked `$?`/`$SECONDS`; **(4) fixtures** for rich toast/flash rows via `TERMDECK_BLESS`. Keep the gate green at HEAD (study names 312; COORDINATION names 257 — implementer re-checks the count, rule is "no red gate", not a number).
- **Q8 SCOPE GUARD: honored as specified.** No heuristic (both halves are explicit-or-decoded); no daemon (in-process only, socket dies with the session); command-exit vs pane-exit stays distinct (shell death already surfaces via `TerminalStatus::Exited` + `status_label` "exit N"); master-drop + precedence preserved except the Q5 identical-message refinement, argued above.

## 7. What I could not verify

- **WezTerm long-running auto-notify:** `notification_handling` and `audible_bell`/`visual_bell` options confirmed to exist (config reference index fetched); whether any threshold-based command-completion notification exists — not verified. Not on the critical path.
- **VS Code auto-notify semantics:** env-var injection mechanism verified (docs fetched); whether stock VS Code notifies on `exit != 0` or long commands without an extension — not verified. Not on the critical path.
- **zellij command-completion notification:** no usable material fetched (docs index only). Cited nowhere in the recommendation.
- **iTerm2 Triggers vs shell-integration notify:** integration page confirms FinalTerm markers + triggers exist; exact "notify on command exit" trigger recipe not extracted. Not on the critical path.
- **`undistract-me` 10 s default:** prior-knowledge precedent, not re-fetched this session. The 10 s default is a recommendation, not a load-bearing fact.
- **Field shell census:** which shells termdeck panes actually run (bash vs zsh vs fish share) — unknowable from here; Slice 1 order (bash+zsh first) assumes the common case and should be checked against shipped default configs at implementation.
- **Timing feel:** 10 s threshold and the Q5 refinement are reasoned, not measured — user testing at implementation, as with v1's 4 s/8 s.
