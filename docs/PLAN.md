# Termdeck implementation plan

## Product

Termdeck is an external, generic Rust TUI for opening the host terminals that
belong to one logical workspace. A Horizon workspace can contain frontend,
backend, and optional app repositories, but Horizon knowledge lives only in
user configuration.

V1 targets Linux and WSL2 x86-64. It does not manage Docker, discover projects,
forward mouse input, persist sessions, or create panes dynamically.

## Runtime architecture

- Ratatui and Crossterm render the outer interface and collect input.
- `alacritty_terminal` maintains VT state, cells, modes, colors, Unicode,
  cursor state, and scrollback.
- `portable-pty` owns PTYs, child processes, input, and resize propagation.
- One reader thread per PTY sends bounded output and lifecycle events to the
  main thread.
- The main thread owns terminal-state mutation and UI rendering.
- All ordinary input is forwarded to the master PTY. `Ctrl+g` is the Termdeck
  command prefix; pressing it twice sends a literal `Ctrl+g`.

The shared contract must use owned application types:

```text
EngineCommand: Input | Resize | Respawn | Shutdown
EngineEvent: FrameReady | StatusChanged
TerminalFrame: dimensions, cells, cursor, revision
TerminalStatus: Starting | Running | Exited(code) | Failed(message)
```

It must not expose Ratatui, Alacritty, or PTY types.

## Master-and-preview-stack interface

- The active terminal is the master and receives approximately 70% of width.
- Other terminals are read-only live previews stacked in the remaining width.
- Selecting a preview promotes it to master and returns the old master to the
  stack.
- Zoom hides previews and gives the master the complete screen.
- Below a usable preview width, use master-only mode with a compact terminal
  status line.
- Support one or more configured terminals. The first starts as master.
- `master_ratio` defaults to `0.85` — the stack starts at its minimum width,
  since the previews start folded — and accepts `0.55..=0.85`. It seeds the
  split; the divider between the master and the stack moves it for the session,
  by drag or by `Ctrl+g -` / `Ctrl+g =`, within the same range. Nothing is
  written back to the configuration file (#41).

Bindings:

```text
Ctrl+g j/k or arrows   Select and promote terminal
Ctrl+g N               Promote by terminal number
Ctrl+g z               Toggle zoom
Ctrl+g -  Ctrl+g =     Narrow / widen the master (the divider, by keyboard)
Ctrl+g [               Enter scrollback mode
Ctrl+g r               Respawn active terminal
Ctrl+g ?               Help
Ctrl+g q               Quit
Ctrl+g Ctrl+g          Send literal Ctrl+g
```

## Configuration and CLI

Load `$XDG_CONFIG_HOME/termdeck/config.yaml`, falling back to
`~/.config/termdeck/config.yaml`; `--config` overrides it.

Commands:

```text
termdeck [WORKSPACE]
termdeck --config PATH [WORKSPACE]
termdeck list
termdeck check
```

No workspace argument opens a searchable picker. Relative terminal paths
resolve against the workspace root. Missing required paths fail before any PTY
starts; missing optional paths are omitted. Commands are argv arrays and are
never implicitly shell-evaluated. Environment overrides apply after inheriting
the parent environment.

## Lifecycle

Closing Termdeck closes its shells. Confirmed exit sends `SIGTERM` to owned
process groups, waits two seconds, then sends `SIGKILL` to survivors. An exited
terminal preserves its frame, scrollback, and exit code and can be respawned.
RAII guards, panic hooks, and handled signals must restore the outer terminal.

## Delivery sequence

1. Codex creates the shared contracts, CLI/config scaffold, and deterministic
   fake terminal engine.
2. From that merged foundation, Claude implements `src/ui/` and visual snapshot
   tests while Codex implements the real engine and lifecycle in parallel.
3. Codex performs the final wiring and WSL acceptance pass.
4. The coordinator independently reviews diffs and tests. Workers never merge.

## Acceptance

- `termdeck idp` opens frontend as master with backend and optional app visible
  as live previews in their configured host directories.
- Promotion, zoom, scrollback, resize, paste, Unicode, colors, Vim, Codex, and
  `Ctrl+C` work correctly.
- Missing paths and invalid configuration produce actionable errors.
- Simultaneous output remains responsive without unbounded memory growth.
- Idle CPU remains below 2% in the acceptance environment.
- Normal exit, panic, `SIGINT`, and `SIGTERM` restore the outer terminal.
- Confirmed exit leaves no owned child processes.
