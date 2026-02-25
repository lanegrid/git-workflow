//! Integration tests for `gw cleanup` command
//!
//! Tests the branch-switching behavior:
//! - `gw cleanup` on current branch → delete + switch to home
//! - `gw cleanup <branch>` when on that branch → delete + switch to home
//! - `gw cleanup <branch>` from different branch → delete only, no switch

use std::path::Path;
use std::process::{Command, Output};

use regex::Regex;
use tempfile::TempDir;

/// Strip ANSI escape codes from a string
fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

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

/// Run a git command in a specific directory
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

/// Run gw command in a specific directory
fn run_gw(dir: &Path, args: &[&str]) -> Output {
    let gw_path = env!("CARGO_BIN_EXE_gw");
    Command::new(gw_path)
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("Failed to run gw command")
}

/// Get the current branch name
fn current_branch(dir: &Path) -> String {
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Check if a local branch exists
fn branch_exists(dir: &Path, branch: &str) -> bool {
    Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a feature branch with a commit (branched from main)
fn create_feature_branch(dir: &Path, name: &str) {
    run_git(dir, &["checkout", "-b", name, "main"]);
    let filename = format!("{}.txt", name.replace('/', "-"));
    std::fs::write(dir.join(&filename), format!("work on {}", name)).expect("Failed to write file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", &format!("feat: work on {}", name)]);
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn test_cleanup_current_branch_switches_to_home() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Create and stay on feature branch
    create_feature_branch(local.path(), "feature-a");
    assert_eq!(current_branch(local.path()), "feature-a");

    // Run `gw cleanup` (no argument = current branch)
    let output = run_gw(local.path(), &["cleanup"]);

    assert!(
        output.status.success(),
        "gw cleanup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Should have switched to main
    assert_eq!(current_branch(local.path()), "main");
    // Branch should be deleted
    assert!(!branch_exists(local.path(), "feature-a"));
    // Should show "Switched to" message
    assert!(
        stdout.contains("Switched to"),
        "Expected switch message, got: {}",
        stdout
    );
    // Should show standard cleanup complete
    assert!(
        stdout.contains("Cleanup complete"),
        "Expected cleanup complete, got: {}",
        stdout
    );
}

#[test]
fn test_cleanup_named_branch_when_on_that_branch_switches_to_home() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Create and stay on feature branch
    create_feature_branch(local.path(), "feature-a");
    assert_eq!(current_branch(local.path()), "feature-a");

    // Run `gw cleanup feature-a` (explicit, same as current)
    let output = run_gw(local.path(), &["cleanup", "feature-a"]);

    assert!(
        output.status.success(),
        "gw cleanup feature-a failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Should have switched to main
    assert_eq!(current_branch(local.path()), "main");
    // Branch should be deleted
    assert!(!branch_exists(local.path(), "feature-a"));
    assert!(
        stdout.contains("Switched to"),
        "Expected switch message, got: {}",
        stdout
    );
}

#[test]
fn test_cleanup_different_branch_does_not_switch() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Create two feature branches
    create_feature_branch(local.path(), "feature-a");
    run_git(local.path(), &["checkout", "main"]);
    create_feature_branch(local.path(), "feature-b");

    // Stay on feature-b, cleanup feature-a
    assert_eq!(current_branch(local.path()), "feature-b");

    let output = run_gw(local.path(), &["cleanup", "feature-a"]);

    assert!(
        output.status.success(),
        "gw cleanup feature-a failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Should still be on feature-b (no switch!)
    assert_eq!(current_branch(local.path()), "feature-b");
    // feature-a should be deleted
    assert!(!branch_exists(local.path(), "feature-a"));
    // feature-b should still exist
    assert!(branch_exists(local.path(), "feature-b"));
    // Should NOT contain "Switched to"
    assert!(
        !stdout.contains("Switched to"),
        "Should not switch branches, but got: {}",
        stdout
    );
    // Should show "stayed on" message
    assert!(
        stdout.contains("stayed on"),
        "Expected 'stayed on' message, got: {}",
        stdout
    );
}

#[test]
fn test_cleanup_different_branch_with_uncommitted_changes_succeeds() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Create two feature branches
    create_feature_branch(local.path(), "feature-a");
    run_git(local.path(), &["checkout", "main"]);
    create_feature_branch(local.path(), "feature-b");

    // Stay on feature-b with uncommitted changes
    std::fs::write(local.path().join("dirty.txt"), "uncommitted work")
        .expect("Failed to write file");

    assert_eq!(current_branch(local.path()), "feature-b");

    // Cleanup feature-a should succeed even with dirty working dir
    let output = run_gw(local.path(), &["cleanup", "feature-a"]);

    assert!(
        output.status.success(),
        "gw cleanup feature-a should succeed with dirty working dir on different branch: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Should still be on feature-b
    assert_eq!(current_branch(local.path()), "feature-b");
    // feature-a should be deleted
    assert!(!branch_exists(local.path(), "feature-a"));
    // Uncommitted file should still be there
    assert!(local.path().join("dirty.txt").exists());
}

#[test]
fn test_cleanup_current_branch_with_uncommitted_changes_fails() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Create and stay on feature branch
    create_feature_branch(local.path(), "feature-a");

    // Create uncommitted changes (modify a tracked file)
    std::fs::write(local.path().join("feature-a.txt"), "modified content")
        .expect("Failed to write file");

    assert_eq!(current_branch(local.path()), "feature-a");

    // Cleanup current branch should fail due to uncommitted changes
    let output = run_gw(local.path(), &["cleanup"]);

    assert!(
        !output.status.success(),
        "gw cleanup should fail with uncommitted changes on current branch"
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));

    // Should still be on feature-a (no switch happened)
    assert_eq!(current_branch(local.path()), "feature-a");
    // Branch should NOT be deleted
    assert!(branch_exists(local.path(), "feature-a"));
    // Should mention uncommitted changes (error messages go to stderr)
    assert!(
        stderr.contains("uncommitted changes"),
        "Expected uncommitted changes error, got: {}",
        stderr
    );
}
