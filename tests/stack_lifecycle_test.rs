//! Real repositories, deterministic gh responses, and injected publish failures.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fs2::FileExt;
use serde_json::json;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}

fn script(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Repo {
    _root: TempDir,
    local: PathBuf,
    remote: PathBuf,
    mock: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let local = root.path().join("local");
        let remote = root.path().join("remote");
        let mock = root.path().join("mock");
        for path in [&local, &remote, &mock] {
            fs::create_dir(path).unwrap();
        }
        git(&remote, &["init", "--bare", "--initial-branch=main"]);
        git(&local, &["init", "--initial-branch=main"]);
        git(&local, &["config", "user.name", "Test"]);
        git(&local, &["config", "user.email", "test@example.com"]);
        git(
            &local,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        let repo = Self {
            _root: root,
            local,
            remote,
            mock,
        };
        repo.commit("initial");
        git(&repo.local, &["push", "-u", "origin", "main"]);
        fs::write(repo.mock.join("children.json"), "[]").unwrap();
        script(
            &repo.mock.join("gh"),
            r#"#!/bin/sh
case "$1 $2" in
  '--version ') exit 0 ;;
  'pr view')
    if test -f "$GW_TEST_FIXTURES/fail-query"; then echo 'API unavailable' >&2; exit 1; fi
    case "$3" in
      parent|10) file=parent.json ;;
      child|20) file=child.json ;;
      *) echo 'no pull requests found' >&2; exit 1 ;;
    esac
    if test -f "$GW_TEST_FIXTURES/$file"; then cat "$GW_TEST_FIXTURES/$file";
    else echo 'no pull requests found' >&2; exit 1; fi ;;
  'pr edit')
    echo edit >> "$GW_TEST_FIXTURES/events"
    if test -f "$GW_TEST_FIXTURES/fail-edit"; then echo 'API unavailable' >&2; exit 1; fi
    cp "$GW_TEST_FIXTURES/child-main.json" "$GW_TEST_FIXTURES/child.json" ;;
  'pr list') cat "$GW_TEST_FIXTURES/children.json" ;;
  *) exit 1 ;;
esac
"#,
        );
        script(
            &repo.remote.join("hooks/pre-receive"),
            r#"#!/bin/sh
if test -f "$GW_TEST_FIXTURES/fail-push"; then
  echo 'injected push failure' >&2
  exit 1
fi
exit 0
"#,
        );
        repo
    }

    fn commit(&self, name: &str) {
        fs::write(self.local.join(name), name).unwrap();
        git(&self.local, &["add", name]);
        git(&self.local, &["commit", "-m", name]);
    }

    fn command(&self, dir: &Path, args: &[&str]) -> Output {
        let mut paths = vec![self.mock.clone()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        Command::new(env!("CARGO_BIN_EXE_gw"))
            .args(args)
            .current_dir(dir)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("GW_TEST_FIXTURES", &self.mock)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    fn gw(&self, args: &[&str]) -> Output {
        self.command(&self.local, args)
    }

    fn stack(&self) {
        git(&self.local, &["checkout", "-b", "parent"]);
        self.commit("parent-change");
        git(&self.local, &["push", "-u", "origin", "parent"]);
        success(self.gw(&["new", "child", "--stack"]));
        self.commit("child-change");
        git(&self.local, &["push", "-u", "origin", "child"]);
    }

    fn merged_parent(&self, destination: &str) {
        if destination != "main" {
            git(&self.local, &["checkout", "-b", destination, "main"]);
        } else {
            git(&self.local, &["checkout", "main"]);
        }
        git(&self.local, &["merge", "--squash", "parent"]);
        git(&self.local, &["commit", "-m", "squashed parent"]);
        let sha = git(&self.local, &["rev-parse", "HEAD"]);
        git(&self.local, &["push", "origin", destination]);
        git(&self.local, &["checkout", "child"]);
        fs::write(
            self.mock.join("parent.json"),
            json!({
                "number":10, "title":"parent", "state":"MERGED", "baseRefName":destination,
                "headRefName":"parent", "mergeCommit":{"oid":sha}
            })
            .to_string(),
        )
        .unwrap();
        self.child_pr("parent");
        fs::write(self.mock.join("child-main.json"), json!({
            "number":20, "title":"child", "state":"OPEN", "baseRefName":"main", "headRefName":"child"
        }).to_string()).unwrap();
    }

    fn child_pr(&self, base: &str) {
        fs::write(self.mock.join("child.json"), json!({
            "number":20, "title":"child", "state":"OPEN", "baseRefName":base, "headRefName":"child"
        }).to_string()).unwrap();
    }

    fn head(&self) -> String {
        git(&self.local, &["rev-parse", "HEAD"])
    }
    fn journal(&self) -> PathBuf {
        self.local.join(".git/gw-sync.json")
    }
}

fn success(out: Output) {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn failure(out: Output, message: &str) {
    assert!(
        !out.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains(message), "missing {message}: {combined}");
}

#[test]
fn cleanup_preserves_parent_for_local_only_child_in_another_worktree() {
    let r = Repo::new();
    r.stack();
    let other = r._root.path().join("other");
    git(&r.local, &["checkout", "parent"]);
    git(
        &r.local,
        &["worktree", "add", other.to_str().unwrap(), "child"],
    );
    failure(r.gw(&["cleanup", "parent"]), "local children");
    assert_eq!(git(&r.local, &["branch", "--show-current"]), "parent");
    assert!(!git(&r.remote, &["show-ref", "refs/heads/parent"]).is_empty());
}

#[test]
fn parent_merged_into_feature_branch_cannot_be_dropped_onto_main() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("grandparent");
    let before = r.head();
    failure(r.gw(&["sync"]), "not 'main'");
    assert_eq!(r.head(), before);
    assert!(!r.journal().exists());
    assert!(r.local.join("parent-change").exists());
    // The same guard applies before the child has a PR.
    fs::remove_file(r.mock.join("child.json")).unwrap();
    failure(r.gw(&["sync"]), "not 'main'");
    assert_eq!(r.head(), before);
}

#[test]
fn failed_push_keeps_base_and_dependencies_then_resumes_without_rebasing_again() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    fs::write(r.mock.join("fail-push"), "").unwrap();
    failure(r.gw(&["sync"]), "injected push failure");
    let rebased = r.head();
    assert!(r.journal().exists());
    assert!(
        !r.mock.join("events").exists(),
        "must publish before editing PR base"
    );
    failure(r.gw(&["cleanup", "parent"]), "unfinished gw sync");
    fs::remove_file(r.mock.join("fail-push")).unwrap();
    success(r.gw(&["sync"]));
    assert_eq!(r.head(), rebased);
    assert_eq!(git(&r.remote, &["rev-parse", "refs/heads/child"]), rebased);
    assert!(!r.journal().exists());
    assert!(r.local.join("parent-change").exists());
    assert_eq!(
        git(&r.local, &["log", "--format=%s", "origin/main..HEAD"]),
        "child-change"
    );
    success(r.gw(&["cleanup", "parent"]));
    assert!(
        git(
            &r.remote,
            &["for-each-ref", "--format=%(refname)", "refs/heads/parent"]
        )
        .is_empty()
    );
}

