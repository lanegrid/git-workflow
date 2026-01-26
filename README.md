# git-workflow

[![Crates.io](https://img.shields.io/crates/v/git-workflow.svg)](https://crates.io/crates/git-workflow)
[![License](https://img.shields.io/crates/l/git-workflow.svg)](https://github.com/lanegrid/git-workflow#license)

A type-safe Git workflow CLI with worktree support and GitHub integration.

## Features

- **Always up-to-date** - Automatically fetches from origin, so you never branch from a stale main
- **Type-safe branch protection** - Prevents accidental deletion of protected branches (main, master, home)
- **Git worktree aware** - Automatically detects and works with git worktrees
- **GitHub integration** - Fetches PR information to make informed decisions about branch operations
- **Smart state detection** - Detects repository state and suggests next actions
- **Minimal dependencies** - Built with Rust for fast execution

## Installation

### From crates.io

```bash
cargo install git-workflow
```

### From source

```bash
git clone https://github.com/lanegrid/git-workflow.git
cd git-workflow
cargo install --path .
```

## Quick Demo

```
$ gw status
ℹ Branch: main (home)
→ Next: start new work

$ gw new fix/typo
✓ Created fix/typo from origin/main

# ... make changes, commit ...

$ gw status
ℹ Branch: fix/typo (home: main)
! Unpushed: 1 commit
→ Next: push and create PR

# ... push, create PR, get merged ...

$ gw cleanup
✓ PR #42 [MERGED]
✓ Deleted fix/typo
✓ Back to main
```

## Usage

The CLI is invoked using the `gw` command.

### Commands

#### `gw home`

Switch to home branch and sync with `origin/main` (or `origin/master`).

```bash
gw home
# Main repo → switches to 'main'
# Worktree → switches to home branch
```

#### `gw new <branch>`

Create a new branch from `origin/main`.

```bash
gw new feature/add-login
gw new fix/memory-leak
```

#### `gw status`

Show current repository state including:
- Repository type (main repo or worktree)
- Current branch and sync status
- Working directory state
- Suggested next action

```bash
gw status
# ℹ Repository: worktree (home: feature-x)
# ✓ Branch: feature-x (home)
# ✓ Working directory: clean
# → Next: start new work
```

#### `gw cleanup [branch]`

Delete a merged branch and return to home. Uses GitHub CLI to verify PR merge status.

```bash
gw cleanup              # cleanup current branch
gw cleanup feature/old  # cleanup specific branch
```

#### `gw pause [message]`

Create a WIP commit with all changes and return to home branch.

```bash
gw pause
gw pause "waiting for API review"
```

#### `gw abandon`

Discard all changes and return to home branch.

```bash
gw abandon
```

#### `gw undo`

Undo the last commit (soft reset to HEAD~1).

```bash
gw undo
```

#### `gw sync`

After a base PR is merged, update the base to main and rebase.

```bash
gw sync
```

### Global Options

- `-v, --verbose` - Show git commands being executed

```bash
gw -v status
gw --verbose cleanup
```

## Prerequisites

- Git
- [GitHub CLI (gh)](https://cli.github.com/) - Required for `cleanup` and `sync` commands

## Workflow Example

```bash
# Start new feature
gw new feature/user-auth

# ... work on feature ...

# Need to switch context? Pause work
gw pause "blocked on API changes"

# Switch to another task
gw new fix/urgent-bug

# ... fix the bug ...

# Clean up after PR is merged
gw cleanup

# Resume previous work
git checkout feature/user-auth

# Check status anytime
gw status
```

## Git Worktree Support

`gw` is designed to work seamlessly with [git worktrees](https://git-scm.com/docs/git-worktree), enabling parallel development on multiple features.

### How it works

Each worktree has its own "home branch":

- **Main repo**: `main` (or `master`)
- **Worktree**: Currently uses the directory name

```bash
# Create a worktree
git worktree add ../feature-a feature-a
cd ../feature-a
gw status   # Home: feature-a
```

### Example: Parallel Development

```bash
# Work on feature-a in one worktree
cd ~/projects/feature-a
gw pause "WIP: need input"

# Work on feature-b in another worktree
cd ~/projects/feature-b
gw status   # Home: feature-b

# Resume feature-a
cd ~/projects/feature-a
gw undo     # Undo WIP commit
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
