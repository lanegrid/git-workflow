---
name: git-workflow
description: Git workflow with the gw CLI. Use before the first git write in a session (branch, commit, push, PR, merge, sync, worktree pool) in a repository managed with gw, or to act on `gw status` output. Covers the pr and trunk flows, stacked PRs, gw await, and worktrees for parallel agents. Not needed for read-only work.
allowed-tools: Bash(gw*), Bash(git-workflow*), Bash(gh*), Bash(git*), Read, Edit, Grep, Glob, TaskStop
---

# Git Workflow with gw

The workflow is *state-driven*: run `gw status` and it prints the repository's
flow and the single next action for wherever you are. The sections below are
organized by **situation** — *when* you're in it, *what* to run, and *why*.
When in doubt, `gw status`.

## Flow: read it, don't assume it

Each repository declares how changes reach its default branch in
`git config gw.flow`, and `gw status` prints it on the `Flow:` line:

| Flow | How a change ships |
|------|--------------------|
| `pr` (default, also when unset) | Feature branch → PR → merge. Never push to the default branch. |
| `trunk` | Commit on the default branch and `git push`. `gw new` still opens a PR for a change that wants review. |

Who merges a PR is set by the project's or the user's instructions
(`CLAUDE.md` / `AGENTS.md`). When nothing says, a human merges.

## Who does what: owner and handed

Roles follow the work item, not the process tree.

- **Owner**: whoever started the work item — wrote its brief, ran `gw new`, or
  acquired its pool worktree. The owner publishes: push, PR, merge,
  `gw await`, cleanup, and the pool release.
- **Handed**: whoever received the item — through a spawn prompt, a brief, or
  a message from another session. Stage and commit on the item's branch, and
  stop there unless the brief says `publish: you`.

## The engine: `gw status` → "Next:"

| When `gw status` says…            | What to run                                  | Why |
|-----------------------------------|----------------------------------------------|-----|
| `Next: start new work`            | `gw new feature/...`                          | Branch off fresh `origin/main` (pr flow, or a worktree home). |
| `Next: commit on main (trunk flow)` | edit, commit, `git push` — or `gw new` for a PR | Trunk flow: the default branch is where work lands. |
| `Next: commit changes`            | stage deliberately, then `git commit` (below) | Record the change once it's coherent. |
| `Next: push to remote`            | `git push -u origin <branch>`                 | Publish the commits. |
| `Next: create pull request`       | `gh pr create -a "@me" -t "..."`              | The branch is pushed; open its PR. |
| `Waiting: PR #N in review`        | `gw await <N> --open` (background)            | Hand CI → merge → cleanup to the watcher. |
| `Next: sync with origin/main (N behind)` | `gw sync`                              | The base moved under you; rebase onto it before publishing. |
| `Next: sync (base 'X' was merged)`| `gw sync`                                     | Restack this PR after its base merged. |
| `Next: base 'X' merged — restack` | `gw sync`                                     | Same, before the PR exists: replay only your commits onto `main`, then open a normal PR. |
| `Next: pull upstream changes`     | `git pull --rebase`                           | Someone pushed to *this* branch (or the trunk); take their commits. |

## Situation: shipping a change — pr flow

| When | What | Why |
|------|------|-----|
| Starting | `gw new feature/your-feature` | New branch from `origin/main`. Run it **from home** — edits already made there are carried onto the branch. From a feature branch it refuses (the base is ambiguous): use `--stack` to build on it, or `gw home` first. |
| Code is ready to record | stage intentionally → `git commit -m "feat: ..."` | See **Staging** below. |
| Committed | `git push -u origin feature/your-feature` | Publish for the PR. |
| Pushed | `gh pr create -a "@me" -t "feat: ..."` | The URL gives you the PR number. |
| PR exists | `gw await <pr#> --open` **in background, same turn** | CI wait → open → merge watch → cleanup, hands-off. |
| Merged | *(automatic)* | `gw await` runs `gw cleanup` on merge. |

