//! Integration tests for `gw sync` command

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

    // Initialize bare repo with main as default branch
    run_git(dir.path(), &["init", "--bare", "--initial-branch=main"]);

    dir
}

/// Create a local repository with origin configured
fn create_local_repo(origin_path: &Path) -> TempDir {
    let dir = TempDir::new().expect("Failed to create temp dir");

    // Initialize git repo
    run_git(dir.path(), &["init"]);

    // Configure git user for commits
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test User"]);

    // Set default branch to main
    run_git(dir.path(), &["checkout", "-b", "main"]);

    // Create initial commit
    std::fs::write(dir.path().join("README.md"), "# Test").expect("Failed to write file");
    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-m", "Initial commit"]);

    // Add origin
    let origin_url = format!("file://{}", origin_path.display());
    run_git(dir.path(), &["remote", "add", "origin", &origin_url]);

    // Push to origin
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
        .env("NO_COLOR", "1") // Disable ANSI colors for easier testing
        .output()
        .expect("Failed to run gw command")
}

/// Get the current HEAD commit hash
fn get_head(dir: &Path) -> String {
    run_git(dir, &["rev-parse", "HEAD"])
}

#[test]
fn test_sync_on_home_branch_pulls_from_origin() {
    // Setup: Create origin and local repos
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Record initial state
    let initial_head = get_head(local.path());

    // Simulate origin advancing: clone origin, add commit, push
    let contributor = TempDir::new().expect("Failed to create temp dir");
    let origin_url = format!("file://{}", origin.path().display());
    run_git(contributor.path(), &["clone", &origin_url, "."]);
    run_git(
        contributor.path(),
        &["config", "user.email", "contrib@example.com"],
    );
    run_git(contributor.path(), &["config", "user.name", "Contributor"]);
    // Ensure we're on main branch (clone might default to master on older git)
    run_git(contributor.path(), &["checkout", "main"]);
    std::fs::write(contributor.path().join("new_file.txt"), "new content")
        .expect("Failed to write file");
    run_git(contributor.path(), &["add", "."]);
    run_git(contributor.path(), &["commit", "-m", "Add new file"]);
    run_git(contributor.path(), &["push", "origin", "main"]);

    // Verify local is behind
    assert_eq!(get_head(local.path()), initial_head);

    // Run `gw sync` on home branch (main)
    let output = run_gw(local.path(), &["sync"]);

    // Check command succeeded
    assert!(
        output.status.success(),
        "gw sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify local was updated
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("Pulled 1 commit") || stdout.contains("Already up to date"),
        "Expected sync output, got: {}",
        stdout
    );

    // Verify HEAD advanced
    let new_head = get_head(local.path());
    assert_ne!(
        new_head, initial_head,
        "HEAD should have advanced after sync"
    );
}

#[test]
fn test_sync_on_home_branch_when_already_up_to_date() {
    // Setup: Create origin and local repos
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Run `gw sync` without any new commits on origin
    let output = run_gw(local.path(), &["sync"]);

    // Check command succeeded
    assert!(
        output.status.success(),
        "gw sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify output says "Already up to date"
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("Already up to date"),
        "Expected 'Already up to date', got: {}",
        stdout
    );
}
