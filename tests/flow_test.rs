//! Integration test: `gw.flow` selects how `gw status` guides work on the
//! default branch.
//!
//! Under the default `pr` flow the home branch only ever suggests starting a
//! branch. Under `trunk`, the default branch is where work is committed and
//! pushed, so `gw status` walks commit → push there instead.

use std::path::Path;
use std::process::{Command, Output};

use regex::Regex;
use tempfile::TempDir;

fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

/// Keep the user's global/system git config (e.g. a global `gw.flow`) out of
/// the test repositories.
fn isolate(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = isolate(Command::new("git").args(args).current_dir(dir))
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
    isolate(Command::new(gw_path).args(args).current_dir(dir))
        .env("NO_COLOR", "1")
        .output()
        .expect("Failed to run gw command")
}

fn status(dir: &Path) -> String {
    let output = run_gw(dir, &["status"]);
    assert!(
        output.status.success(),
        "gw status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    strip_ansi(&String::from_utf8_lossy(&output.stdout))
}

/// A local repo on `main`, pushed to a bare origin. Both dirs must stay alive.
fn setup_repo() -> (TempDir, TempDir) {
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

    (origin, local)
}

#[test]
fn test_unset_flow_defaults_to_pr() {
    let (_origin, local) = setup_repo();
    std::fs::write(local.path().join("README.md"), "# Changed").unwrap();

    let out = status(local.path());
    assert!(out.contains("Flow: pr (default)"), "{out}");
    assert!(out.contains("Next: start new work"), "{out}");
}

#[test]
fn test_trunk_flow_walks_commit_then_push_on_main() {
    let (_origin, local) = setup_repo();
    run_git(local.path(), &["config", "gw.flow", "trunk"]);

    let out = status(local.path());
    assert!(out.contains("Flow: trunk (gw.flow)"), "{out}");
    assert!(out.contains("Next: commit on main (trunk flow)"), "{out}");
    assert!(out.contains("git push"), "{out}");

    std::fs::write(local.path().join("README.md"), "# Changed").unwrap();
    let out = status(local.path());
    assert!(out.contains("Next: commit changes"), "{out}");

    run_git(local.path(), &["commit", "-am", "Change"]);
    let out = status(local.path());
    assert!(out.contains("Next: push to remote"), "{out}");
}

#[test]
fn test_trunk_flow_without_remote_does_not_suggest_push() {
    let local = TempDir::new().unwrap();
    run_git(local.path(), &["init", "--initial-branch=main"]);
    run_git(local.path(), &["config", "user.email", "test@example.com"]);
    run_git(local.path(), &["config", "user.name", "Test User"]);
    run_git(local.path(), &["config", "gw.flow", "trunk"]);
    std::fs::write(local.path().join("README.md"), "# Test").unwrap();
    run_git(local.path(), &["add", "."]);
    run_git(local.path(), &["commit", "-m", "Initial commit"]);

    let out = status(local.path());
    assert!(out.contains("Next: commit on main (trunk flow)"), "{out}");
    assert!(!out.contains("git push"), "{out}");
}

#[test]
fn test_invalid_flow_is_an_error() {
    let (_origin, local) = setup_repo();
    run_git(local.path(), &["config", "gw.flow", "direct"]);

    let output = run_gw(local.path(), &["status"]);
    assert!(!output.status.success());
    let err = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(err.contains("Invalid gw.flow value 'direct'"), "{err}");
}
