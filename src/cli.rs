//! CLI argument parsing with clap

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gw")]
#[command(about = "\
Worktree-aware git workflow: branch -> PR -> cleanup, safely.

Start here:  gw new feature/login     # the full flow is in --help")]
#[command(long_about = "\
Worktree-aware git workflow: branch -> PR -> cleanup, safely.

For developers using a PR-per-branch workflow. gw drives each change from a
fresh branch through review to a cleaned-up merge, and keeps parallel worktrees
in sync so day-to-day work never touches `main` directly.

\"home branch\": the branch a worktree returns to (the main worktree's home is
`main`). `gw home` switches to it and syncs with origin/main.

TYPICAL FLOW:
  gw new feature/login    # branch off a fresh origin/main
  # ...edit, then: git commit  ->  git push -u origin <branch>  ->  gh pr create
  gw await <pr> --open    # wait for CI, open the PR, watch to merge, then clean up

COMMANDS BY SITUATION:
  Everyday:   new, status, open, sync, cleanup, home
  Recovery:   pause, abandon, undo
  Worktrees:  worktree pool   (give parallel agents isolated worktrees)

Lost? Run `gw status` -- it prints the single next command for where you are.")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Show verbose output (git commands being run)
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Switch to home branch and sync with origin/main
    #[command(long_about = "\
Switch to your home branch and sync it with origin/main.

\"home branch\" = the branch a worktree returns to (the main worktree's home is
`main`). Use this instead of `git checkout main`, which conflicts across
worktrees.

Example:
  gw home")]
    Home,

    /// Create new branch from origin/main
    #[command(long_about = "\
Create a new branch off a freshly fetched origin/main.

If you already edited on your home branch, gw new carries those changes onto the
new branch -- nothing is stranded on `main`.

Example:
  gw new feature/add-login")]
    New {
        /// Name of the branch to create (e.g., feature/add-login)
        branch: Option<String>,
    },

    /// Delete merged branch and return to home
    #[command(long_about = "\
Delete a merged branch and return to your home branch.

Verifies the PR was merged before deleting, then switches home and syncs with
origin/main. Usually run for you by `gw await` -- reach for it directly only to
clean up a leftover branch.

Example:
  gw cleanup                 # the current branch
  gw cleanup feature/old     # a specific branch")]
    Cleanup {
        /// Branch to delete (defaults to current branch if not on home)
        branch: Option<String>,
    },

    /// Show current repository state
    #[command(long_about = "\
Show the current repository state and the single next command to run.

Inspects your working dir, upstream sync, home branch, and PR state, then prints
one `Next:` line -- the engine of the gw workflow. When in doubt, run this.

Example:
  gw status")]
    Status,

    /// Pause current work: WIP commit and return to home branch
    #[command(long_about = "\
Pause current work: record a WIP commit, then return to your home branch.

A safe way to switch tasks mid-change -- the WIP commit travels across worktrees,
unlike `git stash`. Resume later by checking the branch back out.

Example:
  gw pause \"investigating flaky test\"")]
    Pause {
        /// Optional message describing why work is paused
        message: Option<String>,
    },

    /// Abandon current changes and return to home branch
    #[command(long_about = "\
Discard all changes on the current branch and return to your home branch.

Destructive: both committed and uncommitted work on this branch is thrown away.
Use it when the branch is a dead end.

Example:
  gw abandon")]
    Abandon,

    /// Undo the last commit (soft reset HEAD~1)
    #[command(long_about = "\
Undo the last commit, keeping its changes as unstaged edits (soft reset HEAD~1).

Lets you re-stage or rewrite the most recent commit. Touches history only -- your
working files are left intact.

Example:
  gw undo")]
    Undo,

    /// Sync current branch after base PR is merged (update base to main, rebase, force push)
    #[command(long_about = "\
Restack this branch after the PR it was based on merged.

Updates the PR's base to main on GitHub, rebases onto the latest main, and
force-pushes. Use this instead of hand-rebasing stacked PRs.

Rewrites history and force-pushes the branch.

Example:
  gw sync")]
    Sync,

    /// Open the PR for the current branch in the browser
    #[command(long_about = "\
Open the current branch's PR in your browser.

The browser command is configured via GW_OPEN_URL_CMD in your dotfiles.

Example:
  gw open")]
    Open,

    /// Watch a specific PR until merged or closed, then clean up its branch
    #[command(long_about = "\
Watch a specific PR until it merges or closes, then clean up its branch.

Takes a PR number (not a branch) so the watcher stays bound to that PR even if
you switch branches. It waits for CI, then -- with --open -- opens the PR,
watches it to merge, and runs `gw cleanup`. If CI fails it stops and reports, so
you can fix, push, and rerun. Launch it in the background right after creating
the PR.

