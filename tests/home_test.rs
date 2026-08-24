//! Integration tests for the stale-tree-flash fix: when returning to a home
//! branch that is behind origin, the home *ref* is fast-forwarded before the
//! checkout, so the working tree makes a single transition instead of
//! rewinding to the stale home and then pulling forward.

use std::path::Path;
use std::process::{Command, Output};

use regex::Regex;
use tempfile::TempDir;

fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

fn create_origin_repo() -> TempDir {
    let dir = TempDir::new().expect("Failed to create temp dir");
    run_git(dir.path(), &["init", "--bare", "--initial-branch=main"]);
    dir
}

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

/// Advance origin's `main` by one commit from a separate clone.
fn advance_origin_main(origin: &Path, file: &str) {
    let contributor = TempDir::new().expect("Failed to create temp dir");
    let origin_url = format!("file://{}", origin.display());
    run_git(contributor.path(), &["clone", "-q", &origin_url, "."]);
    run_git(
        contributor.path(),
        &["config", "user.email", "contrib@example.com"],
    );
    run_git(contributor.path(), &["config", "user.name", "Contributor"]);
    run_git(contributor.path(), &["checkout", "-q", "main"]);
    std::fs::write(contributor.path().join(file), "content").expect("Failed to write file");
    run_git(contributor.path(), &["add", "."]);
    run_git(
        contributor.path(),
        &["commit", "-q", "-m", &format!("Add {file}")],
    );
    run_git(contributor.path(), &["push", "-q", "origin", "main"]);
}

fn commit_file(dir: &Path, name: &str, subject: &str) {
    std::fs::write(dir.join(name), name).expect("write file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", subject]);
}

fn rev(dir: &Path, reference: &str) -> String {
    run_git(dir, &["rev-parse", reference])
}

#[test]
fn test_home_fast_forwards_ref_before_checkout() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    // On a feature branch while origin/main moves ahead of local main.
    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(p, "x.txt", "feat: x");
    advance_origin_main(origin.path(), "main_moved.txt");

    let output = run_gw(p, &["home"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "gw home failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        stdout
    );

    // The ref moved before the checkout (no stale-tree flash)...
    assert!(
        stdout.contains("Fast-forwarded main to origin/main (1 commit(s), ref only)"),
        "{stdout}"
    );
    // ...so the pull afterwards had nothing left to do.
    assert!(stdout.contains("Already up to date"), "{stdout}");
    assert_eq!(rev(p, "HEAD"), rev(p, "origin/main"));
    assert!(p.join("main_moved.txt").exists());
}

#[test]
fn test_home_diverged_main_is_left_alone() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    // Local main gains its own commit (diverged once origin moves too).
    commit_file(p, "local_only.txt", "local commit");
    let local_main = rev(p, "HEAD");
    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    advance_origin_main(origin.path(), "main_moved.txt");

    let output = run_gw(p, &["home"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // No forced ref move: the divergence is reported by the ff-only pull,
    // exactly as before, and the local commit is still on main.
    assert!(!stdout.contains("Fast-forwarded"), "{stdout}");
    assert!(!output.status.success(), "ff-only pull must fail: {stdout}");
    assert_eq!(rev(p, "main"), local_main);
}

#[test]
fn test_cleanup_fast_forwards_ref_before_checkout() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    // Published feature branch; origin/main advances (as if the PR merged).
    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(p, "x.txt", "feat: x");
    run_git(p, &["push", "-q", "-u", "origin", "feature/x"]);
    advance_origin_main(origin.path(), "main_moved.txt");

    let output = run_gw(p, &["cleanup"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "gw cleanup failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        stdout
    );
    assert!(
        stdout.contains("Fast-forwarded main to origin/main (1 commit(s), ref only)"),
        "{stdout}"
    );
    assert_eq!(rev(p, "HEAD"), rev(p, "origin/main"));
}

#[test]
fn test_abandon_fast_forwards_ref_before_checkout() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    std::fs::write(p.join("scratch.txt"), "wip").expect("write file");
    advance_origin_main(origin.path(), "main_moved.txt");

    let output = run_gw(p, &["abandon"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "gw abandon failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        stdout
    );
    assert!(
        stdout.contains("Fast-forwarded main to origin/main (1 commit(s), ref only)"),
        "{stdout}"
    );
    assert_eq!(rev(p, "HEAD"), rev(p, "origin/main"));
    assert!(
        !p.join("scratch.txt").exists(),
        "abandon discards untracked"
    );
}
