# Collapsible preview stack — design spec (issue #27)

Status: design only. No production code, no engine or contract changes.

## 0. Source of truth and how to read this document

Everything below is derived from the updated design export at
`reference/Termdeck TUI.dc.html`, specifically:

- **Screen 05 — "Collapsed previews — `ctrl+g c`"** (the only rendered collapse
  state in the export): app and worker folded to title rows, backend grown into
  the freed height, stack still 44 columns, master still 98 columns.
- The spec board **"COLLAPSING A PREVIEW — ^g c"** (four prose rules).
- Screens 01–04 and the **BORDERS & SPACING**, **PALETTE**, **STATUS &
  ACTIVITY**, **FOCUS & PROMOTION** and **RESPONSIVE** boards, which the
  collapse state must not contradict.

Where the export is silent or self-contradictory this document says so
explicitly under **Ambiguity** and states the closest reading it picks. Nothing
here is invented beyond those marked points.

### The one correction to the issue framing

The issue describes the feature as "the stack's height shrinks, master takes
more room". **The export says otherwise and the export wins:**

> Collapse is vertical only; the stack never changes width.
> Freed rows redistribute to the panes still open, so one open preview grows to
> fill the column.

In screen 05 the master is still 98 columns and still 40 body rows — byte for
byte the same frame as screen 01. The master gains nothing. The rows freed by a
collapsed preview go to the **remaining open previews**. `master_ratio` is not
consulted, not changed, and not affected.

---

## 1. Geometry

### 1.1 The frame that everything sits in (unchanged, from BORDERS & SPACING)

Grid 144×42 at the reference size. The status row sits at the **top** since
#101 — the canvas puts it on row 1 in every layout — with one blank row under
it and the body beneath that. The export's specification panel still says
`content rows 40, blank row 41, status row 42`; the screens are what the
implementation follows.

| Region | Extent |
| --- | --- |
| Status | row 0, `STATUS_BG` |
| Blank | row 1 |
| Body | rows 2–41 (40 rows) |
| Master | columns 0–97 (98) |
| Gutter | columns 98–99 (2) |
| Stack | columns 100–143 (44) |

Inside the stack column the layout is a list of children, each followed by one
blank row, with the **last row of the stack area reserved for the stack
footer**. So:

```
budget B = stack_rows - 1          # 39 at the reference size
sum over children of (height + 1) == B
```

This is exactly the invariant the current renderer already satisfies
(`top += preview + 1`, footer at `area.y + area.height - 1`).

### 1.2 The two per-pane states

The export shows **exactly two** states for a stacked preview. There is no
partial, half-height, or animated intermediate state anywhere in the export, and
none should be invented.

| State | Height | Chrome |
| --- | --- | --- |
| Open | `PREVIEW_HEIGHT` (12) plus its share of freed rows | full box, title inside the top border, right slot, viewport |
| Collapsed | **1 row** | no box, no viewport, no right slot; a single title row on `#101317` |

### 1.3 Redistribution — the exact rule

Screen 05 pins this down completely. Stack area is 40 rows, so `B = 39`. Three
previews, two collapsed:

```
backend  open       34 rows   (rows  0–33)
blank                1 row    (row  34)
app      collapsed   1 row    (row  35)
blank                1 row    (row  36)
worker   collapsed   1 row    (row  37)
blank                1 row    (row  38)
footer row           1 row    (row  39, reserved — §3.6)
                    ------
                    40 rows
```

`34 = 12 + 22`, and `22 = 2 collapsed × (12 − 1)`. That is the whole rule:

```
base   = PREVIEW_HEIGHT (12) at stack width >= WIDE_COLUMNS,
         COMPACT_PREVIEW_HEIGHT (9) below it        # unchanged
c      = number of collapsed previews
k      = number of open previews
freed  = c * (base - 1)
open pane height = base + freed / k          (integer division)
                   + 1 more for the first (freed % k) open panes, top-down
```

**Invariant that falls out of this and should be asserted in a test:** collapse
never changes the stack's total used height. Used height is always
`n * (base + 1)` whatever the mix, because every collapsed pane hands over
exactly the rows it gave up. A collapse can therefore never overflow a stack
that fitted before it.

Consequences, all consistent with the export:

- `c = 0` → every open pane is `base`. Screens 01 and 02 are unchanged, so
  `frontend-active.txt` and `backend-promoted.txt` stay byte-identical.
