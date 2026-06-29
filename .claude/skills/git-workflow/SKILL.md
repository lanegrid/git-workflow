---
name: git-workflow
description: Development workflow using the gw CLI for feature branches, PRs, and worktrees
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
| `Next: rebase on latest main`     | `git fetch --prune && git rebase origin/main` | Catch up to `main` before continuing. |
| `Next: sync (base 'X' was merged)`| `gw sync`                                     | Restack this PR after its base merged. |

## Situation: shipping a change (the normal path)

**Every code change becomes a PR.** The path, by state:

| When | What | Why |
|------|------|-----|
| Starting | `gw new feature/your-feature` | New branch from `origin/main`. Run it **from home** — if you already edited there, `gw new` carries those changes onto the branch. From a feature branch it refuses (the base is ambiguous): use `--stack` to build on it, or `gw home` first to start fresh. |
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

## Situation: stacking a PR on top of another

When the next change depends on a branch whose PR is still open, stack on it
instead of waiting:

```sh
gw new feature/child --stack          # base on the CURRENT branch, not origin/main
git commit -m "feat: ..."
git push -u origin feature/child
gh pr create -a "@me" -B feature/parent -t "..."   # -B sets the PR base to the parent
```

| When | What | Why |
|------|------|-----|
| Next change builds on an open PR's branch | `gw new <child> --stack` (from the parent branch) | Bases the child on the parent's HEAD, not `origin/main`. Records the parent (and its tip SHA) so the rest of the flow knows it's stacked. |
| Creating the stacked PR | `gh pr create -B <parent> ...` (or follow `gw status`) | A locally-stacked branch doesn't make GitHub default the base to the parent — set it explicitly with `-B`. `gw status` fills the `-B` in for you while the PR doesn't exist yet. |
| Parent PR merged, child PR **open** | `gw sync` (on the child) | Restacks the child onto `main`: `git rebase --onto` replays only the child's commits (not the merged parent's), moves the PR base to `main`, force-pushes. Don't hand-rebase. |
| Parent PR merged **before** the child got a PR | follow `gw status` | It detects the merged base and tells you to `git rebase --onto origin/main <recorded-base>` — replaying only your commits — then open a normal PR. |

Don't worry about cleaning up the parent yourself: **`gw cleanup` refuses to
delete a branch while an open PR still targets it as base** (deleting it would
make GitHub close that child PR). It deletes the parent only once the child has
been `gw sync`'d onto `main`.

`gw new` chooses a base unambiguously: it auto-bases on `origin/main` only from
home; from a feature branch you must say `--stack` (or `gw home` first). A dirty
tree is carried on the current HEAD, so creating the branch never hits a merge
conflict.

> **Why `--onto`, not a plain rebase?** After a squash merge, the parent's
> commits exist on `main` only as a *new* squashed commit. A plain
> `git rebase origin/main` would replay the parent's original commits too —
> doubling them and inviting conflicts. `gw sync` (and the `gw status` hint)
> use `git rebase --onto origin/main <old-base>` so only the child's own commits
> move. Let `gw` do it.

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

It waits for CI, then (with `--open`) opens the PR, watches it to merge, and
runs `gw cleanup` — hands-off. **If CI fails it stops and reports**, so you fix
→ push → rerun `await` (or pass `--ignore-ci-failure` to watch regardless).

Flags: `--open`, `--no-wait` (skip the CI wait), `--no-cleanup` (stop after
merge), `--ignore-ci-failure`, `--interval <secs>`.

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

**gw owns worktrees in this repo — don't open a second path.** Claude Code's
agent worktree isolation (`isolation: "worktree"`) creates worktrees `gw` can't
see, and it leaves them behind once an agent commits. That collides with `gw`'s
worktree-aware `cleanup` (which then can't delete a branch a stray worktree still
holds). So for isolated/parallel agent work, **use the pool above — never the
agent's own `isolation: worktree`.**

**A branch's lifecycle stays in one worktree.** Whatever worktree a branch is
born in is where it's pushed, watched, and torn down. Concretely:

- **Don't run `gw await`/`gw cleanup` from a different worktree than the branch
  lives in** — `cleanup` can't delete a branch another worktree has checked out.
- **Release (or remove) the worktree before cleanup deletes the branch** — `gw
  worktree pool release` resets it off the branch, freeing it for deletion.

## Worktree model & hard "don'ts"

Each worktree has a **home branch**; the main worktree's home is `main`. `gw`
handles worktree boundaries for you. Because of them:

- **Don't `git checkout main`** — use `gw home` (switches to home + syncs with
  `origin/main`). A direct checkout conflicts across worktrees.
- **Don't `git stash`** — use `gw pause` (a WIP commit travels across worktrees
  safely; a stash doesn't).
- **Don't hand-rebase stacked PRs** — use `gw sync`.
- **Don't push to `main`** — every change goes through a PR.
- **Don't create worktrees outside `gw`** — no `git worktree add`, no agent
  `isolation: worktree`. `gw` is the single worktree authority here; a second
  path produces worktrees `gw cleanup` can't reconcile (see the pool section).

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
