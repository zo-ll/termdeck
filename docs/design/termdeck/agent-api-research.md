# Agent API — independent research brief (#72)

Researcher (muse-spark) · 2026-09-04 · study: `/tmp/shipwright/termdeck/72-agent-api-research/study.md`
Related: issue [#72](https://github.com/zo-ll/termdeck/issues/72) (agent API), issue
[#71](https://github.com/zo-ll/termdeck/issues/71) (notifications), prior candidate brief
`docs/design/termdeck/agent-api-design-brief.md` (coordinator's draft — critiqued in §7,
not taken as authority).

## 1. Executive summary (the decision in 6 lines)

1. **Transport:** unix-domain socket + newline-delimited JSON request/response, polled
   nonblockingly once per UI frame — std-only, no daemon, dies with the session. (§5 Q1)
2. **Trust:** socket dir `0700` under `$XDG_RUNTIME_DIR/termdeck/<pid>/`, `SO_PEERCRED`
   same-uid enforcement, `TERMDECK_SOCK` inherited by every spawned PTY (Neovim
   `NVIM_LISTEN_ADDRESS` precedent). (§5 Q2)
3. **Schema:** one `termdeck ctl` CLI + one JSON envelope (`ctl.v1`); read verbs ungated,
   `input` (byte injection) gated behind session opt-in or per-call `--force` (Kitty
   `allow_remote_control` precedent). (§5 Q3–Q5)
4. **MCP is an adapter, not a transport:** one `termdeck mcp` stdio server reusing the
   same schema over the same socket; validate schema first via the CLI itself. (§5 Q6)
5. **Hard constraint found:** true `peek` (scrollback *history* text) is impossible
   through today's frozen contracts — `TerminalEngine::frame()` exposes only the visible
   viewport. One additive read-only trait method with a default body is required. (§4)
6. **Phasing:** rendezvous+read verbs → control verbs → MCP adapter; each shippable alone.
   Estimated surface: one new module + CLI entries + ~30 lines of loop hook. (§6, §5 Q8)

## 2. What exists today (grounding, from evidence)

- **One in-process loop, no server.** `session::run()` (`src/session.rs`) is
  `keys.read(POLL_INTERVAL)` → input dispatch → `engine.drain_events()` → render, with
  `POLL_INTERVAL = 20ms` (`src/session.rs:57`). PTY readers live on threads feeding mpsc
  channels (`src/engine/pty.rs`, `src/engine/native.rs`). A grep for
  `TERMDECK_SOCK|XDG_RUNTIME_DIR|SO_PEERCRED|uds|unix` over `src/` returns **nothing** —
  there is no IPC of any kind today. [fact, verified]
- **Frozen command surface.** `src/contracts/command.rs` (full file, 30 lines):
  `InputCommand::{Bytes, Paste}`, `ActionCommand::{SelectPrevious, SelectNext,
  SelectPosition, Promote(TerminalId), ToggleZoom, ToggleScrollback, RespawnActive,
  ShowHelp, RequestQuit}`, `UserCommand::{Input, Action}`. `TerminalId(String)` is the
  stable per-terminal identity (`src/contracts/terminal.rs`). [fact, verified]
- **Engine seam is viewport-only.** `TerminalEngine` (`src/contracts/engine.rs`) exposes
  exactly `dispatch / drain_events / frame / status / metadata`. `frame()` returns the
  **current viewport grid** (`TerminalFrame`, `src/contracts/screen.rs`) — there is **no
  accessor for retained history lines**. History exists one layer down: the VT adapter
  wraps `alacritty_terminal` with `scrolling_history: DEFAULT_SCROLLBACK`
  (`src/engine/vt.rs:46`), renders via `display_iter` (`vt.rs:93`), and reports
  `history_size()` (`vt.rs:127-131`); `NativeEngine::{spawn_sized, add, close, frame}`
  live at `src/engine/native.rs:180,215,244,403`. So the bytes `peek` needs exist in the
  process but are unreachable through the frozen trait. [fact, verified — see §4]
- **Close/last-pane semantics already decided (#84).** `request_close`
  (`src/session/lifecycle.rs:96`): non-last pane closes immediately; the **last pane
  routes to the quit-confirm modal** (`RequestQuit`) and closes nothing yet. `add_terminal`
  (`lifecycle.rs:48`) sizes the new PTY from renderer geometry. The API must mirror both.
  [fact, verified]
- **CLI shape to extend.** `src/cli/mod.rs`: hand-rolled parser (`--config`, positional
  `check|list|FOLDER|FILE`), `CliCommand::{Launch, Folder, Picker, Check, List}`. A `ctl`
  subcommand family fits this module without restructuring it. [fact, verified]
- **#71 interplay.** Issue #71's recommended notify path (BEL / OSC-777 decoded from the
  PTY stream) rides the **existing PTY** — no socket needed for notify-from-inside-a-pane.
  The socket proposed here is still needed for notify-from-outside and for everything
  else (list/promote/peek). The two designs converge on one UI flash/overlay surface;
  keep them compatible but do not merge the work items. [inference from #71 text + this
  grounding]

## 3. Possibilities (all mechanisms considered before judging)

| # | Mechanism | Control live session | Peek history | Deps / effort | Fit with no-daemon + single loop | Verdict |
|---|-----------|---------------------|--------------|---------------|----------------------------------|---------|
| A | **Unix socket + JSON lines**, nonblocking accept polled per frame, one connection per call | ✅ | ✅ (with §4 accessor) | std-only (`std::os::unix::net`), S | native: fd beside `keys.read()`, socket dies with process | **RECOMMENDED** |
| B | FIFO / state-file rendezvous | ⚠️ half-duplex, blocking-open semantics, racy files | ⚠️ racy | std-only, S | fights the loop (blocking reads); no credential passing | rejected |
| C | Pure subprocess (`termdeck open` spawns, no IPC) | ❌ cannot touch the running session's deck or engine buffers | ❌ | none | n/a — answers a different question | rejected (agrees with prior brief) |
| D | TCP loopback + JSON/HTTP | ✅ | ✅ | std-only at minimum, S–M | implies remote reachability; port allocation; contradicts the no-remote constitution | rejected |
| E | D-Bus session bus | ✅ | ✅ | `zbus` dep, M; WSL/bus friction | daemon-adjacent; heavyweight for one-window app | rejected |
| F | HTTP/gRPC server in-process | ✅ | ✅ | async runtime or threads + deps, M–L | thread/runtime beside a single-threaded loop for localhost-only traffic | rejected for v1 |
| G | OSC / escape-sequence control channel over PTY | ⚠️ one-way, pane-local | ❌ no query/response | none, M (parser work) | complementary (great for notify, §5 Q6/#71), not a control transport | companion, not transport |
| H | MCP-only (no CLI, "MCP server" as the surface) | ⚠️ | ⚠️ | MCP SDK + still needs a transport | **category error:** an MCP stdio server spawned per client cannot reach a running session without a socket underneath — MCP is an adapter layer (§5 Q6) | rejected as transport |
| I | Do nothing (defer API; agents use tmux-style keystroke injection via `EngineCommand::Input` only through the UI) | ❌ | ❌ | none | preserves status quo; contradicts the AI-first user principle | rejected (listed for completeness) |
| J | Smallest slice (read-only: status/list/peek + notify; no control verbs) | partial | ✅ | S | shippable phase 1 of A | **adopt as Phase 1**, not as end state |

**Precedent matrix (web research + training knowledge; verification status per row):**

| Precedent | Shape | Trust model | Peek equivalent | What we take |
|-----------|-------|-------------|-----------------|--------------|
| tmux | **server + control mode** (`-C`/`-CC` machine protocol; man page `CONTROL MODE` section **verified present** in fetched `tmux.1`, incl. `read-only`, `pause-after` flags) | socket dir `/tmp/tmux-<uid>/`, perms-only auth | `capture-pane -p -S -N` (exact peek precedent) | control-mode shape; `send-keys` ungated precedent; `read-only` client flag idea. NOTE: tmux server **persists** — we invert this (die with session). |
| Kitty remote control | `kitty @ <cmd>` over `--listen-on` unix socket (**verified**: `allow_remote_control` + password/authorization hooks exist on official docs page) | **opt-in** (`allow_remote_control=password/socket-only`), `KITTY_RC_PASSWORD` | `kitty @ get-text --extent=all` | **opt-in gating for powerful verbs** (our Q4 answer); per-command auth hooks |
| WezTerm CLI | `wezterm cli (list/split-pane/send-text/get-text)` over per-domain unix socket | socket perms | `get-text` | same-uid socket-perms trust; verb naming |
| Neovim RPC | `--listen` unix socket + msgpack-RPC; **`NVIM_LISTEN_ADDRESS` inherited by children** (from knowledge — spec page fetch returned no content, see §8) | path secrecy + fs perms | `nvim_buf_get_lines` | **env-inheritance rendezvous** (`TERMDECK_SOCK`); msgpack rejected (JSON is debuggable, std-serde-free) |
| Zellij | WASM plugins + `zellij action` CLI over socket (**verified only** that a plugin system exists) | same-user socket | `dump-screen` | CLI-over-socket shape; nothing deeper |
| Helix | deliberately **no IPC** | — | — | the "do nothing" precedent (J/I) — a conscious choice, not an oversight |
| MCP (spec) | JSON-RPC 2.0 over **stdio** (per-client subprocess) or Streamable HTTP; `tools/list` + `tools/call` (from knowledge — spec site is JS-rendered, fetch unverified, see §8) | client config files (`.mcp.json`, `config.toml`) | n/a | **MCP = adapter**: stdio server that dials our socket; no second schema |

[labels: rows marked **verified** were confirmed by fetch 2026-09-04; the rest are
from-model knowledge, flagged in §8.]

## 4. The one loud architectural finding (no surprises)

**True `peek` cannot be built on frozen contracts as they stand.** `TerminalEngine`
offers `frame()` (visible viewport) and `metadata()` (counts, incl. `ScrollbackPosition`
lines above/below) but no history-text accessor (`src/contracts/engine.rs`, verified).
`peek <id> [lines]` for `lines > viewport height` therefore needs exactly one of:

- (a) **Additive read-only trait method** (e.g. `fn history_lines(&self, terminal, max: usize) -> Option<Vec<String>>`) **with a default body returning `None`** — existing
  implementors (`NativeEngine`, `FakeEngine`) keep compiling; behavior contracts
  (`EngineCommand`/`UserCommand`) untouched. Recommended.
- (b) Frame-only peek (visible grid rendered to text via existing `frame()`): zero
  contract change but a crippled peek — rejected as the end state (acceptable as a
  Phase-1 stopgap only if (a) is contested).
- (c) Replaying a `FrameReady` stream: lossy, stateful, racy — rejected.

Framing for the decision: the constitution freezes contracts against *behavior/engine*
change; a default-bodied read-only query is the smallest possible extension and keeps
every current implementor building. If the user holds "frozen" as absolute, the fallback
is (b) — say so explicitly rather than discovering it mid-implementation. [inference
built on verified file evidence]

## 5. Recommendation (reasoned; the user decides)

**Adopt A (unix socket + JSON lines) with the §2–§4 grounding, phased J → control → MCP.**

- *Why A over D/E/F:* the loop already polls every 20 ms; a nonblocking
  `UnixListener::accept()` beside `keys.read()` adds ~microseconds per frame, zero
  threads, zero deps, and the socket's lifetime is naturally the process's lifetime —
  the no-daemon requirement falls out for free instead of needing enforcement. Every
  alternative either needs a runtime/dep (E, F), implies remote access the constitution
  forbids (D), or cannot answer queries at all (B, C, G). [inference]
- *Why JSON lines, not msgpack/protobuf:* agents and humans debug it with `nc`/`jq`;
  no schema compiler; the envelope carries its own version. Cost is bytes on localhost —
  irrelevant. [inference; Neovim precedent deliberately not followed here]
- *Why one connection per call, one request per frame max:* bounds the work the loop
  absorbs (no head-of-line blocking, no framing state machine); backpressure is implicit
  (client retries next frame). tmux control-mode's persistent connection is richer but
  buys nothing for call/response verbs. [inference]

## 6. Open questions 1–8 — answered (each: answer + why)

**Q1 TRANSPORT — unix-socket+JSON inline-poll, ranked.** 1st: A (see §5). 2nd: G as
companion for notify-from-pane only (#71's BEL/OSC-777, no IPC needed). 3rd: J as Phase 1
of A. B/C/D/E/F/H rejected per the §3 table. The prior brief's answer (A) **survives
critique**: its transport analysis is correct; where it overreaches is presenting MCP as
a co-equal phase-3 without naming H's category error (MCP needs the socket underneath).
[decision-recommendation]

**Q2 TRUST MODEL — same-uid + `SO_PEERCRED` + env inheritance; the session terminal is
inside the boundary, loudly.** Socket at `$XDG_RUNTIME_DIR/termdeck/<pid>/ctl.sock`, dir
`0700`; every `accept()` checks `SO_PEERCRED` uid == ours (`libc` is already a dependency
— `src/session/outer.rs` uses it — so no new dep). Every spawned PTY inherits
`TERMDECK_SOCK=<path>` (Neovim `NVIM_LISTEN_ADDRESS` precedent), so an agent's
`termdeck ctl` child launched **inside** a termdeck terminal resolves with zero config;
out-of-session same-uid callers pass `SO_PEERCRED` identically. Consequence, stated
plainly: **any same-uid process — including one running inside a session pane — can
drive the session.** That is the point ("any agent fully controls termdeck"); the
boundary is uid + machine (no remote, no cross-user), exactly tmux's boundary (socket-dir
perms) plus an explicit credential check tmux doesn't even do. A rogue process in a pane
is constrained by §Q4's input gate, not by identity — identity cannot distinguish "the
user's agent" from "a compromised dependency in the same pane", and any allowlist
claiming otherwise would be theater. [decision-recommendation; tmux/kitty shapes verified,
`SO_PEERCRED` availability on Linux = from-knowledge, `libc` presence verified]

**Q3 SCHEMA + verbs — minimal v1.** Envelope (every reply, both transports):
`{schema:"ctl.v1", ok:bool, data?:…, error?:{code,message}}`; `--json` on every verb
(plain text default for humans); exit codes `0 ok · 1 runtime error · 2 bad
request/usage · 3 refused/policy`; idempotency (`close` of closed id → `ok:true,
already:true`; `promote` of current master → no-op ok); versioning = `schema` field +
`ctl version` verb (never a flag-day). **v1 MUST:** `status` (session: name, pid,
terminal count, master id, size, zoom, sheet-open), `list` (per terminal: id, path,
state live|exited, master/active flags), `peek <id> [--lines N]` (needs §4a),
`open <path>`, `close <id> [--force]`, `promote <id>`, `zoom [--on|--off]`,
`notify <msg>`. **v2 / later:** `scrollback <id> <n>` (viewport steering),
`respawn <id>`, `input` ungated mode, `watch`/subscribe streaming, stable numeric
pane numbers in addition to `TerminalId`s. `input` ships in v1 but **gated** (Q4).
[decision-recommendation]

**Q4 INPUT — gate it: session opt-in or per-call `--force`.** Sending bytes into a live
pane is the only verb that converts read-access into **code execution in the user's
shell** (a same-uid observer today can *see* `/dev/pts/N` output but cannot inject input
— `TIOCSTI` is disabled on modern kernels — so `input` genuinely expands capability;
tmux's ungated `send-keys` accepts the same expansion, Kitty's `send-text` does not —
it requires `allow_remote_control` opt-in). Follow Kitty, not tmux: default-refuse `input`
with error code `refused/policy` (exit 3) unless (i) the session was started with
`TERMDECK_ALLOW_INPUT=1` in its environment (one-switch trusted setup, inherited by the
whole session), or (ii) the call passes `--force` (explicit intent, visible in the
agent's tool-call log = auditability). Rationale: the AI-first principle wants agents
fully capable, but the Q2 analysis shows identity can't separate good from compromised
in-pane processes — so the gate must be *intent*, and intent lives in the session owner's
environment plus the per-call flag. [decision-recommendation; TIOCSTI claim from
knowledge, flagged §8]

**Q5 PEEK — fine, unrestricted same-uid.** Scrollback of other terminals in the session
is the user's own processes' output (tmux `capture-pane` and Kitty `get-text` are both
ungated beyond connection auth). Caveat to document: panes running fullscreen apps
(vim, `claude`, password prompts — `alt_screen` in `TerminalMetadata`,
`src/contracts/terminal.rs`) may hold secrets; same-uid is still the boundary, and
agents are told to treat peek output as sensitive. No per-terminal ACLs in v1 (theater
per Q2; revisit with evidence of harm). [decision-recommendation]

**Q6 MCP — ONE `termdeck mcp` stdio adapter over the same schema; test order: CLI → Claude
Code → Codex.** The adapter speaks MCP (JSON-RPC 2.0, `tools/list`+`tools/call`, one tool
per ctl verb, same envelope) on stdio and dials `TERMDECK_SOCK` for every call — no
second API, no second schema, no daemon (the adapter is a per-client subprocess that dies
with the client; the session never hosts MCP). Test order: (1) **pi custom-tools wrapper
around `termdeck ctl`** — zero new code, validates the schema end-to-end (cheapest
possible proof); (2) Claude Code via `.mcp.json` stdio entry (`termdeck mcp`); (3) Codex
via `config.toml` MCP entry. MCP spec version to target: latest stable at implementation
time (from-knowledge checkpoint: JSON-RPC 2.0 + stdio transport + tools primitives;
**could not verify** the current version number — spec site is JS-rendered, §8 — so pin
it when Phase 3 starts, and note the prior brief's silence on versioning as a gap it
should have named). [decision-recommendation]

**Q7 ERROR/SEMANTICS — write-through, synchronous, mirroring the UI.**
- *No terminals / empty session:* unreachable in practice (a session always has ≥1 pane;
  the loop owns the list) — if observed, read verbs return `ok` with empty data, mutate
  verbs return `2/bad-request`. [inference]
- *Sheet open:* reads always live; `open` while the runtime-add sheet is open → refuse
  (`3/refused`, "sheet open; close it or retry") to avoid two add-flows racing the same
  list; `promote/close/zoom` apply beneath the sheet (they touch deck state the sheet
  doesn't own). [decision-recommendation]
- *Zoom/collapsed:* verbs operate on the model, not the view — `promote` works zoomed;
  `zoom` toggles regardless of collapse. [decision-recommendation]
- *Close-last (#84 interplay):* mirror the UI exactly — `close` on the last pane without
  `--force` returns `3/refused` ("would end session; confirm --force"), the analogue of
  the quit-confirm modal; with `--force` it takes the quit path (same modal result as
  answering `y`). No new modal machinery over the socket. [decision-recommendation]
- *Write-through vs queued:* write-through — each request dispatches inline during the
  loop's poll (same path as the equivalent keypress: `input` → `EngineCommand::Input`,
  `promote` → `ActionCommand::Promote`, `close` → `request_close`, `open` →
  `add_terminal`) and the reply is the post-dispatch state (blocking ack per call).
  At most one request is served per frame; beyond that clients queue client-side.
  [decision-recommendation]

**Q8 PLATFORM/ROBUSTNESS.**
- *`XDG_RUNTIME_DIR` fallback:* `$XDG_RUNTIME_DIR/termdeck/<pid>/ctl.sock`; if unset,
  `$TMPDIR/termdeck-$UID/<pid>/ctl.sock`, else `std::env::temp_dir()` equivalent —
  always with `0700` dir. Never `$HOME` (stale NFS handles) and never a fixed path
  (two sessions must coexist). [decision-recommendation]
- *Stale socket on crash:* the `<pid>` path component makes collisions impossible across
  sessions; on startup, unlink a pre-existing sock at our own path and best-effort sweep
  sibling `<pid>` dirs whose pid is dead (same-uid only). Crash debris is an empty dir,
  never a hijackable socket. [decision-recommendation]
- *Poll cost:* one nonblocking `accept()` + (at most) one bounded JSON-line read per
  20 ms frame — negligible next to a full Ratatui redraw; no latency impact on input.
  [inference]
- *Size of surface (estimate, ±50%):* new `src/ctl/` module (envelope + verb dispatch,
  ~300–500 lines + tests), `src/cli` entries (~100 lines + tests), loop hook (~30 lines:
  accept→read→dispatch→reply→close), §4a trait method + native/fake impls (~60 lines +
  tests). No engine-behavior change, no UI change. [inference]
- *Tests (required, not optional):* socket round-trip tests against `FakeEngine`
  (envelope goldens, exit codes 0–3, idempotency cases), `SO_PEERCRED`/perms tests,
  stale-socket recovery test, and a `peek`-history test proving §4a returns retained
  lines beyond the viewport. Plus the repo gate (`fmt`, `clippy -D warnings`, full
  test suite). [decision-recommendation]

## 7. Critique of the prior candidate brief (as instructed — one possibility among many)

The coordinator's `agent-api-design-brief.md` gets the big calls right (socket+JSON,
`TERMDECK_SOCK` inheritance, `termctl`+MCP sharing one schema, phased delivery,
no-daemon) and this research **concurs** on all of them — it is a sound draft. Gaps and
overreaches found under pressure:

1. It never notices the **§4 contracts tension** (peek needs a trait accessor) — the
   highest implementation risk in the whole proposal, and it reads asoncern-free.
2. It presents MCP as a co-equal phase without naming the **category error** (H): MCP
   cannot be the transport to a running session; the socket must come first and MCP
   stays a thin adapter. Phasing is right; the reasoning is under-argued.
3. Its trust section documents "a process inside a terminal can drive the session" as a
   consequence but draws **no consequence**: no `input` gating, no statement that
   identity can't separate good from compromised in-pane processes. §Q4 above closes this.
4. `notify` is claimed for the API phase while its visual half lives in parked #71 —
   correct call, but the brief should state the **compatibility contract** (shared flash/
   overlay surface, §2) so the two tracks can't diverge.
5. Verb list omits `zoom` state flags, sheet-open behavior, and close-last `--force`
   semantics (all answered in §Q7) — implementation will stall on these without answers.

## 8. Risks, unknowns, and what I could NOT verify

- **Could not verify (web):** current MCP spec version number + any 2025 breaking
  changes (spec.modelcontextprotocol.io is JS-rendered; curl returned no content);
  WezTerm `cli` subcommand exact names/URLs (docs site restructured — old
  `/wezterm/cli/cli/*` paths 404 or redirect); Neovim `NVIM_LISTEN_ADDRESS` inheritance
  mechanics (neovim.io docs fetch empty — cited from knowledge); Zellij socket details
  beyond "a plugin system + action CLI exist". Pin all four at implementation time.
- **Could not verify (repo):** exact `run()` line number (~line 229 per the study; the
  file I read shows the same loop — line drift across branches, so I cite by symbol, not
  line); #84's merged post-rebase shape (I read the pre-merge-era tree state at hand —
  `request_close` semantics verified as found); multi-session socket-dir sweep races
  under hostile `/tmp` (recommend `0700` + pid-dirs; a symlink-attack audit belongs in
  review, ideally with the critic).
- **`TIOCSTI`-disabled claim** (Q4's "input expands capability" argument) is from
  knowledge of mainstream distro kernels, not verified on the maintainer's machine —
  flag for the implementer to confirm or drop from the docs.
- **Windows:** unix sockets don't exist there; the study scopes POSIX-first with Windows
  aspirational — the whole transport is therefore POSIX-gated (`#[cfg(unix)]`, following
  `src/session/outer.rs:104` precedent). Named-pipe parity is a later track, not v1.
- **Scope risk:** the AI-first principle pulls toward "everything, ungated"; the Q2/Q4
  analysis pulls toward "intent gates on injection". The recommended split (reads +
  structural control ungated, `input` gated) is the balance point — expect the user to
  move it, and keep the gate a two-line check so moving it is trivial.
- **Biggest delivery risk:** §4a — if "contracts frozen" is read as absolute, `peek`
  degrades to viewport-only (4b) and the headline introspection verb disappoints. Settle
  this before Phase 1, not during it.

---
*Research-only; I recommend, I do not decide. No tracked code touched — this file is the
sole deliverable. Grounding: file evidence cited inline; web precedent verified where
marked, labeled from-knowledge elsewhere. Anything unmarked as [fact] above is the
researcher's inference — treat accordingly.*
