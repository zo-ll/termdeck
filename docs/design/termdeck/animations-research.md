# Animations (#33) — research brief

Researcher (muse-spark) · 2026-09-04 · study: `/tmp/shipwright/termdeck/33-animations/study.md`
Related: issue #33 (animations, design-first), DESIGN.md authority, #71 notifications (concurrent; shares the transient-state idiom).

## 1. Executive summary (the decision in 6 lines)

1. **Nothing animates today** — demotion highlight IS implemented but as a static binary 1.5s state, not motion; collapse/zoom/modal/sheet are all instant. (§2, §5 Q1)
2. **The DESIGN bans the big category itself:** "quiet color or glyph change rather than continuous animation" for activity; only the demotion 1.5s settle is blessed motion-adjacent. Animate chrome color/attributes only — never geometry, never live output. (§5 Q1)
3. **No timer threads needed:** the 20ms `poll()` input timeout already wakes the loop at 50Hz idle — transients are pure `phase(start, now)` functions feeding the existing `dirty` flag, exactly like `input.expire`. (§2, §5 Q3/Q5/Q7)
4. **Determinism is solved by precedent:** injected `Timestamp` (`fixture::NOW`), keyframe fixtures via `TERMDECK_BLESS`, plus a `NO_COLOR`-respecting kill-switch. (§5 Q3–Q4)
5. **Rank:** demotion settle-pulse first (design-mandated, S cost); close-fade optional; geometry slides / hover / modal entrances rejected (jerk, cost, no value). (§5 Q1)
6. **Design-first deliverable:** a per-animatable mockup spec (frames, timestamps, exact colors, trigger/cancel rules) for user approval before any slice; fixtures then serve as implementation mockups. (§5 Q6)

## 2. What exists today (grounding, from evidence)

