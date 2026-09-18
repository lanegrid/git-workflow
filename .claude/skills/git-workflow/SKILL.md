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
| `Next: sync with origin/main (N behind)` | `gw sync`                              | The base moved under you; rebase onto it before publishing. |
| `Next: sync (base 'X' was merged)`| `gw sync`                                     | Restack this PR after its base merged. |
| `Next: base 'X' merged — restack` | `gw sync`                                     | Same, before the PR exists: replay only your commits onto `main`, then open a normal PR. |
| `Next: pull upstream changes`     | `git pull --rebase`                           | Someone pushed to *this* branch; take their commits. |

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
| Parent PR merged, child PR **open** | `gw sync` (on the child) | Restacks the child onto `main`: `git rebase --onto` replays only the child's commits (not the merged parent's), force-pushes, then moves the PR base to `main`. Don't hand-rebase. |
| Parent PR merged **before** the child got a PR | `gw sync` | Replays only your commits onto `main` (`rebase --onto` the recorded base tip), then open a normal PR. |
| Parent PR still open but gained commits | `gw sync` (on the child) | Rebases the child onto the parent's latest tip and force-pushes. |

**`gw cleanup` defers both local and remote deletion while known children
still depend on the parent.** It checks local recorded children (including
branches without a PR) and open GitHub child PRs. Run `gw sync` on each child,
then rerun `gw cleanup <parent>` in the parent's worktree. An await task that
encounters this guard exits; it does not wait for children to restack.

Stack mutations are serialized across linked worktrees. If `sync` is
interrupted, `gw status` identifies its owning worktree: rerun `gw sync` there
instead of hand-rebasing or editing the PR base. For conflicts, finish
`git rebase --continue` first. To cancel before publication, abort any active
Git rebase, then use `gw sync --abort`. After publication, resume to completion.
An unfinished sync blocks other stack mutations and cleanup until resolved.

A stacked sync refuses missing fork-point metadata and parents merged into a
non-default branch. Do not bypass these guards with a plain rebase or branch
deletion. See [stack safety and remaining limitations](../../../docs/stack-safety.md).

`gw new` chooses a base unambiguously: it auto-bases on `origin/main` only from
home; from a feature branch you must say `--stack` (or `gw home` first). A dirty
tree is carried on the current HEAD, so creating the branch never hits a merge
conflict.

> **Why `--onto`, not a plain rebase?** After a squash merge, the parent's
> commits exist on `main` only as a *new* squashed commit. A plain
> `git rebase origin/main` would replay the parent's original commits too —
> doubling them and inviting conflicts. `gw sync` uses
> `git rebase --onto origin/main <recorded base tip>` so only the child's own
> commits move. Let `gw` do it.

## Situation: work gets interrupted or goes wrong

