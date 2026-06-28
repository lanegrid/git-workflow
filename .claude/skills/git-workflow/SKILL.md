---
name: git-workflow
description: The steps for changing any code in this project, using a helper tool called `gw`. In plain words — make your own copy of the code, save your changes, and ask for them to be added to the shared code. Read this before you change any file here.
allowed-tools: Bash(gw*), Bash(git-workflow*), Bash(gh*), Bash(git*), Read, Edit, Grep, Glob, TaskStop
---

# Git Workflow (how to change code here)

This project changes code with a small helper tool called `gw`. You also use
`git` and `gh` (GitHub's command tool). You run all three directly.

**Words you'll see (read once):**

- **branch** — your own copy of the code to work in, so you don't touch the shared version.
- **commit** — save a snapshot of your changes, with a short note about what you did.
- **push** — upload your branch to GitHub.
- **PR (pull request)** — asking for your changes to be added to the shared code, so people can look first.
- **CI** — robots that test your change automatically.
- **merge** — your change gets added into the shared code.
- **rebase** — move your changes on top of the newest shared code.
- **worktree** — a separate folder that holds one branch.

**You don't have to memorize the steps.** Run `gw status`. It looks at where you
are and tells you the one thing to do next. When unsure, run `gw status` again.

## The engine: `gw status` tells you what to do next

`gw status` checks your situation and prints one `Next:` line. Do what it says.
Here is each line, what to run, and what it means:

| When `gw status` says…            | Run this                                      | What it means |
|-----------------------------------|----------------------------------------------|-----|
| `Next: start new work`            | `gw new feature/...`                          | Make a new branch from the newest shared code. Never work on `main`. |
| `Next: commit changes`            | look first, then `git commit` (see below)     | Save your changes once they make sense together. |
| `Next: push to remote`            | `git push -u origin <branch>`                 | Upload your branch so you can open a PR. |
| `Next: create pull request`       | `gh pr create -a "@me" -t "..."`              | Every change goes in through a PR. |
| `Waiting: PR #N in review`        | `gw await <N> --open` (in the background)     | Let the watcher handle testing → merge → cleanup for you. |
| `Next: rebase on latest main`     | `git fetch --prune && git rebase origin/main` | Catch up to the newest shared code before you keep going. |
| `Next: sync (base 'X' was merged)`| `gw sync`                                     | Fix up this PR after the PR it was built on got merged. |

## When you want to ship a change (the normal path)

**Every change becomes a PR.** Here is the path, step by step:

| Where you are | What to do | Why |
|------|------|------|
| Just starting | `gw new feature/your-feature` | Makes a new branch from the newest shared code. If you already edited files, `gw new` carries those edits onto the new branch. |
| Changes are ready to save | look first, then `git commit -m "feat: ..."` | See **Saving your changes** below — check before you save. |
| Saved | `git push -u origin feature/your-feature` | Uploads your branch for the PR. |
| Uploaded | `gh pr create -a "@me" -t "feat: ..."` | Opens the PR. The link it prints has the PR number. |
| PR is open | `gw await <pr#> --open` **in the background, right away** | Waits for tests → opens it → watches for merge → cleans up. Hands-off. |
| Merged | *(happens by itself)* | `gw await` cleans up the branch when it merges. |

> **🚨 The moment `gh pr create` gives you a link, start `gw await <pr#> --open`
> in the background in the same turn.** Don't ask "what next?", don't sit and
> wait for tests, don't stop. `gw await` waits for tests → opens the PR →
> watches for merge → cleans up, all on its own. If you skip it, the cleanup
> never happens and the old branch is left behind. This is not optional.

### Saving your changes: pick what you save, don't grab everything

Before you save, look at what changed and save only the files that belong to
this change:

```sh
git status                 # what changed
git diff                   # read the actual edits
git add <paths>            # pick the files on purpose
git commit -m "feat: ..."
```

**Why not `git add -A` / `git commit -a`:** grabbing everything sweeps in junk
files, unrelated edits, and stray settings — stuff you didn't mean to send.
Pick the exact files for *this* change instead.

## When work gets interrupted or goes wrong

| What happened | What to do | Why |
|------|------|------|
| You need to drop this and do something else | `gw pause [message]` | Makes a quick save and sends you home — a safe way to switch (don't use `git stash`). |
| The changes are a dead end | `gw abandon` | Throws everything away and sends you home. |
| Your last save was a mistake | `gw undo` | Undoes the last commit but keeps your edits. |
| The shared code moved under you | `git fetch --prune && git rebase origin/main` | Replays your work on top of the newest shared code. |
| The PR yours is built on just merged | `gw sync` | Updates the base, rebases, and re-uploads — don't do this by hand. |

## When a PR is in flight — `gw await`

Start it in the background right after you create the PR. Give it the **PR
number** (not a branch name), so the watcher stays with that PR even if you
switch branches, and cleans up that PR's branch when it merges.

```
[Bash(run_in_background=true)] gw await <pr#> --open
```

It waits for the tests, then (with `--open`) opens the PR, watches it until it
merges, and cleans up — hands-off. **If the tests fail it stops and tells you**,
so you fix it → push → run `await` again (or add `--ignore-ci-failure` to watch
no matter what).

Options: `--open`, `--no-wait` (skip waiting for tests), `--no-cleanup` (stop
after merge), `--ignore-ci-failure`, `--interval <secs>`.

**Only one watcher per PR.** Start `gw await <pr#>` once, after your last push.
If the tests fail and you need to push a fix, **stop the running watcher with
`TaskStop` first**, then fix, push, and start it again. Never run two watchers
for the same PR.

**When the background watcher prints something** (you'll see a `<system-reminder>`):
1. Read the watcher's output file.
2. Tell the user the result right away (merged or closed, cleanup worked or failed).

## When you run several agents at once — worktree pool

Use the ready-made pool so each agent gets its own separate folder (worktree).

```sh
gw worktree pool warm 3                  # 1. make them ahead of time (once)
gw worktree pool status                  # 2. check available > 0
WORKTREE_PATH=$(gw worktree pool acquire)  # 3. take one (prints its path)
#                                          # 4. run the agent inside it
gw worktree pool release <name>          # 5. give it back when done
gw worktree pool drain                   # remove all pool folders
```

> **Always give it back, even if something fails** — forgetting empties the pool.

## The worktree rules (things to never do)

Each worktree has a **home branch**; the main folder's home is `main`. `gw`
handles the folder boundaries for you. Because of that:

- **Don't `git checkout main`** — use `gw home` (it sends you home and catches up
  to the newest shared code). A plain checkout clashes between folders.
- **Don't `git stash`** — use `gw pause` (its quick save moves safely between
  folders; a stash doesn't).
- **Don't rebase stacked PRs by hand** — use `gw sync`.
- **Don't push to `main`** — every change goes through a PR.

## How to write commit notes

Start the note with one of these labels, then a short description:

```
feat:     new feature          docs:     documentation
fix:      bug fix              refactor: cleanup (no behavior change)
chore:    build / tooling      test:     tests
```

Examples: `feat: add gw await command` · `fix: handle detached HEAD in status` ·
`chore: bump version to 0.6.0`

## Notes

- Opening the browser (`gw open` / `--open`) and the merge notification
  (`gw await`) are set up through your own settings, not the tool:
  - `GW_OPEN_URL_CMD` → a script that opens a link (e.g. a separate Chrome profile)
  - `GW_NOTIFY_CMD` → a script that shows a notification (e.g. macOS `osascript`)
