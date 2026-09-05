# Termdeck implementation plan

## Product

Termdeck is an external, generic Rust TUI for opening the host terminals that
belong to one logical workspace. A workspace can contain frontend, backend,
and optional app repositories, but knowledge of any particular workspace lives
only in user configuration.

V1 targets Linux and WSL2 x86-64. It does not manage Docker and does not
persist sessions: closing Termdeck closes its terminals, and nothing is
written back for a later run.

Three of the original exclusions no longer hold, and the plan states what
shipped instead (corrected 2026-09-05, #129):

- **Projects are discovered.** `termdeck FOLDER` reads the folder:
  repositories under `frontends/`, `backends/` and `apps/` become `fe-`,
  `be-` and `app-` terminals, other repositories in it become one terminal
  each, and a folder with nothing to discover becomes a single terminal. The
  picker finds repositories too — by browsing, and by a filter that searches
  the configured roots three folders deep — and a running session is
  discoverable from outside through the control socket (`termctl status`,
  `list`, `peek`).
- **Mouse input is handled.** The deck takes clicks, drags on the divider,
  and the wheel; the picker and the runtime-add sheet take clicks and ranges.
  A wheel over a pane running a full-screen application is forwarded to it,
  and otherwise scrolls Termdeck's own scrollback.
- **Panes are created and closed dynamically.** `Ctrl+g a` opens the
  runtime-add sheet and `Ctrl+g x` closes the pane in the master frame;
  `termctl open` and `termctl close` do the same from another shell. The
  configured set is where a session starts, not what it is fixed to.

## Runtime architecture

- Ratatui renders the outer interface. Termdeck owns the terminal itself: its
  own ANSI backend writes the frames and its own decoder reads keys, mouse
  reports, and bracketed paste, so Crossterm is not a dependency (corrected
  2026-09-05, #129).
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
EngineCommand: Input | Resize | Scroll | Respawn | Shutdown
EngineEvent: FrameReady | StatusChanged | MetadataChanged | Notify
             | InputDropped | InputQueued
TerminalFrame: dimensions, cells, cursor, revision
TerminalStatus: Starting | Running | Exited(code) | Failed(message)
```

The list has grown additively since v1 — scrollback (`Scroll`), pane metadata,
notifications (#97), and the two acknowledgements a bounded input queue owes
its caller (#118) — and every addition kept the rule below.

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
Ctrl+g PgUp/PgDn       Page the stack window
Ctrl+g z               Toggle zoom
Ctrl+g c               Collapse or expand every preview
Ctrl+g p               Pin the master to the top of the stack, or unpin it
Ctrl+g -  Ctrl+g =     Narrow / widen the master (the divider, by keyboard)
Ctrl+g a               Add terminals to the running session
Ctrl+g x               Close the terminal in the master frame
Ctrl+g [               Enter scrollback mode
Ctrl+g r               Respawn active terminal
Ctrl+g ?               Help
Ctrl+g q               Quit
Ctrl+g Ctrl+g          Send literal Ctrl+g
```

## Configuration and CLI

`check` and `list` read `$XDG_CONFIG_HOME/termdeck/config.yaml`, falling back
to `~/.config/termdeck/config.yaml`, unless `--config` names another file.
Launching reads only the file it is given.

Commands:

```text
termdeck                            the picker
termdeck FOLDER                     discovery: open that folder
termdeck CONFIG_FILE                open that file's workspace
termdeck --config PATH [WORKSPACE]  open a named workspace
termdeck list
termdeck check
```

No argument opens the picker. A bare positional argument is a path that has to
exist — a directory is a folder launch, a file is a configuration — so a
workspace is named only alongside `--config`, and only when the file holds
more than one (corrected 2026-09-05, #129: the plan had a bare `[WORKSPACE]`
shape the CLI does not accept).

Relative terminal paths resolve against the workspace root. Missing required
paths fail before any PTY starts; missing optional paths are omitted. Commands
are argv arrays and are never implicitly shell-evaluated. A terminal inherits
the environment Termdeck was started in, plus the pane variables Termdeck sets
for the control socket and the shell hook; the configuration schema has no
environment-override key and rejects unknown ones (corrected 2026-09-05, #129:
the plan promised overrides no schema ever carried).

## Lifecycle

Closing Termdeck closes its shells. Confirmed exit sends `SIGTERM` to owned
process groups, waits two seconds, then sends `SIGKILL` to survivors. An exited
terminal preserves its frame, scrollback, and exit code and can be respawned.

Ownership is enforced session-wide, not group-wide: interactive shells create
separate process groups for jobs, so on Linux shutdown enumerates session
members and descendants per PID via `/proc` (`kill(-id)` only addresses a
process group; there is no session-signalling syscall), guarded against PID
reuse by start times recorded at spawn, with zombies excluded. The force stage
fires when the process group OR session is still alive — a dead shell with live
jobs still gets killed. Reader/waiter joins are bounded (~0.5 s) and then
detach stragglers instead of hanging, so confirmed exit completes within grace
(2 s) + settle (0.5 s) + join (0.5 s). Non-Linux retains group-only behavior
(documenting in the code).

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
