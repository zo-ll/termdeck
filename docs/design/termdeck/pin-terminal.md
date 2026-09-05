# Pinning a terminal to the top of the stack — decisions (issue #113)

Status: implemented on `coord/113-pin`. UI only; no engine, contract,
configuration or CLI change. Runtime state, like the split ratio (#41): the
session holds it and writes nothing back.

The design export has no vocabulary for a pinned pane — stack order in every
screen is promotion order and nothing holds a slot. This document records what
was added, what it was derived from, and the ambiguities it resolves, in the
same form as `scrollable-stack.md` and `split-divider.md`.

## 0. What this is derived from

The export fixes the idioms the pin borrows, and nothing else:

- **FOCUS & PROMOTION**: "selecting a preview promotes it to master and the old
  master returns to the stack" — the rule the pin bends, in exactly one place.
- **BORDERS & SPACING** and the title chrome: `▾ ` / `▸ ` disclosure markers,
  `{n} {name} · {dot}`, the right-aligned slot, the `×` close mark.
- **PALETTE**: `ACCENT` is what the interface uses for the state the user put
  the deck in (master border, `+ add`, an active mode's key in the status row).
- The status row's precedent from #39/#101: **while a state is in play, the row
  advertises the key that undoes it** (`^g c collapse`, accented).

Everything below is marked **Chosen** where the export is silent, which is
everywhere the pin itself is concerned.

## 1. The model

**Chosen: the pin is a property of a terminal, not of a slot.** One terminal
may be pinned. Whenever the pinned terminal is not the master it stands at the
top of the preview stack, and every reordering re-establishes that.

`DeckState` gains one field, `pinned: Option<usize>` — a configured position,
the same currency `order`, `collapsed` and `demotion` already trade in — and
one private invariant:

```
after any reordering: if `pinned` is in the stack, it is stack()[0]
```

`promote` and `close` re-establish it by lifting the pinned position to
`order[1]` (remove-and-insert, so everything between it and the head shifts
down one and nothing else changes relative order). Pinning does the same thing
once. There is no second ordering rule: `order` is still the single source of
stack order, and `stack()` is still a slice of it, so every reader — the
renderer, the hit tests, the zoom census, the scroll window — is unchanged.

### 1.1 Pin versus promotion (decision 1)

**Chosen: a pinned terminal returns to the pin slot when it is demoted.**

The pin is a property of the terminal, so it holds while that terminal is
master; the master frame simply outranks the pin slot for as long as the
promotion lasts. The moment the terminal is demoted it is back at the top of
the stack — not in the slot the newly promoted pane vacated, which is where an
ordinary demotion lands it.

This is the only reading that serves the issue. The use case is a monitoring
pane the user promotes to read properly and then leaves again; if the pin were
spent by the promotion, the one gesture the pin exists to survive would be the
one that broke it.

Worked example, from a fresh four-terminal deck:

```
^g 4      order [3,1,2,0]   worker master, stack: backend app frontend
^g p      pinned = 3        worker master and pinned
^g 1      order [0,3,1,2]   frontend master, stack: worker backend app
                            ^ worker demoted straight into the pin slot,
                              not into slot 3 that frontend vacated
^g 2      order [1,3,0,2]   backend master, stack: worker frontend app
                            ^ the pin slot is untouched by promotions
                              around it — the whole of the issue
```

**Rejected:** *the pin is spent by promotion* (the pane returns to ordinary
order once it has held the frame) — it makes the pin a one-shot arrangement and
loses it on the gesture it is meant to survive. **Rejected:** *a pinned
terminal cannot be promoted at all* — promotion is how a terminal is read and
typed into in Termdeck; a pane that cannot take the frame is a pane the user
cannot use, and the issue asks to anchor a terminal, not to demote it to a
gauge.

### 1.2 One pin (decision 2)

**Chosen: exactly one terminal is pinned at a time.** `^g p` on a second
terminal moves the pin; there is no pin order, because there is no second pin.

- The indicator is then unambiguous: the pinned pane is *the* top of the stack,
  and every reader that wants to know "is this the pin slot" asks one question.
- Two pins reintroduce, one level down, the ordering question the pin exists to
  answer — which of the two is on top, what happens when the lower one is
  promoted, whether the census names both.
- It matches the deck's other singular runtime states: one master, one demotion
  highlight, one drag, one notice.

**Rejected:** *many pins held in pin-insertion order* — a pinned band at the top
of the stack. It is a strictly larger model (`Vec<usize>`, a rule for demotion
into the middle of the band, a census that has to count) for a use case nobody
has yet stated: the issue names a terminal, singular. If a second pin is ever
asked for, the field widens to a `Vec` and §1's invariant becomes "the pinned
positions are `stack()[..n]`, in pin order" — nothing in the rendering or the
input layer would have to move.

### 1.3 What the pin does not do

- **It does not open a folded pane.** Collapse is per terminal and survives
  promotion (`collapse-stack.md`); the pin is order, not disclosure, so a pinned
  preview can be a folded strip like any other and wears its mark there.
- **It does not hold the scroll window.** The column is a window over whole
  previews (`scrollable-stack.md` §1); paging past the pinned pane is allowed,
  exactly as the window does not chase a promotion (that document's B5). The
  pin fixes a pane's place in the *list*; the window is still the user's. In
  practice the pinned pane is at offset 0, so it is on screen until the user
  deliberately pages away from it.
- **It does not change promotion, zoom or collapse semantics** — it only says
  where a demoted pane lands.

## 2. The gesture

**Chosen: `^g p` toggles the pin on the terminal holding the master frame.**

- A new verb rather than an extension of promote or cycle. `^g j/k` and `^g N`
  are promotion, and promotion is selection in Termdeck; a modifier on them
  would make the same key mean two things depending on state, which is the
  reason `^g pgup/pgdn` exists as its own verb rather than as a mode of
  `^g j/k` (`scrollable-stack.md` §2).
- `p` is free under the prefix, is the first letter of what it does, and is
  ordinary input unprefixed, so the shell still sees it.
- Like `^g c` and `^g -` / `^g =` this is the deck's own arrangement: it needs
  no engine and no process, so it carries **no `ActionCommand`** and no contract
  changes (decision 5). `Input::press` applies it to `DeckState` and returns
  `None`.

### 2.1 Why it acts on the master, and what unpinning costs

Every prefixed command that acts on a pane acts on the pane in the master
frame: `^g x` closes it, `^g r` respawns it, `^g [` scrolls it, `^g c` is
explicitly "the master is always the selected pane". The pin follows the rule
rather than inventing a second target vocabulary, so:

- **Pinning** is `^g p` on whatever you are looking at — usually exactly when
  you have just finished reading the pane you want to keep at the top.
- **Unpinning** is `^g p` on the pinned pane, which means promoting it first if
  it is not already the master (`^g N` — its number is in its own title, and it
  is always the top of the stack). Two keys, and the pane you are unpinning is
  in front of you while you do it.
- While the master **is** the pinned pane, the status row advertises
  `^g p unpin` in accent — the `^g c collapse` precedent, and the discoverable
  explicit unpin the issue asks for.

**Rejected:** *a second binding that clears the pin from anywhere* (`^g P`) — a
whole verb for the rare half of a toggle, and the only command in the interface
that acts on a pane the user is not looking at. **Rejected:** *`^g p N`, pin by
number* — the number capture (`^g 1 6`, with its 600 ms window) belongs to
promotion; a second numeric gesture would double it for no reach that
`^g N` + `^g p` does not already have. **Rejected:** *a click target on the
mark* — the mouse-first epic's parity rule (`split-divider.md`) says anything
the pointer can reach the keyboard can, and the other way round; the pointer
offers no pin gesture here, so parity holds trivially. A clickable mark is a
follow-up (§5), not a hole.

## 3. The indicator

**Chosen: `↑` in `ACCENT`, leading the title row of whichever pane holds the
pinned terminal**, after the disclosure marker where there is one.

The four places a terminal is drawn, with terminal 4 pinned:

```
open preview   ┌─ ▾ ↑ 4 worker · ○ ───── idle 6m × ─┐   (marker, then mark)
folded strip   ▸ ↑ 4 worker · ○ · idle 6m         ×
master         ┌─ ↑ > 4 worker  ·  ●  ·  uv run worker ─
narrow chip     ↑ 4 worker
zoom census    hidden: ↑4○ 2● 3✕
```

- It costs two columns and survives the minimum stack width (22 columns at
  `MAX_MASTER_RATIO`), where the strip has already given up its tail. Nothing
  else in the title row is given up for it: the name absorbs the two columns
  through the truncation rule the title already applies.
- It never contends with the border vocabulary, which is fully spoken for —
  idle, demoted, notify warning, drag warning/accent, master accent. The pin is
  ink on a glyph, so a pinned pane can be demoted, flashing or held in a drag
  and still say it is pinned.
- The disclosure marker keeps its two cells at the head of the row, so the fold
  affordance and its hit test (`marker_at`) are untouched, and `▾` stays in the
  same column down the stack whether a pane is pinned or not.
- `↑` says the one thing the pin means — *held at the top*. The arrow is the
  stack column's own idiom for "up the list" (the footer's `↑ 2 more`), used
  here in accent in the title rather than in hint in the footer, and it is on
  the export's safe glyph list.

**Rejected:** *a border colour* — the palette's border slots are taken, and a
fifth meaning would be read as a mode. **Rejected:** *a ` PIN ` chip in the
right slot*, like ` ZOOM ` and ` SCROLL ` — those tags mark modes that end,
and the slot they borrow holds the activity meter; the pin is permanent, so it
would permanently cost every pinned pane its meter and its idle age.
**Rejected:** *the word `pin` in the title tail* — six columns, which the
minimum stack width does not have, so the name would clip hard at exactly the
width the affordance matters most. **Rejected:** *marking only the master's
status-row entry* — the mark has to be on the pane, or the state is invisible
in precisely the arrangement the pin creates.

### 3.1 The status row

Two additions, both conditional, so a deck with nothing pinned renders byte for
byte as it did before:

1. `^g p unpin`, accented, in the key row **while the master is the pinned
   pane** — the key that undoes the state that is in play, per `^g c`. The
   narrow fallback's collapsed key row names no state keys at all, `^g c`
   included, so it names this one no more than it names that one.
2. Nothing else. The pinned pane wears its own mark in the stack, in the narrow
   chip strip and in the zoom census, so a pin census in the status row would
   state a third time what the frame already says twice.

## 4. Where it applies (decision 6)

| Mode | Behaviour |
| --- | --- |
| Stacked | The pinned terminal is `stack()[0]`; promotions around it never move it. |
| Zoom | The stack is hidden, so the pin is inert but not lost: `hidden: ↑2● 3●` keeps it stated, and unzooming finds the order unchanged. |
| Narrow (master-only) | Same: no stack to hold a slot, the chip carries the mark, and the order is waiting when the width comes back. |
| Scrollback | A mode of the master pane only. Pinning is still the master's own arrangement, so `^g p` works and the mode is untouched. |
| Collapse | Independent (§1.3). A pinned preview folds and unfolds like any other and keeps its slot folded. |
| Stack paging | The window may scroll past the pinned pane (§1.3); the pin is order, the window is geometry. |
| Close | Closing the pinned terminal clears the pin — a pin is a property of a terminal, and that terminal is gone. Closing anything else renumbers the pin with everything else (#84). |
| Runtime add | A new terminal lands at the end of the stack (#50); the pin slot is untouched. Pinning is allowed with an empty stack — the pin is a property of a terminal, and it takes effect as soon as there is a stack to hold. |

## 5. Ambiguities

| # | Ambiguity | Resolution |
| --- | --- | --- |
| P1 | The export never draws a held slot. | The pin is a property of a terminal; the slot is where that terminal stands when it is not master (§1). |
| P2 | Whether promotion spends the pin. | It does not: demotion returns the pinned pane to the pin slot (§1.1). |
| P3 | How many pins. | One (§1.2). The invariant generalises to a band if that is ever asked for. |
| P4 | Where the mark goes when the pinned pane is the master. | It leads the master's title too, ahead of the `>` caret. The state belongs to the terminal, so it is stated wherever that terminal is drawn. |
| P5 | Whether the pin holds the scroll window. | No (§1.3): the window is a window, per `scrollable-stack.md` B5. |
| P6 | Whether the pin is written to configuration. | No — runtime only, per `split-divider.md` §4 and PLAN.md's rule for the split ratio. |

## 6. Known follow-ups

- **A pointer half.** A click on the pin mark could toggle it, which would put
  unpinning one gesture away wherever the pane is drawn. It needs a hit test of
  its own (the two cells the mark owns, ahead of drag and double-click, the way
  `marker_at` and `close_at` already work) and belongs with the next pass over
  the pointer, not to this slice.
- **`docs/PLAN.md`'s binding table** does not list `^g p`; that file is
  coordinator-owned, so this slice states the binding in the help overlay and
  here instead.
- **A second pin**, if a workspace ever wants a band rather than a slot (§1.2).
