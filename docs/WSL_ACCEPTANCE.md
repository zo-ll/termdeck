# WSL manual acceptance

**Status (2026-09-05): this is the procedure, not a record of a run.** No pass
against any revision is recorded here or in COORDINATION.md, where the
integration-and-acceptance issue (#4) is still open; the audit that raised
#129 read the file as evidence and found none. Whoever runs it should replace
this paragraph with what they ran, on what revision, and what failed —
"verified against main @ `<commit>` on `<date>`, all steps passed except …" —
so the next reader can tell a procedure from a result.

Run these steps from WSL in a real terminal. Build the binary first, then
keep a second WSL shell available for the CPU, signal, and orphan checks.

```bash
cargo build
td=target/debug/termdeck
```

## Entry points

`$td` opens the folder picker without reading a configuration file. Confirm
the available roots are listed; use `Enter` or a single click to select,
`Right` (or a second click) to descend, and `o` to launch. `Escape` returns to
the normal terminal and prints `picker cancelled` without starting a shell.

In the picker, select several repositories with `Enter`, add an extra instance
with `+`, and launch with `o`. Confirm the first selected repository is master
and every selected instance is a live pane. Check keyboard/mouse parity for
selection (`Enter`/click), descent (`Right`/second click), and ranges
(`Shift+Up`/`Shift+Down` and Shift-click).

Prepare a disposable folder surface for direct-folder launches:

```bash
acceptance_root=/tmp/termdeck-accept
rm -rf "$acceptance_root"
mkdir -p "$acceptance_root/single" \
  "$acceptance_root/repos/frontends/web" \
  "$acceptance_root/repos/backends/api" \
  "$acceptance_root/repos/apps/mobile"
git -C "$acceptance_root/repos/frontends/web" init -q
git -C "$acceptance_root/repos/backends/api" init -q
git -C "$acceptance_root/repos/apps/mobile" init -q
```

- Run `$td .` from a repository. It must open exactly one `bash -l` terminal
  in that repository; `pwd` confirms the directory.
- Run `$td "$acceptance_root/single"`. It must also open exactly one terminal,
  even though the folder is not a repository.
- Run `$td "$acceptance_root/repos"`. It must discover and launch `fe-web`,
  `be-api`, and `app-mobile` in that order.
- Run `$td --config examples/<example>.yaml check`, `list`, and `idp`, using
  one of the configurations shipped under `examples/`. The configured launch
  remains unchanged: frontend is master; backend and an available app are live
  previews in their configured directories.

For every launch shape, verify `Ctrl+g 2`, `Ctrl+g 3`, `Ctrl+g j`, and
`Ctrl+g k` promote panes without stopping the old master. Check `Ctrl+g z`,
`Ctrl+g [`, scrollback navigation, `Escape`, coloured Unicode output,
bracketed paste, Vim, Codex, `Ctrl+C`, resize, and `Ctrl+g r` after exiting a
shell.

While a session is running, press `Ctrl+g a` or click the status-bar `+`.
Mark repositories in the add sheet, including a second instance with `+`, and
press `o` (or click the add button). New terminals must appear live without
disturbing existing panes. `Escape` closes the sheet without adding anything.

## Errors, lifecycle, and performance

These failure paths must be actionable and must not enter the alternate screen
or start a shell:

```bash
$td "$acceptance_root/missing"
$td --not-an-option
```

The first names the missing path; the second names the unknown option. In the
picker, navigate to a folder unreadable by the normal WSL user: it must show a
`cannot read` message and a route back rather than pretending the folder is
empty. For configuration validation, temporarily point a required terminal
`cwd` at a missing directory and rerun the configured launch; it must name the
terminal and path before starting any shell.

With the workspace idle (no terminal output or input), sample Termdeck's CPU
usage from the second WSL shell for at least ten seconds:

```bash
pidstat -p "$(pgrep -n termdeck)" 1 10
```

The reported `%CPU` must remain below 2% in the acceptance environment.

Use `Ctrl+g q`, then `y`, and confirm it exits promptly, restores normal echo,
cursor, and screen handling, and leaves no shell behind. Repeat with `kill
-TERM $(pgrep -n termdeck)` and `kill -INT $(pgrep -n termdeck)` from the
second shell; each must restore the outer terminal. Finally check:

```bash
pgrep -af '/bin/bash -l'
```

No shell owned by the accepted session may remain. Remove the disposable
surface when finished:

```bash
rm -rf "$acceptance_root"
```
