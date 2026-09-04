# Agent API — research & design brief (#72)

Status: DESIGN (research phase — no implementation yet; user to pick direction).
Principle (user): termdeck is AI-FIRST — a first-class, machine-readable
surface any coding agent can drive fully ("any agent should be able to fully
control termdeck"). Design + plan first; do not implement blindly.

## Architecture reality (grounding)

- `session::run()` is ONE in-process loop: `keys.read(POLL_INTERVAL)` →
  input dispatch → `engine.drain_events()` → render. PTY readers run on
  threads feeding mpsc channels. There is no server; there is no process
  boundary.
- A CLI **subprocess cannot mutate the running session's deck or read its
  in-memory buffers** (scrollback lives in the engine's terminal model).
  Control/introspection of a *running* session therefore REQUIRES an IPC
  rendezvous owned by the termdeck process.
- `contracts::TerminalId` is already the stable, addressable per-terminal
  identity (used by `Promote(TerminalId)` at the frozen contracts boundary).
  The agent surface is an OUTWARD API — it translates into existing
  `UserCommand`/`ActionCommand`; contracts stay frozen.

## Transport options (the open question, answered)

| Option | Control of running session | Introspection (peek) | Deploy effort | Verdict |
|---|---|---|---|---|
| A. Unix socket + short JSON lines, stateless request/response, one connection per call; run loop polls accept fd each frame like `keys.read(POLL_INTERVAL)` | ✅ | ✅ | +1 socket poll in the loop; no deps | **RECOMMENDED** |
| B. FIFO/state-file rendezvous | ⚠️ (blocking fifo reader; state files racy) | ⚠️ | less | rejected (racy) |
| C. Pure subprocess (`termdeck open` = new external session) | ❌ | ❌ | none | rejected (cannot drive the user's session) |

A is the classic tmux-server→tmux-client shape, but in-process: the run loop
accepts nonblockingly, reads ONE JSON request line, dispatches through the
existing command path, writes ONE JSON response, closes. No async runtime, no
new dependencies; the socket fd fits beside `keys.read()` in the same poll.

## Trust model (the second open question)

- Socket bound at `$XDG_RUNTIME_DIR/termdeck/<pid>/ctl.sock`, dir mode 0700.
- Caller authenticated via `SO_PEERCRED` (same uid only on arrival).
- Session env: every spawned PTY inherits `TERMDECK_SOCK=<path>` (prepended
  to shell env). So an agent's `termdeck ctl` child — launched from INSIDE a
  termdeck terminal — resolves the socket with zero configuration, and
  `SO_PEERCRED` covers out-of-session same-uid callers.
- Consequence to document: a process inside a session terminal can drive that
  session. That IS the point ("any agent fully controls termdeck"); the
  boundary is your own uid + the machine (no remote exposure, no cross-user).

## Outward API (JSON schema v1 — surface only, not internal changes)

Introspection:
- `session status` — session name, pid (via the socket), terminal count,
  master id, size, zoom/collapsed, sheet open.
- `termctl list` — `[{id, path, state(live|exited), master, active, page?}]`.
- `termctl status <id>` / `peek <id> [lines]` — read the scrollback tail from
  the engine buffer (their output — what agents need to check their work).

Control (all map to existing commands):
- `termctl open <path>` (add a terminal / runtime-add), `termctl close <id>`
  (reuses the #84 engine-path kill; last-pane → same quit-confirm question
  unless `--force`), `promote <id>`, `zoom`, `scrollback <id> <n>`,
  `input <id> [--text|--paste]` (send bytes to a terminal — the "fully
  control" surface), `notify <msg>` (typed message surface, siblings #71).

Conventions:
- `--json` on every command (schema-versioned envelope:
  `{schema:"ctl.v1", ok, data|error:{code,message}}`); plain text default
  for humans; stable documented exit codes (0 ok, 1 runtime error, 2 bad
  request, 3 refused/policy); idempotent verbs (close an already-closed id
  → `ok:true already`, promote no-op if already master); `termctl help`
  renders the agent surface.

## Agent coverage ("any agent")

- One `termctl` CLI binary implements the whole surface (works for pi,
  shells, notebooks — any harness or agent that can run a command).
- `termdeck mcp` — a thin MCP server wrapping the SAME schema over the same
  socket, so Claude Code / Codex native MCP tool calls drive the session
  identically. No second API to learn; MCP is a transport adapter only.
- Env conveniences (from #72): `TERMDECK_SOCK` auto-resolution, and the
  optional `TERMDECK_*` finger-print list per terminal for name-addressable
  control.

## Phases (each plannable, reviewable, mergeable alone)

1. **Rendezvous core**: socket + poll-in-loop + `peek/list/status` (read-only
   introspection; proves transport + trust). No UI change.
2. **Control verbs**: open/close/promote/zoom/scrollback/input; last-pane
   interplay with the quit-confirm; idempotency + exit codes. UI change:
   none inside the loop beyond dispatch wiring.
3. **MCP server** (`termdeck mcp`; one tool per verb; live in Claude/Codew)
   + `notify` typed message (overlay/flash ties to #71's visual work).

## Open items for the user to pick

- **Transport**: A (socket+JSON) assumed above — confirm.
- **`input <id>` byte-writing**: powerful (typing into a live pane). Allow
  same-uid by default, or require `--force`/an env opt-in?
- **`peek` privacy**: agents reading scrollback of OTHER terminals in the
  session — fine (they're your own processes) or restrict per-terminal?
- **MCP server**: which agent SDKs to test against first (Claude Code via
  stdin MCP? Codex? pi custom tools wrapper is trivially the CLI itself).

## Scope guards
- Contracts stay frozen; the API is an outward adapter (new crate/module
  `src/ctl/` + `src/cli` entry), no engine internals change.
- No daemon: the socket dies with the session (unlike tmux), matching the
  project constitution. Workspace integration stays configuration-only here
  too.
- Research precedent: #71 stays parked; `notify` ships in the API now, the
  visual flash/overlay lands with #71's design.