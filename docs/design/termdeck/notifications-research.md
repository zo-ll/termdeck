# Terminal notifications (#71) — research brief

Researcher (muse-spark) · 2026-09-04 · study: `/tmp/shipwright/termdeck/71-notifications/study.md`
Related: issue #71 (notifications), #72/#90–#94 (agent API; Phase 1 `ctl notify` wire shipped in PR #91).

## 1. Executive summary (the decision in 6 lines)

1. **Transport BOTH, phased:** explicit `ctl notify` (wire already shipped, stubbed) is the rich path; BEL decode is the zero-onboarding implicit path; OSC-777 decode follows behind a pre-scan seam, not in v1. (§5 Q1)
2. **Key constraint found:** alacritty_terminal 0.26 *drops* unknown OSC (catch-all `_ => unhandled`, debug-logged) — OSC-777 needs a raw-byte pre-scan, not the existing parse path; BEL is free (`Event::Bell` already fires). (§4)
3. **UX both-by-state:** visible preview → border flash in the demotion-highlight idiom; collapsed strip → strip flash; zoom/narrow (stack undrawn) → non-focus toast overlay, never a focus-stealing modal. (§5 Q2)
4. **BEL in v1, stack-only + coalesced** (master bells ignored; ~10 lines to add). Explicit message always outranks BEL attention. (§5 Q3)
5. **Batch box, not a queue:** overlay lists ≤4 newest-first + "+N more"; any dismiss clears the batch; flashes clear on promotion (no clock) or timeout (injected clock). (§5 Q4–Q5)
6. **Determinism is solved already:** `demoted(now: Timestamp)` + `FakeEngine::advance_time` + `TERMDECK_BLESS` fixtures are the exact precedents to copy; render reads state only. (§5 Q7)

## 2. What exists today (grounding, from evidence)