#[test]
fn failed_retarget_resumes_after_publish_and_preserves_local_edge() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    fs::write(r.mock.join("fail-edit"), "").unwrap();
    failure(r.gw(&["sync"]), "API unavailable");
    let rebased = r.head();
    assert_eq!(git(&r.remote, &["rev-parse", "refs/heads/child"]), rebased);
    assert_eq!(git(&r.local, &["config", "branch.child.gwBase"]), "parent");
    failure(r.gw(&["cleanup", "parent"]), "unfinished gw sync");
    fs::remove_file(r.mock.join("fail-edit")).unwrap();
    success(r.gw(&["sync"]));
    assert_eq!(r.head(), rebased);
    assert!(!r.journal().exists());
}

#[test]
fn retargeted_pr_without_boundary_refuses_plain_rebase() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    r.child_pr("main");
    git(&r.local, &["config", "--unset", "branch.child.gwBaseSha"]);
    let before = r.head();
    failure(r.gw(&["sync"]), "No valid recorded fork point");
    assert_eq!(r.head(), before);
}

#[test]
fn lifecycle_lock_is_shared_across_worktrees_and_released_on_close() {
    let r = Repo::new();
    r.stack();
    let other = r._root.path().join("other");
    git(
        &r.local,
        &["worktree", "add", other.to_str().unwrap(), "parent"],
    );
    let lock = fs::File::create(r.local.join(".git/gw-lifecycle.lock")).unwrap();
    lock.lock_exclusive().unwrap();
    failure(
        r.command(&other, &["new", "other-child", "--stack"]),
        "Another gw stack mutation",
    );
    failure(r.gw(&["cleanup", "parent"]), "Another gw stack mutation");
    failure(r.gw(&["sync"]), "Another gw stack mutation");
    drop(lock);
    success(r.command(&other, &["new", "other-child", "--stack"]));
}

#[test]
fn retry_does_not_overwrite_remote_changes_even_after_fetch() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    fs::write(r.mock.join("fail-push"), "").unwrap();
    failure(r.gw(&["sync"]), "injected push failure");
    let remote_change = git(&r.remote, &["rev-parse", "main"]);
    git(
        &r.remote,
        &["update-ref", "refs/heads/child", &remote_change],
    );
    git(&r.local, &["fetch", "origin"]);
    fs::remove_file(r.mock.join("fail-push")).unwrap();
    failure(r.gw(&["sync"]), "Remote changed");
    assert_eq!(git(&r.remote, &["rev-parse", "child"]), remote_change);
    assert!(r.journal().exists());
}

