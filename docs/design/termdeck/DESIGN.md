# Termdeck design contract

## Authoritative design source

The accepted Claude Design export is committed under `reference/`:

- `Termdeck TUI.dc.html` — complete design canvas and specification
- `support.js` — runtime required to render the exported design
- `thumbnail.webp` — exported project thumbnail

The original project is available through Claude Design at:

```text
https://claude.ai/design/p/8aa66d51-e314-4df7-8b93-fcfadbb60c36?file=Termdeck+TUI.dc.html
```

Claude Code may inspect that project after authorising design access with
`/design-login`. The committed export remains authoritative for review and must
not be regenerated from an older prose prompt.

### Remote access

Reading the live project requires a one-time `/design-login` on each machine,
using the same claude.ai account. The grant is stored per machine in
`~/.claude/.credentials.json` under `designOauth`; it is not part of this
repository and does not travel with a clone. It persists across sessions on
that machine and refreshes automatically, so repeated logins are not needed.

The grant unlocks two ways in, and either is legitimate: the built-in
`DesignSync` tool, and the `claude_design` MCP tools (`get_project`,
`list_files`, `read_file`, `render_preview`, …). The #101 worker read the
canvas through the latter. What is *not* required is registering an MCP server
of your own — there is no `.mcp.json` in this repository and none is needed;
the tools arrive with the login.

The project is a plain design project, not a design-system project, so
`list_projects` does not return it. Address it directly by the project ID
carried in the URL above:

```text
8aa66d51-e314-4df7-8b93-fcfadbb60c36
```

The grant includes design write scope. Because the committed export is
authoritative, treat the remote project as read-only and do not write to the
canvas without coordinator approval.

## Layout

- Design canvas: 144 columns by 42 rows.
- **Row order: the status row first, then one blank row, then the body.** At
  the reference size that is status row 0, blank row 1, body rows 2–41 (40
  rows), in every layout — stacked, zoom, single and narrow alike. The canvas
  puts the bar on row 1 in all of its screens; the export's specification
  panel still reads `Content rows 40, blank row 41, status row 42`, which is
  the pre-#101 order and is superseded (see below).
- Master terminal: left side, 70% width.
- Preview stack: right side, 30% width, equal-height live previews.
- Preview selection promotes that terminal to master. The key is `^g N`, one
  or two digits — the deck is not capped at the canvas's four terminals, and
  `^g 1 6` reaches terminal 16.
- Zoom mode hides the preview stack.
- Narrow mode shows only the master plus a compact terminal status line.

## Visual direction

- Modern, restrained developer-tool aesthetic.
- Dark background with strong readable contrast.
- Monospace text and Unicode box drawing only.
- Minimal borders and visual noise.
- Green or teal active border; inactive borders remain subdued.
- No gradients, decorative illustrations, oversized branding, or GUI cards.

## Pane information

Every pane title shows the terminal number, its name, and its status. The
master adds the command it is running, and zoom adds the pid and uptime behind
it. The command is the only elastic field and truncates right-first.

Statuses must distinguish starting, running, recent activity, and exited with
an exit code. Activity should use a quiet color or glyph change rather than
continuous animation. A live state is carried by the glyph alone; only an exit
is also named in words.

### Declutter pass (2026-09-03)

The canvas was edited to take repeated and inferable chrome out of the session
frame. This is authoritative and supersedes the export's own specification
panel wherever the two disagree — the panel was not edited with the screens and
still describes the pre-pass chrome in three places, noted below.

Removed:

- The working directory from every pane title, master and preview, wide and
  narrow. The number and the name identify the terminal; the path was the field
  that shrank to `…/name` on anything but a full-width master anyway. The
  export's `TITLE FORMAT` panel still shows `· {cwd} ·`.
- The `running` / `starting` word after the status glyph. The glyph says it.
- The `MASTER` tag in the master's right-hand slot. Four cues already state
  focus: the teal border, the `>` caret, full-contrast foreground, and the
  status-bar pointer. `ZOOM` stays, because nothing else states zoom. The
  export's `TITLE FORMAT` panel still says the slot carries `MASTER / ZOOM`.
- The stack footer's `ctrl+g N promote · j/k cycle` and its
  `promoted {name} · ^g 1 back` demotion line. The demoted pane's own 1.5s
  highlight reports the swap, and the keys live in the help overlay. The footer
  kept the two censuses it earned — hidden previews, then folds — until the
  2026-09-04 update took the fold census as well (below).
- The `{n} stacked` census in the status bar, which the `{n} terminals` count
  beside it already implies. The exit summary that followed it stays.
- `^g N select` and `^g [ scroll` from the status bar's key row. Both stay
  bound, stay in the help overlay, and stay in the narrow collapsed row; the
  pane numbers and the scrollback marker already point at them.

