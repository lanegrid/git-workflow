//! Integration test: the default branch is detected, not hardcoded to `main`.
//!
//! A repository whose trunk is `master` must be treated just like a `main`
//! repository — `gw status` should recognize `master` as the home branch.

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

#[test]
fn test_status_detects_master_as_home_branch() {
    // Bare origin whose default branch is `master`.
    let origin = TempDir::new().unwrap();
    run_git(
        origin.path(),
        &["init", "--bare", "--initial-branch=master"],
    );

    // Local repo on `master`, pushed to origin.
    let local = TempDir::new().unwrap();
    run_git(local.path(), &["init", "--initial-branch=master"]);
    run_git(local.path(), &["config", "user.email", "test@example.com"]);
    run_git(local.path(), &["config", "user.name", "Test User"]);
    std::fs::write(local.path().join("README.md"), "# Test").unwrap();
    run_git(local.path(), &["add", "."]);
    run_git(local.path(), &["commit", "-m", "Initial commit"]);
    let origin_url = format!("file://{}", origin.path().display());
    run_git(local.path(), &["remote", "add", "origin", &origin_url]);
    run_git(local.path(), &["push", "-u", "origin", "master"]);

    let output = run_gw(local.path(), &["status"]);
    assert!(
        output.status.success(),
        "gw status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let out = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(out.contains("main repo"), "expected main-repo label: {out}");
    // master must be recognized as the home branch, not flagged as a feature branch.
    assert!(
        out.contains("master (home)"),
        "expected master to be the home branch: {out}"
    );
}
