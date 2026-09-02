# The split divider — design note (issue #41, slice S1)

Status: implemented. This is the first slice of the mouse-first epic, whose
standing rule is **parity**: anything the pointer can reach, the keyboard can
reach, and the other way round.

## 0. What this is derived from

Nothing. The design export has no divider, no splitter and no resize gesture:
screens 01–05 all draw the same 98/2/44 split at 144 columns and the export
calls `master_ratio` a configuration value. So this note states an invention,
constrained by what the export *does* fix — the geometry of the gutter, the
palette, and the way an active gesture states itself — rather than derived
from it. Every choice below is marked **Chosen** where the export is silent,
which is everywhere.

## 1. Geometry

The gutter is the two columns between the master and the stack (`GUTTER = 2`),
at 144 columns those are 98 and 99. It already carries one thing: the stack's
scroll track, drawn in the column beside the stack (#34b).

**Chosen:** the divider takes the *other* gutter column — the one beside the
master, column 98 at the reference size.

- It costs neither pane a column, so the master's 98 and the stack's 44 are
  untouched at the default ratio and every existing fixture changes by exactly
  one column.
- The two pieces of gutter chrome never overlap: divider beside the master,
  scroll track beside the stack.
- The divider is drawn for the full body height (rows 0–39), stopping before
  the blank row and the status row.

```
 master … ┐│┃│  ▸ 2 backend …
          ^^ divider (98) and scroll track (99)
```

### Where the divider is, given a ratio

The layout already fixes it: `master = width - GUTTER - stack_width(width,
ratio)`, and the divider is the first gutter column, so its column *is* the
master's width. The drag needs the inverse — the ratio that puts the divider
under column `c`:

```
ratio_at(c) = (c + GUTTER) / width
```

`stack_width` takes the ceiling of the stack's share, and that division's last
bit can land a hair above a whole column, which would round the split one
column past where the pointer dropped it. The ceiling therefore ignores a
final 1e-9 (`src/ui/mod.rs`, `stack_width`). Without it, dragging to column 85
at 144 columns lands on 84 — found by driving the real binary, not by a test.

## 2. The gesture pair

| | Pointer | Keyboard |
| --- | --- | --- |
| Move the split | Press the divider, drag, release | `^g -` narrows the master, `^g =` widens it (`_` and `+` are the same keys) |
| Granularity | Any column in range | `MASTER_RATIO_STEP` = 0.05, six steps across the range |
| Feedback | The divider takes the accent while held | The divider redraws in its new column |

The keys snap to the 0.05 grid rather than adding to whatever the pointer left
behind, so they always reach the same six splits however the pointer got
there. That is what makes the pair equivalent rather than merely similar: a
drag can reach more splits than the keys, but every split the keys reach is
one the pointer can reach, and both go through the same `DeckState` method.

**Chosen:** `-` / `=` rather than the epic's suggested `[` / `]`, because `^g [`
is already scrollback. They are unbound only under the prefix; unprefixed they
are ordinary input and still reach the shell.

## 3. Bounds

`MIN_MASTER_RATIO = 0.55`, `MAX_MASTER_RATIO = 0.85` — deliberately the same
range `defaults.master_ratio` accepts in the configuration, so:

- the live range needs no validator change, and
- a split reached by dragging is always a split the configuration file would
  also have accepted.

Both gestures clamp; a drag past either end stops at it. At 144 columns that
is column 77 (0.55) to column 120 (0.85).

The interface holds its own copy of the range (`src/ui/state.rs`) rather than
importing the configuration's, because the architecture boundary keeps
`src/config` out of `src/ui`. The two copies — both ends of the range and the
0.70 default — are held equal by a cross-check in the configuration's own
tests (`the_interfaces_split_range_is_the_one_this_file_validates`), which
fails if either side moves. Sharing one constant from `src/contracts` would
retire the duplication outright, and belongs to the owner of contracts.

## 4. Persistence

**Per session only.** The split lives in `DeckState`, seeded from the
configuration at startup and dropped when the session ends. Nothing is written
back to the configuration file: writing YAML the user hand-maintains is not
trivial (comments, formatting, the `--config` path, multiple workspaces), and
the epic asked for it only if it were. A user who wants a different default
still edits `defaults.master_ratio`.

## 5. Where it does not apply

- **Zoom** hides the stack, so there is no split to move and no divider drawn.
- **The narrow fallback** has no stack either.
- **Below `WIDE_COLUMNS` (120)** the export fixes the stack at `COMPACT_STACK`
  columns, so the ratio says nothing. The divider is neither drawn nor
  draggable there rather than drawn-but-inert: an affordance that cannot do
  what it offers is worse than none.

## 6. Interplay with the gestures that were already there

The divider sits in the gutter, which belongs to no pane, so `position_at`,
`swap_position_at` and `marker_at` all answer `None` there and the pointer can
never confuse a resize with a pane drag-swap, a double-click promotion or a
marker click. Once the divider is held it keeps the pointer until release,
wherever the pointer travels, and a key or a wheel event releases it.

The wheel over the gutter still pages the preview list (#34b): the wheel and
the drag are different gestures over the same chrome, so they do not contend.
