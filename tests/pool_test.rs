//! Integration tests for `gw worktree pool` commands
//!
//! Pool worktrees are now per-leader: each worktree creates its own pool
//! under `.worktrees/` with names prefixed by the leader name.

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

/// Get the leader name as gw computes it (dir name with leading dots stripped)
fn leader_name_for(path: &Path) -> String {
    let raw = path.file_name().unwrap().to_string_lossy().to_string();
    raw.trim_start_matches('.').to_string()
}

// --- warm ---

#[test]
fn test_warm_creates_worktrees() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    let output = run_gw(local.path(), &["worktree", "pool", "warm", "3"]);
    assert_success(&output, "warm 3");

    let out = stdout_str(&output);
    assert!(
        out.contains(&format!("Created {prefix}001")),
        "output: {out}"
    );
    assert!(
        out.contains(&format!("Created {prefix}002")),
        "output: {out}"
    );
    assert!(
        out.contains(&format!("Created {prefix}003")),
        "output: {out}"
    );
    assert!(
        out.contains("3 created, 3 available, 3 total"),
        "output: {out}"
    );

    // Verify worktree directories exist
    assert!(
        local
            .path()
            .join(format!(".worktrees/{prefix}001"))
            .exists()
    );
    assert!(
        local
            .path()
            .join(format!(".worktrees/{prefix}002"))
            .exists()
    );
    assert!(
        local
            .path()
            .join(format!(".worktrees/{prefix}003"))
            .exists()
    );

    // Verify branches were created with leader prefix
    let branches = run_git(local.path(), &["branch", "--list", &format!("{prefix}*")]);
    assert!(
        branches.contains(&format!("{prefix}001")),
        "branches: {branches}"
    );
    assert!(
        branches.contains(&format!("{prefix}002")),
        "branches: {branches}"
    );
    assert!(
        branches.contains(&format!("{prefix}003")),
        "branches: {branches}"
    );

    // Verify .worktrees/ was added to .git/info/exclude (not .gitignore)
    let exclude = std::fs::read_to_string(local.path().join(".git/info/exclude"))
        .expect(".git/info/exclude should exist");
    assert!(
        exclude.contains(".worktrees/"),
        ".git/info/exclude should contain .worktrees/: {exclude}"
    );
    // .gitignore should NOT be modified
    let gitignore = std::fs::read_to_string(local.path().join(".gitignore")).unwrap_or_default();
    assert!(
        !gitignore.contains(".worktrees/"),
        ".gitignore should not contain .worktrees/: {gitignore}"
    );
}

#[test]
fn test_warm_exclude_idempotent() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    // Warm twice
    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);
    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);

    // .worktrees/ should appear exactly once in .git/info/exclude
    let exclude = std::fs::read_to_string(local.path().join(".git/info/exclude"))
        .expect(".git/info/exclude should exist");
    let count = exclude
        .lines()
        .filter(|l| l.trim() == ".worktrees/")
        .count();
    assert_eq!(count, 1, ".worktrees/ should appear once: {exclude}");
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
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);

    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&output, "acquire");

    // stdout should contain exactly the worktree path (plus newline)
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path_normalized = path.replace('\\', "/");
    assert!(
        path_normalized.ends_with(&format!(".worktrees/{prefix}001")),
        "Expected worktree path, got: {path}"
    );
    assert!(
        Path::new(&path).exists(),
        "Acquired path should exist: {path}"
    );

    // stderr should show the acquire info
    let err = stderr_str(&output);
    assert!(
        err.contains(&format!("Acquired {prefix}001")),
        "stderr: {err}"
    );
    assert!(err.contains("0 remaining"), "stderr: {err}");
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
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    assert_success(&output, "status");

    let out = stdout_str(&output);
    // Summary line shows counts
    assert!(out.contains("1 available"), "output: {out}");
    assert!(out.contains("1 acquired"), "output: {out}");
    assert!(out.contains("2 total"), "output: {out}");
    // Default: shows acquired worktrees
    assert!(out.contains(&format!("{prefix}001")), "output: {out}");
    assert!(
        out.contains("BRANCH"),
        "output should have BRANCH column: {out}"
    );
}

#[test]
fn test_status_verbose_shows_all() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    let output = run_gw(local.path(), &["worktree", "pool", "status", "-v"]);
    assert_success(&output, "status -v");

    let out = stdout_str(&output);
    // Verbose shows all entries
    assert!(out.contains(&format!("{prefix}001")), "output: {out}");
    assert!(out.contains(&format!("{prefix}002")), "output: {out}");
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
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);

    let output = run_gw(local.path(), &["worktree", "pool", "drain"]);
    assert_success(&output, "drain");

    let out = stdout_str(&output);
    assert!(out.contains("Drained 2 worktree(s)"), "output: {out}");

    // Worktree directories should be gone
    assert!(
        !local
            .path()
            .join(format!(".worktrees/{prefix}001"))
            .exists()
    );
    assert!(
        !local
            .path()
            .join(format!(".worktrees/{prefix}002"))
            .exists()
    );

    // Pool branches should be gone
    let branches = run_git(local.path(), &["branch", "--list", &format!("{prefix}*")]);
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

// --- release ---

#[test]
fn test_release_returns_worktree_to_pool() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    // Status: 1 available, 1 acquired
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&output);
    assert!(out.contains("1 available"), "before release: {out}");
    assert!(out.contains("1 acquired"), "before release: {out}");

    // Release all
    let output = run_gw(local.path(), &["worktree", "pool", "release"]);
    assert_success(&output, "release");

    // Status: 2 available, 0 acquired
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&output);
    assert!(out.contains("2 available"), "after release: {out}");
    assert!(out.contains("0 acquired"), "after release: {out}");
}

