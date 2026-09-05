# Probe sources — second astra audit of `b3df768`

Reproduction material for `docs/AUDIT-2026-09-05.md` and issues #139-#147.
Originally written to `/tmp/termdeck-audit-_1hmdxuj/`, which does not survive a
reboot; preserved here on 2026-09-05.

**Sources only.** The six ELF binaries in the original directory (~46 MB each,
~280 MB total) were not copied — they rebuild from the `.rs` files beside this
README. The two `.log` files were renamed to `.txt` because the repository's
`.gitignore` excludes `*.log`, which would have dropped them silently.

## Building the Rust probes

They link the built library directly rather than living in the crate, so they
do not add test targets or affect the gate:

```bash
cargo build            # debug, for the overflow-check probes
cargo build --release  # release, for what actually ships
rustc --edition 2024 -L target/debug/deps \
  --extern termdeck=target/debug/libtermdeck.rlib \
  <probe>.rs -o /tmp/<probe>
```

`deck-geometry.rs` additionally needs `--extern ratatui=$(ls
target/debug/deps/libratatui-*.rlib | head -1)`.

## `astra/` — the audit's own probes

| File | Finding | What it does |
| --- | --- | --- |
| `probe.rs` | #141 | Forks a `setsid` child and checks it survives shutdown |
| `memory.rs` | #142 | Feeds an unterminated `ESC ] 0 ;` in 4 KiB chunks, samples RSS |
| `ui.rs`, `ui_verbose.rs` | #140 | The render boundary sweep (2,860 cases) |
| `discovery.rs` | #144b | Duplicate terminal identities from folder discovery |
| `socket_path.rs` | audit §security | Symlinked socket directory chmod |
| `live.py` | #140a, #143 | Drives a live session and resizes it to 12 rows |
| `paste.py` | #139 | Sends a paste payload carrying an embedded terminator |
| `perf.py` | audit §perf | 16 idle panes, 10-second CPU sample |
| `child.py` | #141 | The escaping child used by `probe.rs` |

Evidence: `ui-results.txt` (sweep summary), `ui-panics.txt` (per-case panic
sites), `live-abort-excerpt.txt` (the abort chain, filtered out of the 219 KB
raw terminal capture, which was not committed), `dependency-audit.json`,
`live.json`, `bash-hook` (#147's captured snippet).

## `verification/` — independent re-verification

Written while checking the audit rather than by it. All 13 findings were
re-derived from source; these two reproduce five of them from scratch.

- `picker-and-discovery.rs` — #144a (`İx` filtered by `x`: `end byte index 4 is
  out of bounds for string of length 3`) and #144b (`["fe-web", "fe-web"]`).
- `deck-geometry.rs` — #140b at 80x4, 120x5 and controls. Build it **twice**:
  debug panics at `deck.rs:1381` with `attempt to subtract with overflow`, while
  release (no `overflow-checks` in `[profile.release]`) panics at 80x4 inside
  ratatui's buffer bounds and does **not** panic at 120x5, where the wrap
  silently renders garbage instead. That divergence is why the report's
  "584 panics" is a debug-only figure.

## Not preserved

`backlog.sock` (a socket), `collision/`, `runtime/`, `symlink-probe/` (fixture
directories the probes recreate), `perf-input.bin` and `dependency-audit.stderr`
(both empty), `escaped.pid`, `audit_path`, and the shell history file.
