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
| `git-workflow cleanup [branch]` | Delete merged branch (checks PR status) |
| `git-workflow pause [msg]` | WIP commit + return home |
| `git-workflow abandon` | Discard changes + return home |
| `git-workflow undo` | Soft reset last commit |

> **Tip**: `gw` is available as a shorthand alias (e.g., `gw status`).

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
