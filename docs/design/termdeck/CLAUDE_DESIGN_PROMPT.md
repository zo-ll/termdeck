# Claude Design prompt

Design high-fidelity terminal screenshots for a developer tool named Termdeck.
It opens the related terminals for one workspace. Use an `idp` example with
frontend, backend, and optional app terminals.

Do not use a grid. The active terminal occupies 70% on the left. Other terminals
are stacked as narrow, continuously updated previews on the right. Selecting a
preview promotes it to master. Zoom hides the stack.

Produce previews for frontend-active, backend-active, and zoomed states at
144x42 characters. Use realistic Vite, Laravel, and npm output. Show terminal
name, shortened path, activity, running/exited state, and exit code. Include a
compact shortcut bar for switching, direct selection, zoom, scrollback, help,
and quit.

Use a dark, restrained, achievable terminal design with monospace type, Unicode
box drawing, minimal borders, strong contrast, and a green or teal active state.
Avoid gradients, illustrations, GUI cards, and excessive decoration. Also
describe colors, spacing, truncation, status treatment, and narrow-screen
behavior.

