# Mouse text selection in a pane — decisions (issue #148)

Status: implemented on `coord/148-mouse-select`; the highlight's auto-expiry
(§3.3) followed on `coord/150-selection-expiry`. UI plus session mouse routing;
no engine, contract, configuration or CLI change, and no new dependency.
Runtime state, like the split ratio (#41), the pin (#113) and the scrollbar
window (#115): the session holds it and writes nothing back.

The design export has no vocabulary for a selected region — no screen in it
shows one, and the palette board names no selection colour. This document
records what was added, what it was derived from, and the ambiguities it
resolves, in the same form as `pin-terminal.md` and `scrollbar.md`.

## 0. What this is derived from

- **The outer terminal already owns the mouse.** `OuterTerminal::enter` writes
  `1000` (button reports), `1002` (motion while a button is held) and `1006`
  (SGR encoding), and `mouse_event` in `session/input.rs` decodes press, move
  and release. A drag already reaches the deck as `Down`, a run of `Move`s and
  an `Up`; nothing new is asked of the input layer.
- **The host terminal's own selection is already gone.** Because `1002` is on,
  the host no longer selects on a plain drag. Selection is therefore not a
  feature termdeck is adding on top of one the user has — it is a feature
  termdeck took away in #74 and is giving back.
- **`Deck::pane_cell` (#74)**, the existing pointer-to-terminal-cell map that
  wheel forwarding stands on. Selection needs a stricter version of the same
  measurement, and gets it from the same layout functions, so the hit test and
  the renderer can never disagree about where a pane's viewport is.
- **`TerminalFrame`** (`contracts/screen.rs`): a revisioned, row-major grid of
  `ScreenCell`s with `Glyph`/`Continuation`/`Empty` content. The copy reads
  exactly this, through the frozen contract, and nothing else.
- **`encode_paste` (#120)**: the one place that turns text into bytes for a
  child, bracketed while the child holds DEC 2004. The yank in §4.3 reuses it
  rather than inventing a second paste path.
- **The "the press decides" rule the divider already follows (#41)**: "The
  divider is in the gutter, which belongs to no pane, so holding it can never
  be a pane drag. Once held it keeps the pointer until release, wherever the
  pointer travels." Selection is the same rule applied to pane content.

## 1. The gesture (decision 1)

**Chosen: where the press lands decides what the drag is. A press on a pane's
*content* selects text; a press on its *chrome* — the border ring and the title
row — reorders panes. No modifier, no mode.**

```
┌─ > 1 frontend  ·  ●  ·  pnpm dev ──────────────── × ─┐   ← press here: reorder
│  12:05:33 [vite] hmr update /src/components/Sta…    │
│  12:05:58 [vite] hmr update /src/routes/Dashboa…    │   ← press here: select
│  12:06:04 [vite] hmr invalidate /src/hooks/useT…    │
└──────────────────────────────────────────────────────┘   ← press here: reorder
```

The press arms both candidates and the first pointer move to a *different* cell
resolves it, so nothing about the click gestures changes:

| gesture | before #148 | after #148 |
| --- | --- | --- |
| press content, release on the same cell | click (double-click promotes) | unchanged |
| press content, drag onto another pane | reorder/promote | **selects text** |
| press title or border, drag onto another pane | reorder/promote | unchanged |
| press the divider, drag | resize the split | unchanged |
| press the `▾`/`▸` marker | fold/unfold | unchanged |
| press the `×` | close the pane | unchanged |
| wheel | scrollback, or the app | unchanged |

So exactly one gesture changed meaning, and it kept a home two cells away: the
title row of every pane is a drag handle, which is what a title row is for in
every window system, and the pane's four border sides are the same handle. A
preview at the minimum stack width (#44) is 34 columns of title — a bigger
target than the marker and the close mark it already carries.

**Rejected:** *a modifier (alt-drag or shift-drag selects)*. Shift is the one
chord many host terminals reserve for bypassing mouse reporting altogether, so
`shift+drag` may never reach termdeck at all — that is precisely why
`MouseAction::RangeUp` exists as a decoded-but-unowned event. Alt-drag is taken
by block-select in iTerm2 and by window dragging in most Linux window managers.
A gesture that is silently eaten by a third of the hosts is not a gesture.

**Rejected:** *a selection mode (`^g v`, then drag)*. Copying a line is the most
casual thing a user does with a terminal; putting a mode in front of it means
the mode is on when they did not want it and off when they did. Every mode this
interface has (zoom, scrollback, the modals) exists because it captures *keys*.
Selection captures no key.

**Rejected:** *keeping reorder on content and moving selection to the chrome*.
The inverse assignment is available and is wrong: the thing you point at to
select text is the text.

**Rejected:** *double-click selects a word, triple-click selects a line*.
Double-click in a pane already promotes it (#34), which is the older gesture and
the one the export's FOCUS & PROMOTION section is about. Word selection would
have to take it. Left for a later issue, where it would arrive as a modifier or
as a second click *inside an existing selection*.

### 1.1 What a press clears

Any press clears the previous selection, wherever it lands. So does any key,
any wheel tick, and any resize — see §3.3. A selection never survives the next
thing the user does.

## 2. What is selected, and where (decisions 2 and 3)

### 2.1 Panes (decision 2)

**Chosen: every pane that draws a viewport — the master and every open preview.
A collapsed strip has none, and is not selectable.**

Unlike the scrollbar (#115), which is the master's alone because a preview says
the same thing in words, there is no second way to get text out of a preview.
The pane where an error appears is very often a preview, and the whole point of
the issue is to get that text out without promoting the pane first. The hit
test and the renderer both go through one function, so a preview costs nothing
extra: `Deck::viewport_rect` is the same measurement `draw_pane` uses to decide
where terminal cells start.

### 2.2 Shape (decision 3)

**Chosen: a linear, text-flow range — anchor cell to head cell in reading
order, with every row between them selected in full.** Not a rectangle.

The thing being selected is nearly always a run of output: a command, a path, a
stack trace, a URL that wrapped. A rectangle would cut those in half. Block
selection is a specialist's gesture that belongs behind a modifier if it is ever
wanted, and the modifiers are all spoken for or unreliable (§1).

The range is normalised on read, so dragging up or leftwards works exactly like
dragging down or rightwards; there is no "backwards" selection state.

### 2.3 The selection is a range of *cells*, not of text

This is the invariant the whole slice rests on:

```
a selection is (pane position, anchor cell, head cell) in the pane's
current frame coordinates — it is never a copy of text, a scrollback
offset, or an anchor in the child's output history
```

The highlight is drawn by inverting exactly those cells of the frame the
renderer is drawing. The copy is built by reading exactly those cells of the
frame the engine holds. Both read the same coordinates from the same frame, so
**what is inverted is what is copied**, with no reconciliation step that could
be wrong.

The honest cost: if the child writes new output under a standing selection, the
highlight stays where it is and now covers different text. That is visible and
self-explanatory — the user can see the highlighted characters change — and the
copy already happened at release (§4.1), so nothing is silently mis-copied. The
alternative, anchoring a selection into scrollback history, needs a stable
line identity the engine does not expose and `ScrollbackPosition` cannot
express; it would be an engine change, which this slice is not.

**Rejected:** *selection that follows content as it scrolls*. See above: it
needs history line ids in the engine contract. Out of scope, and not needed —
the issue asks for *visible* text.

### 2.4 Which cells the viewport holds

Selection is clamped to the drawn viewport, which is the pane's content rect
less whatever footer that pane is currently showing: the two-row scrollback
footer while `^g [` is on, the two-row exit footer on a dead pane, the one-row
`↑ n lines below` marker on a detached preview. That measurement was inline in
`draw_pane`; it is now a function both `draw_pane` and the hit test call, so a
footer row can never be selected and a selectable row can never be a footer.

Dragging past the pane's edge clamps the head to the edge rather than ending the
gesture or spilling into the next pane — the divider's "keeps the pointer until
release" rule again. There is no drag-past-the-edge auto-scroll: the selection
is of visible cells, so there is nothing off-screen for it to reach.

## 3. Rendering (decision 4)

### 3.1 Inversion, not a colour

**Chosen: a selected cell swaps its foreground and background and drops the
`REVERSED` attribute it may have carried.**

`pin-terminal.md` §3 found the border vocabulary fully spoken for and answered
it by staying out of the border. The same finding applies to the palette:
`ACCENT` is the state the user put the deck in, `WARNING` is a held pane and an
attention notice, `DEMOTED_BG` is a just-demoted pane. A new selection colour
would collide with all three, and it would have to survive being drawn over
arbitrary child output whose own colours termdeck does not choose. Inversion is
the one highlight that is defined for every cell, whatever colour that cell
already has — and it is what every terminal on the user's machine already does
for a selection.

Dropping `REVERSED` is what makes it uniform: a cell the child had already
inverted (a status line, `less`'s prompt) would otherwise re-invert to look
*unselected*. The swap is applied to the resolved colours after `cell_style`, so
the rule is "the selection is the photographic negative of what is there",
with no exceptions to explain.

**Rejected:** *a background wash* (`DEMOTED_BG`, or a new selection colour) —
illegible against the many child backgrounds termdeck does not control, and it
disappears entirely on any cell the child already painted.
**Rejected:** *`Modifier::REVERSED` on top of the cell's own style* — it is not
idempotent over child output, which is the case above.

### 3.2 Nothing else changes

No border colour changes, no title changes, no status-row text is added while a
selection stands. The inverted cells are the whole of the feedback, and they
appear at the moment the copy happens (§4.1) and go a couple of seconds later
(§3.3), which is what makes them a copy receipt rather than a mode indicator.

**Rejected:** *a `copied n lines` status notice*. `DeckState::notice` has no
expiry — it is the surface for a refused command and is cleared by the next
command that succeeds — so a copy notice would sit in the status row until the
user typed a pane number. The highlight says the same thing in the place the
user is already looking.

### 3.3 How it clears

| event | why |
| --- | --- |
| ~2s after the copy, by itself (#150) | the receipt has been read; nothing is waiting on it |
| any mouse press | the next gesture starts; §1.1 |
| any key press | the user moved on, and typing changes what is under the highlight |
| any wheel tick | the content scrolls out from under the cell range |
| a canvas resize | the child reflows, so the cells mean something else |
| any deck command (`DeckState::apply`, close, fold, pin, page, split) | the pane may have moved, been renumbered, or changed size — including when it arrives from `termctl`, which drives the same methods |

The last row is why the invalidation lives in `DeckState` rather than in the
session loop: `termctl select`/`zoom`/`close` reach the deck without passing
through the key reader, and a selection left standing across a remote promotion
would be drawn over a different terminal's output.

The first row is #150. Every other row is something the *user* does, and a
receipt that waits for the user to do something else is not a receipt: with the
copy already on the clipboard, the inverted cells kept the pane reading as
still-selected for as long as it was left alone. So the release that copies
(§4.1) stamps the selection with the time it copied, and `DeckState::selection`
takes the `now` the frame is drawn at and answers `None` once
`SELECTION_WINDOW` (2s) has passed — the pattern the demotion highlight (1.5s)
and the scrollbar (#115, 4s) already use, which means the clock is injected and
every frame of the countdown is a fixture rather than a sleep. It is the
shortest of the three because nothing is still going on: the copy has landed,
and the highlight is only saying which cells went. A selection still being
dragged carries no stamp and does not expire — the gesture is not over.

`schedule_expiry_repaint` (#125/#132) gains the receipt as its third transient,
because a highlight that goes by the clock has no event behind it: without that
pass the frame that takes the inversion off the screen is never asked for.

The manual clears still come first and are unchanged: a press, key, wheel,
resize or command inside the window takes the highlight immediately. The
expiry is only the floor under them.

What does *not* expire is the copy itself. The clipboard write (§4.2) has
already left, and the session's own copy of the text — what `^g v` pastes
(§4.3) — is a separate field that no highlight lifetime touches, so the yank
works just as well after the pane has settled back.

## 4. The copy (decisions 5 and 6)

### 4.1 When (decision 5)

**Chosen: the release copies. The gesture that selects is the gesture that
copies; there is no second step.**

There is nothing else a standing selection could be *for* in termdeck — no
extend-with-shift, no context menu, no drag-and-drop. Making the user press a
key afterwards would add a binding whose only job is to finish a gesture they
have already finished. This is the X11 primary-selection convention, which is
what a terminal user's hands already expect.

A drag that ends with nothing but blank cells under it copies nothing and
leaves no highlight: an accidental twitch on a blank pane cannot clear the
clipboard. A release that does copy starts the highlight's own countdown
(§3.3): the copy is where the receipt begins, not where it ends.

### 4.2 Where (decision 6)

**Chosen: OSC 52 to the host terminal, which puts the text on the OS clipboard.
No new dependency.**

The text is base64-encoded and written to the outer terminal as
`ESC ] 52 ; c ; <base64> BEL` — about twenty lines beside `encode_paste`, tested
the way `session/backend.rs` tests its cell bytes, by asserting what is written
to a `Write`.

Why this and not a clipboard crate:

- **It works where termdeck runs.** termdeck is a terminal application, and the
  common case for a terminal application is a session over SSH. `arboard` talks
  to the local X11/Wayland/AppKit clipboard — over SSH, that is the *server's*
  clipboard, which is the wrong machine. OSC 52 travels the same channel the
  pixels do and lands on the user's real clipboard.
- **It adds no dependency.** `arboard` pulls in an X11 or Wayland stack, on a
  four-dependency project whose only platform crate is `libc`. `crossterm`'s
  `SetClipboard` is not an alternative provider at all — it emits the same OSC
  52 — and crossterm is not a dependency of this project (`ratatui` is used with
  `default-features = false`, and the session writes its own ANSI backend).
- **It spawns nothing.** `xclip`/`wl-copy`/`pbcopy` mean shelling out to a
  binary that may not exist, differs per platform, and fails silently when the
  session has no display.

The honest cost, stated because it is the reason §4.3 exists: OSC 52 is a
*request*, and some hosts refuse it. xterm needs `allowWindowOps` (or the
narrower `disallowedWindowOps` tweak), tmux needs `set-clipboard on`, and a few
terminals do not implement it. The write is fire-and-forget: there is no reply
to wait for, so termdeck cannot report success, and it must not block the event
loop pretending to.

**Rejected:** `arboard` — wrong machine over SSH, and a large dependency tree.
**Rejected:** `xclip`/`wl-copy`/`pbcopy` subprocesses — platform-specific,
absent on a bare server, silent on failure.
**Rejected:** `crossterm::SetClipboard` — the same bytes, behind a dependency
this project deliberately does not have.

### 4.3 The fallback: `^g v`

**Chosen: the session also keeps the copied text, and `^g v` pastes it into the
active terminal.**

This is what makes the feature verifiable and usable on a host that refuses
OSC 52: the text is demonstrably somewhere, and it can be got back out. It costs
one help row and no new mechanism — the paste goes through `encode_paste`
(#120), so it is bracketed for a child that holds DEC 2004 and raw bytes for one
that does not, exactly like every other paste in the session.

It is a session-lifetime buffer, one entry, replaced by each copy. There is no
kill ring and no history: this is the counterpart of one clipboard, not a
second clipboard.

### 4.4 What the text is

Row by row, from the selection's first cell to its last:

- a `Glyph` contributes its `text`, combining marks and all;
- a `Continuation` contributes nothing — the wide glyph to its left already
  contributed both columns' worth;
- an `Empty` cell contributes a space;
- each row is right-trimmed, and rows are joined with `\n`.

Right-trimming is what makes a selection of a short log line paste as that line
rather than as that line plus forty spaces. There is no trailing newline: the
selection is a span, not a set of lines, so appending one would submit a
command the user only meant to inspect.

## 5. Apps that own the mouse (decision 7)

**Chosen: selection does not yield. It works identically in an alternate-screen
app in mouse-reporting mode, because termdeck forwards no pointer events to any
child today — there is no protocol to yield to.**

The decoder says so in one line: "Mouse reports stay in the outer UI and never
leak into the active shell." The one exception is the wheel, which #74 routes
into an alternate-screen app as an encoded report or as cursor keys — and the
wheel is not this gesture. `TerminalMetadata::mouse_reporting` and
`mouse_protocol` exist for that path alone.

So `vim`, `claude`, `less` and `htop` all *believe* they have the mouse and none
of them has ever received a click through termdeck. Selection therefore adds
nothing to their situation: a drag over `htop` previously reordered panes and
now selects its text, which is strictly closer to what the user meant.

This is the decision to revisit if a later issue forwards clicks to children.
The seam is exactly one function — the session's `Mouse` arm — and the
resolution then is the one every multiplexer reaches: the app gets the plain
drag and selection moves behind a modifier, which is a decision to make when
there is an app on the other end of it, not before.

**Rejected:** *yielding now, on `mouse_reporting`* — it would make the gesture
unavailable in precisely the panes where output is hardest to get out by other
means, in exchange for feeding a protocol nothing is listening to.

## 6. Boundary

- `src/contracts/`: **unchanged**. The selection reads `TerminalFrame` through
  the frozen accessors and writes nothing.
- `src/engine/`: **unchanged**. No new command, no new event, no metadata field.
- `src/ui/`: the selection type and its invalidation (`state.rs`), the viewport
  hit test (`deck.rs`), the inversion (`chrome.rs`), one help row.
- `src/session/`: the gesture state machine and the two encoders
  (`input.rs`), the routing arm and the clipboard buffer (`session.rs`).
- `src/config/`, `src/cli/`, `src/ctl/`: **unchanged**. Nothing is configurable
  and nothing is written back, like the split ratio and the pin before it.

## 7. Verification

Deterministic and fixture-based, per the repository's pattern — no sleeps, no
wall clock, no host clipboard:

- the hit test answers content and refuses border, title, footer rows, the
  gutter and a collapsed strip, at the reference 144x42 canvas;
- a rendered canvas has exactly the selected cells inverted, and its
  neighbours untouched;
- an inverted child cell renders *un*-inverted when selected (§3.1);
- the range normalises in both drag directions, and multi-row text is built
  with full middle rows, right-trimmed, `\n`-joined, `Continuation` skipped;
- the gesture machine: press-and-release selects nothing and leaves the
  double-click promotion intact; press-and-move selects; a press on chrome
  never selects; the release copies once;
- the OSC 52 bytes are asserted against a `Write`, including the base64
  padding cases;
- `^g v` reaches the session as its own reaction, and the paste it hands to
  `encode_paste` comes out bracketed for a child that holds DEC 2004 and raw
  for one that does not;
- every deck command drops a standing selection — promotion, fold, split and
  close, which is the path `termctl` arrives by.
