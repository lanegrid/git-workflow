//! Integration tests for `gw new`, focused on the `--stack` flag.
//!
//! Plain `gw new` branches off `origin/main`; `--stack` branches off the
//! CURRENT branch instead so stacked PRs can be built. `--stack` on the home
//! branch is refused, since plain `gw new` already covers that case.

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

/// A local repo on `main` with an `origin` it can fetch from.
fn setup_repo() -> TempDir {
    let origin = TempDir::new().unwrap();
    run_git(origin.path(), &["init", "--bare", "--initial-branch=main"]);

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

    // Keep both TempDirs alive for the duration of the test by leaking the
    // origin; the local dir is what the caller drives.
    std::mem::forget(origin);
    local
}

fn current_branch(dir: &Path) -> String {
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

#[test]
fn test_stack_branches_off_current_branch() {
    let local = setup_repo();
    let dir = local.path();

    // Build a parent feature branch with its own commit.
    assert!(run_gw(dir, &["new", "feature/parent"]).status.success());
    std::fs::write(dir.join("parent.txt"), "parent work").unwrap();
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", "feat: parent work"]);
    let parent_head = run_git(dir, &["rev-parse", "HEAD"]);

    // Stack a child on top of the parent.
    let output = run_gw(dir, &["new", "feature/child", "--stack"]);
    assert!(
        output.status.success(),
        "gw new --stack failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let out = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        out.contains("from feature/parent"),
        "expected base to be the parent branch: {out}"
    );
    // PR hint must carry the explicit base so GitHub uses the parent.
    assert!(
        out.contains("-B feature/parent"),
        "expected stacked PR hint with -B feature/parent: {out}"
    );

    assert_eq!(current_branch(dir), "feature/child");
    // The child must start at the parent's HEAD, not origin/main.
    assert_eq!(run_git(dir, &["rev-parse", "HEAD"]), parent_head);

    // The stacked base is recorded locally so the PR base can be suggested
    // before the PR exists.
    assert_eq!(
        run_git(dir, &["config", "--get", "branch.feature/child.gwBase"]),
        "feature/parent"
    );
}

#[test]
fn test_status_surfaces_recorded_stacked_base() {
    let local = setup_repo();
    let dir = local.path();

    // Parent branch with an open-PR-shaped history, pushed.
    assert!(run_gw(dir, &["new", "feature/parent"]).status.success());
    run_git(dir, &["commit", "--allow-empty", "-m", "feat: parent"]);
    run_git(dir, &["push", "-u", "origin", "feature/parent"]);

    // Stack a child (records gwBase), commit and push so it's PR-ready.
    assert!(
        run_gw(dir, &["new", "feature/child", "--stack"])
            .status
            .success()
    );
    run_git(dir, &["commit", "--allow-empty", "-m", "feat: child"]);
    run_git(dir, &["push", "-u", "origin", "feature/child"]);

    // With no PR yet, status must surface the recorded base and put it in the
    // create-PR hint as `-B`, so the stacked base can't be forgotten.
    let output = run_gw(dir, &["status"]);
    assert!(
        output.status.success(),
        "gw status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        out.contains("Base: feature/parent (stacked"),
        "expected the pre-PR stacked base to be shown: {out}"
    );
    assert!(
        out.contains("gh pr create") && out.contains("-B feature/parent"),
        "expected the create-PR hint to carry -B feature/parent: {out}"
    );
}

#[test]
fn test_plain_new_records_no_base() {
    let local = setup_repo();
    let dir = local.path();

    assert!(run_gw(dir, &["new", "feature/x"]).status.success());

    // Plain new (base = origin/main) must not record a stacked base.
    let output = Command::new("git")
        .args(["config", "--get", "branch.feature/x.gwBase"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "plain new should not record a gwBase"
    );
}

#[test]
fn test_stack_on_home_branch_is_refused() {
    let local = setup_repo();
    let dir = local.path();

    let output = run_gw(dir, &["new", "feature/child", "--stack"]);
    assert!(
        !output.status.success(),
        "gw new --stack on home should fail"
    );

    let err = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        err.contains("--stack requires a non-home branch"),
        "expected refusal message on stderr: {err}"
    );
    // The branch must not have been created.
    assert_eq!(current_branch(dir), "main");
}

#[test]
fn test_plain_new_from_home_bases_on_origin_main() {
    let local = setup_repo();
    let dir = local.path();

    let origin_main = run_git(dir, &["rev-parse", "origin/main"]);

    // Plain `gw new` from the home branch bases on origin/main and emits no
    // `-B` PR hint.
    let output = run_gw(dir, &["new", "feature/x"]);
    assert!(
        output.status.success(),
        "gw new failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let out = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        !out.contains("-B "),
        "plain new should not emit a -B PR hint: {out}"
    );
    assert_eq!(current_branch(dir), "feature/x");
    assert_eq!(run_git(dir, &["rev-parse", "HEAD"]), origin_main);
}

#[test]
fn test_plain_new_on_feature_branch_is_refused() {
    let local = setup_repo();
    let dir = local.path();

    // Move onto a feature branch.
    assert!(run_gw(dir, &["new", "feature/parent"]).status.success());

    // Plain `gw new` (no --stack) from a feature branch must refuse rather than
    // silently base on origin/main.
    let output = run_gw(dir, &["new", "feature/sibling"]);
    assert!(
        !output.status.success(),
        "plain gw new on a feature branch should fail"
    );

    let err = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        err.contains("not the home branch"),
        "expected refusal pointing at the home branch: {err}"
    );
    // Nothing created; still on the feature branch.
    assert_eq!(current_branch(dir), "feature/parent");
    assert!(
        run_gw(dir, &["status"]).status.success(),
        "feature/sibling should not exist"
    );
}

#[test]
fn test_new_from_home_carries_uncommitted_changes() {
    let local = setup_repo();
    let dir = local.path();

    // Dirty the home branch, then start a branch. The work must travel onto the
    // new branch (still uncommitted), not be stranded on main.
    std::fs::write(dir.join("wip.txt"), "in progress").unwrap();

    let output = run_gw(dir, &["new", "feature/x"]);
    assert!(
        output.status.success(),
        "gw new with dirty tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(current_branch(dir), "feature/x");
    // The untracked file is still present and uncommitted on the new branch.
    assert!(dir.join("wip.txt").exists());
    let status = run_git(dir, &["status", "--porcelain"]);
    assert!(
        status.contains("wip.txt"),
        "uncommitted change should remain on the new branch: {status}"
    );
}
