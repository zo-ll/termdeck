# Agent API — implementation spec (#72)

Status: **IMPLEMENTATION SPEC** (adopted from the research brief
`docs/design/termdeck/agent-api-research.md` + in-session requirements).
Decisions here are DECIDED, not recommendations. Workers implement to this
file; the critic judges against it.

## 1. Decisions (from research; now fixed)

1. **Transport**: unix-domain socket + newline-delimited JSON request/response.
   One connection per call; at most ONE request served per UI frame; polled
   nonblockingly in `session::run()` beside `keys.read(POLL_INTERVAL)`.
   `std::os::unix::net` only. No daemon — socket dies with the process.
2. **Contracts**: exactly ONE additive change —
   `TerminalEngine::history_lines(&self, terminal, max: usize) -> Option<Vec<String>>`
   with a **default body returning `None`** (existing implementors keep
   compiling; behavior contracts untouched). Everything else stays frozen; the
   agent surface is an outward adapter (`src/ctl/`).
3. **Trust**: same-uid only. Socket at `$XDG_RUNTIME_DIR/termdeck/<pid>/ctl.sock`
   (fallback `$TMPDIR/termdeck-$UID/<pid>/`, else `std::env::temp_dir()`), dir
   mode 0700; every `accept()` verifies `SO_PEERCRED` uid == ours (`libc`
   already a dependency). No allowlists (identity cannot separate good from
   compromised same-uid processes — that's the documented boundary).
4. **Env inheritance** (the in-session case is first-class):
   - `TERMDECK_SOCK=<path>` — every spawned PTY inherits it; an agent inside a
     pane self-resolves with zero config.
   - `TERMDECK_PANE=<id>` — every spawned PTY knows its own pane id
     (`termctl whoami` equivalent via env; agents must not promote/zoom/close
     themselves accidentally).
   - NESTED SESSIONS: each session OVERWRITES `TERMDECK_SOCK`/`TERMDECK_PANE`
     for its own children (inner session wins — an agent inside a nested
     termdeck controls ITS host, never the outer one).
5. **Input gate** (byte injection into a live pane = code execution in the
   user's shell): refused (`3/refused`) unless the session env had
   `TERMDECK_ALLOW_INPUT=1` OR the call passes `--force`. Both mechanisms ship.
6. **Close-last mirrors #84**: `close` on the LAST pane without `--force` →
   `3/refused` ("would end session; confirm --force"); with `--force` it takes
   the quit path (same result as answering the quit-confirm modal). No new
   modal machinery.
7. **Close-self guard**: `close <TERMDECK_PANE-of-the-caller>` refused
   (`3/refused`) unless `--force` (a slip would kill the agent's own shell
   mid-turn). Detect self via the caller's peer + our env mapping.
8. **Peek privacy**: unrestricted same-uid (tmux `capture-pane` precedent).
   Document: panes running fullscreen apps may hold secrets; agents treat
   peek output as sensitive. No per-terminal ACLs in v1.
9. **MCP is an adapter, not a transport**: `termdeck mcp` (Phase 3) is a stdio
   JSON-RPC 2.0 server that dials `TERMDECK_SOCK` per call; one tool per verb,
   same envelope. The socket comes first; MCP never replaces it.

## 2. Wire protocol (ctl.v1)

Request (newline-delimited JSON, one object per connection):

```json
{"schema":"ctl.v1","verb":"peek","id":"2","lines":30}
```

Reply (same connection, then close):

```json
{"schema":"ctl.v1","ok":true,"data":{...}}
{"schema":"ctl.v1","ok":false,"error":{"code":2,"message":"bad request: unknown verb"}}
```

- Every reply carries `schema:"ctl.v1"`. Errors: `code 1` runtime · `2` bad
  request/usage · `3` refused/policy.
- CLI (`termctl <verb> ...`) wraps the same wire calls; `--json` prints the
  envelope verbatim, plain text is the human default.
- Idempotency: `close` of already-closed id → `ok:true, already:true`;
  `promote` of current master → no-op ok; `open` twice → two panes (add
  semantics, like `^g a`).
- Versioning: `schema` field + `ctl version` verb. No flag days.

### Verbs v1 (Phase 1 ✦ || Phase 2 ✦✦)

| Verb | Args | Read/Control | Phase | Reply data |
|---|---|---|---|---|
| `status` | — | read ✦ | 1 | `{name, pid, terminals, master, size{cols,rows}, zoom, collapsed, sheet}` |
| `list` | — | read ✦ | 1 | `[{id, path, state:live\|exited, master, active}]` |
| `peek` | `id`, `lines` | read ✦ | 1 | `{id, lines:[..], alt_screen:bool}` (uses `history_lines`) |
| `notify` | `msg` | write ✦ | 1 | `{delivered:true}` (typed message; visual overlay lands with #71) |
| `open` | `path` | control ✦✦ | 2 | `{id}` |
| `close` | `id`, `--force` | control ✦✦ | 2 | `{id, last:bool}`; `3/refused` for last-pane/self without `--force` |
| `promote` | `id` | control ✦✦ | 2 | `{id, master:true}` |
| `zoom` | `--on\|--off` | control ✦✦ | 2 | `{zoom:bool}` |
| `input` | `id`, `--text\|--paste` | control ✦✦ | 2 | `{id}`; `3/refused` unless gated |
| `version` | — | meta ✦ | 1 | `{schema:"ctl.v1", version}` |

v2 (later, not scoped now): `scrollback <id> <n>` (viewport steering),
`respawn <id>`, `watch`/subscribe streaming, numeric pane aliases.

## 3. Semantics (write-through, mirroring the UI)

- Each request dispatches INLINE in the loop's poll, on the SAME code paths as
  the equivalent keypresses: `input`→`EngineCommand::Input`, `promote`→
  `ActionCommand::Promote`, `close`→`request_close`, `open`→`add_terminal`.
  Reply is post-dispatch state (blocking ack). At most one request per frame;
  clients queue client-side.
- `open` while the runtime-add sheet is open → `3/refused` (avoids two
  add-flows racing one list). Reads always live. `promote/close/zoom` apply
  beneath the sheet (deck state the sheet doesn't own).
- Zoom/collapsed: verbs operate on the model, not the view — `promote` works
  zoomed; `zoom` toggles regardless of collapse.
- Empty session is unreachable (the loop owns ≥1 pane); if observed: reads →
  ok-empty, mutates → `2`.

## 4. Socket lifecycle & robustness

- Bind at startup (after `OuterTerminal::enter`), `0700` dir; unlink a
  pre-existing socket at our own `<pid>` path (crash debris); best-effort
  sweep sibling `<pid>` dirs whose pid is dead (same-uid only, ignore errors).
- Nonblocking accept in the existing poll; bounded read (e.g. 64 KiB line
  cap → `2` on overflow); write reply; close.
- POSIX-gated (`#[cfg(unix)]`, precedent `src/session/outer.rs`). Windows is
  a later named-pipe track, not v1.
- The API surface never touches `src/contracts/` beyond the ONE default-bodied
  method; no engine-behavior change; no UI change beyond the notify surface
  contract (shared flash/overlay descriptor with #71).

## 5. Components & sizing (from research, ±50%)

- `src/ctl/` — envelope + verb dispatch + socket listener (~300–500 ln + tests).
- `src/cli` — `ctl` subcommand family (~100 ln + tests).
- Loop hook in `session::run()` (~30 ln: accept→read→dispatch→reply→close).
- Contracts: `history_lines` default-bodied method + `NativeEngine`/
  `FakeEngine` impls (~60 ln + tests).
- Env: `TERMDECK_SOCK`/`TERMDECK_PANE` injection at PTY spawn; nested
  override; `TERMDECK_ALLOW_INPUT` gate check.

## 6. Phases & acceptance criteria

### Phase 1 — rendezvous + read surface (issue: one)
Ships: socket + listener + poll hook + env injection (`TERMDECK_SOCK`,
`TERMDECK_PANE`, nested override) + `history_lines` + `status/list/peek/
notify/version` + stale sweep + trust checks.
ACCEPT: `termctl status/list/peek` work from inside a pane AND same-uid
outside; `peek` returns retained lines BEYOND the viewport (proves
`history_lines`); wrong-uid rejected; `ctl version`; stale socket self-heals;
gate green; a live repro (agent-in-pane `peek`s a busy pane).

### Phase 2 — control verbs (issue: one)
Ships: `open/close/promote/zoom/input` + gates (`TERMDECK_ALLOW_INPUT`,
`--force`), close-last ↔ #84, close-self guard, sheet/zoom/collapsed
semantics.
ACCEPT: each verb's post-dispatch state matches the equivalent keypress;
close-last without `--force` → refused, with `--force` → clean quit path;
close-self refused; `input` refuses unless gated; idempotency cases; gate
green; live repro (an agent drives a second pane end-to-end).

### Phase 3 — MCP adapter (issue: one)
Ships: `termdeck mcp` stdio server (JSON-RPC 2.0, `tools/list`+`tools/call`,
one tool per verb, same envelope).
ACCEPT: tools work via (in order) pi custom-tools wrapper → Claude Code
`.mcp.json` → Codex `config.toml`; pin MCP spec version at impl time
(research could not verify the current number — check
modelcontextprotocol.io then; record the pin in the PR).

## 7. Tests (required)

- Socket round-trip vs `FakeEngine` (envelope goldens; exit codes 0/1/2/3;
  idempotency cases; line-cap overflow → 2).
- Trust: wrong-uid rejected; dir perms 0700; stale-socket recovery.
- Peek: `history_lines` returns retained lines beyond the viewport; alt-screen
  flag honest.
- Semantics: sheet-open `open` → refused; close-last/close-self gating;
  input gating both mechanisms; zoom/collapsed model-behavior.
- Repo gate: `fmt --check`, `clippy --all-targets --all-features -D warnings`,
  full suite.

## 8. Not in scope (v1)

- No Windows transport (POSIX-gated; named pipes later).
- No `watch`/subscribe, no streaming, no daemon, no persistence.
- No per-terminal ACLs or auth beyond same-uid.
- No remote access of any kind (localhost socket only).
- Workspace integration stays configuration-only; agents reach us via this
  API, not via the tools a workspace happens to run.

## 9. Confirm-at-impl pins

- MCP spec version (Phase 3 start).
- TIOCSTI-disabled claim (Phase 2 `input` docs — verify on this machine or
  drop the line from docs).
- Final `run()` hook line placement (symbol-anchored, not line-anchored).