Example:
  gw await 41 --open")]
    Await {
        /// PR number to watch (required so the watcher stays bound to one PR
        /// even if you switch branches, e.g. while working a stacked PR)
        pr: u64,

        /// Also open the PR in the browser before watching
        #[arg(long)]
        open: bool,

        /// Skip waiting for CI checks
        #[arg(long = "no-wait")]
        no_wait: bool,

        /// Do not clean up the branch after the PR is merged
        #[arg(long = "no-cleanup")]
        no_cleanup: bool,

        /// Continue watching for merge even if CI checks fail
        /// (default: stop and report the failure)
        #[arg(long = "ignore-ci-failure")]
        ignore_ci_failure: bool,

        /// Seconds between merge-status polls
        #[arg(long, default_value_t = 30)]
        interval: u64,
    },

    /// Manage worktrees
    #[command(long_about = "\
Manage git worktrees for running parallel work in isolation.

Subcommands live under `gw worktree pool` -- a pre-warmed set of ready-to-use
worktrees so parallel agents each get their own checkout.

Example:
  gw worktree pool warm 3")]
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommands,
    },
}

#[derive(Subcommand)]
pub enum WorktreeCommands {
    /// Manage a pre-warmed worktree pool
    #[command(long_about = "\
Manage a pre-warmed pool of worktrees for parallel, isolated work.

Warm the pool once, then acquire a worktree per task and release it when done so
the next task can reuse it. Always release, even on error -- a forgotten release
drains the pool.

Example:
  gw worktree pool warm 3                 # pre-create 3 worktrees
  gw worktree pool acquire                # take one (prints its path)
  gw worktree pool release <name>         # return it when done
  gw worktree pool drain                  # remove them all")]
    Pool {
        #[command(subcommand)]
        command: PoolCommands,
    },
}

#[derive(Subcommand)]
pub enum PoolCommands {
    /// Pre-warm the pool with ready-to-use worktrees
    #[command(long_about = "\
Pre-create worktrees so the pool has `count` ready to acquire.

Run once before fanning out parallel work.

Example:
  gw worktree pool warm 3")]
    Warm {
        /// Target number of available worktrees in the pool
        count: usize,
    },

    /// Acquire a worktree from the pool (prints path to stdout)
    #[command(long_about = "\
Take a worktree from the pool and print its path to stdout.

Capture the path and run the task inside it; release it when done. Acquire fails
if the pool is empty -- `gw worktree pool warm <n>` first.

Example:
  WORKTREE_PATH=$(gw worktree pool acquire)")]
    Acquire,

    /// Show pool status
    #[command(long_about = "\
Show how many pool worktrees exist and how many are available to acquire.

Example:
  gw worktree pool status")]
    Status,

    /// Release acquired worktree(s) back to the pool
    #[command(long_about = "\
Return an acquired worktree to the pool so it can be reused.

With no name, releases all worktrees acquired by this process. Always release,
even on error -- a forgotten release drains the pool.

Example:
  gw worktree pool release          # all acquired
  gw worktree pool release wt-2     # a specific one")]
    Release {
        /// Name of the worktree to release (defaults to all acquired)
        name: Option<String>,
    },

    /// Remove all worktrees and clean up the pool
    #[command(long_about = "\
Remove every pool worktree and tear the pool down.

Refuses to drain while worktrees are still acquired unless you pass --force.

Example:
  gw worktree pool drain
  gw worktree pool drain --force")]
    Drain {
        /// Force drain even if worktrees are acquired
        #[arg(long)]
        force: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn await_requires_pr_number() {
        // Without a PR number, parsing must fail.
        assert!(Cli::try_parse_from(["gw", "await"]).is_err());
    }

    #[test]
    fn await_defaults() {
        let cli = Cli::try_parse_from(["gw", "await", "42"]).unwrap();
        match cli.command {
            Commands::Await {
                pr,
                open,
                no_wait,
                no_cleanup,
                ignore_ci_failure,
                interval,
            } => {
                assert_eq!(pr, 42);
                assert!(!open);
                assert!(!no_wait);
                assert!(!no_cleanup);
                assert!(!ignore_ci_failure);
                assert_eq!(interval, 30);
            }
            _ => panic!("expected Await command"),
        }
    }

    #[test]
    fn await_flags() {
        let cli = Cli::try_parse_from([
            "gw",
            "await",
            "42",
            "--open",
            "--no-wait",
            "--no-cleanup",
            "--ignore-ci-failure",
            "--interval",
            "5",
        ])
        .unwrap();
        match cli.command {
            Commands::Await {
                pr,
                open,
                no_wait,
                no_cleanup,
                ignore_ci_failure,
                interval,
            } => {
                assert_eq!(pr, 42);
                assert!(open);
                assert!(no_wait);
                assert!(no_cleanup);
                assert!(ignore_ci_failure);
                assert_eq!(interval, 5);
            }
            _ => panic!("expected Await command"),
        }
    }
}