- Fewer previews than fit does **not** grow them. Growth is caused only by
  collapse. (Screens 01/02 are 3 previews at exactly `base`; the export never
  shows 2 previews filling 39 rows, so nothing licenses that.)
- `k = 0` (everything collapsed, reachable via `^g c` on the master): the strips
  pack to the top of the stack column, the remaining rows stay canvas-blank, and
  the reserved footer row stays the last one.
  **Ambiguity:** the export never renders an all-collapsed stack. Chosen
  reading: strips top-aligned, the footer row still reserved at the bottom,
  master untouched — because "collapse is vertical only; the stack never
  changes width" forbids the master from claiming the empty column.

### 1.4 Minimum heights and the sub-30-row regime

From the collapse board:

> Below 30 rows the stack auto-collapses from the bottom up rather than
> shrinking panes past 6 rows.

`MIN_OPEN = 6` rows (border, 4 content rows, border). Rule:

```
loop:
    c = collapsed count (user flags + auto), k = n - c
    if k == 0: break
    avail = B - 2*c - k              # rows left for open pane bodies
    if avail / k < MIN_OPEN:
        auto-collapse the bottom-most currently-open preview
        continue
    break
```

Then, if `k * base + freed + n > B` (the pre-collapse layout did not fit in the
first place), distribute `avail` evenly among the open panes instead of using
§1.3 — floor for all, remainder to the topmost, never below `MIN_OPEN`.

An auto-collapsed pane is marked distinctly from a user-collapsed one so that
growing the terminal restores it; the user's own flags are never rewritten by a
resize.

**Ambiguity:** the RESPONSIVE board still says `<30 rows previews reduce to 2,
then to the strip`, which is the pre-collapse behaviour and contradicts the
collapse board. The collapse board is newer and specific to this feature, so it
governs: **a preview is never dropped from the stack; it becomes a strip.** Also
note that with 3 previews the 6-row floor actually starts biting at about 22
body rows, not 30 — "below 30 rows" is a coarse heading and the 6-row floor is
the operative clause. Implement the floor, not the number 30.

### 1.5 Narrow threshold

Today: `body.height < preview + 2` → `Layout::Narrow`. Previews can now be one
row, so the height trigger becomes:

```
Narrow when body.height < 2 * n + 1        # every preview a strip, plus the footer
```

The width trigger (`body.width < NARROW_COLUMNS`) is unchanged.

---

## 2. The control

### 2.1 What the export names

> `^g c` toggles the selected pane; on the master it collapses every preview at
> once. `^g 2-4` promotes a collapsed pane directly — it expands as it takes the
> master frame.

Screen 05's status row carries the fold census and its hint row advertises
`^g c`. The stack column states no census of its own: the updated canvas took
the footer's `{c} collapsed · ^g c expand all` out (#103), because the strip,
its marker and the status row already say it. See §3.5 and §3.6.

### 2.2 Ambiguity, and the resolution

The export says `^g c` "toggles the selected pane", but **Termdeck has no
preview selection**: selection *is* promotion, the master is always the selected
pane, and every key that addresses a preview (`^g N`, double-click, drag)
promotes or swaps it. Read literally, `^g c` can therefore only ever mean "on
the master", i.e. all-at-once — yet screen 05 shows a *mixed* stack (backend
open, app and worker collapsed) that all-at-once cannot produce.

So a per-preview control must exist. Resolution, in two parts:

**(a) `^g c` — the keyboard control (required).**
The master is always the selected pane, so `^g c` toggles the whole stack:

- any preview open → collapse every preview;
- every preview collapsed → expand every preview.

This is the language the export puts on screen: the hint row names `^g c`
while a fold is in play.

**(b) Click the disclosure marker — the per-preview control (required).**
The export draws the affordance itself: `▾` on an open pane's title row, `▸` on
a collapsed strip, both in `HINT`, at the left of the title. A press-and-release
inside the marker's two cells (`▾ ` / `▸ `) toggles **that** preview.

The marker click is chosen because:

- it is the only affordance the export actually draws for collapse;
- it is the only per-preview address left — issue #26 already binds
  double-click to promote and drag to swap, and a bare single click merely arms
  the double-click timer, so a two-cell target does not collide with either;
- it needs no new key, no prefix chord, and no preview cursor.

A marker click **consumes** the gesture: it must not begin a drag and must not
arm the double-click promote timer.

### 2.3 Where the affordance lives visually

- Open preview: `▾ ` prefixed to the title, inside the top border, at the
  title's existing 2-column inset. Screen 05: `▾ 2 backend · ●`.