**When `gh pr create` returns a URL, launch `gw await <pr#> --open` as a
background task in that same turn.** Without it cleanup never runs and the
branch is left behind.

## Situation: shipping a change — trunk flow

| When | What | Why |
|------|------|-----|
| Change is ready | stage intentionally → `git commit` → `git push` | The default branch is where work lands. |
| `git push` is rejected / status says diverged | `git pull --rebase`, then push | Take the remote's commits first. **Never force-push the trunk.** |
| The change wants review | `gw new feature/...`, then the pr-flow path | A PR is still available per change. |
| A handed agent committed on a pool branch | rebase it onto `main`, fast-forward `main` (below) | The owner integrates; handed agents don't touch the trunk. |

Integrating a pool worktree's branch (owner, from the main worktree on `main`):

```sh
git -C "$WORKTREE_PATH" rebase main       # the worker's branch onto local main
git merge --ff-only <worker-branch>       # main moves forward, no merge commit
git push
(cd "$WORKTREE_PATH" && gw home)          # back to the pool home branch
git branch -d <worker-branch>
gw worktree pool release <name>
```

## Staging: commit deliberately, not `-A`

```sh
git status                 # what changed
git diff                   # review the actual edits
git add <paths>            # stage intentionally
git commit -m "feat: ..."
```

A blanket `git add -A` / `git commit -a` sweeps in scratch files, unrelated
edits and stray config. Stage the specific paths for *this* change.

## Situation: stacking a PR on top of another (pr flow)

```sh
gw new feature/child --stack          # base on the CURRENT branch, not origin/main
git commit -m "feat: ..."
git push -u origin feature/child
gh pr create -a "@me" -B feature/parent -t "..."   # -B sets the PR base to the parent
```

| When | What | Why |
|------|------|-----|
| Next change builds on an open PR's branch | `gw new <child> --stack` (from the parent branch) | Records the parent and its tip SHA so the rest of the flow knows it's stacked. |
| Creating the stacked PR | `gh pr create -B <parent> ...` (or follow `gw status`) | GitHub won't default the base to the parent. |
| Parent merged, child PR open | `gw sync` (on the child) | `rebase --onto` replays only the child's commits, force-pushes, moves the PR base to `main`. |
| Parent merged before the child got a PR | `gw sync` | Replays only your commits onto `main`; then open a normal PR. |
| Parent still open but gained commits | `gw sync` (on the child) | Rebases onto the parent's latest tip and force-pushes. |

Don't hand-rebase a stack: after a squash merge a plain `git rebase origin/main`
replays the parent's commits too. `gw cleanup` defers deleting a parent while
recorded children or open child PRs depend on it — run `gw sync` on each child,
then rerun `gw cleanup <parent>` in the parent's worktree. Stack mutations are
serialized across worktrees; an interrupted sync is resumed with `gw sync` in
the worktree `gw status` names (finish `git rebase --continue` first; before
publication you may `git rebase --abort` + `gw sync --abort` instead). Missing
fork-point metadata and parents merged into a non-default branch are refused —
don't bypass them. Details: [stack safety](../../docs/stack-safety.md).

## Situation: work gets interrupted or goes wrong

