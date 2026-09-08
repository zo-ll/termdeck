# Agent Instructions

Read `docs/PLAN.md` and `docs/design/termdeck/DESIGN.md` before changing code.
Read `COORDINATION.md` for current workstream status and `docs/RESUME.md` when
adopting work from a new machine or agent session.

## Architecture boundaries

- Shared engine/UI contracts belong in `src/contracts/` and must not expose
  Ratatui, Alacritty, or PTY implementation types.
- Presentation code belongs in `src/ui/`.
- PTY and terminal-emulation code belongs in `src/engine/`.
- Configuration and CLI code belong in `src/config/` and `src/cli/`.
- Horizon integration must remain configuration-only.
- Do not add tmux, OpenMux, or a background daemon. Session persistence is
  limited to on-disk snapshots of layout + visible pane text restored into
  fresh shells (see docs/design/termdeck/sessions*); live-process resume stays
  out of scope.

## Agent ownership

- Claude Code owns visual implementation, UI fixtures, and rendering snapshots.
- Codex owns contracts, CLI/configuration, PTYs, terminal emulation, lifecycle,
  and integration.
- Do not change another worker's ownership area without coordinator approval.

## Verification

Run before handoff:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Workers must not merge their own branches. Report changed files, verification
results, risks, and deviations in the final handoff.