#[test]
fn cleanup_waits_for_all_siblings_including_unpublished_branches() {
    let r = Repo::new();
    r.stack();
    git(&r.local, &["checkout", "parent"]);
    success(r.gw(&["new", "sibling", "--stack"]));
    r.commit("sibling-change");
    r.merged_parent("main");
    success(r.gw(&["sync"]));
    failure(r.gw(&["cleanup", "parent"]), "sibling");
    git(&r.local, &["checkout", "sibling"]);
    success(r.gw(&["sync"]));
    assert!(r.local.join("parent-change").exists());
    assert_eq!(
        git(&r.local, &["log", "--format=%s", "origin/main..HEAD"]),
        "sibling-change"
    );
    success(r.gw(&["cleanup", "parent"]));
}

#[test]
fn remote_only_open_child_defers_both_local_and_remote_cleanup() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    git(&r.local, &["config", "--unset", "branch.child.gwBase"]);
    fs::write(
        r.mock.join("children.json"),
        format!(
            "[{}]",
            fs::read_to_string(r.mock.join("child.json")).unwrap()
        ),
    )
    .unwrap();
    failure(r.gw(&["cleanup", "parent"]), "Cleanup deferred");
    assert!(!git(&r.local, &["show-ref", "refs/heads/parent"]).is_empty());
    assert!(!git(&r.remote, &["show-ref", "refs/heads/parent"]).is_empty());
}

#[test]
fn abort_unpublished_sync_restores_head_and_preserves_dependencies() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    let before = r.head();
    fs::write(r.mock.join("fail-push"), "").unwrap();
    failure(r.gw(&["sync"]), "injected push failure");
    let status = r.gw(&["status"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("Unfinished sync on 'child'"));
    success(r.gw(&["sync", "--abort"]));
    assert_eq!(r.head(), before);
    assert!(!r.journal().exists());
    assert_eq!(git(&r.local, &["config", "branch.child.gwBase"]), "parent");
    failure(r.gw(&["cleanup", "parent"]), "local children");
}

#[test]
fn abort_cannot_roll_back_published_sync() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    fs::write(r.mock.join("fail-edit"), "").unwrap();
    failure(r.gw(&["sync"]), "API unavailable");
    let published = r.head();
    failure(r.gw(&["sync", "--abort"]), "Refusing rollback");
    assert_eq!(r.head(), published);
    assert!(r.journal().exists());
}

#[test]
fn conflict_can_be_aborted_without_losing_the_stack_record() {
    let r = Repo::new();
    r.stack();
    git(&r.local, &["checkout", "parent"]);
    fs::write(r.local.join("initial"), "parent edit").unwrap();
    git(&r.local, &["commit", "-am", "parent edit"]);
    git(&r.local, &["push", "origin", "parent"]);
    git(&r.local, &["checkout", "child"]);
    fs::write(r.local.join("initial"), "child edit").unwrap();
    git(&r.local, &["commit", "-am", "child edit"]);
    let before = r.head();
    failure(r.gw(&["sync"]), "recorded for recovery");
    failure(r.gw(&["sync", "--abort"]), "git rebase --abort first");
    git(&r.local, &["rebase", "--abort"]);
    success(r.gw(&["sync", "--abort"]));
    assert_eq!(r.head(), before);
    assert_eq!(git(&r.local, &["config", "branch.child.gwBase"]), "parent");
    assert!(!r.journal().exists());
}

#[test]
fn resolved_conflict_resumes_publication_and_records_the_frozen_parent() {
    let r = Repo::new();
    r.stack();
    git(&r.local, &["checkout", "parent"]);
    fs::write(r.local.join("initial"), "parent edit").unwrap();
    git(&r.local, &["commit", "-am", "parent edit"]);
    git(&r.local, &["push", "origin", "parent"]);
    let parent = r.head();
    git(&r.local, &["checkout", "child"]);
    fs::write(r.local.join("initial"), "child edit").unwrap();
    git(&r.local, &["commit", "-am", "child edit"]);
    failure(r.gw(&["sync"]), "recorded for recovery");
    fs::write(r.local.join("initial"), "resolved edit").unwrap();
    git(&r.local, &["add", "initial"]);
    git(
        &r.local,
        &["-c", "core.editor=true", "rebase", "--continue"],
    );
    let resolved = r.head();
    success(r.gw(&["sync"]));
    assert_eq!(r.head(), resolved);
    assert_eq!(git(&r.remote, &["rev-parse", "child"]), resolved);
    assert_eq!(git(&r.local, &["config", "branch.child.gwBaseSha"]), parent);
    assert_eq!(
        fs::read_to_string(r.local.join("initial")).unwrap(),
        "resolved edit"
    );
    assert!(!r.journal().exists());
}

#[test]
fn failed_pr_query_does_not_fall_back_to_a_different_base() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    let before = r.head();
    fs::write(r.mock.join("fail-query"), "").unwrap();
    failure(r.gw(&["sync"]), "API unavailable");
    assert_eq!(r.head(), before);
    assert!(!r.journal().exists());
}