| When | What | Why |
|------|------|-----|
| Need to drop this and do something else | `gw pause [message]` | WIP commit + return home — safe worktree switch (don't `git stash`). |
| Changes are a dead end | `gw abandon` | Discard everything, return home. |
| Last commit was a mistake | `gw undo` | Soft reset `HEAD~1`; the changes stay staged, ready to re-commit. |
| `main` moved under you | `gw sync` | Rebases onto the latest `origin/main` and force-pushes (with lease) if the branch is published. |
| Stacked PR's base just merged | `gw sync` | Rebases, force-pushes, then updates the GitHub base — don't rebase stacked PRs by hand. |

## Situation: a PR is in flight — `gw await`

Launch as a background task right after the PR is created. It takes the **PR
number** (not a branch) so the watcher stays bound to that PR even if you switch
branches, and cleans up *that PR's* head branch on merge.

```
[Bash(run_in_background=true)] gw await <pr#> --open
```

It waits for CI, then (with `--open`) opens the PR, watches for merge, and
runs `gw cleanup`. **Either a human or an agent can merge while it runs.**
It observes the PR; it does not perform or block the merge. Even during CI
waiting, a detected merge triggers cleanup. A closed, unmerged PR ends the
watcher without cleanup.

Flags: `--open`, `--no-wait` (skip the CI wait), `--no-cleanup` (stop after
merge), `--ignore-ci-failure`, `--interval <secs>`.

**One watcher per PR.** Check the existing task before launching another.
Additional pushes do not require restarting a live watcher: it follows the PR
number across commits. Once it reaches merge-watching, however, it does **not**
return to CI-waiting on a new push. The agent must check CI and review readiness
for the latest head before merging; an earlier "CI checks passed" is not proof
that the latest commit passed.

**When the agent merges:**

1. Leave the watcher running. Run `gh pr merge <pr#>` with the appropriate
   merge method, without `--delete-branch`. Use `--match-head-commit <SHA>` when
   needed to bind the merge to the reviewed head.
2. Let the watcher detect the merge, run cleanup, and exit. Do not run branch
   deletion or `gw cleanup` concurrently. Auto-merge or merge-queue acceptance
   is not a completed merge; keep watching until the PR actually merges.
3. Read the task's output before starting new work in that worktree or releasing
   it to the pool. Cleanup may switch it to home. Do not add follow-up commits
   to the merged branch: cleanup permits force deletion for merged PRs and
   skips the unpushed-commit check.

**Failure and recovery:**

- A CI failure detected during CI-waiting ends the watcher by default. Check
  that it has exited, fix → push → restart `await`. An exited task needs no
  `TaskStop`. With `--ignore-ci-failure`, it continues to watch for merge.
- Stop a live watcher only when deliberately replacing it or taking over its
  cleanup responsibility. Merging or pushing alone is not a reason to stop it.
- If no watcher is running when the PR merges, rerun `gw await <pr#>` in the
  owning worktree: an already-merged PR proceeds directly to cleanup.
- Cleanup is one attempt, not a retry loop. Uncommitted changes on the target
  checkout can abort it; dependent child PRs can defer remote deletion. Some
  deletion failures are warnings even with a successful exit and a "Cleanup
  complete" message. Inspect the output, resolve the cause, then rerun
  `gw cleanup <branch>` after the watcher has exited.

**When background output arrives**, read the watcher's output file and report
the result immediately. Distinguish PR merged/closed from cleanup completed,
failed, or partially deferred; task exit alone does not prove cleanup finished.

## Situation: running multiple agents in parallel — worktree pool

Use the pre-warmed pool so parallel agents each get an isolated worktree.

```sh
gw worktree pool warm 3                  # 1. pre-create once
gw worktree pool status                  # 2. confirm available > 0
WORKTREE_PATH=$(gw worktree pool acquire)  # 3. acquire (path → stdout)
# (cd "$WORKTREE_PATH" && <project setup>) # 4. install deps in the fresh worktree
#                                          # 5. run the agent inside it
gw worktree pool release <name>          # 6. release when done
gw worktree pool drain                   # remove all pool worktrees
```

> **Always release, even on error** — a forgotten release drains the pool.

> **Set up the worktree before launching the agent.** A freshly acquired pool
> worktree has the code but not its dependencies or build artifacts
> (`node_modules`, compiled output, etc.). Run the project's setup step inside
> `$WORKTREE_PATH` before starting the agent, or its first commands fail.

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
- **Run `gw cleanup` inside the worktree, then `gw worktree pool release`** —
  cleanup switches the worktree back to its pool home branch and deletes the
  feature branch; release only clears the acquire marker (no git operations),
  so a worktree released while still on the branch keeps that branch pinned.

## Worktree model & hard "don'ts"

Each worktree has a **home branch**; the main worktree's home is `main`. `gw`
handles worktree boundaries for you. Because of them:

- **Don't `git checkout main`** — use `gw home` (switches to home + syncs with
  `origin/main`). A direct checkout conflicts across worktrees.
- **Don't `git stash`** — use `gw pause` (a WIP commit travels across worktrees
  safely; a stash doesn't).
- **Don't hand-rebase** — `gw sync` brings a branch onto its latest base (`main` or the stacked parent) and restacks after the parent merges.
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
