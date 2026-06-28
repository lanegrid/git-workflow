//! Integration test: switching to a branch held by another worktree must
//! produce a clear, actionable message instead of git's raw fatal error.

use std::path::Path;
use std::process::{Command, Output};

use regex::Regex;
use tempfile::TempDir;

fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to run git command");
    if !output.status.success() {
        panic!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn run_gw(dir: &Path, args: &[&str]) -> Output {
    let gw_path = env!("CARGO_BIN_EXE_gw");
    Command::new(gw_path)
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("Failed to run gw command")
}

fn stderr_str(output: &Output) -> String {
    strip_ansi(&String::from_utf8_lossy(&output.stderr))
}

#[test]
fn test_home_reports_branch_checked_out_in_another_worktree() {
    // Bare origin on `main`.
    let origin = TempDir::new().unwrap();
    run_git(origin.path(), &["init", "--bare", "--initial-branch=main"]);

    // Main worktree on `main`, pushed.
    let local = TempDir::new().unwrap();
    run_git(local.path(), &["init", "--initial-branch=main"]);
    run_git(local.path(), &["config", "user.email", "test@example.com"]);
    run_git(local.path(), &["config", "user.name", "Test User"]);
    std::fs::write(local.path().join("README.md"), "# Test").unwrap();
    run_git(local.path(), &["add", "."]);
    run_git(local.path(), &["commit", "-m", "Initial commit"]);
    let origin_url = format!("file://{}", origin.path().display());
    run_git(local.path(), &["remote", "add", "origin", &origin_url]);
    run_git(local.path(), &["push", "-u", "origin", "main"]);

    // Move the main worktree onto a feature branch so `main` is free to be
    // checked out elsewhere, then add a linked worktree that holds `main`.
    run_git(local.path(), &["checkout", "-b", "feature"]);
    let wt_parent = TempDir::new().unwrap();
    let wt_path = wt_parent.path().join("main-wt");
    run_git(
        local.path(),
        &["worktree", "add", &wt_path.to_string_lossy(), "main"],
    );

    // `gw home` from the main worktree wants to check out `main`, which is now
    // held by the linked worktree. It must explain the conflict, not dump git's
    // raw fatal, and must point at the recovery commands.
    let output = run_gw(local.path(), &["home"]);
    assert!(
        !output.status.success(),
        "gw home should fail when home branch is checked out elsewhere"
    );

    let err = stderr_str(&output);
    assert!(
        err.contains("checked out in another worktree"),
        "expected a clear conflict message, got: {err}"
    );
    // The conflicting worktree path should be surfaced.
    assert!(
        err.contains("main-wt"),
        "expected the conflicting worktree path in the message, got: {err}"
    );
}
