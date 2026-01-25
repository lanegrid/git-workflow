//! Tests for git operations
//!
//! These tests use a helper that runs git commands in a temp directory
//! without changing the process's working directory (to avoid race conditions).

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Helper to create a temporary git repository
fn create_temp_repo() -> TempDir {
    let dir = TempDir::new().expect("Failed to create temp dir");

    // Initialize git repo
    run_git(dir.path(), &["init"]);

    // Configure git user for commits
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test User"]);

    // Create initial commit
    std::fs::write(dir.path().join("README.md"), "# Test").expect("Failed to write file");
    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-m", "Initial commit"]);

    dir
}

/// Run a git command in a specific directory
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to run git command");

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Check if git command succeeds
fn git_check(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_is_git_repo() {
    let dir = create_temp_repo();
    let result = git_check(dir.path(), &["rev-parse", "--git-dir"]);
    assert!(result);
}

#[test]
fn test_is_not_git_repo() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let result = git_check(dir.path(), &["rev-parse", "--git-dir"]);
    assert!(!result);
}

#[test]
fn test_current_branch() {
    let dir = create_temp_repo();
    let branch = run_git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);

    // Git default branch could be main or master depending on config
    assert!(branch == "main" || branch == "master");
}

#[test]
fn test_branch_exists() {
    let dir = create_temp_repo();

    // Create a test branch
    run_git(dir.path(), &["branch", "test-branch"]);

    let exists = git_check(
        dir.path(),
        &["show-ref", "--verify", "--quiet", "refs/heads/test-branch"],
    );
    assert!(exists);

    let not_exists = git_check(
        dir.path(),
        &[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/nonexistent-branch",
        ],
    );
    assert!(!not_exists);
}

#[test]
fn test_has_uncommitted_changes_clean() {
    let dir = create_temp_repo();
    let has_unstaged = !git_check(dir.path(), &["diff", "--quiet"]);
    let has_staged = !git_check(dir.path(), &["diff", "--cached", "--quiet"]);
    assert!(!has_unstaged);
    assert!(!has_staged);
}

#[test]
fn test_has_uncommitted_changes_unstaged() {
    let dir = create_temp_repo();

    // Modify a file
    std::fs::write(dir.path().join("README.md"), "# Modified").expect("Failed to write file");

    let has_unstaged = !git_check(dir.path(), &["diff", "--quiet"]);
    let has_staged = !git_check(dir.path(), &["diff", "--cached", "--quiet"]);

    assert!(has_unstaged);
    assert!(!has_staged);
}

#[test]
fn test_has_uncommitted_changes_staged() {
    let dir = create_temp_repo();

    // Modify and stage a file
    std::fs::write(dir.path().join("README.md"), "# Modified").expect("Failed to write file");
    run_git(dir.path(), &["add", "README.md"]);

    let has_unstaged = !git_check(dir.path(), &["diff", "--quiet"]);
    let has_staged = !git_check(dir.path(), &["diff", "--cached", "--quiet"]);

    assert!(!has_unstaged);
    assert!(has_staged);
}

#[test]
fn test_is_worktree_main_repo() {
    let dir = create_temp_repo();
    let git_dir = run_git(dir.path(), &["rev-parse", "--git-dir"]);
    let common_dir = run_git(dir.path(), &["rev-parse", "--git-common-dir"]);

    // In a main repo, git-dir should equal git-common-dir
    assert_eq!(git_dir, common_dir);
}

#[test]
fn test_head_commit() {
    let dir = create_temp_repo();
    let commit = run_git(dir.path(), &["rev-parse", "HEAD"]);
    assert!(!commit.is_empty());
    assert_eq!(commit.len(), 40); // Full SHA
}

#[test]
fn test_short_commit() {
    let dir = create_temp_repo();
    let commit = run_git(dir.path(), &["rev-parse", "--short", "HEAD"]);
    assert!(!commit.is_empty());
    assert!(commit.len() <= 10); // Short SHA
}

#[test]
fn test_head_commit_message() {
    let dir = create_temp_repo();
    let msg = run_git(dir.path(), &["log", "-1", "--format=%s"]);
    assert_eq!(msg, "Initial commit");
}

#[test]
fn test_stash_count_empty() {
    let dir = create_temp_repo();
    let output = run_git(dir.path(), &["stash", "list"]);
    let count = if output.is_empty() {
        0
    } else {
        output.lines().count()
    };
    assert_eq!(count, 0);
}

#[test]
fn test_checkout_new_branch() {
    let dir = create_temp_repo();

    // Get current branch to use as start point
    let current = run_git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);

    // Create and checkout new branch
    run_git(
        dir.path(),
        &["checkout", "-b", "new-feature", &current, "--quiet"],
    );

    let branch = run_git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(branch, "new-feature");
}

#[test]
fn test_delete_branch() {
    let dir = create_temp_repo();

    // Create a branch
    run_git(dir.path(), &["branch", "to-delete"]);

    // Verify it exists
    assert!(git_check(
        dir.path(),
        &["show-ref", "--verify", "--quiet", "refs/heads/to-delete"]
    ));

    // Delete it
    run_git(dir.path(), &["branch", "-d", "to-delete"]);

    // Verify it's gone
    assert!(!git_check(
        dir.path(),
        &["show-ref", "--verify", "--quiet", "refs/heads/to-delete"]
    ));
}
