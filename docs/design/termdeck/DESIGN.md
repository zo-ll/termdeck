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

Claude Code may use the `claude_design` MCP to inspect that project after
authenticating with `/design-login`. The committed export remains authoritative
for review and must not be regenerated from an older prose prompt.

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

The HTML export contains the frontend-active, backend-active, zoomed, narrow,
status, palette, and responsive reference states required for implementation.
