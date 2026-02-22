//! Integration tests for `gw worktree pool` commands

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

/// Get stdout from output as stripped string
fn stdout_str(output: &Output) -> String {
    strip_ansi(&String::from_utf8_lossy(&output.stdout))
}

/// Get stderr from output as stripped string
fn stderr_str(output: &Output) -> String {
    strip_ansi(&String::from_utf8_lossy(&output.stderr))
}

/// Assert command succeeded, panic with stderr if not
fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed (exit {}):\nstdout: {}\nstderr: {}",
        output.status.code().unwrap_or(-1),
        stdout_str(output),
        stderr_str(output),
    );
}

// --- warm ---

#[test]
fn test_warm_creates_worktrees() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    let output = run_gw(local.path(), &["worktree", "pool", "warm", "3"]);
    assert_success(&output, "warm 3");

    let out = stdout_str(&output);
    assert!(out.contains("Created pool-001"), "output: {out}");
    assert!(out.contains("Created pool-002"), "output: {out}");
    assert!(out.contains("Created pool-003"), "output: {out}");
    assert!(
        out.contains("3 created, 3 available, 3 total"),
        "output: {out}"
    );

    // Verify worktree directories exist
    assert!(local.path().join(".worktrees/pool-001").exists());
    assert!(local.path().join(".worktrees/pool-002").exists());
    assert!(local.path().join(".worktrees/pool-003").exists());

    // Verify branches were created (now pool-NNN, not pool/pool-NNN)
    let branches = run_git(local.path(), &["branch", "--list", "pool-*"]);
    assert!(branches.contains("pool-001"), "branches: {branches}");
    assert!(branches.contains("pool-002"), "branches: {branches}");
    assert!(branches.contains("pool-003"), "branches: {branches}");
}

#[test]
fn test_warm_is_idempotent() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Warm to 2
    let output = run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    assert_success(&output, "warm 2");

    // Warm to 2 again — should do nothing
    let output = run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    assert_success(&output, "warm 2 (idempotent)");

    let out = stdout_str(&output);
    assert!(out.contains("already has 2 available"), "output: {out}");
}

#[test]
fn test_warm_incremental() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Warm to 1
    let output = run_gw(local.path(), &["worktree", "pool", "warm", "1"]);
    assert_success(&output, "warm 1");

    // Warm to 3 — should create 2 more
    let output = run_gw(local.path(), &["worktree", "pool", "warm", "3"]);
    assert_success(&output, "warm 3");

    let out = stdout_str(&output);
    assert!(
        out.contains("2 created, 3 available, 3 total"),
        "output: {out}"
    );
}

// --- acquire ---

#[test]
fn test_acquire_prints_path_to_stdout() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);

    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&output, "acquire");

    // stdout should contain exactly the worktree path (plus newline)
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path_normalized = path.replace('\\', "/");
    assert!(
        path_normalized.ends_with(".worktrees/pool-001"),
        "Expected worktree path, got: {path}"
    );
    assert!(
        Path::new(&path).exists(),
        "Acquired path should exist: {path}"
    );

    // stderr should show the acquire info
    let err = stderr_str(&output);
    assert!(err.contains("Acquired pool-001"), "stderr: {err}");
    assert!(err.contains("0 remaining"), "stderr: {err}");

    // Acquire marker should exist
    assert!(
        local
            .path()
            .join(".git/worktree-pool/acquired/pool-001")
            .exists(),
        "Acquire marker should exist"
    );
}

#[test]
fn test_acquire_fails_when_exhausted() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);

    // Acquire the only one
    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&output, "acquire 1");

    // Second acquire should fail
    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert!(
        !output.status.success(),
        "Expected acquire to fail when pool is exhausted"
    );

    let err = stderr_str(&output);
    assert!(err.contains("No available worktrees"), "stderr: {err}");
}

#[test]
fn test_acquire_fails_when_not_initialized() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert!(!output.status.success());

    let err = stderr_str(&output);
    assert!(err.contains("not initialized"), "stderr: {err}");
}

// --- status ---

