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
mkdir -p ../termdeck-worktrees
git worktree add ../termdeck-worktrees/01-contracts coord/01-contracts
git worktree add ../termdeck-worktrees/03-ui coord/03-ui
```

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