- **Explicit wire shipped, visual half missing.** `Request{verb:"notify",msg}` → `dispatch_notify(&message)`, currently a documented no-op stub ("#71 owns the visual overlay", `src/ctl/mod.rs:232–248`). The CLI already parses `termctl notify MSG` (`src/bin/termctl.rs:106`), and every spawned PTY inherits `TERMDECK_SOCK` + `TERMDECK_PANE` (`src/engine/pty.rs:62–63`). So an agent inside a pane can already call out; the last mile (sender identity → session notify state → pixels) is what's missing. [fact, verified]
- **BEL anchor is free; OSC anchor is not.** `Handler::bell()` sends `Event::Bell` to the existing `EventListener` (`alacritty_terminal-0.26.0/src/term/mod.rs:1437–1439`; our listener is `PtyReplyListener`, `src/engine/vt.rs:22–30`). Extending it to count bells is ~10 lines with no parser changes. But `osc_dispatch` (`vte-0.15.0/src/ansi.rs:1329+`) handles only 0/2, 4, 8, 10/11/12, 22, 50, 52, 104, 110/111/112 with a trailing `_ => unhandled(params)` catch-all — **OSC 777 is silently dropped**, so the study's "same parse path" assumption is wrong for OSC (right for BEL). Supporting 777 means pre-scanning raw PTY bytes before `VtFrameAdapter::feed`, or a second `vte::Processor` with a custom `Perform`. [fact, vendored source]
- **OSC-777 is foot-origin, not a standard.** Foot defines `ESC ] 777 ; notify ; title ; msg ESC \` (verified primary source: `doc/foot-ctlseqs.7.scd` fetched from codeberg). Kitty uses **OSC 99** for desktop notifications (verified primary source: `sw.kovidgoyal.net/kitty/desktop-notifications`, which also cites iTerm2's OSC 9). Implication: real-world agents emit BEL far more often than any OSC notify; 777 covers foot/ghostty-lineage tools only. [fact, fetched docs]
- **UI already owns every idiom the feature needs.** `Pane::Demoted` + `DEMOTION_WINDOW` 1500ms + clock-injected `demoted(now: Timestamp)` (`src/ui/state.rs:9,381–389`) is the flash pattern to copy. Collapsed `▸` strips with tails (`deck.rs:draw_strip`), zoom `hidden_summary` dots (`2● 3○`), narrow chips, and a status-row `notice` slot (`deck.rs:1494`, WARNING on STATUS_BG, set/cleared from `input.rs:210,301–304`) all exist. Overlays today are focus-stealing `Modal::{Help,Quit}` only (`state.rs:37–42`), rendered centered at fixed sizes (`HELP_SIZE (60,26)`, `QUIT_SIZE (52,10)`, `src/ui/mod.rs`) — a background completion must NOT use this path (it captures every key). Snapshot fixtures are byte-compared with a `TERMDECK_BLESS` workflow (`src/ui/tests.rs:113,193`). Contracts are frozen at `EngineEvent::{FrameReady,StatusChanged,MetadataChanged}` (`src/contracts/engine.rs:35+`); the #72 spec already blessed the additive-change pattern (`history_lines` default-body precedent). Session loop polls ctl once per frame and drains engine events (`src/session.rs`, `src/ctl/mod.rs:Listener::poll`). [fact, verified]

## 3. Possibilities

| # | Mechanism | Pros | Cons | Effort | Fit |
|---|-----------|------|------|--------|-----|
| A | **Explicit only** (`ctl notify` → session state → flash/toast) | Wire + env + CLI already shipped; rich text; sender identity via `TERMDECK_PANE`; no parser work | Zero onboarding is false: random agents don't know the command exists unless their harness tells them | M (1–2 slices: state + render + fixtures) | High — Phase-1 stub names #71 as owner |
| B | **BEL decode only** (`Event::Bell` → per-terminal attention) | ~10 lines; rides existing PTY, no transport; what tools already emit (`tput bel`, `echo -e '\a'`, shell beeps); true zero-onboarding | Name-only, no message; noisy emitters (vim, readline beeps) need taming | S (<1 slice) | High for attention, insufficient alone |
| C | **OSC-777 decode only** | Rich title+body from cooperating tools; foot/ghostty-lineage precedent | Non-standard (kitty=99, iTerm2=9); **requires pre-scan** (alacritty drops it); narrowest emitter base of the three | S–M (pre-scan + parse + tests) | Medium — phase it, don't lead with it |
| D | **BOTH, phased (A+B now, C behind a seam)** ✅ | Covers cooperating agents (rich) + arbitrary tools (attention); dedupe in one state object; each shippable alone | Slightly more design (precedence/coalesce rules) | M total, sliceable | Highest — matches issue's lanes (engine+UI+CLI) |
| E | Separate socket/IPC channel for in-pane notify | — | Redundant: PTY-riding needs no transport; `TERMDECK_SOCK` already exists for outside-pane callers | — | Rejected (listed for completeness) |
| F | Heuristic idle/activity detection | — | **Explicitly rejected by user** (constraint) | — | Rejected (listed for completeness) |
| G | OS-level desktop notifications (passthrough or `notify-rust`) | Reaches user when terminal unfocused | Needs daemon/DBus surface; generic-user TUI has no business claiming the notification server; scope explosion | L | Non-goal; note only |

Precedent table (UX shape): tmux `bell-action`/visual-bell marks the *window*, never steals focus [inference — man page not re-fetched this session]; iTerm2 triggers badge-on-BEL; VS Code renders a bell dot on the inactive terminal tab; foot flashes the whole terminal on BEL (foot-ctlseqs: BEL = "flash", a foot extension) [fact, fetched]. The unanimous precedent: **background-event UX is non-modal**. A focus-stealing modal for a completion event has no precedent and would interrupt typing — the strongest argument for the toast (Q2).

## 4. Recommendation (not a decision)

Ship **D in three slices**, reusing existing idioms only:

1. **Slice 1 — explicit + BEL state, stack flash.** Session owns `notifies: Map<TerminalId, Notify{kind, at}>`; `EngineEvent::Notify{terminal, kind}` added as an *additive* variant (copies the `history_lines` default-body pattern — actually simpler: new variant needs no trait change at all, only construction sites + `FakeEngine` trigger helper). `dispatch_notify` stops being a stub and routes `(msg, caller_pane)` into that map. BEL flows through the extended `PtyReplyListener` → same map as `kind: Attention`. Render: `Pane::Notify` styled exactly like `Pane::Demoted` (border + `DEMOTED_BG`), with its own injected-clock window (recommend 4s — longer than demotion's 1.5s so it reads as "needs you", shorter thanactivity meter's 30s so it settles). Collapsed strips flash by swapping the status glyph to WARNING and inverting the strip (strips already sit on `DEMOTED_BG`, so reuse must be a glyph/color change, not the bg).
2. **Slice 2 — toast for hidden stacks.** New *non-modal* centered box (visual size language of `QUIT_SIZE`, not its focus capture): lists ≤4 newest-first `n name · msg|attention · age` + `+N more`; renders only when the notifying pane is not drawn (zoom, narrow, or scrolled out of the stack window — reuse `stack_window()`); dismiss on Esc/click/any-master-action + auto-timeout via injected `Timestamp` (recommend 8s: readable, not sticky). Also mark hidden panes in existing censuses (`3!` in `hidden_summary`, `!` chip in narrow strip) so the toast has a persistent echo after dismissal.
3. **Slice 3 — OSC-777 pre-scan seam.** Raw-byte scan in the PTY read path (`src/engine/pty.rs`/`native.rs` read loop, *before* `feed`) matching `ESC ] 777 ; notify ; title ; msg (BEL|ST)` → `kind: Message{title, body}`, consumed from the stream so alacritty never sees it. Design the seam in Slice 1 (a `fn scan_notify(&[u8]) -> (Vec<Notify>, Vec<u8>)` pure function = unit-testable without PTYs); fill it here. Consider OSC 99 (`kitty`, params differ) only if a concrete agent asks — do not generalize prematurely.

Dedupe/precedence (one object, no second path): per-terminal single slot; explicit `Message` always overwrites `Attention` and re-arms the flash; `Attention` arriving while a fresh `Message` (< coalesce 2s) is displayed is dropped; identical consecutive messages re-arm but don't duplicate. `ctl notify` returns `{delivered:true}` as today (wire unchanged — scope guard).

No separate mockup phase: snapshot fixtures ARE the mockups (byte-compared, `TERMDECK_BLESS`); Claude-Code visual ownership per AGENTS.md applies at implementation.

## 5. Open questions answered (Q1–Q8)

- **Q1 TRANSPORT:** BOTH on the existing PTY + existing socket — no new transport. In-pane implicit rides the PTY (BEL now, 777 later); in-pane explicit and outside-pane callers use `TERMDECK_SOCK` (already inherited). Dedupe per §4. A separate IPC channel (E) is redundant.
- **Q2 UX SHAPE:** both-by-state. Visible open preview → border flash (demotion idiom). Collapsed strip → strip flash (glyph+color, strips are already demoted-bg). Zoom/narrow/scrolled-out → non-modal toast + persistent census echo. Never a focus modal. Reads in every layout because every layout keeps either the pane, its strip, or its census row.
- **Q3 BEL IN V1:** Yes — attention-only (terminal name, no message), stack panes only, master bells ignored, 2s coalesce. Rationale: cheapest true-zero-onboarding signal; noise tamed by stack-only + coalesce rather than by exclusion.
- **Q4 MULTIPLE:** batch list, not a queue. Toast shows ≤4 newest-first + "+N more" (4 ≈ what a 10-row box holds after chrome; exact count is implementation detail). Per-pane flashes need no queue (state, not events). New notify re-arms the batch.
- **Q5 DISMISSAL:** toast: Esc / click / any master action / 8s timeout. Flash: clears on promotion of that pane (you've seen it — deterministic, no clock), or 30s timeout, or any-dismiss-all. Status `notice` slot is for command rejections, not notifies — don't overload it.
- **Q6 DISCOVERABILITY:** (1) BEL = zero onboarding (works today for beeping tools). (2) `AGENTS.md` convention snippet (`termdeck notify "done: <summary>"` — agents already read repo AGENTS.md; ours should document it). (3) Docs page + `termctl notify --help`. (4) Documented opt-in shell-rc one-liner (e.g. long-command hook) — explicit opt-in, not a heuristic, so inside the constraint. Honest caveat: random agents learn the explicit command only via (2)/(3); BEL is the only path that finds them untaught.
- **Q7 DETERMINISM:** copy the solved pattern: notify state stores `Timestamp`, render takes `now` like `demoted(now)`; tests advance a fake clock (`FakeEngine::advance_time` precedent); promotion-clear path needs no clock at all; `TERMDECK_BLESS` fixtures pin the flash/toast pixels. No `Instant::now` in render/deck code — assert it in review.
- **Q8 SCOPE GUARD:** `dispatch_notify` keeps its signature (stub → route, wire untouched); no daemon (all in-process, socket dies with session); no heuristic detection anywhere; contracts change is one additive enum variant + constructor sites (no frozen-trait churn; `history_lines` precedent if a trait method is preferred instead).

## 6. Risks & unknowns

- **Noisy BEL emitters** (vim visual-bell configs, readline, `fzf`?): stack-only + coalesce mitigates; if field reports say otherwise, the escape hatch is a per-pane mute (`^g` binding) — do NOT build the mute in v1. [inference]
- **OSC-777 param dialects**: foot's is `notify;title;msg`, but other emitters may vary; pre-scan must fail-open (pass bytes through untouched on parse failure) or a malformed sequence eats terminal output. Property-test the scanner. [inference]
- **Toast vs modal focus code**: session input routes all keys to the modal when one is open; the toast needs a parallel "render without capture" path plus Esc handling that doesn't fight `Modal` — small but must be designed, not bolted. [inference from `deck.rs:modal` + `state.rs:open`]
- **Nested sessions**: inner session overwrites `TERMDECK_SOCK` (decided, agent-api-spec §4) — a `notify` from a nested pane correctly targets the inner session; BEL bubbles to whichever session owns the outer pane. No conflict, but e2e-test it. [fact + inference]
- **Could not verify**: Ghostty's OSC-777 support (docs fetches 404'd; foot + kitty-99 verified instead); exact tmux `bell-action` semantics (not re-fetched — prior brief verified tmux control-mode shape only); which real-world agents emit OSC-777 vs BEL in practice (field data, unknowable from here); whether 4s/8s/30s timings feel right (user testing at implementation).

## 7. What I could not verify

- Ghostty OSC-777 support status (two doc URLs 404'd; omitted from the recommendation's critical path deliberately).
- tmux bell-monitoring exact semantics from the man page this session (cited as precedent from prior brief + general knowledge, labeled inference).
- Emitter census: no data on how often arbitrary agents emit BEL vs OSC-777 vs nothing — the phased BOTH recommendation is robust to any mix, which is why it doesn't depend on this unknown.
- Timing constants (flash 4s, toast 8s, coalesce 2s): reasoned, not measured.