Consequence for the responsive ladder: at four keys the bare key row is
narrower than the collapsed `^g j/k · N · z · [ · ? · q` form, so that form
is now reached only by the narrow layout, which is where the export shows it.

The runtime-add sheet (screen 08) was not edited in the pass, and its mockup
still *depicts* a background session whose master title carries a `cwd`. That
is stale mockup decoration, not a second title format: the sheet is composited
over a live render of the ordinary deck (`Deck::render`, then `Sheet::render`
on top, `src/session.rs`), so what actually sits behind the sheet is the
decluttered title. No pane in the running program draws a working directory.

### Canvas update (2026-09-04)

The user edited the canvas again and the export under `reference/` was swapped
for the new one. Two of its changes are behaviour, and both have landed:

- **The status row moved to the top** (#101). Every screen — stacked, zoom,
  narrow, picker, runtime sheet — now opens with the bar on row 1, followed by
  a blank row. The row order in **Layout** above is the authoritative
  statement of it.
- **The stack's fold census is gone** (#103). Screen 05 no longer draws
  `{c} collapsed · ^g c expand all` in the stack column. The row is still
  reserved, but it states only what nothing else states: scrollback, and the
  `↑ N more / ↓ N more` window count. A fold is already declared by its own
  strip, its `▸` marker, and the status row's `{c} collapsed` beside an
  accented `^g c`, so the column was saying it twice.

Three further places in the updated canvas are illustration rather than
instruction. Where the canvas and this document conflict the canvas wins, but
these three are what the canvas *cannot* mean, and the reading below is
binding:

- **The specification panel's grid.** It still reads `Grid 144×42. … Content
  rows 40, blank row 41, status row 42`, the pre-#101 order. The panel was not
  edited with the screens — the same way it was not edited in the declutter
  pass — and the screens govern. See **Layout**.
- **Screen 03's zoom corner card.** The hidden pane's notification is drawn as
  a bordered amber card floating *above* the frame, outside the 144×42 grid. A
  TUI has no cells there. It ships as the centred notification toast (#97),
  and the part of the card that does translate is the census: a pane with a
  pending notification is marked in the status row's `hidden:` summary in the
  warning colour, which is what screen 03's amber `4●` beside the green
  `2● 3●` is stating.
- **Screen 04's two status bars.** The narrow screen draws the full session
  bar on row 1 *and* the compact `84×22  stack hidden` bar along the bottom.
  That is the row-1 edit applied without removing the bar it replaced. The
  narrow layout has exactly one bar, on row 1, in the compact form; the pane
  strip follows it after the blank row, which is also why the RESPONSIVE
  board's `one-line pane strip on row 1` now means the row below the blank
  one.

## Required reference states

UI snapshots must cover:

1. Frontend master with backend and app previews.
2. Backend promoted to master.
3. Master-only zoom.
4. Exited preview with an exit code.
5. Narrow-terminal fallback.
6. Help and quit-confirmation overlays.
7. Scrollback mode.

The HTML export contains the frontend-active, backend-active, zoomed, narrow,
status, palette, and responsive reference states required for implementation.

The export does not contain the help overlay, the quit-confirmation overlay, or
scrollback mode. Those three states are specified by the reviewed supplement at
`drafts/termdeck-draft-missing-states.html`, which is authoritative for them and
derives its palette, geometry, borders, and key-hint language from the export.
The export itself remains unchanged and authoritative for every other state.

## Reviewed interaction decisions

These resolve the ambiguities raised against the supplement and are binding on
implementation.

- Focus stays singular. While a modal is open it takes the accent border and the
  master gives its own up.
- Modals recede the interface by dimming foreground only. The two accepted
  background values are never supplemented with a scrim.
- Letter spacing in the export is illustrative web chrome. The TUI does not
  emulate it.
- A destructive modal keeps the accent border and carries the warning color on
  its consequence line only.
- Quit confirmation: `y` confirms, `n` and `Escape` cancel.
- Quit confirmation states the number of terminals that will be closed, not
  their names.
- `Escape` exits scrollback mode and returns to live output.
- While scrollback mode is active, Termdeck captures `j`, `k`, the arrow keys,
  `PageUp`, `PageDown`, `g`, `G`, and `Escape`, and must not forward other input
  to the hidden live shell. This is an explicit exception to ordinary input
  forwarding, and it applies while any modal or mode is active.
- The scrollback mode tag uses the warning color so it does not read as ordinary
  accent focus.
- Scroll position is stated once. The pane title carries the mode tag alone, the
  pane footer carries the navigation keys alone, and the absolute position
  appears in the status line.
