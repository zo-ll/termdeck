# The repository picker — design note (issue #42, slice A2)

Status: design only. No production code, no engine or contract changes. This
note is what slices A2–A4 build against.

## 0. Source of truth and how to read this document

Everything below is derived from the updated design export at
`reference/Termdeck TUI.dc.html` (synced in `63c5b4c`, verified byte-identical
to the live Claude Design project on import), specifically:

- **Screen 06 — "Repository picker — `termdeck` with no path"**: browse left,
  selection right, on the same 98/2/44 split as the session.
- **Screen 07 — "Filter — `/hor`"**, with its three edge-state cards: zero
  results, empty folder, and the root list with nothing selected.
- **Screen 08 — "Add at runtime — `ctrl+g a`"**: the same picker language
  reduced to a centred sheet over the live session.
- The spec boards **PICKER — LAYOUT & GLYPHS**, **SELECTION STATES**,
  **FILTER & EMPTY STATES**, **RUNTIME ADD — ^g a**, and **KEY MAP — MOUSE
  PARITY**.

The update is purely additive: screens 01–05, the narrow fallback, the
vertical-collapse board and every pre-existing spec board are byte-identical to
the export the session was built from, so nothing already implemented is
contradicted here.

Where the export is silent or contradicts itself this document says so under
**Ambiguity** and states the reading it picks. Nothing here is invented beyond
those marked points.

### The one requirement the export does not show

A user requirement arrived after the export: **the same project path may be
opened as two or more terminals** (`fe-a` and `fe-a-2` in the same directory).
The export draws no such thing — its selection box holds one ordinal per row
and its runtime sheet locks a repo that is already open. §3.1, the `[·]`
relaxation in §6, and the `+` / `-` rows in §7 are therefore **Chosen**, not
derived, and are marked as such where they appear.

The seam below it is already real, which is why this is a surface question
only. Verified against the build on this branch: a workspace with two
terminals sharing one `cwd` and distinct names passes `termdeck check`, lists
as two terminals, and runs as two panes over two separate shells. Nothing in
the engine, the contracts or the configuration has to move for the picker to
offer it.

### Where this fits