- **The loop is already a frame clock.** `KeyReader::read(POLL_INTERVAL)` is `libc::poll` on stdin with a 20ms timeout (`src/session/input.rs:148–168`, `POLL_INTERVAL`, `src/session.rs:49`) — the session wakes ~50 times/second even with zero input. Rendering is gated by a `dirty` flag (`src/session.rs:427–441, 835–858`): engine events, ctl polls, and `input.expire(...)` each OR into it. `now()` wraps `SystemTime` into the injectable `Timestamp` contract (`src/session.rs:870+`). Consequence: any animation is a state machine advanced once per loop pass with expiry reported as `dirty` — the scope guard's "no timer threads" is satisfied structurally, not by discipline. [fact, verified]
- **Time-boxed binary states exist; motion does not.** `DEMOTION_WINDOW` 1500ms + `demoted(now)` (`src/ui/state.rs:11,381–389`) renders `Pane::Demoted` (border + `DEMOTED_BG`) as a pure function of `(promotion_time, now)` — this answers the study's parenthetical: the demotion highlight IS implemented, as a static highlight, not an animation. Same pattern: `NUMBER_TIMEOUT` 600ms (`src/ui/input.rs:21,172–183`), `DOUBLE_CLICK_WINDOW` 500ms (`src/session.rs:52,126`). Collapse, zoom, modal open, sheet open, divider drag/rest are all instant single-frame switches — grep for easing/lerp/fade/interpolate over `src/` returns nothing. [fact, verified]
- **Snapshots pin time already.** Fixture rendering constructs `Deck { ..., now: fixture::NOW }` (`src/ui/tests.rs:21–33`) and byte-compares buffers against `testdata/`, with `TERMDECK_BLESS` to re-bless (`tests.rs:186–193`). Any transient rendered purely from `(state, now)` is therefore snapshot-testable at arbitrary phases with zero flakiness — the mechanism Q3 needs already exists. [fact, verified]
- **The DESIGN authority constrains this study.** Demotion: "The demoted pane's own 1.5s highlight reports the swap" (`docs/design/termdeck/DESIGN.md:93–94`). Activity: "Activity should use a quiet color or glyph change rather than continuous animation. A live state is carried by the glyph alone" (`DESIGN.md:71`). So the visual contract both mandates the one blessed transient and forbids continuous activity animation — implementation must stay inside color/glyph steps. [fact, quoted]
- **No accessibility or hover hooks exist.** No `NO_COLOR`/`reduced-motion` handling in `src/` (only "reduced" hit is the picker's "reduced to a sheet" prose); mouse is press/drag/release with no `Moved`/hover tracking (`src/session.rs`, `src/ui/input.rs`); ratatui `Terminal::draw` diffs buffers so per-frame cost scales with changed cells (ratatui core behavior — small chrome regions are cheap to pulse). [fact, verified]

## 3. Possibilities

| # | Mechanism | Pros | Cons | Effort | Fit |
|---|-----------|------|------|--------|-----|
| A | **Color/attribute stepped transients on chrome only** ✅ (2–4 discrete steps, 100–500ms apart; e.g. demotion WARNING→demoted→settle) | Inside the DESIGN's allowance; cheap (diff touches borders/titles); snapshot-pinnable; 50Hz poll drives it free | Not "smooth" — but smoothness is dishonest here anyway (Q2) | S per transient | Highest |
| B | Geometry interpolation (collapse heights / zoom widths slide over N frames) | Looks modern in mockups | Layout recompute per frame; strip/preview heights quantize to whole rows (≤2 distinguishable steps — motion unreadable); snapshot explosion; high jerk/tear risk on slow links | M–L | Reject |
| C | Spinner-style frame sequences for busy states (braille/dot cycling on the `Starting ○` glyph) | Honest TUI idiom (k9s/lazygit precedent, inference); S cost; one glyph of diff | Only fits genuine "working" states; termdeck has exactly one (`Starting`) | S | Accept as optional |
| D | Textual-style easing/opacity curves | The one real animated-TUI precedent (Textual `animate()` + easing functions — verified docs page) | ANSI has no alpha; the only lever is stepped color ≈ A with fancier math; needs an async frame driver we deliberately don't have | M | Reject (A captures 95%) |
| E | Do nothing (today's static time-boxed states) | Zero risk, zero fixtures | Leaves the design-blessed 1.5s settle as a flat highlight; user asked for motion | — | Viable fallback |
| F | GPU/kitty-graphics-protocol animation | True smoothness | Non-portable (breaks generic-terminal + config-only posture); scope explosion | L | Reject |

**Q2 — TUI animation, honestly.** The observable consensus (tmux, lazygit, helix, newsboat: instant discrete switches, no easing [inference from observable behavior, not source-dived]) is that credible terminal motion = small discrete steps, because every frame is bytes over a pty: local emulators cap around 60fps, SSH/mosh links far less, and a missed frame must *skip*, never *stretch*, or timing drifts. Frame-rate reality for termdeck: the loop can wake at 50Hz but should only redraw when a transient's discrete phase actually changes (`dirty |= phase_changed`, the `expire` pattern) — typical cadence 2–10fps of tiny diffs, which is exactly the demotion-settle shape. Textual is the honest exception that proves the rule: it ships easing, but against its own async driver and modern-terminal assumptions we don't hold. Pulse/frame hacks that survive: border-color steps, glyph swaps (●→○→·), meter-style fills (`[#···]` — the activity `meter()` in `chrome.rs` is already this idiom, static). Slide/fade geometry does not survive row quantization.

## 4. Recommendation (not a decision)

Adopt **A (+C optional), E as fallback**, in this order — mockup approval first per the design-first constraint:

1. **Slice 1 — demotion settle pulse.** Pure `fn demote_phase(start: Timestamp, now: Timestamp) -> DemoteStep` (3 steps over 1500ms, e.g. 0ms accent-bright → 500ms demoted → 1500ms settled; exact colors in mockup spec). Replaces the flat highlight; same call sites. Keyframe fixtures at t=0/600/1600ms.
2. **Slice 2 — close-fade (optional) + starting-pulse (C, optional).** Exiting pane dims over ~300ms in 2 steps; `Starting ○` cycles 2 glyphs at 250ms while starting. Both color/glyph-only.
3. **Slice 3 — kill-switch + docs.** `TERMDECK_ANIM=off` (env-flag convention per no-color.org, verified) + auto-static under `NO_COLOR`/`TERM=dumb`; document in help/docs. Tests pin timestamps directly and are unaffected by the switch.
4. **Explicitly not built:** geometry slides, modal/sheet entrances (focus surfaces must appear instantly), hover (no tracking exists; keyboard-first tool), anything touching terminal cells or the activity meter's live semantics.

Coordinate with #71: the notification toast/flash (concurrent research) should reuse this exact transient idiom (`phase()` + `dirty` + keyframe fixtures) — one pattern, two features. Recommend the coordinator sequence #33's pattern decision before #71's visual slice.

## 5. Open questions answered (Q1–Q7)

- **Q1 SCOPE (ranked by value/cost/jerk):** (1) demotion settle-pulse — high value (design-mandated), S, low jerk (chrome color only); (2) close-fade — medium value, S, low jerk; (3) starting-pulse — low-medium value, S, no jerk (one glyph); (4) promote/zoom/collapse transitions — the demoted highlight IS the promote transition per DESIGN; geometry slides rejected (rows quantize motion to ≤2 visible steps; layout churn per frame); (5) modal entrance — rejected (latency perception: focus surfaces must be instant); (6) hover — rejected (no tracking, keyboard-first, constant-wakeup cost). MUST NOT animate: live shell output (design ban + engine frames are truth), typing echo, wheel scroll (must stay 1:1), cursor.
- **Q2 TUI HONESTLY:** discrete 2–4-step color/glyph transitions at 100–500ms cadence; wall-clock phases (dropped frames skip steps, durations never stretch); diff cost ∝ changed chrome cells. See §3.
- **Q3 DETERMINISM:** (a) render reads `phase(start, now)` only — no `Instant::now` in deck code (review-gate it); (b) keyframe fixtures at pinned timestamps via `fixture::NOW` + `TERMDECK_BLESS`; (c) expiry reports `dirty` through an `expire`-style hook (pattern exists at `session.rs:441`). No flaky fixtures possible: time is an input, not a reading.
- **Q4 DEFAULT & ACCESSIBILITY:** ON by default (the blessed 1.5s highlight ships regardless; motion is subtle and bounded), with static fallback under `NO_COLOR` (verified convention: no-color.org), `TERM=dumb`, or `TERMDECK_ANIM=off`. No terminal equivalent of prefers-reduced-motion exists to honor instead [inference — no such convention found]. Zoomed/narrow: same rule (instant geometry + flash); collapse-while-zoomed animates nothing (stack undrawn — nothing to phase).
- **Q5 PERF BUDGET:** zero added wakeups (poll timeout is the clock); redraws only on phase change; transients confined to borders/titles (never terminal cells); cap: one demotion + one toast + one fade concurrent in practice; input ordering untouched (poll-first, animation never blocks keys). Wheel/typing paths gain no work.
- **Q6 MOCKUP SPEC (proposed contract for the user):** per animatable — trigger event, cancel/interrupt rule (e.g. re-promotion restarts demotion; any key ends toast), start frame, end frame, each keyframe (timestamp + exact palette color/glyph + affected cell region), total duration, step count/cadence, and the static fallback frame. Durations to approve: settle 1500ms/3 steps, fade 300ms/2 steps, pulse 250ms cycle. Fixtures then serve as the implementation mockups (bless = approve).
- **Q7 SCOPE GUARD:** transients live in UI state with injected time (no threads — the poll IS the clock); engine frames and live output are never touched (design ban quoted in §2); no daemon; no new dependencies.

## 6. Risks & unknowns

- **Slow-link flicker:** stepped border colors can tear on laggy emulators; mitigation is fewer steps + longer cadence (the 500ms settle steps are conservative). [inference]
- **Fixture growth:** each transient × keyframes = committed snapshots; keep steps ≤3 to bound it. [reasoning]
- **Wall-clock vs frame-count phases:** must use elapsed-millis phases (like `demoted()`), never "frame N of M" — frame counts stretch under load. Stating explicitly because it's the easiest implementation mistake. [reasoning]
- **#71 interplay:** if notifications land first with a different transient pattern, #33 inherits inconsistency — sequence the pattern decision before either visual slice (see §4). [inference]
- **Taste risk:** durations/steps are reasoned, not measured; the mockup approval (Q6) is the de-risking step, not more research.

## 7. What I could not verify

- Real-world frame rates over users' links (SSH/mosh variance) — the discrete-step recommendation is robust to any rate, which is why it doesn't depend on this unknown.
- Textual animation internals beyond the (verified present) docs guide; ratatui draw-diff internals for 0.29 specifically (assumed stable core behavior).
- tmux/lazygit/zellij/helix animation behavior from source (survey is from observable behavior + general knowledge, labeled inference throughout).
- Whether 1500/300/250ms timings feel right — needs the mockup approval loop, not more research.
- Ghostty/shader-layer animation precedent is a different layer (GPU emulator) and out of scope for a portable TUI.
