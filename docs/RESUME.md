# Resuming Termdeck work

The repository is the durable handoff. The tmux session and agent conversation
history are conveniences, not required state.

## Before leaving a machine

1. Ask each worker to commit its finished or safe checkpoint on its assigned
   `coord/*` branch.
2. Update `COORDINATION.md` with the commit, checks, remaining work, and any
   blocker.
3. Commit coordination-document changes on `main`.
4. Push `main` and all active worker branches:

   ```bash
   git push -u origin main
   git push -u origin coord/01-contracts coord/03-ui
   ```

Never depend on uncommitted files, tmux scrollback, ignored `.scratch` files,
or a previous agent conversation when moving machines.

## Fresh-machine setup

Install Git, tmux, Rust through rustup, Codex, and Claude Code. Configure the
requested Claude launcher if it is not already present:

```bash
alias claudep='claude --dangerously-skip-permissions'
```

Persist that alias in the shell startup file used on the new machine. Then:

```bash
git clone https://github.com/zo-ll/termdeck.git termdeck
cd termdeck
git fetch origin '+refs/heads/coord/*:refs/remotes/origin/coord/*'
rustup show
cargo test --all-targets
```

Read, in order:

1. `AGENTS.md`
2. `COORDINATION.md`
3. `docs/PLAN.md`
4. `docs/design/termdeck/DESIGN.md`
5. the relevant section of `docs/WORKSTREAMS.md`

## Recreate isolated worktrees

Only recreate branches still marked active in `COORDINATION.md`. For example:

```bash
mkdir -p ~/.worktrees/termdeck
git worktree add ~/.worktrees/termdeck/01-contracts coord/01-contracts
git worktree add ~/.worktrees/termdeck/03-ui coord/03-ui
```

Worktree location is a local convention; this machine uses `~/.worktrees/termdeck/`.

If a branch only exists on the remote, create its local tracking branch first:

```bash
git branch --track coord/01-contracts origin/coord/01-contracts
```

## Start completely new agent sessions

Create a visible tmux session with one window per active worktree:

```bash
tmux new-session -d -s termdeck-agents -n codex -c ../termdeck-worktrees/01-contracts
tmux new-window -t termdeck-agents -n claude -c ../termdeck-worktrees/03-ui
tmux send-keys -t termdeck-agents:codex \
  'codex -m gpt-5.6-terra -c model_reasoning_effort="high" -s workspace-write -a never --no-alt-screen' Enter
tmux send-keys -t termdeck-agents:claude \
  'bash -ic "claudep --model opus --effort high"' Enter
tmux attach -t termdeck-agents
```

Give each new agent this bootstrap prompt, followed by its workstream section:

```text
Adopt the active Termdeck workstream in this worktree. Read AGENTS.md,
COORDINATION.md, docs/PLAN.md, docs/design/termdeck/DESIGN.md, and the relevant
section of docs/WORKSTREAMS.md. Inspect the branch history and working tree.
Verify existing claims yourself, continue only your assigned ownership area,
run the required checks, commit on this branch, and do not merge or push.
```

The coordinator reviews worker commits and updates `COORDINATION.md`. Workers
never merge their own branches.

## Verify synchronization

The private remote is `https://github.com/zo-ll/termdeck`. The new machine must
authenticate to GitHub as an account with access. Check synchronization with:

```bash
git remote -v
git branch -vv
```

## Session snapshot — 2026-09-07 EOD

State: main `30eaa4e`, CI GREEN (first since 2026-09-05 — the second astra
audit queue #136-#147 is fully closed, plus user-visible #148/#149/#150/#151).

Fresh-machine/local notes:
- Local zsh/fish absent by default. To run the zsh shell-hook tests locally
  (they skip without zsh), use a real zsh: `PATH=/tmp/zsh-local/bin:$PATH
  ZDOTDIR=/tmp/zshlocal-home` (apt zsh extracted to /tmp/zsh-local with a
  module_path fix; /tmp/zshlocal-home/.zshrc sets module_path). CI's apt
  zsh is the authoritative judge.
- The zsh ready-gate (#136 r4) works by opting out of Ubuntu's global
  compinit (skip_global_compinit=1 in the hook's .zshenv) — see
  src/engine/shell_hook.rs.
- Login-bash single-prompt (#151) uses a profile shim: HOME scoped to a
  generated dir whose .bash_profile restores the real HOME, runs the user's
  login chain, then installs the hook. Accepted residual (issue #152):
  /etc/profile runs before the shim, so the sudo hint may repeat and user
  bash_completion may be missed.
- CI retry-step guard (#130) was race-fixed 2026-09-07: it captures the
  `--list` output into a var before grep (a live pipe's -q early-exit gave
  cargo a BrokenPipe that failed the pipeline under pipefail).

Remaining open: audit P1s #143 #139 #142 #141 → P2s #144 #145 #146 #147 →
#134; user-gated #94 (MCP merge/drop), #112 (persistence gate), #114
(discovery), #33 (parked); #152; NB backlog (~65).