A1 (#48) has landed the entry points: no path resolves to `CliCommand::Picker`,
which currently returns `"folder picker pending A2"`. That error is the seam
this design fills. `<folder>` and `--config` already resolve without the
picker, so the picker is reached only when the user names nothing.

---

## 1. Geometry

### 1.1 The frame

The picker uses the session's frame exactly — that is the point of it, per the
spec board: *"Same 98 / 2 / 44 split as the session, so the picker teaches the
layout before the workspace exists."*

| Region | Extent (at 144×42) |
| --- | --- |
| Top bar | row 0, `STATUS_BG` |
| Blank | row 1 |
| Panels | rows 2–39 (38 rows) |
| Blank | row 40 |
| Bottom bar | row 41, `STATUS_BG` |
| Browse panel | columns 0–97 (98) |
| Gutter | columns 98–99 (2) |
| Selection panel | columns 100–143 (44) |

Both panels are bordered boxes with 2-column inner padding, titles inset in the
top border the way a pane's title is. The **focused** panel takes the accent
border (`ACCENT`); the other takes `IDLE_BORDER`. In every drawn screen the
browse panel is focused — the selection panel is a display, not a cursor
target, and the keys that act on it (`K/J`, `m`, `x`, `X`) work from the browse
panel without moving focus there.

**Ambiguity.** The export never draws the selection panel focused, and offers
no key that would focus it. **Chosen:** there is no focus switch. One cursor,
always in the listing; the selection panel is operated at a distance. `⇥`
toggles that cursor row, alongside the row's checkbox.

**Ambiguity.** 98/2/44 is `master_ratio` 0.70, but since #44 a *session* starts
at 0.85 (the stack at its minimum width). **Chosen:** the picker's split is its
own fixed geometry, not a `master_ratio` reading — it is not a master and a
stack, it is a browser and a basket, and the export draws it at 98/44
regardless of what a later session will open at. The divider (#41) is a session
affordance and is **not** drawn in the picker: there is no stack to widen and
no terminal to give the columns to.

### 1.2 The listing grid

Every listing row in the browse panel, in columns relative to the panel's inner
left edge:

```
col 0–2   checkbox: [ ]  [x]   (three blanks for `..` and a plain file)
col 3     blank
col 4     kind glyph: ◆ ▸ ▴ ·
col 5–6   blank
col 7     name, 26 columns (7–32)
col 33    ·  separator (SEPARATOR)
col 35    meta: "git · <branch>" | "N items" | "N items · no repos"
col 55    tail: relative time, repos only
```

Measured from screen 06, and the same grid holds in screen 07's filtered
listing. The runtime-add sheet uses the same grid with the separator at **col
30** instead of 33, because the sheet is 78 columns wide rather than 98 — the
name field is what gives up the difference (§6).

### 1.3 The detail block

The cursor row's detail is printed at the **bottom of the listing**, not as a
popup: a rule reading `─── cursor on <name> ───────`, then the absolute path,
then one line of facts (`package.json · expo 51 · last commit 4d ago by asha`).
Three lines, indented to col 3, separated from the listing by one blank row.

**Ambiguity.** The fact line's contents are illustrative — the export shows a
manifest name, an ecosystem version, and the last commit's age and author,
which is a lot of filesystem and git work for one row. **Chosen:** the path
line is required; the fact line is best-effort and degrades to whatever is
cheap to know (last commit age and author from git, manifest name if one of a
small known set is present). It must never block the cursor moving.

---

## 2. The listing

### 2.1 Glyph vocabulary

| Glyph | Meaning | Colour |
| --- | --- | --- |
| `◆` | git repository | `ACCENT` |
| `▸` | folder | `WARNING` (amber) |
| `▴` | parent (`..`) | `HINT` |
| `·` | plain file — dimmed, **not selectable** | `HINT` glyph, `MUTED` name |

Every colour the picker uses is already in the palette: `ACCENT`, `WARNING`,
`MASTER_FG` (names), `PREVIEW_FG`, `MUTED` (meta and tails), `HINT`
(unselected boxes, key labels, rules), `SEPARATOR` (dot separators),
`CANVAS`/`STATUS_BG`/`IDLE_BORDER` for the frame. **No new colour is
introduced**, and the amber that marks a folder is the same amber that marks a
dirty count and the narrow fallback's `stack hidden`.

### 2.2 Meta fields

- **Repos** carry `git · <branch>`, a dirty count appended in `WARNING` when
  non-zero (`git · main +3`), and a relative time in the tail column.
- **Folders** carry `N items`, and `N items · no repos` when they contain none
  — the export states that case explicitly, so a folder worth entering is
  distinguishable from one that is not before entering it.
- **Files** carry nothing.
- A repo selected **more than once** carries an instance badge `×2`, `×3` …
  in `ACCENT`, right-aligned in the name field at cols 30–31 (29–31 once the
  count reaches two digits), keeping col 32 blank so the badge never touches
  the separator. It is a selection fact, so it takes the selection's colour and
  sits with the name rather than in the meta the filesystem owns. A name long
  enough to reach the badge is truncated by it, the way the field already
  truncates at col 32.

**Ambiguity.** The tail's relative time is unlabelled. In screen 06 the cursor
row's detail says `last commit 4d ago` for the same repo whose tail says
`4d ago`. **Chosen:** the tail is the last commit's age for a repo. A folder
has no tail, which sidesteps the question of what mtime would even mean for a
directory.

### 2.3 Order

`..` first, then folders and repositories interleaved, then plain files last.

**Ambiguity.** Screen 06 lists `archive/`, `horizon-frontend`,
`horizon-backend`, `horizon-app`, `horizon-infra`, `notes/`, `termdeck`,
`vendor/` — alphabetical *except* the four `horizon-*` repos, which are in
frontend/backend/app/infra order. That is the reading order of the workspace
the export is about, not a sort. **Chosen:** case-insensitive alphabetical
across folders and repos together, files last. The export's `horizon-*` run is
illustrative; sorting them by selection order would make the list jump under
the cursor as the user selects, which nothing in the export suggests.

---

## 3. The selection model

The selection panel's ordinal is the pane number; the listing keeps its
per-row checkbox focused on whether the path is selected.

```
[ ]   unselected
[x]   selected — its pane position is shown in the selection panel
[+]   marked to append (runtime add only)
[·]   already open, not selectable (runtime add only)
```

Rules, all stated on the spec boards:

- **Selection order is pane order.** The first repo selected becomes pane 1,
  which is the master. The right-hand panel lists the selection in that order,
  tagging pane 1 `MASTER`.
- **Deselecting renumbers immediately.** No gaps, ever.
- **`m` promotes the cursor row to master** — it becomes 1 and everything above
  it shifts down.
- **`K`/`J` reorder within the selection panel**, operated from the listing.
- **`x` removes the cursor row from the selection, `X` clears it.**
- **`a` selects every repo in the current folder; `A` recurses the whole root.**
- **Selection is workspace-wide**, not per-folder: it survives navigation,
  filtering and root switches. Screen 07 states this in the panel itself
  ("selection survives filtering and navigation — it is workspace-wide, not
  per-folder"), which makes it a promise to the user, not just an
  implementation note.
- **With nothing selected the launch button renders `disabled`.** (Since the
  key map was revised, `⏎` on a folder selects it rather than navigating, and
  `→` is what goes inside — §7.)

### 3.1 More than one terminal for the same path

**Chosen** (§0): a path may be selected repeatedly. Each selection is an
**instance** with its own pane number and its own terminal name, and the
selection panel lists instances rather than unique repos — which is what keeps
the first-selected-is-master rule and the ordering coherent when a path appears
twice.

```
[x] ◆  horizon-frontend       ×2 · git · main          2h ago
```

- **The checkbox states whether the path has any instance.** The selection
  panel is the authority for order; the badge states how many instances share
  that path.
- **`+` adds another instance of the cursor row; `-` removes the most recent
  one.** The new instance appends at the end of the order, exactly as a fresh
  selection would — `+` is "another one of these", never a reordering.
- **`space` and `x` address the whole path**: toggling off or removing a repo
  drops every instance of it at once, and the remaining panes renumber. `-` is
  the key for shedding one.
- **`m` promotes that path's first instance**; inside the selection panel `m`,
  `K` and `J` address instances individually, because that panel is the
  instance list.
- **`a` and `A` stay idempotent.** Selecting every repo in a folder leaves a
  repo that is already selected at whatever count it has: a bulk key must not
  multiply what a deliberate one built.
- **Every count is an instance count.** `selected · 4`, `o Open 4 as
  terminals`, and `selection kept (4)` all count panes, not distinct repos.
  The listing's own `9 items · 5 repos` still counts what is on disk.

#### Names

The picker generates them, because it is the only party that knows both the
instance ordinal and what is already taken. The first instance keeps the name
the folder entry point would derive; the second takes `-2`, the third `-3`,
and a suffix already spoken for — a sibling repo genuinely called `foo-2`, or
an open terminal of that name in the runtime sheet — is skipped rather than
duplicated. Names are what the engine keys a terminal by, so they must come out
of the picker unique or the workspace is invalid.

The selection panel shows them, so a doubled path reads as two rows that differ
by exactly the thing that distinguishes the panes:

```
1 horizon-frontend    MASTER
  ~/code/horizon-frontend
2 horizon-backend
  ~/code/horizon-backend
3 horizon-frontend-2
  ~/code/horizon-frontend
```

The selection panel also carries the **workspace name** — a chip in accent,
`e` to rename — and the launch button `o  Open N as terminals`, both pinned to
the bottom of the panel. (The export drew `⏎` on that button; §7's revision
moved launch to `o`, and everything that names the key follows it.)

**Ambiguity.** The export shows the name `idp` for a selection of `horizon-*`
repos under `~/code`, so the default name is derivable from neither the root
nor the repos. **Chosen:** default to the common parent folder's name, and let
`e` rename. It has to have *some* default, because the launch button is
reachable without ever visiting the field.

**Ambiguity.** Nothing says what the picker does with a selected repo's command
or scrollback. **Chosen:** the workspace it builds uses the configuration's
`defaults`, exactly as `discover_workspace` (A1) already does for the
`<folder>` entry point. The picker chooses *which* directories become panes and
in what order; everything else about a terminal stays configuration's business.

---

## 4. Filter

`/` opens a query line **pinned to the bottom of the listing — never a modal**:

```
   /hor                                              esc clear · ⏎ select
```

- Matching substrings highlight in `ACCENT` inside the row's name
  (`hor` teal, `izon-frontend` in the ordinary name colour).
- The listing header becomes `MATCH <query> · <n> of <total> · recursive from
  <root>`, so the count states both how much matched and what was searched.
- **Matches from other configured roots appear below a labelled rule**
  (`─── also in other roots ───`), each carrying its root in the meta column
  instead of its branch. A filter is therefore a *workspace-wide search*, not a
  folder one — the strongest claim on this board, and the one that decides the
  implementation: the query has to reach every configured root, not just the
  current listing.
- The top bar reflects the mode (`filtering · 3 selected`), and the bottom bar
  swaps to filter keys (`type to narrow · ↑↓ move · ⏎ select · +/- instance ·
  esc clear filter`, with `o open · esc esc quit` on the right). The export
  wrote that row before the key map was revised; §7 is what it says now.

`esc` clears the filter; a second `esc` quits — the export spells out the
double press, so a filtered picker never quits on the first `esc`.

---

## 5. Empty and edge states

Each states the condition in one line plus the key that escapes it, and **none
of them clears the selection**:

| State | Listing says | Escape |
| --- | --- | --- |
| Zero results | `MATCH zzq · 0 of 9`, `no match for zzq`, `in ~/code or 3 other roots` | `⌫ edit · esc clear`, and `selection kept (3)` is printed |
| Empty folder | the path, `..`, `empty folder` | `h go up · ~ home` |
| At root, nothing selected | `ROOTS` and the configured roots as folder rows with repo counts (`~/code · 5 repos`) | `nothing selected · o disabled` |
| **Unreadable folder** (added in A2) | `cannot read ~/code/secret` in `ERROR`, then the reason the filesystem gave | `h go up · ~ home` |

The root list is the picker's own top level: the configured roots are drawn as
folder rows, so `h` from a root lands somewhere legible rather than at `/`.

**Chosen** (not in the export): a folder the picker cannot read states why,
where its rows would have been. The export has no such card because it never
shows a failure, but the alternative — letting a permission error fall through
to the empty state — would have the picker tell the user their folder holds
nothing, which is a lie about the filesystem and the one thing a browser must
never say. Both states keep the `..` row, because a folder you cannot read is
one you especially need a way out of.

**Ambiguity.** Where configured roots come from is never stated — the top bar
says `4 roots configured`. **Chosen:** configuration, a `roots:` list beside
`workspaces:`, defaulting to the user's home when unset. This is
configuration's business (Codex's area), so A2 consumes a resolved list and
does not decide the schema.

---

## 6. Runtime add — `^g a`

A **78-column sheet centred over the dimmed session** (the export dims the
session to 34% and keeps it live). It is the picker's language with exactly
three differences, per the spec board:

1. Repos already open are listed but **locked** for ordinary selection: `[·]`,
   meta `already open · pane 2`. **Chosen** (§0): the lock is what stops
   `space` from re-adding a pane by accident, not a rule that a project may
   only be open once — `+` on a locked row marks it `[+]` and appends *another
   instance* of it, meta `another instance · appends as pane 5`. A repo open
   more than once states every pane it holds: `already open · panes 1, 5`.
2. Marks are `[+]`, because they **append** rather than order — the footer says
   `appends as pane 5`.
3. **The master never changes**: `esc cancel · master unchanged` is printed on
   the sheet itself.

`⇧⇥` cycles configured roots in place (the sheet has no room for a browse
crumb), the sheet header reads `ROOT ~/work · 12 repos · 4 already open`, and
the filter line works exactly as in §4. The launch line reads `o  Add N
terminal(s)` — A3 inherits §7's revised map, so `⏎` marks a row there too and
`o` is what commits the sheet.

**As built (A3).** Three things the export leaves open, decided here:

- **Repositories only.** The sheet has no crumb, so it cannot browse; it lists
  what the current root's search finds and leaves plain folders to the launch
  picker, which can walk to them.
- **Marks survive a root switch.** `⇧⇥` changes what is listed, not what is
  going to be added — the sheet's selection is as workspace-wide as the
  picker's.
- **The `+` in the status bar keeps its label only where there is room.**
  Below `WIDE_COLUMNS` the affordance shrinks to a bare `+`, the way the key
  hints shed their labels, because it is the only pointer path to the sheet
  and so goes last rather than first. In the narrow fallback there is no row
  to put it on and no `+` is drawn.

**Mouse parity inside the sheet.** Every key it has is also a target, because
the epic's rule does not stop at the session's edge:

| Keys | Action | Pointer |
| --- | --- | --- |
| `⏎` | mark the row (locked rows refuse both) | click the row |
| `+` | another instance, locked or not | click the row's instance slot |
| `-` | shed the most recent instance | secondary-click the same slot |
| `⇧⇥` | next configured root | click `⇧⇥ switch root` in the header |
| `/` | filter | click the query line |
| `o` | add what is marked | click the button |
| `esc` | close without adding | — |

The instance slot is what makes `+` reachable at all: it is drawn on **every**
row — a dim `+` when nothing is marked, an accent `+` at one, and `×N` beyond
that — so a repository the session already holds still has somewhere to click
for another instance of it. It takes two columns of the row and wins over the
row beneath it, the way the picker's marker cells do.

`esc` has no twin, deliberately: the sheet is a modal, and a click outside it
should not be able to throw away a set of marks by accident.

Adding commits through the same naming rule the picker uses, applied to the
running session: a repository already open takes `-2`, then `-3`, skipping
any name a live terminal already holds. The new panes land at the end of the
stack, folded like every other new preview (#39), and the master keeps the
frame — which is what `esc cancel · master unchanged` promises.

The affordance lives in the **status bar**: `> 1 frontend · + add`, and the
spec board says clicking the `+` opens the same sheet — *"No corner buttons, no
divider handles."* `^g a` is the **only new session binding** in the whole
update.

---

## 7. Key map and mouse parity

The export gives every picker key a click equivalent, which makes this the
first screen designed to the mouse-first epic's parity rule from the start.

**Revised by the user, after A2 shipped.** The export's map gave `⏎` two jobs
(launch, and navigate when nothing was selected) and left entering a folder to
`l`/`⇥`, which meant a repository could not be looked inside at all — a
repository holding `projects/` was a dead end. The revision is three keys and
one rule: **`⏎` selects, `→` goes inside, `←` comes back**. Anything that made
those three ambiguous was removed rather than kept beside them.

| Keys | Action | Pointer |
| --- | --- | --- |
| `↑↓` / `j` `k` | move cursor | hover |
| `⏎` / `space` / `⇥` | toggle the cursor row | click row — **any row**, folder or repository |
| `⇧↓` / `⇧↑` | toggle every selectable row from the cursor to the end · to the start | **shift-click** a row: the same span, bounded by where it landed |
| `→` / `l` | go inside — a folder **or a repository** | click the row again |
| `←` / `h` | back one level | — |
| `~` / `g` | home · root | click crumb |
| `a` | all repos here | click count |
| `m` | set master | click pane number |
| `x` `X` | remove the path · clear all | click the checkbox to toggle its path |
| `+` | another instance of this path | click the `×N` badge |
| `-` | drop the most recent instance | click the badge with the secondary button |
| `/` | filter | click the filter slot |
| `o` | open the selection as terminals | click the button |
| `esc` | clear the filter · quit | click away |

**Amended again by the user: `⇧↓` and `⇧↑` range-toggle.** They take every
selectable row between the cursor and that end of the listing — repositories
and folders alike, never a file and never `..`. Three properties keep them
predictable:

- **First-row rule.** The first selectable row in the span decides the whole
  gesture: if it is selected, remove every path in the span (and every one of
  those paths' instances); otherwise, add each missing path once. Thus the
  same gesture selects, deselects, then selects again without multiplying a
  deliberate `+` instance.
- **Listing order**, whichever way the range runs, so pane numbers read top to
  bottom the way the screen does. `⇧↑` from the fifth row makes the *first*
  row pane 1, not the fifth.
- **The cursor does not move.** The range is what travelled, not the cursor.

**The pointer twin is shift-click**, not a drag. Holding shift and clicking a
row toggles everything between the highlight and that row, in the same listing
order and on the same first-row terms, and then moves the highlight there. A
drag was the other candidate and was refused: dragging would give the press
that already selects a second meaning, and the rule here is that when two
actions collide the simple one wins. Shift is a modifier on a gesture the
pointer already has, which costs the plain click nothing.

The bottom bar states the keys (`⇧↑↓ range`); shift-click needs no row of its
own, because it is the same gesture the table pairs it with.

Every selectable repository and folder also starts with a three-column
checkbox, `[ ]` or `[x]`; files and `..` leave those columns blank. Its hit
region is separate from the row body: clicking it toggles the whole path, just
as `space`, `⇥`, and `x` do, including removing all `×N` instances. It never
descends; only a second click on the row body does that.

Two consequences of the revision, both **Chosen** because the export cannot
answer them:

- **Launch moved to `o`.** `⏎` cannot both select a row and open the
  selection, and select is the job the user gave it. The launch button reads
  ` o  Open N as terminals ` and the status bar says `o open`, so the key is
  stated wherever the action is.
- **Any directory can be selected, not only a repository.** `⏎` selects "the
  row's path", and a plain folder is a perfectly good working directory. `◆`
  and `▸` still mean what they meant — the glyph says whether git is there,
  not whether the row may be picked. A plain file still cannot be selected,
  and neither can `..`. This holds for **every** way of selecting: the
  pointer's click, `⏎`, and the two range keys all take a folder on the same
  terms as a repository. `a` is the one exception, and stays what the export
  named it: *all repos here*.

Because a single click selects, the pointer's "go inside" is a **second click
on the same row**, which undoes the selection the first click made: the click
was the user reaching for the folder, not for a pane. `click name` leaves the
table for the same reason — with one click meaning select, a separate hit
region inside the row would have made half of it do something else.

`+` and `-` are **Chosen** (§0). They read as "one more of this / one fewer",
they pair with the `+ add` affordance the export already puts in the session's
status bar, and they leave `space` meaning exactly what the export says it
means. They do not collide with the divider's `^g -` / `^g =`: those are
session bindings and stay prefixed, while picker keys are bare and never reach
a session or a shell.

**Picker keys are bare** — no `^g` prefix, because no terminal has focus yet.
Inside a session everything stays prefixed, so `a`, `m`, `x` and `/` are
picker-local and never reach a shell. The board names the session bindings the
picker must not reuse: `^g z`, `^g c`, `^g [`, `^g 1-4`, `^g q`.

**Contradiction in the export.** Screen 06's browse-panel title hint reads
`h up · l enter · ~ home · / root`, but both the bottom bar and the key-map
board give `/` to the filter and `~ / g` to `home · root`. **Chosen:** the key
map board wins — `~` is home, `g` is root, `/` is filter. The panel hint now
reads `← back · → inside · ~ home · g root`, which is the revised map.

---

## 8. Interplay with what already exists

| Situation | Behaviour |
| --- | --- |
| **`termdeck <folder>`** | No picker: A1 already resolves a folder to a root and its terminals. |
| **`termdeck --config`/`WORKSPACE`** | No picker. |
| **Launching** | The picker builds a `Workspace` and hands it to the existing session; from the first frame the session is the session, with previews folded (#39) and the stack at its minimum width (#44). |
| **`^g a` while zoomed** | The sheet is centred over whatever the session is drawing; zoom is a session mode and is untouched. The sheet takes every key while it is open, like a modal. |
| **`^g a` while a modal is open** | Unreachable — help and quit already own every key. |
| **Collapse, scrollback, promotion** | Not the picker's business. Nothing in the picker changes a session that already exists except appending panes at the end. |
| **The divider (#41)** | Session-only. The sheet does not draw it; the picker does not either (§1.1). |

---

## 9. What each remaining slice needs from this

- **A2 (this note, then the picker UI)** — the browse listing, the selection
  panel, filter, empty states, and the key map, rendered against a filesystem
  the UI does not itself walk. It needs a read-only listing contract: entries
  with kind (repo/folder/file), name, and the meta each kind carries.
- **A3** — the runtime-add sheet, which is this same view with the three
  differences in §6, plus the `^g a` binding and the status-bar `+`. Its
  instance support is the same `+` key, with the extra duty of skipping names
  the running session already holds.
- **A4** — wiring: `CliCommand::Picker` returns a `Workspace` instead of
  `"folder picker pending A2"`, and the sheet appends terminals to a running
  engine.

The seam this note assumes between the interface and everything else: the
picker asks for *a directory's entries, already classified and already
annotated*, and returns *an ordered list of (terminal name, path) pairs plus a
workspace name* — pairs rather than paths, because instances (§3.1) mean the
same path can appear more than once and only the picker knows what to call
each one. Git
inspection, filesystem walking and root configuration all sit behind that seam,
on the engine/config side of the architecture boundary — the interface neither
opens a repository nor stats a file.
