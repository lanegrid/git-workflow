# git-workflow

A type-safe Git workflow CLI with worktree support and GitHub integration.

## Features

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
git clone https://github.com/zawakin/git-workflow.git
cd git-workflow
cargo install --path .
```

## Usage

The CLI is invoked using the `gw` command.

### Commands

#### `gw home`

Switch to home branch and sync with `origin/main`.

```bash
gw home
```

#### `gw new <branch>`

Create a new branch from `origin/main`.

```bash
gw new feature/add-login
gw new fix/memory-leak
```

#### `gw status`

Show current repository state including:
- Current branch and sync status
- Working directory state
- Suggested next action

```bash
gw status
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

## Home Branch

The "home branch" is determined by:
1. In a git worktree: the worktree directory name
2. Otherwise: `main` (or `master` if main doesn't exist)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
