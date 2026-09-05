# The per-pane scrollbar — decisions (issue #115)

Status: implemented on `coord/115-scrollbar`. UI only; no engine, contract,
configuration or CLI change. Runtime state, like the split ratio (#41) and the
pin (#113): the session holds it and writes nothing back.

The design export has no scrollbar on a pane. It has one on the *list* — the
gutter track the scrollable stack added (`scrollable-stack.md` §3) — and this
document borrows that idiom and states where it had to differ, in the same form
as `pin-terminal.md` and `collapse-stack.md`.

## 0. What this is derived from

- **`scrollable-stack.md` §3**, the accepted scroll-indicator vocabulary: a
  one-column track of `│` with a `┃` thumb, its length and position the
  window's share of the list, anchored to whichever end the window has reached,
  and **drawn only on overflow** so a pane that fits renders exactly as the
  export draws it.
- **`ScrollbackPosition { lines_above, lines_below }`** in `TerminalMetadata`,
  which the status row already turns into `line 2217/2431`
  (`Deck::scroll_position`). Nothing new is asked of the engine: the same three
  numbers give the thumb its length and its place.
- **The injected clock.** `DeckState::demoted(now)` + `Deck { now }` +
  `TERMDECK_BLESS` is the pattern for a state that expires; the countdown here
  reads the same `now` the frame is drawn at and never a wall clock, so every
  keyframe is a fixture.
- **`pin-terminal.md` §3's finding that the border vocabulary is spoken for** —
  idle, demoted, notify warning, drag warning/accent, master accent. The pin
  answered it by staying out of the border; the scrollbar answers it by adding
  weight to the border rather than colour.

## 1. Which panes (decision 1)

**Chosen: the master only.**

- A preview already says the same thing in words. A pane holding a detached
  viewport draws `↑ 214 lines above · ^g [` on its last content row
  (`Deck::scroll_marker`), which is more precise than a ten-cell thumb and
  states the key that opens the pane properly. A second indicator for the same
  fact is what the declutter pass spent #101/#103 removing.
- The stack column already carries a track, one column to the *left* of the
  previews, for the list. A per-preview bar on the right edge would flank the
  same column with two scrollbars measuring two different things.
- A preview is ten rows: a thumb has ten cells of resolution, and at the
  minimum stack width (#44) those ten rows are the whole of what the pane has
  to say.

**Rejected:** *master and previews* — three more bars for a column that already
has one, saying what its marker says. **Rejected:** *previews only while the
pointer is over them* — a hover state the interface has nowhere else, and the
wheel over a preview is exactly when the user is looking at the marker.

The model is per pane, not per master: `DeckState` stores which configured
position was scrolled, so widening this to previews later is a change to one
`pane.master()` guard in `Deck::scrollbar` and nothing else.

## 2. Where it draws (decision 2)

**Chosen: the pane's right border column. The border is the track; the thumb is
the border drawn heavy — `┃` where the border draws `│`, in the border's own
colour.**

```
 ┌─ > 1 frontend  ·  ●  ·  pnpm dev ─────────────── [####··] × ─┐
 │  12:05:33 [vite] hmr update /src/components/StatusBar.tsx    │
 │  12:05:58 [vite] hmr update /src/routes/Dashboard.tsx        ┃  ← thumb
 │  12:06:04 [vite] hmr invalidate /src/hooks/useTerminals.ts   ┃
 │  12:06:11 [vite] hmr update /src/routes/Dashboard.tsx        │
 └──────────────────────────────────────────────────────────────┘
```

- **It costs no columns.** The master has no gutter to spare — the two gutter
  columns belong to the divider (#41) and the stack's own track (#34b), and in
  zoom, the narrow fallback and an empty stack there is no gutter at all. The
  border is the one column every master already spends on chrome.
- **It costs no terminal cells**, so `terminal_sizes` is unchanged and no
  engine reflow, no re-wrap and no fixture churn follows from the bar
  appearing. This is the whole reason for preferring the border to a content
  column: a scrollbar that resizes the terminal it measures would change the
  thing it is describing every time it appeared.
- **It adds no colour.** `pin-terminal.md` §3 found the border's colours fully
  spoken for; a grey thumb on an accent border would read as a mode. Weight is
  free: `│` → `┃` is the same distinction the stack track already relies on,
  and it stays legible whatever state the border is in (accent master, warning
  drag source, demoted).

**Rejected:** *stealing the rightmost content column* — it narrows the terminal
by one column whenever the bar appears, so live output re-wraps as a side
effect of scrolling. **Rejected:** *the bottom edge* — scrollback is vertical;
a horizontal bar would have to be read against the grain of the thing it
measures. **Rejected:** *a grey (`HINT`) thumb over the accent border* — see
above; the stack's dim-track/bright-thumb pairing exists because its track is
drawn on empty canvas, and inverting it here (bright track, dim thumb) says the
wrong thing. **Rejected:** *the gutter column beside the master* — it is the
divider's, and it does not exist in three of the four layouts.

The divider, when it is drawn, sits in the gutter column immediately right of
the master's border, so the two can stand side by side. They are told apart the
way the export tells chrome apart everywhere else: different colours
(`SEPARATOR`/`HINT` for the divider, the pane's border colour for the thumb),
different lengths (the divider runs the whole body; the thumb is a fraction of
one pane) and different behaviour (the divider is always there once the split
is adjustable; the thumb comes and goes).

## 3. When it is visible (decisions 3, 4 and 6)

Three conditions, all of which must hold:

```
overflow    lines_above + lines_below > 0        (decision 6)
and
in play     scrollback mode is on                (decision 4)
            or the pane was scrolled less than SCROLLBAR_WINDOW ago
and
master      the pane holds the master frame      (decision 1)
```

### 3.1 Only on overflow (decision 6)

The stack track's rule, unchanged: a pane with nothing above or below it has no
position to report, so it draws nothing — in scrollback mode too, where the bar
would otherwise be a full-height thumb saying "all of it". A fresh deck, a
short-lived process, a pane whose history has not filled a screen: all render
exactly as they do today.

### 3.2 The window (decision 3)

**Chosen: `SCROLLBAR_WINDOW = 4s`, restarted by every scroll.**

The interface has two windows already, and they mean different things: the
demotion highlight's **1.5s** is a report that something has *finished* — it
fires once and settles — while the notification flash's **4s** is a state that
is still *going on* and wants an answer. A scrollbar is the second kind: it is
up while the user is working the wheel, and the pause between two wheel flicks
while a line is read is comfortably longer than a second and a half. Four
seconds is long enough that reading between flicks does not make the bar
flicker, and short enough that a pane left alone is a pane with no chrome — the
"auto-hides on idle" the issue asks for. The 30s activity window is the wrong
end of the scale: it measures whether a process is alive, not whether a person
is doing something.

**Every scroll restarts it** rather than the first one arming a fixed window:
that is what makes a slow, deliberate scroll one continuous appearance instead
of a stutter.

### 3.3 Live tail, and what "being scrolled" means (decision 4)

**Chosen: the bar reports the viewport, and only a scroll — or the mode — puts
it on screen.** Output arriving at the tail is not a scroll and does not arm it.

- At the live tail the pane is where it always is. A bar that appeared on every
  burst of output would be permanently up on a busy pane and permanently
  flickering on a quiet one, which is the flicker the issue asks to avoid, and
  it would say nothing the pane does not already say by showing its newest
  line.
- Returning to the tail *is* a scroll (`esc` dispatches `ScrollCommand::Bottom`),
  so the bar stays for its window and shows the thumb snapping to the bottom.
  That is the countdown the issue describes starting when `esc` returns to live.
- Once the window passes at the tail, the pane renders byte for byte as it does
  today.

**Rejected:** *arming on output at the tail* — see above. **Rejected:** *staying
up for as long as the viewport is detached* — it is a permanent bar on a pane
the user has stopped touching, which is the feature the issue explicitly did
not ask for; the pane's newest line and the status row's `line N/M` (in mode)
carry that state instead.

### 3.4 Scrollback mode (decision 4)

While `^g [` is on the bar **stands, with no countdown**: the mode is the
signal, not the scroll. It is the mode's own gauge, beside the `SCROLL` tag,
the pane footer's keys and the status row's `line 2217/2431` — the one of the
four that says *where in the history* rather than *which line*. `esc` leaves
the mode and hands the bar to the window in §3.2.

## 4. Geometry

Read from the same three numbers the status row reads, so the two can never
disagree:

```
window  = the pane's frame rows          total = lines_above + window + lines_below
track   = the pane's inner height        (the border less its two corners)
length  = ceil(window x track / total), clamped to 1..=track
top     = lines_below == 0 ? track - length                  (anchored at the tail)
                           : min(lines_above x track / total, track - length)
```

This is `Deck::scroll_track`'s arithmetic with the list's counts swapped for
the viewport's, including its anchoring rule: at the bottom of the history the
thumb is flush with the bottom of the track, so "there is nothing further down"
is never a rounding question. A thumb is never shorter than one cell and never
longer than the track.

## 5. Interaction (decision 5)

**Chosen: display-only.** No hit test, no drag, no click-to-jump.

`scrollable-stack.md` §2 rejected dragging the stack's track for two reasons
that apply here word for word — a one-column drag target, and a wheel that
already covers the region — and one more that is stronger here: the pane's
right border is one cell from the divider's own drag target, so a mis-grab
would resize the deck. The keyboard half already exists in full (`^g [`, then
`j/k`, `pgup/pgdn`, `g/G`), so parity (`split-divider.md`) is satisfied by a
bar that takes no gesture at all.

**Rejected:** *click-to-jump* — a `ScrollCommand` in absolute lines, which the
frozen contract does not carry (it is `Up`/`Down`/`Bottom`), so it would need
an engine change for a gesture the wheel already performs.

## 6. State and plumbing

`DeckState` gains one field, `scrolled: Option<(usize, Timestamp)>` — the
configured position that was scrolled and when — and reads it back through
`scrolling(now)`, exactly as `demotion`/`demoted(now)` work, down to following
its pane through the renumbering a close causes (#84) and going when that pane
goes.

The session arms it wherever it already dispatches a scroll, and nowhere else:
the keyboard `Reaction::Scroll`, the `esc`-to-live `ScrollCommand::Bottom`, and
the wheel's `WheelRoute::Scrollback` (which arms the position under the pointer,
not the master, so wheeling a preview does not raise the master's bar). While a
bar is up the loop keeps drawing, the way a settling notification already does,
so the bar hides itself on time instead of waiting for the next keystroke.

No contract, engine or configuration change: `ScrollbackPosition` already
carried everything the design needs, as the stack gutter did before it.

## 7. Ambiguities

| # | Ambiguity | Resolution |
| --- | --- | --- |
| S1 | The export has no per-pane scrollbar. | Borrow the list's track vocabulary (`│`/`┃`, anchored, overflow-only) and put it on the border, which the master already spends (§2). |
| S2 | "Appears while the pane is being scrolled" — is output a scroll? | No. A scroll is a scroll command; output at the tail arms nothing (§3.3). |
| S3 | What "idle" is measured from. | The last scroll, on the injected clock, restarted by each one (§3.2). |
| S4 | Whether the mode counts as activity. | It replaces it: in `^g [` the bar stands with no countdown (§3.4). |
| S5 | Whether a bar and the divider can be confused where they touch. | Different colour, length and behaviour; and the bar takes no gesture, so a mis-grab is impossible (§2, §5). |
| S6 | Which pane a wheel over a preview arms. | The pane under the pointer. v1 draws only the master, so wheeling a preview draws nothing (§6). |

## 8. Known follow-ups

- **Previews**, if the marker ever proves too coarse: one guard in
  `Deck::scrollbar` and a decision about the marker it would double.
- **A pointer half** (drag or click-to-jump) needs an absolute
  `ScrollCommand`, which is a contract change and so another lane's call (§5).
- **Fade rather than cut.** The bar disappears in one frame; #33 (animations,
  parked) is where a fade would belong, and its research note already pins the
  keyframe-fixture pattern this slice uses for the on/off frames.
