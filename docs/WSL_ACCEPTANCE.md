# WSL manual acceptance

Run these steps from WSL in a real terminal, after the Horizon paths in
`examples/horizon.yaml` exist:

```bash
cargo run -- --config examples/horizon.yaml check
cargo run -- --config examples/horizon.yaml list
cargo run -- --config examples/horizon.yaml idp
```

Confirm `frontend` starts as master and `backend` and (when present) `app` are
live previews in their configured directories. Run `pwd` in each after using
`Ctrl+g 2`, `Ctrl+g 3`, `Ctrl+g j`, and `Ctrl+g k`; the promoted shell must be
the master and the old master must remain live in the stack. Check `Ctrl+g z`
toggles zoom, `Ctrl+g [` enters scrollback, navigation keys move it, and
`Escape` returns to live output.

In the master, verify a coloured Unicode command, a bracketed paste, Vim, a
Codex session, and `Ctrl+C`. Resize the outer terminal and run `stty size` in
each promoted terminal to confirm the new dimensions. Exit one shell, then use
`Ctrl+g r` to respawn it and confirm its pane becomes running again.

With the workspace idle (no terminal output or input), sample Termdeck's CPU
usage from another WSL shell for at least ten seconds. For example:

```bash
pidstat -p "$(pgrep -n termdeck)" 1 10
```

The reported `%CPU` must remain below 2% in the acceptance environment.

Use `Ctrl+g q`, then `y`, and confirm the outer terminal has its normal echo,
cursor, and screen back. Repeat once with `kill -TERM $(pgrep -n termdeck)` and
once with `kill -INT $(pgrep -n termdeck)` from another WSL shell; each must
restore the outer terminal. Finally, check that no configured shell remains:

```bash
pgrep -af '/bin/bash -l'
```

For validation failure, temporarily point a required terminal `cwd` at a
missing directory and rerun the launch command. It must print the configured
terminal name and path before entering the alternate screen or starting any
shell.
