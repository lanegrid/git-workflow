# git-workflow

[![Crates.io](https://img.shields.io/crates/v/git-workflow.svg)](https://crates.io/crates/git-workflow)
[![License](https://img.shields.io/crates/l/git-workflow.svg)](https://github.com/lanegrid/git-workflow#license)

**Git guardrails for AI coding agents.**

A CLI that wraps common git workflows with safety checks and clear state feedback—designed so AI agents (and humans) can operate git repositories without footguns.

## Why?

AI coding agents like Claude Code are powerful, but git is full of sharp edges:

- Branching from stale `main` → merge conflicts later
- Forgetting to check PR status before deleting branches → lost work
- Running `git pull` on a diverged branch → unexpected merge commits
- Deleting protected branches → disaster

`git-workflow` solves this by providing:

| Problem | gw Solution |
|---------|-------------|
| Stale branches | Auto-fetches before every operation |
| Accidental deletions | Type-safe branch protection |
| Unclear state | `gw status` shows exactly what to do next |
| Diverged branches | Fast-forward only, with clear error + hints |
| Context switching | `gw pause` / `gw abandon` for clean exits |

## For AI Agents

`git-workflow` is designed to give AI agents the context they need to make good decisions:

```
$ git-workflow status
ℹ Branch: feature/add-auth
ℹ PR: #42 (open, 2 commits ahead)
✓ Working directory: clean
→ Next: ready to push or create PR
```

The structured output and `→ Next:` hints make it easy for agents to understand repository state and choose appropriate actions.

### Claude Code Integration

Add to your project's `CLAUDE.md`:

```markdown
## Git Workflow

Use `git-workflow` commands for git operations:

- `git-workflow new feature/name` - Create branch (auto-fetches from origin)
- `git-workflow status` - Check state and see next action
- `git-workflow pause "message"` - Save WIP and switch context
- `git-workflow cleanup` - Delete merged branch safely
- `git-workflow sync` - Update after base PR merged
- `git-workflow open` - Open the current branch's PR in the browser
```

Or use the `/git-workflow` skill if configured.

## Quick Start

```bash
# Install
cargo install git-workflow

# Start work
git-workflow new feature/user-auth
# → Creates branch from latest origin/main

# Check status anytime
git-workflow status
# → Shows state + suggested next action

# Clean up after merge
git-workflow cleanup
# → Verifies PR merged, deletes branch, returns home
```

## Commands

| Command | Description |
|---------|-------------|
| `git-workflow new <branch>` | Create branch from `origin/main` (fetches first) |
| `git-workflow status` | Show state and suggested next action |
| `git-workflow home` | Return to home branch, sync with origin |
| `git-workflow sync` | Sync current branch with origin/main |
| `git-workflow open` | Open the current branch's PR in the browser |
| `git-workflow cleanup [branch]` | Delete merged branch (checks PR status) |
| `git-workflow pause [msg]` | WIP commit + return home |
| `git-workflow abandon` | Discard changes + return home |
| `git-workflow undo` | Soft reset last commit |

> **Tip**: `gw` is available as a shorthand alias (e.g., `gw status`).

### Opening PRs in the browser

`gw open` looks up the PR for the current branch with `gh` and opens its URL.
The program used to open the URL is resolved in this order:

1. `$GW_OPEN_URL_CMD` — gw-specific override
2. `$OPEN_URL_CMD` — generic override (shared with other tooling)
3. OS default — `open` (macOS) or `xdg-open` (Linux)

Set `GW_OPEN_URL_CMD`/`OPEN_URL_CMD` to a script that takes the URL as `$1`.
This keeps environment-specific behavior (e.g. a dedicated Chrome profile) in
your dotfiles instead of the CLI. Example:

```bash
# ~/.config/fish/config.fish (or your shell's rc)
set -gx GW_OPEN_URL_CMD "$HOME/dotfiles/scripts/open-chrome-pr.sh"
```

### Safety Features

- **Auto-fetch**: Every command fetches from origin first
- **Protected branches**: Cannot delete `main`, `master`, or home branch
- **PR verification**: `cleanup` checks GitHub PR status before deletion
- **Fast-forward only**: `sync`/`home` refuse to create merge commits on diverged branches

## Git Worktree Support

Works seamlessly with git worktrees. Each worktree has its own "home branch":

```bash
# Main repo → home is 'main'
# Worktree → home is the worktree's branch

git worktree add ../feature-x feature-x
cd ../feature-x
git-workflow status  # Home: feature-x
```

### Worktree Pool

Pre-warm a pool of ready-to-use worktrees for AI agent teams. Instead of each agent spending 1-2 minutes on `git worktree add` + project setup, they acquire a pre-built worktree in ~10ms.

```bash
# Pre-create 5 worktrees from origin/main
gw worktree pool warm 5

# Agent acquires a worktree (prints path to stdout)
path=$(gw worktree pool acquire)
cd "$path" && # do work...

# Agent releases when done (resets to clean state)
gw worktree pool release          # auto-detects from cwd
gw worktree pool release pool-001 # or by name

# Check pool state
gw worktree pool status
# Pool: 4 available, 1 acquired, 5 total
#
# NAME         STATUS       PID      PATH
# pool-001     acquired 12345   /path/.worktrees/pool-001
# pool-002     available -       /path/.worktrees/pool-002
# ...

# Tear down everything
gw worktree pool drain
```

| Command | Description |
|---------|-------------|
| `gw worktree pool warm <n>` | Pre-create N available worktrees |
| `gw worktree pool acquire` | Claim a worktree (prints path to stdout) |
| `gw worktree pool release [id]` | Reset and return worktree to pool |
| `gw worktree pool status` | Show pool state |
| `gw worktree pool drain [--force]` | Remove all pool worktrees |

**Setup hook**: Place an executable at `.gw/setup` to run project-specific initialization (e.g., `npm install`) on each worktree. It receives the worktree path as `$1`.

## Prerequisites

- Git
- [GitHub CLI (gh)](https://cli.github.com/) - Required for `cleanup` and `sync`

## Installation

```bash
# Quick install (macOS/Linux)
curl -fsSL https://github.com/lanegrid/git-workflow/releases/latest/download/install.sh | sh

# With Cargo
cargo install git-workflow

# From source
git clone https://github.com/lanegrid/git-workflow.git
cd git-workflow
cargo install --path .
```

Pre-built binaries are available for:
- macOS (Intel & Apple Silicon)
- Linux (x64 & ARM64)
- Windows (x64)

## License

MIT OR Apache-2.0

## Contributing

Contributions welcome! Please submit a Pull Request.
