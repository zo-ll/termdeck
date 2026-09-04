# Termdeck

Termdeck is a standalone, configurable terminal workspace. One interactive
terminal occupies the master area while related terminals remain visible as a
stack of live previews.

The application is generic. Support for any particular workspace is provided
through configuration, not through workspace-specific source code or changes to
the tools that workspace runs.

## Status

The configured workspace opens as an interactive master-and-preview terminal
deck. See [the WSL manual acceptance pass](docs/WSL_ACCEPTANCE.md) for the
end-to-end check.

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

## Configuration commands

The example configurations under `examples/` can be validated and listed
without starting any terminals:

```bash
cargo run -- --config examples/<example>.yaml check
cargo run -- --config examples/<example>.yaml list
```

The command shape is `termdeck [--config PATH] [WORKSPACE|check|list]`. Without
`--config`, Termdeck loads `$XDG_CONFIG_HOME/termdeck/config.yaml`, falling
back to `$HOME/.config/termdeck/config.yaml`.

`check` validates workspace roots, required terminal paths, and command argv
arrays. Missing optional terminal paths are omitted by `list`.

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
