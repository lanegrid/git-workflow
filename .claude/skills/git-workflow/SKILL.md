---
name: git-workflow
description: Development workflow using gw CLI for feature branches, PRs, and cleanup
argument-hint: <command> [args]
allowed-tools: Bash(gw*), Bash(gh*), Bash(git*), Bash(mise*), Bash(cargo*), Read, Edit, Grep, Glob
---

# git-workflow Development

Use `gw` CLI for all git workflow operations in this project.

## Current State

Branch:
!`git rev-parse --abbrev-ref HEAD`

Status:
!`gw status 2>/dev/null || echo "gw not installed - run: cargo install --path ."`

## Commands

Parse `$ARGUMENTS` and execute the appropriate workflow:

### `new <branch-name>` - Start new feature/fix

1. Run `gw new <branch-name>` to create branch from origin/main
2. Confirm branch created successfully

### `pr [title]` - Create Pull Request

1. Ensure all changes are committed
2. Run `mise run verify` to check code quality
3. Push branch: `git push -u origin <branch>`
4. Create PR: `gh pr create --fill` or with provided title
5. Return PR URL

### `cleanup [branch]` - Clean up merged branch

1. Run `gw cleanup [branch]` to delete merged branch
2. Returns to home branch automatically

### `status` - Show current state

1. Run `gw status` to show repository state
2. Display suggested next action

### `home` - Return to home branch

1. Run `gw home` to switch to home branch and sync

### `pause [message]` - Pause current work

1. Run `gw pause [message]` to create WIP commit and return home

### `sync` - Sync after base PR merged

1. Run `gw sync` to update base and rebase

### `undo` - Undo last commit

1. Run `gw undo` to soft reset HEAD~1

## Workflow Examples

**Start a new feature:**
```
/git-workflow new feature/add-command
```

**Create PR after implementing:**
```
/git-workflow pr "Add new command for X"
```

**Clean up after PR merged:**
```
/git-workflow cleanup
```

## Before PR Checklist

Always run before creating PR:
```bash
mise run verify
```

## Notes

- Always use `gw` commands for branch operations
- Run `mise run verify` before pushing
- Use conventional commit messages (feat:, fix:, chore:, etc.)

## After Cleanup (Dogfooding)

After merging a PR, reinstall gw to use the latest version:

```bash
mise run install  # Build and install gw
```

**Note:** Only run `mise run install` from the main worktree to avoid overwriting with an in-progress version from another worktree.
