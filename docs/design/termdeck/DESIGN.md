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

Claude Code may inspect that project with the built-in `DesignSync` tool after
authorising design access with `/design-login`. The committed export remains
authoritative for review and must not be regenerated from an older prose
prompt.

### Remote access

Reading the live project requires a one-time `/design-login` on each machine,
using the same claude.ai account. The grant is stored per machine in
`~/.claude/.credentials.json` under `designOauth`; it is not part of this
repository and does not travel with a clone. It persists across sessions on
that machine and refreshes automatically, so repeated logins are not needed.

No MCP server registration is required. Access is provided by the built-in
`DesignSync` tool, which `/design-login` unlocks.

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
- Master terminal: left side, 70% width.
- Preview stack: right side, 30% width, equal-height live previews.
- Preview selection promotes that terminal to master.
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

Every pane title shows terminal name, shortened working directory, and status.
The master may show the complete title. Previews truncate long paths from the
left while preserving the repository name.

Statuses must distinguish starting, running, recent activity, and exited with
an exit code. Activity should use a quiet color or glyph change rather than
continuous animation.

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
