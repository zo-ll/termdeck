# The agent-aware, persistent deck — research brief

Prepared at user request (2026-09-04, EOD snapshot `b5b0608`, main green 320
lib + 4 integration). Same structure as a researcher-window deliverable so it
can be routed for a decision. Related: #72/#90/#92/#94 (agent API; #94 MCP
held), #71/#97/#108 (notifications + shell integration), #33 (animations,
parked), #84 (close-last semantics).

Scope: one combined product question — the deck should persist across
Termdeck restarts *and* become genuinely agent-aware — because the two are
one thesis: **"a persistent terminal deck that supervises agents" is the
defensible product; either half alone is a worse version of something that
already exists.** This brief enumerates the possibilities for both threads,
grounds them in the current architecture, and recommends a direction. It
decides nothing.

## 1. Executive summary (the decision in 8 lines)

1. **PERSISTENCE IS NOT ONE FEATURE.** "Like tmux" conceals three desires:
   workspace-layout resumption (A, no daemon), live detach/reattach +
   background execution (B, requires a server/daemon), and the myth that a
   running process can be saved (C — no tool can rewind a live `vim`; even
   tmux-resurrect *recreates*, it doesn't restore). Decide which desire is
   real *before* designing. (§3.1)
2. **B is an architecture inversion, not an add-on.** It contradicts three
   constitution documents (AGENTS.md, PLAN.md, DESIGN.md: "no tmux, no
   OpenMux, no daemon, no session persistence"), rewrites the lifecycle
   acceptance criterion ("confirmed exit leaves no owned child processes",
   PLAN.md), and turns the app into a direct tmux competitor. Feasible — the
   `TerminalEngine` contract + `EngineCommand`/`EngineEvent` + the ctl socket
   were built as exactly the remote-engine seam B needs — but it is a
   constitution decision, not an engineering one. (§3.1, §2)
3. **Even A (resume) touches the constitution.** "Persist sessions" is
   banned in writing; a resume feature is arguably layout/config
   persistence, but the user must adjudicate the letter vs the spirit.
   Amendment cost: A is a one-line scope clause; B is architectural. (§3.1)
4. **D (delegate to tmux as backend) is the cheapest way to *test* B's
   value before owning a daemon.** Termdeck keeps the UI + agent layer and
   drives tmux as the persistence provider. Contradicts "no tmux", rewires
   the PTY provider, but converts a 10–20-issue daemon build into an
   experiment. Do D only as a detection experiment, not as the end state.
   (§3.1)
5. **AGENT AWARENESS ≠ AGENT CONTROL.** What shipped (#72/#90/#92) is a
   remote control: peek/promote/zoom/input. Awareness means the reverse —
   the deck knows what runs in its panes and *tells* the agent. The
   substrate already exists: `EngineEvent` (contracts) and the shell→deck
   OSC-7777 pre-scan channel shipped in #108. (§3.2)
6. **The two highest-ROI increments are cheap and daemon-free:** Pa —
   expose the event stream (`watch`/`drain` — the v2 verb pulled forward)
   so agents stop polling; Pb — invert `TERMDECK_PANE` into an advisory
   agent-identity model surfaced in `status`/`list` and the UI. Each is
   1–2 slices on the existing socket. (§3.2, §5)
7. **Pc (terminal-native approval: agent asks, human says allow once/
   always) is the differentiating-but-risky piece.** It is the one feature
   tmux semantically cannot offer. It needs its own design decision
   (modal vs keybinding vs toast; who holds "always allow") and must honor
   the #71 finding that background-event UX is non-modal. (§3.2, §5)
8. **Recommended shape: A + Pa + Pb now (no daemon, both worker lanes);
   Pc as a designed phase 2; B as the phase-3 bet gated on the thesis —
   with D as its cheap falsification test first; Pd/Pd turn-marker history
   and Pe (agent workbench spawn) ride along later.** Build B only if the
   answer to Q1/Q2 is the agent story. (§5)

## 2. What exists today (grounding, from evidence)

- **One process per session; the process IS the session.** `session::run()`
  polls input + the ctl socket at `POLL_INTERVAL` (~20ms), drains engine
  events, renders (`src/session/`; poll facts verified in the #72 study,
  `agent-api-research.md` §2). There is no IPC other than the ctl socket and
  no daemon of any kind. A session is unambiguously bounded by one process
  lifetime. [fact, verified]
- **Lifecycle is kill-everything.** "Closing Termdeck closes its shells.
  Confirmed exit sends `SIGTERM` to owned process groups, waits two seconds,
  then sends `SIGKILL` to survivors" (PLAN.md), with shutdown-path tests and
  the acceptance criterion "confirmed exit leaves no owned child processes".
  VT state (alacritty_terminal scrollback) lives only in process memory; PTYs
  are owned by `portable-pty`. None of it survives the process. [fact,
  verified — PLAN.md]
- **The constitution is explicit in three places.** AGENTS.md: "Do not add
  tmux, OpenMux, a background daemon, or session persistence." PLAN.md: does
  not "persist sessions". COORDINATION.md decisions: "No tmux, OpenMux,
  daemon, persistence, or mouse forwarding in v1." [fact, verified]
- **The remote-engine seam already exists.** `TerminalEngine` (dispatch /
  drain_events / frame / status / metadata / + `history_lines`, the #72
  additive default-body method), `EngineCommand::{Input,Resize,Respawn,
  Shutdown}`, `EngineEvent::{FrameReady,StatusChanged,MetadataChanged}`
  (`src/contracts/engine.rs`, `src/contracts/screen.rs`). This is exactly the
  abstraction a remote engine (B's server) needs — the contract layer is
  persistence-friendly even though the constitution forbids it. [fact,
  verified — agent-api-research.md §2/§4]
- **Agent API shipped (control, not awareness).** Unix socket +
  newline-JSON `ctl.v1`; reads `status/list/peek/notify/version` + control
  `open/close/promote/zoom/input` (gated: `TERMDECK_ALLOW_INPUT` /
  `--force`); same-uid `SO_PEERCRED` trust; `TERMDECK_SOCK`/`TERMDECK_PANE`
  inherited by every PTY; one request per frame, one connection per call;
  `watch`/subscribe streaming explicitly deferred to v2; MCP adapter (#94)
  held on `coord/94-ctl-mcp` pending a live handshake test. [fact, verified —
  agent-api-spec.md, COORDINATION.md]
- **Termdeck already receives "what finished" events from inside panes.**
  #108 (merged, PR #110) installs bash/zsh/fish completion hooks that emit a
  **private OSC 7777** caught by a pre-scan decoder on the PTY byte stream —
  the shell already whispers "command exited 1, took 12s" into the deck.
  This is the awareness substrate: an event channel from process → deck that
  required no socket. (Note: alacritty 0.26's parser *drops* unknown OSC, so
  the channel is the pre-scan, not the parse path — verified in
  `notifications-research.md` §2.) [fact, verified]
- **UI idioms for awareness already exist.** Non-modal status: the #71
  flash/toast surface; modals are `Help`/`Quit` only (focus-stealing,
  `src/ui/state.rs`); demotion-flash pattern with injected clock;
  `TERMDECK_BLESS` fixture workflow; sheet + picker for paths; zoom/collapse
  model semantics. [fact, verified — notifications-research.md §2]
- **Trust precedent (binding for anything new):** same-uid is the boundary;
  identity cannot separate a good agent from a compromised same-uid process
  inside a pane ("any allowlist claiming otherwise would be theater", #72
  research Q2). Any agent-identity feature (#2) must be **advisory UI**, never
  a security gate. [fact, verified — agent-api-research.md §5 Q2]

## 3. Possibilities

### 3.1 Persistence

| # | Mechanism | What it gives | Pros | Cons / cost | Fit with constraints |
|---|-----------|---------------|------|-------------|----------------------|
| A | **Workspace resume** (`termdeck --resume`): on exit (or on demand), serialize deck state — terminal defs, paths, cwd, master id, zoom, divider ratio, sheet — to a session file; relaunch recreates the deck with **fresh** PTYs | "My three project terminals are back where I left them" — the editor-tabs UX | No daemon; cheap (S, 1–2 slices, mirrors config model); processes start clean (no stale-shell rot); pairs with the agent story (the deck survives to be peeked at) | NOT live continuation — running `vim`/`cargo` die and restart; session file lifecycle (auto-save policy, stale files); letter-of-constitution question ("persist sessions" is banned) | Strong — architecture unchanged; needs only a scope-clause amendment |
| B | **Server/daemon** (tmux model): a separate process owns PTYs + VT state; the UI becomes a client; `EngineCommand`/`EngineEvent` travel the local socket; session naming/listing/attach; lifecycle = graceful handoff | True detach/reattach; background execution (`cargo build`, a long agent run) survives client close | The only way to get Desire B; contract seam already fits; differentiates with the agent story (supervised detached agents) | Architecture inversion (L, 10–20 issues across both lanes); contradicts 3 constitution docs + the "no owned child processes" acceptance criterion; becomes a tmux competitor | The seam says feasible; the constitution says no; a decision, not a build task |
| C | "Persist my live work" (reboot survival) | — | — | **Myth**: no tool restores a live process; tmux-resurrect *recreates* commands; VT state is memory | N/A — name to defuse the desire, not to build |
| D | **Delegate: termdeck drives tmux** as the persistence backend (own UI + agent layer; tmux owns PTYs) | Cheap path to B-full; detect whether "detach + reattach + background" is actually the need | Rewires the PTY provider (`portable-pty` → tmux as provider) — ripples across engine/contracts/tests; "no tmux" is in the constitution; runs against a WSL acceptance env where tmux must be present | M–L; best used as a **falsification experiment**, not the end state |
| E | Do nothing (die-with-session status quo) | — | Zero cost; matches the current constitution | Desire A/B both unmet; the product stays "tiled terminals + a control socket" | Baseline |

### 3.2 Agent awareness (degrees beyond the shipped control API)

| # | Move | What it gives | Mechanic | Cost | Constraint fit |
|---|------|---------------|----------|------|----------------|
| Pa | **Event push** (`watch`/`drain`): bounded event ring (PaneExited, StatusChanged, CommandFinished — already decoded from OSC-7777 — + new AgentTurnStarted/Ended) served over the ctl socket | Agents stop polling per-frame; a spinner in pane 3 becomes "exit 1" as an event | Bounded ring buffer; pull-style `drain` verb keeps the one-request-per-frame invariant; or a persistent-connection mode (needs a second fd — breaks the invariant; user call) | S–M (1–2 slices) | No daemon, no threads (tee into the existing drain); the v2 `watch` verb pulled forward |
| Pb | **Advisory agent identity**: invert `TERMDECK_PANE` — agent emits kind/model/busy-state in a handshake; `status`/`list` report "pane 3: claude, idle"; UI badge per pane | The deck *knows* what runs in it; notifications differentiate "agent finished, exit 1" from a normal command | New verb + envelope fields; model is advisory by construction (trust precedent §2) | S (1 slice) | Trust: **advisory only, never a gate** (identity is self-reported) |
| Pc | **Terminal-native approval**: agent requests a command; deck asks the human "allow once/always?" and routes the reply back over the socket | The one thing tmux semantically cannot offer; makes a long-lived supervised agent tolerable at the keyboard | Decision needed: modal vs dedicated key (`Ctrl+g a` approve) vs toast+keybind; who holds "always allow" (per-agent, per-session, per-command-class); audit log reuses the `--force` visibility idea | M–L; needs its own design + security review; cross-lane (engine + UI) | Must honor the #71 non-modal principle for *background* cases; foreground approval is legitimately modal (Help/Quit precedent) |
| Pd | **Turn-aware history**: OSC-7777 already marks shell command boundaries; add *turn* boundaries (input region / output region / exit code) to `history_lines` or a structured `peek` | An agent can align its tool calls to lines; audit value neither tmux capture-pane nor anything else offers | Additive contract method or envelope fields (the `history_lines` default-body precedent) | S–M (1 slice) | Additive pattern already blessed; needs the shell hooks to emit agent-turn markers (extension of #108's seam) |
| Pe | **Agent workbench spawn** (`termdeck agent` / `ctl spawn`): open a pane running a code agent pointed at the workspace, with `TERMDECK_SOCK`/`TERMDECK_PANE` pre-set | "Run an agent on this repo, watch it in the stack, approve from the keyboard" as one gesture | New verb + rc wiring (what hooks exist per agent CLI) + UI affordance | M (2+ slices) | Product bet, not engineering; workspace integration stays config-only |

Precedent matrix (persistence + awareness; labels per evidence):

| Precedent | Persistence shape | What we take | Verified? |
|-----------|-------------------|--------------|-----------|
| tmux | server owns sessions; `ls`/`attach`/`detach`; control mode; `capture-pane` peek; ungated `send-keys` | server/client shape (B); session naming UX | control-mode + capture-pane verified in the #72 study; attach/detach/resurrect semantics from knowledge, re-verify at impl time |
| screen | server/client; BSD old-timer | historical baseline; not a differentiator | knowledge |
| zellij | server + plugin system; session restore | CLI-over-socket precedent (already noted in #72) | plugin existence verified in #72; restore mechanics knowledge |
| tmux-resurrect / continuum | *recreates* panes/commands after reboot; never restores live state | the C-myth refutation; recreation semantics | knowledge |
| VS Code / editor workspace restore | tabs/layout re-open, fresh processes | Desire-A UX precedent (§3.1-A) | knowledge (ubiquitous) |
| Windows Terminal (experimental) | session restore at launch | A-adjacent precedent | knowledge |
| Foot / Kitty notify | OSC 777 (foot) / OSC 99 (kitty) desktop notify | the shell→deck channel (already shipped via pre-scan for 7777) | verified in #71 study (primary sources fetched) |
| Neovim `--listen` / kitty remote control | env-inherited rendezvous; opt-in gating | the `TERMDECK_SOCK` env-trust design (already shipped) | verified in #72 study |
| Our own ctl socket (#72) | — | the transport B would reuse for client↔server; the seam Pa/Pb/Pc extend | fact (this repo) |

## 4. The one loud architectural finding (no surprises)

**B is feasible, and the constitution is the only blocker.** The contract
layer (`TerminalEngine`, `EngineCommand`, `EngineEvent`) plus the ctl socket
are, structurally, a remote-engine API: a server process owning PTYs/VT and
serving them over the socket is a *new implementor of an existing seam*, not
a new architecture. What blocks it is written policy (three documents), the
lifecycle acceptance criterion ("confirmed exit leaves no owned child
processes" — under B, exit *detaches*, the criterion becomes false by
design), and the WSL idle-CPU/no-daemon acceptance posture. Anyone
implementing B without first amending the constitution and re-scoping those
criteria is violating the repo's own rules. [fact + inference]

## 5. Recommendation (not a decision)

**Phase 1 (no daemon, both worker lanes, constitution amendment = one line):**
ship **A (workspace resume)** + **Pa (event drain)** + **Pb (advisory agent
identity)**. All three are small, additive, socket/config-shaped, and
directly test the thesis: a deck that comes back and an agent that knows it
is being watched. A lives in config/lifecycle (Codex lane); Pa in
`src/ctl/` (Codex lane); Pb's UI badge in `src/ui/` (Claude lane) with the
envelope/verb in `src/ctl/` (Codex lane) — cross-lane by design, follow #84's
pattern.

**Phase 2 (designed, then built):** **Pc (approval surface)** — its own
design decision first (interaction + "always allow" ownership + audit),
then a security-minded slice. This is the differentiation; do not rush it
into a modal that steals the user's typing.

**Phase 3 (the big bet, gated):** **B (server)** — only if the user's answer
to Q1/Q2 is genuinely the agent story. Before committing to owning a daemon,
run **D** as a cheap falsification experiment: wire the UI+agent layer onto
tmux for a week of use and see whether "detach, leave an agent running,
reattach" is actually the daily workflow. D's verdict — not enthusiasm —
unlocks B. Pd (turn markers) rides with the agent-history work; Pe (agent
spawn) rides with/after B.

**Explicit non-recommendations:** do not build B without the agent thesis
(that is how you maintain a worse tmux); do not build Pc as a
focus-stealing interrupter without honoring the #71 non-modal principle for
background cases; do not treat D as the end state; do not build C at all.

## 6. Open questions (the decision list — user answers, then dispatch)

- **Q1 PERSISTENCE DESIRE:** A (layout resume), B (live detach/reattach), or
  both phased? The whole architecture decision hangs on this.
- **Q2 B'S JUSTIFICATION:** if B — what does termdeck give that `tmux attach`
  does not? If the answer is the agent layer (supervised detached agents),
  the thesis holds; if it isn't, tmux wins and B should be dropped or
  delegated (D).
- **Q3 CONSTITUTION AMENDMENT:** willingness to amend AGENTS.md / PLAN.md /
  DESIGN.md — one-line clause for A; architectural re-scope for B (including
  the "no owned child processes" acceptance criterion, idle-CPU posture,
  and "no tmux" line if D).
- **Q4 EVENT PUSH SHAPE (Pa):** keep one-request-per-frame with a pull
  `drain` verb (my recommendation) or allow a persistent `watch` connection
  (second fd, breaks the invariant)?
- **Q5 APPROVAL UX (Pc):** modal vs `Ctrl+g a` keybinding vs toast+keybind;
  who owns "always allow" (per-agent / per-session / per-command-class)?
  Grabs a whole design turn.
- **Q6 IDENTITY TRUST (Pb):** accept that self-reported agent identity is
  advisory-only in the UI and never a security gate (per #72's same-uid
  finding)? Any gate-shaped use is out of scope.
- **Q7 RESUME SEMANTICS (A):** auto-save on exit vs on-demand `termdeck save
  <name>`; keep session files in `$XDG_STATE_HOME/termdeck/`? Fresh
  processes on resume (recommended) vs anything exotic?
- **Q8 SERVER SCOPE (B, if adopted):** session listing UX, multi-client
  attach (collaborative stack), Windows transport (none in v1 — POSIX-gated),
  and `TERMDECK_SOCK` nested-override precedence under a server.

## 7. Risks, unknowns, and what I could NOT verify this session

- **Not re-fetched (web) this session:** tmux attach/detach/resurrect
  mechanics beyond control-mode (verified in the #72 study, not now); zellij
  session-restore internals; Windows Terminal session-restore details;
  tmux-resurrect recreation semantics. All from knowledge — pin at
  implementation time, per house rule.
- **alacritty_terminal snapshot/serialization API:** believed absent (VT
  state is memory) — if B is adopted, confirm early; a server that crashes
  also loses scrollback (same fragility as tmux), which is why A's fresh
  restart remains the honest baseline for "come back tomorrow".
- **WSL acceptance under a server:** idle-CPU <2% posture (PLAN.md) with a
  daemon resident; process-group kill tests vs graceful handoff; the rewritten
  exit criterion — all re-open under B/D.
- **OSC-7777 turn markers (Pd):** the pre-scan decoder exists (#108); whether
  each agent CLI's hooks can emit turn boundaries (and how noisy that is) is
  unverified — cost is real but the channel is proven.
- **B under the one-request-per-frame ctl model:** streaming + backpressure
  policy for slow clients (bounded ring + drop policy; PLAN.md's
  "no unbounded memory growth" applies).
- **Windows:** the whole ctl surface is POSIX-gated (`#[cfg(unix)]`,
  precedent `src/session/outer.rs`); B/D deepen that gating. Named pipes stay
  a later track.
- **Constitution drift risk:** implementing any of A–E, Pa–Pe without the
  explicit amendment + user verdict would repeat the #101/#106 lesson
  (authority edits require prior approval). The brief proposes; the user
  decides; workers implement to a decided spec.

---
*Research-only brief; recommendation, not decision. Grounding: repo docs and
code cited inline; labels: [fact, verified] = read in this session's sources;
[fact, verified in prior study] = confirmed by the recorded #71/#72 research;
[knowledge] = from training, flagged for impl-time verification. No tracked
code was changed to produce this file.*