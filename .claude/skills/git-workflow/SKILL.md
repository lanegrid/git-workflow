---
name: git-workflow
description: Development workflow using the gw CLI for feature branches, PRs, worktrees, and cleanup
allowed-tools: Bash(gw*), Bash(git-workflow*), Bash(gh*), Bash(git*), Read, Edit, Grep, Glob, TaskStop
---

# Git Workflow and Conventions

Worktree-aware Git workflow for this repo. We dogfood our own `gw` CLI, so use
`gw`, `git`, and `gh` directly.

**How to use this skill.** The workflow is *state-driven*: you rarely need to
recall steps. Run `gw status` and it prints the single next action for wherever
you are. The sections below are organized by **situation** — *when* you're in it,
*what* to run, and *why*. When in doubt, `gw status`.

## The engine: `gw status` → "Next:"

`gw status` inspects working dir, upstream sync, home-branch, and PR state, then
prints one `Next:` line. Follow it. This is the situation → action → reason map:

| When `gw status` says…            | What to run                                  | Why |
|-----------------------------------|----------------------------------------------|-----|
| `Next: start new work`            | `gw new feature/...`                          | Branch off fresh `origin/main`; never work on `main`. |
| `Next: commit changes`            | stage deliberately, then `git commit` (below) | Record the change once it's coherent. |
| `Next: push to remote`            | `git push -u origin <branch>`                 | Publish the branch so a PR can open. |
| `Next: create pull request`       | `gh pr create -a "@me" -t "..."`              | Every change ships through a PR. |
| `Waiting: PR #N in review`        | `gw await <N> --open` (background)            | Hand CI → merge → cleanup to the watcher. |
| `Next: cleanup merged branch`     | `gw cleanup`                                  | Delete the merged branch, return home. |
| `Next: rebase on latest main`     | `git fetch --prune && git rebase origin/main` | Catch up to `main` before continuing. |
| `Next: sync (base 'X' was merged)`| `gw sync`                                     | Restack this PR after its base merged. |

## Situation: shipping a change (the normal path)

**Every code change becomes a PR.** The path, by state:

| When | What | Why |
|------|------|-----|
| Starting | `gw new feature/your-feature` | New branch from `origin/main`. If you already edited on home, `gw new` keeps the changes and moves them onto the branch. |
| Code is ready to record | stage intentionally → `git commit -m "feat: ..."` | See **Staging** below — review before committing. |
| Committed | `git push -u origin feature/your-feature` | Publish for the PR. |
| Pushed | `gh pr create -a "@me" -t "feat: ..."` | Open the PR; the URL gives you the PR number. |
| PR exists | `gw await <pr#> --open` **in background, same turn** | CI wait → open → merge watch → cleanup, hands-off. |
| Merged | *(automatic)* | `gw await` runs `gw cleanup` on merge. |

> **🚨 The moment `gh pr create` returns a URL, launch `gw await <pr#> --open` as
> a background task in that same turn.** Do not ask "what next?", wait for CI, or
> stop. `gw await` runs CI wait → browser open → merge watch → cleanup on its
> own; skipping it means cleanup never runs and the branch is left behind. Not
> optional.

### Staging: commit deliberately, not `-A`

Before committing, see what you're about to record and stage only what belongs:

```sh
git status                 # what changed
git diff                   # review the actual edits
git add <paths>            # stage intentionally
git commit -m "feat: ..."
```

**Why not `git add -A` / `git commit -a`:** a blanket add sweeps in scratch
files, unrelated edits, and stray config — things you didn't mean to ship.
Stage the specific paths for *this* change instead.

## Situation: work gets interrupted or goes wrong

| When | What | Why |
|------|------|-----|
| Need to drop this and do something else | `gw pause [message]` | WIP commit + return home — safe worktree switch (don't `git stash`). |
| Changes are a dead end | `gw abandon` | Discard everything, return home. |
| Last commit was a mistake | `gw undo` | Soft reset `HEAD~1`; keeps the changes unstaged. |
| `main` moved under you | `git fetch --prune && git rebase origin/main` | Replay your work on the latest `main`. |
| Stacked PR's base just merged | `gw sync` | Updates the GitHub base, rebases, force-pushes — don't rebase stacked PRs by hand. |

## Situation: a PR is in flight — `gw await`

Launch as a background task right after the PR is created. It takes the **PR
number** (not a branch) so the watcher stays bound to that PR even if you switch
branches, and cleans up *that PR's* head branch on merge.

```
[Bash(run_in_background=true)] gw await <pr#> --open
```

It's a state machine:

1. **Wait for CI** *(skip with `--no-wait`)* — polls until the checks reach a
   verdict. "Not registered yet" (the window right after `gh pr create`) and
   "pending" both just mean *keep waiting*; only pass/fail end the wait. There's
   no timeout — for a repo with genuinely no CI, use `--no-wait`.
   - **CI fails** → stop and report, so you can fix → push → rerun `await`
     (or pass `--ignore-ci-failure` to watch regardless).
2. **CI passes** → **open the PR in the browser** *(only with `--open`)*, then:
3. **Watch for merge** — polls every `--interval`s (default 30):
   - `MERGED` → `$GW_NOTIFY_CMD` notification → `gw cleanup <head branch>` → exit
   - `CLOSED` → `$GW_NOTIFY_CMD` notification → exit

Flags: `--open`, `--no-wait`, `--no-cleanup` (stop after merge),
`--ignore-ci-failure`, `--interval <secs>`.

**One watcher per PR.** Launch `gw await <pr#>` once, after the final push. If CI
fails and you must push a fix, **stop the existing watcher with `TaskStop`
first**, then fix, push, relaunch. Never run two watchers for the same PR.

**When background output arrives** via `<system-reminder>`, you MUST:
1. Read the watcher's output file.
2. Report the result to the user immediately (merged/closed, cleanup ok/failed).

## Situation: running multiple agents in parallel — worktree pool

Use the pre-warmed pool so parallel agents each get an isolated worktree.

```sh
gw worktree pool warm 3                  # 1. pre-create once
gw worktree pool status                  # 2. confirm available > 0
WORKTREE_PATH=$(gw worktree pool acquire)  # 3. acquire (path → stdout)
#                                          # 4. run the agent inside it
gw worktree pool release <name>          # 5. release when done
gw worktree pool drain                   # remove all pool worktrees
```

> **Always release, even on error** — a forgotten release drains the pool.

## Worktree model & hard "don'ts"

Each worktree has a **home branch**; the main worktree's home is `main`. `gw`
handles worktree boundaries for you. Because of them:

- **Don't `git checkout main`** — use `gw home` (switches to home + syncs with
  `origin/main`). A direct checkout conflicts across worktrees.
- **Don't `git stash`** — use `gw pause` (a WIP commit travels across worktrees
  safely; a stash doesn't).
- **Don't hand-rebase stacked PRs** — use `gw sync`.
- **Don't push to `main`** — every change goes through a PR.

## Commit conventions

Conventional Commits:

```
feat:     new feature          docs:     documentation
fix:      bug fix              refactor: refactor (no behavior change)
chore:    build / tooling      test:     tests
```

Examples: `feat: add gw await command` · `fix: handle detached HEAD in status` ·
`chore: bump version to 0.6.0`

## Notes

- Browser open (`gw open` / `--open`) and merge notification (`gw await`) are
  configured via env in your dotfiles, not the CLI:
  - `GW_OPEN_URL_CMD` → script that opens a URL (e.g. a dedicated Chrome profile)
  - `GW_NOTIFY_CMD` → script that shows a notification (e.g. macOS `osascript`)
