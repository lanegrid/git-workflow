---
name: git-workflow
description: Development workflow using the gw CLI for feature branches, PRs, worktrees, and cleanup
allowed-tools: Bash(gw*), Bash(git-workflow*), Bash(gh*), Bash(git*), Read, Edit, Grep, Glob, TaskStop
---

# Git Workflow and Conventions

Worktree-aware Git workflow for this repo. We dogfood our own `gw` CLI, so use
`gw`, `git`, and `gh` directly. `gw` is worktree-aware and always tells you the
next action via `gw status`.

## Quick Reference

```sh
# Basic operations
gw status              # Show current state + next action
gw home                # Switch to home branch + sync with origin/main
gw new <branch>        # Create new branch from origin/main
gw cleanup [branch]    # Delete merged branch + return to home

# Pause / discard / undo
gw pause [message]     # WIP commit + return to home
gw abandon             # Discard all changes + return to home
gw undo                # Soft reset HEAD~1 (undo last commit)

# Stacked PRs
gw sync                # Sync current branch after base PR merged (rebase + force push)

# PR lifecycle
gh pr create -a "@me"  # Create the PR
gw open                # Open current branch's PR in the browser
gw await <pr#> --open  # Open, then watch CI → merge → cleanup (run in background)

# Worktree pool (parallel agent execution)
gw worktree pool warm <count>   # Pre-create N worktrees
gw worktree pool status         # Show available/total
gw worktree pool acquire        # Acquire one (prints path to stdout)
gw worktree pool release <name> # Release back to the pool
gw worktree pool drain          # Remove all pool worktrees
```

> **⚠️ Pitfalls**
> - **Do not run `git checkout main`** — use `gw home` instead (worktree conflict).
> - **Do not use `git stash`** — use `gw pause` instead (WIP commit, safer worktree switching).
> - **Do not manually rebase stacked PRs** — use `gw sync` instead (updates the GitHub PR base + rebases + force pushes).

> **🚨 Mandatory rule: after creating a PR, immediately launch `gw await <pr#> --open` as a background task.**
> The moment `gh pr create` returns a URL (and thus the PR number), in that same
> turn start `gw await <pr#> --open` with `Bash(run_in_background=true)`. Do
> **not** ask the user "what next?", wait for CI, or stop — `gw await` runs CI
> wait → browser open → merge watch → cleanup on its own. Skipping it means the
> post-merge cleanup never runs and the branch is left behind. This is not optional.

## Standard Workflow: Code → PR

**Every code change becomes a PR.** Follow this flow:

```
1. Branch   → gw new feature/your-feature
2. Code     → make changes
3. Commit   → git add -A && git commit -m "feat: ..."
4. Push     → git push -u origin feature/your-feature
5. PR       → gh pr create -a "@me" -t "feat: ..."
6. Await    → gw await <pr#> --open  (REQUIRED — background task)
7. Cleanup  → (auto: gw await runs gw cleanup on merge)
```

**Never skip step 6.** As soon as the `gh pr create` URL appears, launch
`gw await <pr#> --open` in the background in the same assistant turn — the user
expects CI / merge / cleanup to be handled end to end the moment the PR exists.

### If you have uncommitted changes on the home branch

This happens when you made changes before creating a branch. Fix it:

```sh
gw new feature/your-feature   # creates the branch, keeps your changes
gw status                     # follow the suggested "Next:" action
```

### gw await — PR lifecycle in one command (background task)

Takes the **PR number** so the watcher stays bound to that one PR even if you
switch branches (e.g. while working a stacked PR), and cleans up the PR's own
head branch on merge — not whatever happens to be checked out. Best launched as
a background task right after the PR is created. Use `--open` to open the PR in
the browser first, then watch:

```
[Bash(run_in_background=true)] gw await <pr#> --open
```

Three phases run automatically:

1. **CI wait** — `gh pr checks --watch` (skip with `--no-wait`).
2. **Browser open** — opens the PR page (only with `--open`).
3. **Merge watch** — polls PR state every `--interval`s (default 30):
   - `MERGED` → `$GW_NOTIFY_CMD` notification → `gw cleanup <head branch>` → exit
   - `CLOSED` → message → exit

If CI fails, `gw await` stops and reports it (the PR can't merge until it's
fixed) instead of watching forever. Fix → push → relaunch, or pass
`--ignore-ci-failure` to keep watching regardless.

Flags: `--open`, `--no-wait`, `--no-cleanup` (stop after merge),
`--ignore-ci-failure`, `--interval <secs>`.

**Singleton rule — only ONE watcher per PR:**

- Launch `gw await <pr#>` **once**, after the final push, when no more changes are expected.
- If CI fails and you must push a fix: **stop the existing watcher** with `TaskStop`
  first, then fix, push, and relaunch.
- Never run multiple `gw await <pr#>` watchers for the same PR.

**When background output arrives** via `<system-reminder>`, you MUST:

1. Read the watcher's output file.
2. Report the result to the user immediately (merged/closed, cleanup success/failure).

## Proactive Workflow

**Always run `gw status` and follow the "Next:" action.** It detects working
directory state, upstream sync state, home-branch state, and PR state, then
suggests what to do:

| Status Output | Action |
|--------------|--------|
| `Next: start new work` | `gw new feature/...` |
| `Next: commit changes` | `git add -A && git commit -m "..."` |
| `Next: push to remote` | `git push -u origin <branch>` |
| `Next: create pull request` | `gh pr create -a "@me" -t "..."` |
| `Waiting: PR #N in review` | `gw await <N> --open` (watch to merge) or `gw open` |
| `Next: cleanup merged branch` | `gw cleanup` |
| `Next: rebase on latest main` | `git fetch --prune && git rebase origin/main` |
| `Next: sync (base 'X' was merged)` | `gw sync` |

## Worktree Model

This project supports git worktrees. Each worktree has a **home branch**.

- The main worktree's home is `main`.
- Never checkout `main` directly from another worktree — use `gw home`.
- `gw` handles worktree boundaries automatically.

### Pool Worktrees (for parallel agent execution)

Use the pre-warmed pool when running multiple agents in parallel.

```sh
# 1. Pre-create once
gw worktree pool warm 3

# 2. Check availability (confirm available > 0)
gw worktree pool status

# 3. Acquire (path is printed to stdout)
WORKTREE_PATH=$(gw worktree pool acquire)

# 4. Run the agent inside that worktree

# 5. Always release when done (success or failure)
gw worktree pool release <name>
```

> **Important:** forgetting to release drains the pool. Always release, even on error.

## Commit Conventions

Conventional Commits style:

```
feat:     new feature
fix:      bug fix
chore:    build / tooling / housekeeping
docs:     documentation
refactor: refactor (no behavior change)
test:     tests
```

Examples:

```
feat: add gw await command
fix: handle detached HEAD in status
chore: bump version to 0.6.0
```

## Notes

- Browser open (`gw open` / `--open`) and merge notification (`gw await`) are
  configured via env in your dotfiles, not the CLI:
  - `GW_OPEN_URL_CMD` → script that opens a URL (e.g. a dedicated Chrome profile)
  - `GW_NOTIFY_CMD` → script that shows a notification (e.g. macOS `osascript`)

> **Dogfooding note (optional):** in the main worktree you can run
> `mise run cleanup` instead of `gw cleanup` to also reinstall `gw` after the
> branch is deleted. The skill itself only uses `gw cleanup`; this is a repo-local
> convenience, not part of the workflow.
