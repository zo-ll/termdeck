# Scrollable preview stack — decisions (issue #34b)

Status: implemented on `coord/34b-scrollable-stack`. UI only; no engine,
contract or configuration change. #34a lifts the 1–4 terminal cap separately,
so until it merges the stack lengths described here are reachable only from
synthetic decks in the unit tests.

The design export has no vocabulary for a preview list longer than the column —
it never draws more than three previews. This document records what was added,
what it was derived from, and the ambiguities it resolves, in the same form as
`collapse-stack.md`.

## 1. The model

The stack column is a **window onto the preview list**, not a viewport onto a
tall surface. It holds whole previews at the heights `collapse-stack.md` §1.2
fixes, and scrolls by whole previews. Nothing is ever half-drawn, so every rule
that already keyed off a preview's rect — hit testing, drag, the disclosure
markers, the strip — is unchanged in kind.

`DeckState` gains one field: `stack_offset`, the index into `stack()` of the
first preview drawn. Everything else about the window is geometry, recomputed
each render by `Deck::stack_window_of`:

```
budget  B = stack_rows - 1                  # the footer keeps the last row
cost(p)   = (collapsed ? 1 : base) + 1      # the blank row travels with it
visible   = previews from `offset` while they fit in B
limit     = len - (previews that fit filling B backwards from the last)
offset    = stored offset, clamped to `limit`
```

`limit` is what stops the column scrolling into empty space: the last window
always ends on the last preview.

### Freed rows, when the list is longer than the column

`collapse-stack.md` §1.3 hands a fold's rows to the previews still open. That
rule assumed the whole list fits, where it is exactly conservative — the stack's
used height never changes. Once the list can be longer, a fold has *already*
bought something else: room for a further preview in the window. So the rule
becomes

```
freed = min(folds x (base - 1), B - used)
```

which is identical to §1.3 whenever the list fits (the slack is then at least
the freed rows), and hands out only what the window has left over when it does
not. Growth is still caused only by collapse — a short list does not grow to
fill the column — and the §1.3 invariant test still passes unchanged.

## 2. The paging gesture

**Keyboard: `^g pgup` / `^g pgdn` page the list by one window.**

- The prefix is free there: unprefixed `pgup`/`pgdn` already belong to the shell
  and, in scrollback mode, to the master's viewport, and neither is reachable
  after `^g`.
- `^g j/k` is promotion and stays promotion. Promotion is selection in Termdeck,
  and there is no preview cursor to move with `j/k`; conflating the two would
  make `^g j` mean two things depending on where the window sits.
- Like `^g c`, this is the deck's own geometry, so it adds no `ActionCommand`.
  Unlike `^g c` it cannot apply itself — one page is however many previews the
  column drew — so it comes back as `Reaction::PageStack(±1)` and the session,
  which holds the rendered size, applies it through `Deck::stack_window`.

**Mouse: the wheel over the stack's own chrome scrolls the list by one preview.**

That chrome is the gutter beside the column, the blank rows between previews,
the empty column below them, and the footer row — everything in the stack
region that is not a preview. Over a preview the wheel stays that preview's
viewport, exactly as #25 and #31 left it, so the two gestures never contend for
the same cell. A collapsed strip is a preview for this purpose and remains the
no-op `collapse-stack.md` A10 made it.

**Rejected:** wheel-over-any-preview paging the list (it would take the
per-preview scroll #25 shipped and #31 fixed); a modal "stack navigation" mode
(a fifth mode for a two-key job); a preview cursor with `j/k` (Termdeck has no
selection separate from promotion); drag on the scrollbar (a 1-column drag
target, and the wheel already covers the region).

## 3. The indicators

Both appear **only while the list is longer than the window**, so a stack that
fits renders byte for byte as screens 01–05 do.

- **A track in the gutter.** One column, at the right of the two-column gutter,
  running the height of the stack less its footer row: `│` in `IDLE_BORDER`
  with a `┃` thumb in `HINT`. Its length and position are the window's share of
  the list, and it is anchored to whichever end the window has reached so
  "nothing further down" is never a rounding question. It sits in space the
  export leaves empty, so it costs the previews no columns.
- **The footer states the counts.** `↑ 2 more · ↓ 3 more · ^g pgup/pgdn`, naming
  only the end that has something behind it. The keys are dropped first when the
  column is too narrow for them, which is the status bar's own rule.

Footer precedence (extending `collapse-stack.md` §3.6), most specific first:

1. scrollback — `scrollback · esc returns to live`
2. demotion, for its 1.5s window — `promoted backend · ^g 1 back`
3. hidden previews — `↑ 2 more · ↓ 3 more · ^g pgup/pgdn`
4. folds — `{c} collapsed · ^g c expand all`
5. default — `ctrl+g 1-4 promote · j/k cycle`

**Chosen, not in the export:** hidden outranks folded. A fold declares itself
three times over — its own strip, its marker, and the status row's census —
while a preview the window has scrolled past says nothing about itself anywhere
else. The status row is unchanged, so the fold census is still stated while the
footer is naming the window.

## 4. Ambiguities

| # | Ambiguity | Resolution |
| --- | --- | --- |
| B1 | The export never draws a list longer than the column. | The column is a window over whole previews; it pages, never part-draws. |
| B2 | §1.3 redistribution assumes the list fits. | `min(freed, slack)` — identical when it fits, bounded by the window when it does not. |
| B3 | The export has no scrollbar or overflow vocabulary. | Gutter track plus a footer count, both drawn only on overflow, both from existing tokens. |
| B4 | Wheel over the stack could mean two things. | Preview → that preview (#25). Stack chrome → the list. Strip → nothing (A10). |
| B5 | Promotion can demote a pane into a slot the window is not showing. | The window does not chase it. Promotion is announced by the footer and the status row, and `^g 1-9` reaches any preview whether or not it is drawn. |

## 5. Known follow-ups

- The status bar still advertises `^g 1-4 select` and the stack footer
  `ctrl+g 1-4 promote`, which is the export's literal text and correct for
  every deck the configuration can build today. With #34a merged and more than
  four terminals configured, those labels want the deck's own count.
- Auto-collapse at the `MIN_OPEN` floor (`collapse-stack.md` §1.4, R3) is still
  not implemented. It is now less pressing: a column too short for its previews
  scrolls rather than dropping them.