#[test]
fn test_status_shows_pool_info() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    assert_success(&output, "status");

    let out = stdout_str(&output);
    assert!(out.contains("1 available"), "output: {out}");
    assert!(out.contains("1 acquired"), "output: {out}");
    assert!(out.contains("2 total"), "output: {out}");
    assert!(out.contains("pool-001"), "output: {out}");
    assert!(out.contains("pool-002"), "output: {out}");
    assert!(out.contains("acquired"), "output: {out}");
    assert!(out.contains("available"), "output: {out}");
    // Should show BRANCH and OWNER columns
    assert!(
        out.contains("BRANCH"),
        "output should have BRANCH column: {out}"
    );
    assert!(
        out.contains("OWNER"),
        "output should have OWNER column: {out}"
    );
}

#[test]
fn test_status_fails_when_not_initialized() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    assert!(!output.status.success());
}

// --- drain ---

#[test]
fn test_drain_removes_all_worktrees() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);

    let output = run_gw(local.path(), &["worktree", "pool", "drain"]);
    assert_success(&output, "drain");

    let out = stdout_str(&output);
    assert!(out.contains("Drained 2 worktree(s)"), "output: {out}");

    // Worktree directories should be gone
    assert!(!local.path().join(".worktrees/pool-001").exists());
    assert!(!local.path().join(".worktrees/pool-002").exists());

    // Pool branches should be gone
    let branches = run_git(local.path(), &["branch", "--list", "pool-*"]);
    assert!(branches.is_empty(), "branches still exist: {branches}");

    // Status should fail (pool gone)
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    assert!(!output.status.success());
}

#[test]
fn test_drain_refuses_with_acquired_worktrees() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    let output = run_gw(local.path(), &["worktree", "pool", "drain"]);
    assert!(
        !output.status.success(),
        "Expected drain to fail with acquired worktrees"
    );

    let err = stderr_str(&output);
    assert!(err.contains("acquired worktree"), "stderr: {err}");
}

#[test]
fn test_drain_force_with_acquired_worktrees() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    let output = run_gw(local.path(), &["worktree", "pool", "drain", "--force"]);
    assert_success(&output, "drain --force");

    let out = stdout_str(&output);
    assert!(out.contains("Drained 2 worktree(s)"), "output: {out}");
}

#[test]
fn test_drain_then_warm_again() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Warm, drain, then warm again
    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);
    run_gw(local.path(), &["worktree", "pool", "drain"]);

    let output = run_gw(local.path(), &["worktree", "pool", "warm", "1"]);
    assert_success(&output, "re-warm after drain");

    let out = stdout_str(&output);
    assert!(out.contains("1 created"), "output: {out}");
}

// --- full workflow ---

#[test]
fn test_full_pool_lifecycle() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // 1. Warm
    let output = run_gw(local.path(), &["worktree", "pool", "warm", "3"]);
    assert_success(&output, "warm");

    // 2. Status shows 3 available
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    assert_success(&output, "status");
    let out = stdout_str(&output);
    assert!(out.contains("3 available"), "output: {out}");

    // 3. Acquire
    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&output, "acquire");
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(Path::new(&path).is_dir());

    // 4. Status shows 2 available, 1 acquired
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&output);
    assert!(out.contains("2 available"), "output: {out}");
    assert!(out.contains("1 acquired"), "output: {out}");

    // 5. Acquire marker should exist
    assert!(
        local
            .path()
            .join(".git/worktree-pool/acquired/pool-001")
            .exists()
    );

    // 6. Drain with force (since we have acquired worktrees)
    let output = run_gw(local.path(), &["worktree", "pool", "drain", "--force"]);
    assert_success(&output, "drain");

    let out = stdout_str(&output);
    assert!(out.contains("Drained 3 worktree(s)"), "output: {out}");
}

// --- acquire/drain cycle ---

#[test]
fn test_acquire_drain_cycle() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);

    // Acquire both
    let out1 = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&out1, "acquire 1");

    let out2 = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&out2, "acquire 2");

    // Pool should be exhausted
    let out3 = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert!(!out3.status.success(), "Expected exhaustion");

    // Force drain
    let output = run_gw(local.path(), &["worktree", "pool", "drain", "--force"]);
    assert_success(&output, "drain --force");

    // Re-warm and acquire again
    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);
    let out4 = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&out4, "re-acquire after drain+warm");
}
