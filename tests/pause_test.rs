//! Integration tests for `gw pause` command
//!
//! Focus: untracked files must be treated as work to preserve. Before the
//! working-directory state accounted for untracked files, `gw pause` saw a repo
//! with only new (untracked) files as "clean", skipped the WIP commit, and
//! switched to home — silently leaving the new files behind.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// Create a bare repository to use as "origin"
fn create_origin_repo() -> TempDir {
    let dir = TempDir::new().expect("Failed to create temp dir");
    run_git(dir.path(), &["init", "--bare", "--initial-branch=main"]);
    dir
}

/// Create a local repository with origin configured
fn create_local_repo(origin_path: &Path) -> TempDir {
    let dir = TempDir::new().expect("Failed to create temp dir");

    run_git(dir.path(), &["init"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test User"]);
    run_git(dir.path(), &["checkout", "-b", "main"]);

    std::fs::write(dir.path().join("README.md"), "# Test").expect("Failed to write file");
    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-m", "Initial commit"]);

    let origin_url = format!("file://{}", origin_path.display());
    run_git(dir.path(), &["remote", "add", "origin", &origin_url]);
    run_git(dir.path(), &["push", "-u", "origin", "main"]);

    dir
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to run git command");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("git {} failed: {}", args.join(" "), stderr);
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

fn current_branch(dir: &Path) -> String {
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

#[test]
fn test_pause_commits_untracked_only_changes() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // On a feature branch with ONLY an untracked file (no tracked edits).
    run_git(local.path(), &["checkout", "-b", "feature-x", "main"]);
    std::fs::write(local.path().join("new_file.txt"), "brand new work")
        .expect("Failed to write file");

    let output = run_gw(local.path(), &["pause", "saving work"]);
    assert!(
        output.status.success(),
        "gw pause failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // pause should have created a WIP commit instead of treating the tree as
    // clean. After pause we are on home; the untracked file must live on the
    // feature branch, not be lost.
    assert_eq!(current_branch(local.path()), "main");

    let branch_files = run_git(local.path(), &["ls-tree", "-r", "--name-only", "feature-x"]);
    assert!(
        branch_files.contains("new_file.txt"),
        "Untracked file should have been committed onto feature-x, got tree: {}",
        branch_files
    );

    let subject = run_git(local.path(), &["log", "-1", "--format=%s", "feature-x"]);
    assert!(
        subject.starts_with("WIP"),
        "Expected a WIP commit on feature-x, got: {}",
        subject
    );
}