| When | What | Why |
|------|------|-----|
| Need to drop this and do something else | `gw pause [message]` | WIP commit + return home (don't `git stash`). |
| Changes are a dead end | `gw abandon` | Discard everything, return home. |
| Last commit was a mistake | `gw undo` | Soft reset `HEAD~1`; the changes stay staged. |
| `main` moved under you | `gw sync` | Rebases onto the latest `origin/main`; force-pushes with lease if published. |
| Syncing with unfinished work | `gw sync --keep-work` | Keeps staged, unstaged and untracked work; never pushes it. |

On a rebase conflict during `--keep-work`, finish `git rebase --continue`, then
rerun `gw sync`. A *restoration* conflict is different (the rebase already
finished): keep the printed `refs/gw/sync-work/...` backup, restore the files,
then `gw sync --finish-work`. Never `rebase --continue` for a restoration conflict.

## Situation: a PR is in flight — `gw await`

`gw await <pr#>` takes the **PR number** (not a branch), waits for CI, then
(with `--open`) opens the PR, watches for merge and runs `gw cleanup`. A human
or an agent can merge while it runs; it observes, it doesn't merge.

- **One watcher per PR.** It follows the PR across pushes, but once in
  merge-watching it doesn't re-check CI: check the latest head yourself before
  merging.
- **Merging as the agent:** leave the watcher running; run
  `gh pr merge -R <owner>/<repo> <pr#> --match-head-commit <SHA>` as a command
  of its own (no `cd … &&`), without `--delete-branch`, binding the merge to the
  head whose CI passed. Let the watcher clean up — don't delete the branch or
  run `gw cleanup` concurrently. Auto-merge / merge-queue acceptance isn't a
  merge yet.
- **After merge:** read the watcher's output before new work in that worktree —
  cleanup may have switched it home. Don't add commits to the merged branch.
- **Failure:** a CI failure ends the watcher (unless `--ignore-ci-failure`): fix →
  push → restart `gw await`. No watcher when the PR merged → rerun
  `gw await <pr#>` in the owning worktree; it goes straight to cleanup. Cleanup is
  one attempt: on a warning or deferral, fix the cause and rerun
  `gw cleanup <branch>`.
- **Report** merged/closed separately from cleanup completed/failed/deferred.

Flags: `--open`, `--no-wait`, `--no-cleanup`, `--ignore-ci-failure`, `--interval <secs>`.

## Situation: parallel agents — worktree pool

```sh
gw worktree pool warm 3                    # 1. pre-create once
gw worktree pool status                    # 2. confirm available > 0
WORKTREE_PATH=$(gw worktree pool acquire)  # 3. acquire (path → stdout)
# (cd "$WORKTREE_PATH" && <project setup>) # 4. deps / build artifacts
#                                          # 5. hand the item to the agent
gw worktree pool release <name>            # 6. release when done
gw worktree pool drain                     # remove all pool worktrees
```

- The one who acquires the worktree **owns** the item: publishing, cleanup and
  release stay with them. The agent working inside it is **handed**: it commits
  on its branch and stops there.
- Set up dependencies after `acquire`, before the agent starts.
- **Always release, even on error** — a forgotten release drains the pool.
- **A branch's lifecycle stays in one worktree**: push, watch and tear it down
  where it was born. Run `gw cleanup` inside the worktree, then release
  (release only clears the acquire marker).
- **gw owns worktrees**: no `git worktree add`, no agent `isolation: worktree` —
  `gw cleanup` can't reconcile worktrees it didn't create.

## Worktree model & hard "don'ts"

Each worktree has a **home branch**; the main worktree's home is the default
branch, a pool worktree's home is its own parking branch (never the trunk).

- **Don't `git checkout main`** — use `gw home` (switch home + sync).
- **Don't manually `git stash`** — use `gw sync --keep-work` or `gw pause`.
- **Don't hand-rebase** — `gw sync` owns rebasing onto the base.
- **Don't push to the default branch in the pr flow**; in the trunk flow, never
  force-push it.
- **Don't create worktrees outside `gw`.**

## Commit conventions

Conventional Commits: `feat:` · `fix:` · `docs:` · `refactor:` · `test:` · `chore:`

Examples: `feat: add gw await command` · `fix: handle detached HEAD in status`

## Notes

Browser open (`gw open` / `--open`) and merge notification (`gw await`) are
configured by env, not the CLI: `GW_OPEN_URL_CMD` (a script that opens a URL)
and `GW_NOTIFY_CMD` (a script that shows a notification).