- Collapsed preview: `▸ ` at the head of the strip, same column.
- The marker is drawn on **every** stack pane, folded or not — see §3.4, as
  revised by issue #32.
- The status row states the census and advertises the key, and is the only
  place that does: see §3.5. The stack footer states no fold (§3.6).

### 2.4 What is *not* the control

No drag handle, no resize gesture, no chevron on the master, no new key beyond
`^g c`, and no wheel-to-collapse. The export shows none of these.

---

## 3. States

### 3.1 Collapsed preview — the strip

From screen 05, verbatim:

```html
height:20px; padding:0 2ch; background:#101317
  "▸ "          #4d555f
  "3 app"       #8a929c
  " · "         #3a424c
  "●"           #98c379
  " · "         #3a424c
  "bundled 1.2s" #6d7580
```

Rendered form:

```
▸ {n} {name} · {dot} · {tail}
```

| Element | Token (existing) | Notes |
| --- | --- | --- |
| Strip background | `DEMOTED_BG` `#101317` | painted across all 44 columns |
| `▸ ` | `HINT` `#4d555f` | |
| `{n} {name}` | `PREVIEW_FG` `#8a929c` | |
| ` · ` | `SEPARATOR` `#3a424c` | |
| `{dot}` | from `status_glyph` | `●` SUCCESS / `○` HINT / `✕` ERROR / `✓` SUCCESS |
| `{tail}` | `MUTED` `#6d7580`, or `ERROR` when it is an exit | |

**No new palette tokens.** `#101317` is already `palette::DEMOTED_BG`.

Geometry: 1 row, full stack width, **no border**. Text starts at
`stack.x + 2`, the export's `padding:0 2ch`. Right inset 2 columns; the tail
clips right-first with `…` using the existing `clip`.

**Note the one-column offset (A11).** An open pane's title text starts at
`stack.x + 3` — the export puts the title box at `left:2ch` and gives it
`padding:0 1ch`, and the committed fixtures already render `┌─ 2 backend` with
`2` at column 103 of a stack at column 100. The strip's `padding:0 2ch` puts its
text one column further left. So `▸` and `▾` do **not** line up vertically. This
is what the export draws; follow it literally rather than moving either one,
since moving the title would change `frontend-active.txt`.

The strip drops, per the export ("loses its box and its cwd"): the border, the
cwd, the right slot (activity meter / `2m ago` / `idle 6m` chip), the viewport,
the exit footer rule and `r restart` line, and the `↑ N lines above` marker.

**`{tail}` — "its last meaningful line".** The export gives three examples:
`bundled 1.2s`, `idle 6m`, `exit 1`. Deterministic rule:

| Terminal state | Tail | Colour |
| --- | --- | --- |
| Exited / Failed | `exit {code}` / `exited` / `failed` (reuse `status_label`) | `ERROR` |
| Running, idle ≥ 30s | `idle {age}` (reuse `age`) | `MUTED` |
| Running, active | last non-blank line of the terminal frame, clipped | `MUTED` |
| Starting | `starting` | `MUTED` |

**Ambiguity:** two of the export's three examples (`idle 6m`, `exit 1`) come out
of this rule exactly. The third, `bundled 1.2s`, is an editorial shortening of
app's real last line `12:06:13 bundled 1.2s 842 mods`; no deterministic renderer
reproduces that particular substring. The rule above matches it in kind. Flagged
rather than special-cased.

### 3.2 Open preview while the stack has a collapsed pane

Screen 05's backend pane. Identical to an ordinary preview (`IDLE_BORDER`
`#232830`, `CANVAS` background, right slot `[##····]`, full title with cwd)
except:

- `▾ ` in `HINT` is prefixed to the title;
- its height is the §1.3 figure (34 rows in screen 05).

The marker costs 2 columns of title budget. At the reference width this still
fits with room to spare: `▾ 2 backend · ●` is 15 columns against a 28-column
budget (38 content columns − 8 slot − 2 clearance). It was 27 before the
declutter pass took the `· …/backend` out of every pane title, which is what
made the 2 columns free.

### 3.3 Demoted, exited, and scrollback interplay inside a strip

- **Demoted + collapsed:** a strip has no border and already sits on
  `DEMOTED_BG`, so it carries no demotion highlight. **Accepted loss:** the
  stack footer's `promoted {name} · ^g 1 back` line went with the declutter
  pass (§3.6), so a demotion into a folded slot is stated only by the status
  row's `>` pointer naming the new master.
