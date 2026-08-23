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

/// Advance origin's `main` by one commit from a separate clone, so the local
/// repo is behind without knowing it.
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

fn is_ancestor(dir: &Path, ancestor: &str, descendant: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(dir)
        .status()
        .expect("git merge-base")
        .success()
}

fn subjects(dir: &Path, range: &str) -> Vec<String> {
    run_git(dir, &["log", range, "--format=%s"])
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn test_sync_feature_branch_rebases_onto_advanced_main_and_force_pushes() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    // Feature branch with one commit, published.
    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(p, "x.txt", "feat: x");
    run_git(p, &["push", "-q", "-u", "origin", "feature/x"]);

    // main moves on without us.
    advance_origin_main(origin.path(), "main_moved.txt");

    let output = run_gw(p, &["sync"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "gw sync failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        stdout
    );
    assert!(stdout.contains("Rebasing onto origin/main"), "{stdout}");
    assert!(stdout.contains("Force pushing"), "{stdout}");

    // Branch now sits on the latest main with exactly its own commit on top.
    assert!(is_ancestor(p, "origin/main", "HEAD"));
    assert_eq!(subjects(p, "origin/main..HEAD"), vec!["feat: x"]);
    // ...and the remote copy was updated to match.
    assert_eq!(
        run_git(p, &["rev-parse", "HEAD"]),
        run_git(p, &["rev-parse", "origin/feature/x"])
    );
}

#[test]
fn test_sync_feature_branch_already_up_to_date() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(p, "x.txt", "feat: x");
    let head = get_head(p);

    let output = run_gw(p, &["sync"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("Already up to date with origin/main"),
        "{stdout}"
    );
    assert_eq!(get_head(p), head, "HEAD must not move when up to date");
}

#[test]
fn test_sync_unpublished_branch_rebases_without_pushing() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(p, "x.txt", "feat: x");
    advance_origin_main(origin.path(), "main_moved.txt");

    let output = run_gw(p, &["sync"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(output.status.success(), "{stdout}");
    assert!(is_ancestor(p, "origin/main", "HEAD"));
    assert!(!stdout.contains("Force pushing"), "{stdout}");
    assert!(
        stdout.contains("git push -u origin feature/x"),
        "should hint publishing: {stdout}"
    );
    assert!(
        !git_ref_exists(p, "origin/feature/x"),
        "an unpublished branch must stay unpublished"
    );
}

fn git_ref_exists(dir: &Path, reference: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", reference])
        .current_dir(dir)
        .status()
        .expect("git rev-parse")
        .success()
}

#[test]
fn test_sync_stacked_child_follows_parent_with_recorded_boundary() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    // parent: main - P1 ; child stacked on it: + C1
    run_git(p, &["checkout", "-q", "-b", "feature/parent"]);
    commit_file(p, "p1.txt", "P1");
    let out = run_gw(p, &["new", "feature/child", "--stack"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    commit_file(p, "c1.txt", "C1");

    // Parent gains a commit after the child was stacked.
    run_git(p, &["checkout", "-q", "feature/parent"]);
    commit_file(p, "p2.txt", "P2");
    let parent_tip = get_head(p);
    run_git(p, &["checkout", "-q", "feature/child"]);

    let output = run_gw(p, &["sync"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(output.status.success(), "{stdout}");

    // The child now sits on the parent's new tip with only its own commit on top.
    assert!(is_ancestor(p, "feature/parent", "HEAD"));
    assert_eq!(subjects(p, "feature/parent..HEAD"), vec!["C1"]);
    // The recorded boundary advanced to the parent's new tip, so the next
    // restack replays only the child's commits.
    assert_eq!(
        run_git(p, &["config", "--get", "branch.feature/child.gwBaseSha"]),
        parent_tip
    );
    assert_eq!(
        run_git(p, &["config", "--get", "branch.feature/child.gwBase"]),
        "feature/parent"
    );
}

#[test]
fn test_status_reports_behind_main_and_suggests_sync() {
    let origin = create_origin_repo();
    let local = create_local_repo(origin.path());
    let p = local.path();

    run_git(p, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(p, "x.txt", "feat: x");
    advance_origin_main(origin.path(), "main_moved.txt");

    let output = run_gw(p, &["status"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("Behind origin/main: 1 commit(s)"),
        "status should report the trunk moved: {stdout}"
    );
    assert!(
        stdout.contains("Next: sync with origin/main (1 commit(s) behind)"),
        "{stdout}"
    );
    assert!(stdout.contains("gw sync"), "{stdout}");
}