#[test]
fn test_release_by_name() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);

    // Acquire both
    run_gw(local.path(), &["worktree", "pool", "acquire"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    // Release only the first one by name
    let name = format!("{prefix}001");
    let output = run_gw(local.path(), &["worktree", "pool", "release", &name]);
    assert_success(&output, "release by name");

    // Status: 1 available, 1 acquired
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&output);
    assert!(out.contains("1 available"), "after release by name: {out}");
    assert!(out.contains("1 acquired"), "after release by name: {out}");
}

#[test]
fn test_release_all() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "3"]);

    // Acquire all 3
    run_gw(local.path(), &["worktree", "pool", "acquire"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    // Release all (no name arg)
    let output = run_gw(local.path(), &["worktree", "pool", "release"]);
    assert_success(&output, "release all");

    // Status: 3 available, 0 acquired
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&output);
    assert!(out.contains("3 available"), "after release all: {out}");
    assert!(out.contains("0 acquired"), "after release all: {out}");
}

#[test]
fn test_release_fails_when_none_acquired() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());

    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);

    // Release with nothing acquired
    let output = run_gw(local.path(), &["worktree", "pool", "release"]);
    assert!(
        !output.status.success(),
        "Expected release to fail when none acquired"
    );

    let err = stderr_str(&output);
    assert!(
        err.contains("No acquired worktrees to release"),
        "stderr: {err}"
    );
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

    // 5. Release (not drain!)
    let output = run_gw(local.path(), &["worktree", "pool", "release"]);
    assert_success(&output, "release");

    // 6. Status shows 3 available again
    let output = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&output);
    assert!(out.contains("3 available"), "after release: {out}");
    assert!(out.contains("0 acquired"), "after release: {out}");

    // 7. Drain
    let output = run_gw(local.path(), &["worktree", "pool", "drain"]);
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

// --- inspection on release / acquire ---

/// Path to a pool worktree directory under the leader's `.worktrees/`.
fn pool_worktree_path(leader_root: &Path, prefix: &str, n: u32) -> std::path::PathBuf {
    leader_root
        .join(".worktrees")
        .join(format!("{prefix}{n:03}"))
}

#[test]
fn test_release_by_name_rejects_dirty_worktree() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);
    let acq = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&acq, "acquire");

    // An agent leaves uncommitted work behind and forgets to clean up.
    let wt = pool_worktree_path(local.path(), &prefix, 1);
    std::fs::write(wt.join("leftover.txt"), "wip").unwrap();

    let name = format!("{prefix}001");
    let output = run_gw(local.path(), &["worktree", "pool", "release", &name]);
    assert!(
        !output.status.success(),
        "release should reject a dirty worktree"
    );
    let err = stderr_str(&output);
    assert!(err.contains("not in a clean state"), "stderr: {err}");

    // It must remain acquired (not silently returned to the pool).
    let status = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&status);
    assert!(out.contains("0 available"), "after rejected release: {out}");
    assert!(out.contains("1 acquired"), "after rejected release: {out}");
}

#[test]
fn test_release_all_keeps_dirty_releases_clean() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);
    run_gw(local.path(), &["worktree", "pool", "acquire"]);

    // Dirty only the second worktree.
    let wt2 = pool_worktree_path(local.path(), &prefix, 2);
    std::fs::write(wt2.join("leftover.txt"), "wip").unwrap();

    let output = run_gw(local.path(), &["worktree", "pool", "release"]);
    assert!(
        !output.status.success(),
        "release all should report the kept dirty worktree"
    );

    // Clean 001 returned, dirty 002 kept acquired.
    let status = run_gw(local.path(), &["worktree", "pool", "status"]);
    let out = stdout_str(&status);
    assert!(out.contains("1 available"), "after partial release: {out}");
    assert!(out.contains("1 acquired"), "after partial release: {out}");
}

#[test]
fn test_acquire_skips_unclean_available_worktree() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "2"]);

    // Dirty the first available worktree (e.g. a crashed agent left it behind).
    let wt1 = pool_worktree_path(local.path(), &prefix, 1);
    std::fs::write(wt1.join("leftover.txt"), "wip").unwrap();

    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert_success(&output, "acquire skipping dirty");

    // It must hand out the clean 002, not the dirty 001.
    let path = String::from_utf8_lossy(&output.stdout)
        .trim()
        .replace('\\', "/");
    assert!(
        path.ends_with(&format!(".worktrees/{prefix}002")),
        "should skip dirty 001 and acquire 002, got: {path}"
    );
    let err = stderr_str(&output);
    assert!(err.contains("Skipping unclean worktree"), "stderr: {err}");
}

#[test]
fn test_acquire_fails_when_all_available_unclean() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let leader = leader_name_for(local.path());
    let prefix = format!("{leader}-pool-");

    run_gw(local.path(), &["worktree", "pool", "warm", "1"]);

    // Dirty the only available worktree.
    let wt1 = pool_worktree_path(local.path(), &prefix, 1);
    std::fs::write(wt1.join("leftover.txt"), "wip").unwrap();

    let output = run_gw(local.path(), &["worktree", "pool", "acquire"]);
    assert!(
        !output.status.success(),
        "acquire should fail when no clean worktree is available"
    );
    let err = stderr_str(&output);
    assert!(err.contains("No clean worktree available"), "stderr: {err}");
}
