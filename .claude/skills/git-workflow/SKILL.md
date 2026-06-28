---
name: git-workflow
description: Development workflow using gw CLI for feature branches, PRs, and cleanup
argument-hint: <command> [args]
allowed-tools: Bash(gw*), Bash(git-workflow*), Bash(gh*), Bash(git*), Bash(mise*), Bash(cargo*), Read, Edit, Grep, Glob, TaskStop
---

# git-workflow Development

Use the `gw` CLI for all git workflow operations in this project. `gw` is
worktree-aware and tells you the next action via `gw status`.

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
5. Open it and watch to completion: launch `gw await --open` as a background task (see `await`)

### `await` - Watch the PR to completion, then clean up

Run from the feature branch — best launched as a background task right after
the PR is created. Use `--open` so it opens the PR in the browser first, then
watches:

```
[Bash(run_in_background=true)] gw await --open
```

Three phases run automatically:

1. **CI wait** — `gh pr checks --watch`
2. **Merge watch** — polls the PR state every `--interval`s (default 30)
   - MERGED → `$GW_NOTIFY_CMD` notification → `gw cleanup` → exit
   - CLOSED → message → exit

Flags: `--open` (open in browser first), `--no-wait` (skip CI wait),
`--no-cleanup` (stop after merge), `--interval <secs>`.

**Singleton rule — only ONE watcher per PR:**

- Launch `gw await` **once**, after the final push, when no more changes are expected.
- If CI fails and you need to push a fix: **stop the existing watcher** with
  `TaskStop` first, then fix, push, and relaunch.
- Never run multiple `gw await` watchers for the same PR.

**When background output arrives** via `<system-reminder>`, you MUST:

1. Read the watcher's output file.
2. Report the result to the user immediately (merged/closed, cleanup success/failure).

### `open` - Open the PR in the browser

1. Run `gw open` to open the current branch's PR in the browser

### `cleanup [branch]` - Clean up merged branch

1. Run `mise run cleanup` to delete merged branch and reinstall gw
2. Returns to home branch automatically
3. Reinstalls gw if running from main worktree (for dogfooding)

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

## Proactive Workflow

**Always run `gw status` and follow the "Next:" action.** It detects working
directory state, upstream sync state, and PR state, then suggests what to do:

| Status Output | Action |
|--------------|--------|
| `Next: start new work` | `gw new feature/...` |
| `Next: commit changes` | `git add -A && git commit -m "..."` |
| `Next: push to remote` | `git push -u origin <branch>` |
| `Next: create pull request` | `gh pr create -a "@me" -t "..."` |
| `Waiting: PR #N in review` | `gw await` (watch to merge) or `gw open` |
| `Next: cleanup merged branch` | `gw cleanup` |
| `Next: rebase on latest main` | `git fetch --prune && git rebase origin/main` |
| `Next: sync (base 'X' was merged)` | `gw sync` |

## Pitfalls

- **Do not `git checkout main`** — use `gw home` instead (worktree conflict)
- **Avoid `git stash`** — use `gw pause` instead (WIP commit, safer worktree switching)
- **Do not manually rebase stacked PRs** — use `gw sync` instead (updates GitHub PR base + rebases)

## Workflow Examples

**Start a new feature:**
```
/git-workflow new feature/add-command
```

**Create PR, then open + watch it to completion:**
```
/git-workflow pr "Add new command for X"
# then, in the background:
gw await --open
```

**Clean up after PR merged (manual):**
```
/git-workflow cleanup
```

## Before PR Checklist

Always run before creating a PR:
```bash
mise run verify
```

## Notes

- Always use `gw` commands for branch operations
- Run `mise run verify` before pushing
- Use conventional commit messages (feat:, fix:, chore:, etc.)
- Browser open (`gw open`/`--open`) and merge notification (`gw await`) are
  configured via env in your dotfiles, not the CLI:
  - `GW_OPEN_URL_CMD` → script that opens a URL (e.g. a dedicated Chrome profile)
  - `GW_NOTIFY_CMD` → script that shows a notification (e.g. macOS `osascript`)

## Cleanup (Dogfooding)

Use `mise run cleanup` instead of `gw cleanup` to automatically reinstall gw after cleanup:

```bash
mise run cleanup  # gw cleanup + reinstall (main worktree only)
```

This automatically detects if you're in the main worktree and reinstalls gw. In a worktree, it skips the install step.