- **Exited + collapsed:** dot `✕` in `ERROR`, tail `exit 1` in `ERROR`. The rule
  line, timestamp and `r restart` line are gone with the box. `^g r` is
  unchanged — it has always restarted the *active* terminal only, so nothing
  regresses.
- **Scrollback tag + collapsed:** the `↑ N lines above · ^g [` marker is not
  drawn on a strip. The export's strip vocabulary has no slot for it and the
  strip has no viewport for it to describe. **Accepted loss:** the tag reappears
  the instant the pane expands. This is the only piece of information collapse
  actually hides.
- **Scrollback tag on a thin open preview:** unchanged behaviour — the marker
  takes the pane's last content row. At the `MIN_OPEN` floor (6 rows → 4 content
  rows) that leaves 3 output lines, which is the intended trade in the export's
  "rather than shrinking panes past 6 rows".

### 3.4 When the disclosure markers appear

**Ambiguity.** The collapse board says "Open panes show `▾`", but screens 01 and
02 — part of the *same* updated export — show preview titles with **no marker
at all**.

**Original reading (superseded):** markers drawn only while at least one preview
is collapsed. That is the only reading under which all three rendered screens
are simultaneously correct, and it kept the two existing snapshots untouched.

**Revised by issue #32 — markers are drawn on every stack pane, always.** The
superseded reading was tested against a user and failed: on a fresh run nothing
on screen said the stack folds, so the feature was reachable only by already
knowing `^g c`. §8 flagged exactly this ("if that bootstrapping is judged wrong,
the fix is a design decision — draw the markers unconditionally — and it would
change the two committed screen fixtures"). It was judged wrong.

So the collapse board's "Open panes show `▾`" is taken at face value and screens
01/02 are treated as pre-dating the collapse feature rather than as constraining
it. Every stacked preview carries `▾` from the first frame; a folded one carries
`▸`. The master never carries either — it has no fold to state. The marker's two
cells are live from the first frame too, so the pointer path needs no keyboard
bootstrap (§2.2b).

This is the *only* change issue #32 makes to the marker: colour (`HINT`),
column (§3.1's A11 one-column offset), the 2-column title budget cost and the
click target are all unchanged.

### 3.4b The state a run starts in

**Revised by issue #39 — every preview starts folded.** The export's screens
01–04 show an open stack, and until #39 `DeckState::new` matched them. The user
inverted the default: a fresh run draws the §5 "all previews collapsed" state —
strips top-aligned at the head of the column, blank column below, status
`0 open  ·  {n} collapsed` — and a marker click (or `^g c`) is what opens a
preview.

Nothing else in this document changes. The geometry, the strip contents, the
marker cells, the footer and status precedence and the §4 interplay rules are
all read from the same flags; only the value they start at is inverted. Two
consequences are worth stating because they are now the *first* frame rather
than an edge case:

- **`^g c` reads as expand-all first.** It is still one toggle — collapse-all
  while any preview is open, expand-all once none is (§2 (a)) — but the stack
  it starts from is folded, so the first press opens it.
- **The demoted master stays open.** §4's "the demoted old master lands in the
  vacated slot **open** (it was never collapsed)" survives as the invariant *a
  pane that has held the master frame is open*: `promote` clears the flag of
  what it promotes, and the terminal that opens as master carries no fold from
  the start. Every other terminal has only ever been a preview, so it starts
  folded.
- **Zoom keeps the unfolded hint row.** §4 says the zoom status line "says
  nothing about collapse", and §3.5's swap of `^g [ scroll` for `^g c collapse`
  is now true from frame one, so the two collided on every zoomed screen. The
  more specific rule wins: while the stack is hidden the fold is inert and the
  hint row keeps `^g [ scroll`. `zoomed.txt` is unchanged by #39 as a result.

### 3.5 Status bar

The status row's left segment (row 0 since #101):

```
 idp   4 terminals   > 1 frontend  ·  1 open  ·  2 collapsed
```

with `2 collapsed` in `WARNING` `#d8a657` and everything else in `HINT`.

Rule: when `c >= 1` the census `{n-1} stacked` is replaced by
`{k} open  ·  {c} collapsed`, and the exited summary (`all running` / `N
exited`) is **dropped**. Screen 05 has nothing exited, so the code's current
`all running` suffix would otherwise appear there and it does not. When `c == 0`
the status bar is exactly as today.

Its right hint row:

```
^g j/k switch  ^g c collapse  ^g z zoom  ^g ? help  ^g q quit
```

`^g c` in `ACCENT` `#2dd4a7`, its label `collapse` in `PREVIEW_FG` — the same
treatment `^g z` gets in screen 03 when zoom is on. Note that `^g [ scroll` is
**absent**; `^g c collapse` takes its place.

`^g N select` is absent too, and permanently: the declutter pass took it and
`^g [ scroll` out of the key row for good. Both stay bound, stay in the help
overlay, and stay in the narrow collapsed row.

Rule: when `c >= 1` the hint row **drops** `^g [ scroll` and **seats
`^g c collapse` ahead of `^g z zoom`**, accenting the key. Note the ordering:
the export does not put collapse into the slot scrollback vacated, it puts it
third. When `c == 0` the hint row is unchanged (screens 01/02).

**Ambiguity:** the export never shows a hint row containing `^g c` with nothing
collapsed, and screens 01/02 in the same export show the pre-collapse row.
Chosen reading: the hint row advertises `^g c` only while the mode is active,
matching the existing "an active mode names its own key in accent" pattern.
Discoverability with nothing collapsed comes from the help overlay (§6, H4).

The narrow status line (`84×22  stack hidden`) is unchanged.

### 3.6 Stack footer precedence

The stack's last row is still reserved, but it now states only what nothing
else states. The declutter pass took out the promotion keys
(`ctrl+g N promote · j/k cycle`) and the `promoted {name} · ^g 1 back`
demotion line; the updated canvas (screen 05) took the fold census out with
them (#103), because a fold is already declared three times over — its own
strip, its marker, and the status row's `{c} collapsed` beside an accented
`^g c`.

What is left, most specific first:

1. scrollback — `scrollback · esc returns to live`
2. hidden previews — `↑ 2 more · ↓ 3 more · ^g pgup/pgdn`, the one thing a
   preview cannot state about itself (`scrollable-stack.md` §3)
3. otherwise the row is blank.

### 3.7 Transitions

Instant, both directions. The export's FOCUS & PROMOTION board says promotion is
"an instant swap — no slide or fade"; collapse inherits that. No animation, no
easing, no reflow step.

---

## 4. Interplay

| Situation | Behaviour | Grounding |
| --- | --- | --- |
| **Promote a collapsed preview** (`^g N`, double-click, drag) | Its collapse flag clears and it expands as it takes the master frame. The demoted old master lands in the vacated slot **open** (it was never collapsed). Freed rows recompute for the remaining stack. | Export: "`^g 2-4` promotes a collapsed pane directly — it expands as it takes the master frame." |
| **Collapse state across promotion** | Flags are keyed by configured position, so they travel with the terminal and survive any number of promotions and swaps. Only the pane being promoted has its flag cleared. | Export: "Collapse state persists per workspace and survives promotion." |
| **The master** | Never collapsed. There is no master collapse flag; `^g c` on the master addresses the previews. | Export: "on the master it collapses every preview at once." |
| **Zoom + collapse** | Zoom hides the stack outright, so collapse is inert but retained; unzoom restores the exact mix. The zoom status line keeps its `hidden: 2● 3● 4○` summary unchanged and says nothing about collapse. | Screen 03 shows the zoom status line with no collapse census; the export never combines the two. **Ambiguity resolved by: zoom hides everything, so it reports everything the same way.** |
| **Narrow fallback** | Stack is gone; collapse is inert but retained. Status stays `stack hidden`. `^g c` is a silent no-op. | Screen 04. |
| **`master_ratio`** | Untouched, in both directions. Collapse never reads it and never changes it. Stack width stays `stack_width(width, master_ratio)`. | Export: "Collapse is vertical only; the stack never changes width." |
| **Mouse wheel over a collapsed strip** | No-op. A strip has no viewport, so no `EngineCommand::Scroll` is dispatched and the wheel-up-on-master → scrollback shortcut does not fire. The wheel over an *open* preview is unchanged. | A strip has nothing to scroll; scrolling it would move an invisible viewport. **Chosen**, not shown in the export. |
| **Double-click a collapsed strip** | Promotes it, which expands it (row 1 of this table). Hit-testing must therefore still resolve a strip to its terminal. | Export's `^g 2-4` rule applied to #26's equivalent gesture. |
| **Drag from / onto a collapsed strip** | Swaps as usual; the promoted pane expands, the demoted one lands open. | Same. |
| **Marker click vs. drag/double-click** | The two marker cells are checked first and consume the gesture. Everywhere else on the pane behaves exactly as #26 defines. | §2.2. |
| **Modals** | A modal owns every key, so `^g c` is unreachable while help or quit is open; the dimmed stack behind it keeps its collapse geometry. | Existing modal rule, unchanged. |

---

## 5. Edge cases

| Case | Behaviour |
| --- | --- |
| **Empty stack** (1 terminal, `n = 0`) | Nothing to collapse. `^g c` is a no-op and returns `false` from the state method. The status census reads `0 stacked` as today; no collapse census, no `^g c` hint. The stack column already draws only its reserved footer row. |
| **Exactly 1 preview** (`n = 1`) | `^g c` toggles it. Collapsed: one strip at the top of the column, the rest of the column blank, status `0 open  ·  1 collapsed`. Open: `base` rows as today. |
| **All previews collapsed** (`k = 0`) | Strips top-aligned, blank column below, the reserved footer row still the last one. Master unchanged in both dimensions. `^g c` expands all. |
| **Exited preview collapsed** | Dot `✕` `ERROR`, tail `exit {code}` `ERROR`. Exit rule/timestamp/`r restart` are not drawn. §3.3. |
| **Preview holding scrollback history, collapsed** | The `↑ N lines above · ^g [` tag is not drawn; it returns when the pane expands. The engine's viewport position is untouched by collapse. §3.3. |
| **Starting terminal collapsed** | Dot `○` in `WARNING` (existing `status_glyph`), tail `starting`. |
| **Resize while collapsed** | User flags survive. Auto-collapse flags (§1.4) are recomputed from scratch on every layout pass, so growing the terminal restores auto-collapsed panes and never resurrects a user-collapsed one. |
| **Collapse then zoom then narrow then back** | Flags are state, not layout; every path restores the same mix. |
| **`n` previews where `c` does not divide evenly** | Remainder rows go to the topmost open panes, top-down (§1.3), so the column stays deterministic and snapshot-stable. |

---

## 6. What the coder must implement

Ordered so each step is independently testable. The core is C1–C5; R3 is the
responsive follow-on and can land second.

### State

- **S1.** `DeckState` gains `collapsed: Vec<bool>`, one entry per configured
  position, all `false` at `new()`. Keyed by configured position so it travels
  with the terminal.
- **S2.** `DeckState::collapsed(position) -> bool`,
  `toggle_collapse(position) -> bool`, `toggle_collapse_all() -> bool`
  (collapse all if any preview is open, otherwise expand all). Each returns
  whether anything changed.
- **S3.** `promote()` clears the promoted position's flag. Nothing else in
  `promote()` changes.
- **S4.** Master's own flag is forced `false` and never read.
- **Constraint:** do **not** add an `ActionCommand` variant. The contracts are
  frozen for this slice; wire `^g c` straight to the `DeckState` method from
  `Input::command`, the way `Reaction::Respawn`/`Quit` already bypass the action
  enum. Update `input.rs`'s module doc, which currently claims every binding
  maps onto a frozen `ActionCommand`. (If contracts are un-frozen later, an
  additive `ToggleCollapseAll` is the natural upgrade; it is not needed now.)

### Geometry

- **C1.** Extract one `stack_layout(area, base) -> Vec<(position, Rect, bool
  collapsed)>` from `Deck::stack`, implementing §1.3 (and §1.4). **Both**
  `Deck::stack` and `Deck::terminal_at` must consume it — preview rects are no
  longer uniform, and the two must not drift.
- **C2.** `COLLAPSED_HEIGHT: u16 = 1` and `MIN_OPEN: u16 = 6` as named
  constants next to `PREVIEW_HEIGHT`.
- **C3.** Draw the strip: §3.1. `DEMOTED_BG` background across the full stack
  width, text at `stack.x + 2`, no `Block::bordered`.
- **C4.** Draw `▾ `/`▸ ` per §3.4 on every stack pane, and shrink the title
  budget by 2 accordingly.
- **C5.** Status bar census + hint swap per §3.5; stack footer per §3.6.
- **R3.** Auto-collapse and the `MIN_OPEN` floor (§1.4) plus the new narrow
  threshold (§1.5). This replaces the current "drop previews that do not fit"
  `break`.

### Control wiring

- **H1.** `Input::command`: `Key::Char('c')` → `deck.toggle_collapse_all()`.
- **H2.** Mouse: a press-and-release inside the two cells the marker actually
  occupies toggles that preview and consumes the gesture — no drag begins, no
  double-click timer arms. Per A11 those cells are
  `stack.x + 3 .. stack.x + 4` on an open pane's top border row, and
  `stack.x + 2 .. stack.x + 3` on a collapsed strip's row.
- **H3.** Wheel: suppress the `EngineCommand::Scroll` dispatch when the hit pane
  is collapsed. Simplest plumbing: have the hit test yield the configured
  position (it already walks the stack) and let the session consult
  `deck.collapsed(position)`. Hit-testing itself must keep resolving a strip to
  its terminal so double-click-promote and drag-swap still work.
- **H4.** Help overlay: add `("^g c", "collapse / expand previews")` under
  `VIEW`, after `^g z`. `HELP` goes 13 → 14 entries, so `HELP_SIZE` goes
  `(60, 21)` → `(60, 22)`; the overlay's composition rule (entries + section
  blanks + `esc close` + borders) is otherwise unchanged. `help.txt` must be
  regenerated.

### Snapshot fixtures

- **F1.** New `src/ui/testdata/collapsed-stack.txt` at 144×42, built from the
  **existing** `fixture::projects()` engine state with `collapsed` set on
  configured positions 2 (app) and 3 (worker), master = frontend. Expected
  chrome, matching screen 05 exactly:

  ```
  row      0  status (from column 2) " idp   4 terminals   > 1 frontend  ·  1 open  ·  2 collapsed"
                     + "^g j/k switch  ^g c collapse  ^g z zoom  ^g ? help  ^g q quit"
  row      1  blank
  rows  2–35  backend, open, 34 rows, title  "▾ 2 backend · ●"
                                     slot   "[##····]"
  row     36  blank
  row     37  strip  "▸ 3 app · ✕ · exit 1"        (background #101317)
  row     38  blank
  row     39  strip  "▸ 4 worker · ○ · idle 6m"    (background #101317)
  row     40  blank
  row     41  the stack's reserved footer row, blank here (§3.6)
  ```

  Row numbers are absolute: the status row is row 0 and the body starts at row
  2 (§1.1). The title carries no `cwd` — the declutter pass took it out of
  every pane title.

  **Deviation to expect:** the export's screen 05 draws app as *running* after a
  restart (`● · bundled 1.2s`) and gives backend 34 rows of fresh output that
  the committed fixture does not contain. The fixture keeps app exited, exactly
  as the same export's screen 01 shows it, so the strip reads `✕ · exit 1`
  rather than `● · bundled 1.2s`. Chrome geometry, tokens and vocabulary match
  screen 05; only the fixture's terminal contents differ, and they are the
  export's own contents from screen 01.

- **F2.** `frontend-active.txt` and `backend-promoted.txt` change by exactly
  three rows each under issue #32's revised §3.4 — the `▾ ` prefix on each of
  the three preview titles, absorbed out of the trailing border rule. Every
  other row, and the whole status line, stays byte identical (§1.3, §3.5). If
  anything else moves, a rule was over-applied.
- **F3.** `zoomed.txt`, `narrow.txt`, `scrollback.txt`, `quit.txt` unchanged.
  `help.txt` changes only by the added `^g c` row and the taller overlay (H4).

### Tests worth naming

- Redistribution: 2 of 3 collapsed → the open pane is 34 rows; used height is
  `n * (base + 1)` for every mix (the §1.3 invariant).
- Remainder: 1 of 3 collapsed → the two open panes are 18 and 17, top-down.
- `toggle_collapse_all` round-trips; from a mixed state it collapses all first.
- Promotion of a collapsed preview clears that flag and leaves the others alone.
- Collapse state survives a promote/demote cycle.
- Wheel over a collapsed strip dispatches no `Scroll`; double-click over one
  still promotes.
- Auto-collapse fires bottom-up and never rewrites a user flag (R3).

---

## 7. Ambiguities, collected

| # | Ambiguity | Resolution |
| --- | --- | --- |
| A1 | Issue says the master grows; the export says the stack keeps its width and the freed rows go to the open previews. | Export wins. Master unchanged in both dimensions. |
| A2 | `^g c` "toggles the selected pane", but there is no preview selection. | `^g c` = all previews (the master is always selected). Per-preview toggle = click the disclosure marker, the only affordance the export draws and the only unbound pointer target left after #26. |
| A3 | "Open panes show `▾`" vs. screens 01/02 showing no marker. | ~~Markers appear only while ≥ 1 preview is collapsed.~~ **Revised by #32:** the board wins — every stack pane always shows a marker, and screens 01/02 are read as pre-dating the feature. |
| A4 | `^g c collapse` in screen 05's hint row vs. its absence in screens 01/02. | The hint row swaps `^g [ scroll` for `^g c collapse` only while ≥ 1 preview is collapsed; discoverability otherwise comes from the help overlay. |
| A5 | "last meaningful line" — `bundled 1.2s` is not derivable from any single field. | Deterministic table in §3.1; matches `idle 6m` and `exit 1` exactly, matches `bundled 1.2s` in kind. |
| A6 | RESPONSIVE board's `<30 rows → previews reduce to 2, then to the strip` vs. the collapse board's auto-collapse rule. | Collapse board governs; previews are never dropped, only folded. The 6-row floor is the operative number, not 30. |
| A7 | All-collapsed stack is never rendered. | Strips top-aligned, remaining column blank, the footer row still reserved at the bottom. |
| A8 | Zoom + collapse never shown together. | Zoom hides the stack; its status line is unchanged and says nothing about collapse. |
| A9 | Stack footer when a demotion and a collapse are both live. | **Moot since #103:** the footer draws neither. The demotion is stated by the demoted pane's own 1.5s highlight, the fold by the status row (§3.6). |
| A10 | Wheel over a strip never shown. | No-op — a strip has no viewport. |
| A11 | The export's strip text sits at `2ch` while a pane title's text sits at `3ch`, so `▸` and `▾` are one column out of line. | Followed literally. Moving the title would change the committed screen-01/02 fixtures; moving the strip would deviate from screen 05. |


---

## 8. Implementation status

Implemented on `coord/27-collapse-stack`. Gate: **114 tests pass, `cargo fmt
--check` clean, `cargo clippy --all-targets -D warnings` clean.**

**Landed: S1–S4, C1–C5, H1–H4, F1–F3.** `origin/main` (carrying #26's mouse
capture, drag-to-swap and double-click-to-promote) is merged into the branch,
and screens 01–04 plus `scrollback` and `quit` remain byte-identical to
`origin/main`; only `help.txt` changes, by the added `^g c` row and the
one-row-taller overlay.

**H2 — the marker click.** `Deck::marker_at` resolves the two cells the export
draws the marker in, and the session's mouse arm consumes the gesture: a press
there cancels any drag, arms no double-click, and the matching release calls
`DeckState::toggle_collapse`. Pressing the marker and releasing elsewhere does
nothing, and `marker_press` is reset alongside the drag on every other event.

Two consequences of A3 worth stating plainly, because they are load-bearing:

- **The marker is only clickable while a fold is in play.** Markers are not
  drawn with nothing collapsed (that is what keeps screens 01/02 byte-identical),
  and `marker_at` returns `None` in that state, so those cells keep their
  ordinary drag and double-click behaviour. `^g c` is therefore how a stack
  enters the folded state; the marker is how you adjust it afterwards. If that
  bootstrapping is judged wrong, the fix is a design decision — draw the markers
  unconditionally — and it would change the two committed screen fixtures.
- **The marker cells sit inside their pane**, so `marker_at` and `position_at`
  both match there. Consuming the gesture is what stops one click from also
  starting a drag or arming a promotion; `the_marker_overlaps_the_pane_it_belongs_to`
  pins that overlap so the precedence cannot be dropped silently.

**Drag and demotion highlights on a folded pane:** a strip has no border and
already sits on the demoted background, so it takes neither the drag-source nor
the drag-target dressing. Consistent with §3.3's demotion rule.

**Two corrections this document absorbed from implementation:**

1. §3.5 — the hint row seats `^g c collapse` *third*, ahead of `^g z zoom`, and
   drops `^g [ scroll`; it does not reuse scrollback's slot. Corrected above.
2. §3.1 — the strip tail clips to the width the 44-column strip has left after
   the marker, number, name and dot, so a long output line renders as e.g.
   `▸ 2 backend · ● · 12:06:09 /api/termina…`. The rule was right; the worked
   example is worth having.

**R3 was deliberately not implemented** (auto-collapse, the `MIN_OPEN` floor,
the new narrow threshold), per the scope chosen for this slice. Nothing depends
on it: the §1.3 invariant means a fold can never overflow a stack that fitted
before it, so the existing "drop previews that do not fit" path stays correct.
