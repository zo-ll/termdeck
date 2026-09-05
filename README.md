# Termdeck

Termdeck is a standalone, configurable terminal workspace. One interactive
terminal occupies the master area while related terminals remain visible as a
stack of live previews.

The application is generic. Support for any particular workspace is provided
through configuration, not through workspace-specific source code or changes to
the tools that workspace runs.

## Status

A workspace opens as an interactive master-and-preview terminal deck, whether
it comes from a configuration file, from a folder Termdeck discovers
repositories in, or from the picker. Terminals can be added and closed while
the session runs, and a running session can be inspected and driven from
another shell with `termctl`. See [the WSL manual acceptance
pass](docs/WSL_ACCEPTANCE.md) for the end-to-end procedure.

## Toolchain

Termdeck is pinned to Rust 1.98.0. Install the official Rust toolchain manager
inside WSL:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup update stable
rustc --version
cargo --version
```

Expected compiler version:

```text
rustc 1.98.0
```

The existing `/usr/bin/rustc` may remain installed. Ensure `$HOME/.cargo/bin`
appears before `/usr/bin` in `PATH` so the rustup-managed toolchain is selected.

## Running Termdeck

The repository builds two binaries, `termdeck` and `termctl`. `termdeck` is
the default Cargo run target; choose `termctl` explicitly when needed:

```bash
cargo run -- [ARGUMENTS]
cargo run --bin termctl -- [ARGUMENTS]
```

What `termdeck` opens is decided by what it is given:

| Invocation | What it opens |
| --- | --- |
| `termdeck` | The repository picker, over the working directory and `$HOME`. No configuration file is read. |
| `termdeck FOLDER` | That folder directly. Repositories under `frontends/`, `backends/` and `apps/` become `fe-`, `be-` and `app-` terminals; other repositories in the folder become one terminal each; a folder with nothing to discover — a repository itself, or a plain directory — becomes a single terminal of its own. |
| `termdeck CONFIG_FILE` | That configuration file's workspace. |
| `termdeck --config PATH [WORKSPACE]` | `WORKSPACE` from `PATH`. |
| `termdeck check` / `termdeck list` | Nothing: it inspects the configuration and exits. |
| `termdeck -- NAME` | `NAME` as a path or a workspace, never as a verb. |

A bare positional argument is a **path**, not a workspace name. It has to
exist, and whether it is a directory or a file decides which of the two shapes
above applies; a name that is neither is an error rather than a guess at a
misspelled workspace. A workspace is named only alongside `--config`, and only
when the file holds more than one — a file with exactly one workspace opens it
without being asked, and a file with several lists them instead of choosing.

`check` and `list` are ordinary words, and a folder or a workspace is allowed
to be called one. A bare `check` is always the command — a command that meant
something different depending on what happened to sit in the working directory
would be worse than the collision — so `--` ends the verbs and whatever follows
it is a name: `termdeck -- check` opens the folder, and `termdeck --config
work.yaml -- check` opens the workspace. A path that is spelled as one, like
`./check`, was never the verb to begin with.

`$XDG_CONFIG_HOME/termdeck/config.yaml`, falling back to
`$HOME/.config/termdeck/config.yaml`, is the configuration `check` and `list`
read when `--config` does not name another. Launching reads only the file it
was given, by `--config` or as a positional path.

## Configuration commands

The example configurations under `examples/` can be validated and listed
without starting any terminals:

```bash
cargo run -- --config examples/<example>.yaml check
cargo run -- --config examples/<example>.yaml list
```

`check` validates workspace roots, required terminal paths, and command argv
arrays. Missing optional terminal paths are omitted by `list`.

A configuration file describes workspaces, their roots, and their terminals.
There is no environment-override key: a terminal's command inherits the
environment Termdeck was started in, plus the pane variables Termdeck sets
itself (`TERMDECK_SOCK`, `TERMDECK_PANE`). Unknown keys are rejected rather
than ignored.

## Shell notifications

Default bash, zsh, and fish panes automatically notify when a command exits
non-zero or runs for at least 10 seconds. Set `TERMDECK_NOTIFY` to `none`,
`error`, `long`, or `all`, and set `TERMDECK_NOTIFY_LONG_SECS` to change the
long-command threshold. Use `termctl notify --help` for explicit notifications.

## Current checks

Once Rust 1.98.0 is installed:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

See [docs/PLAN.md](docs/PLAN.md) and
[docs/design/termdeck/DESIGN.md](docs/design/termdeck/DESIGN.md) before making
implementation changes.

To continue this coordinated build from a fresh machine or fresh agent
sessions, follow [docs/RESUME.md](docs/RESUME.md). Current ownership and branch
status live in [COORDINATION.md](COORDINATION.md).
