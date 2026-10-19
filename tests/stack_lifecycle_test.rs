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

fn local_work(r: &Repo) -> String {
    fs::write(r.local.join("initial"), "staged\n").unwrap();
    git(&r.local, &["add", "initial"]);
    fs::write(r.local.join("initial"), "staged\nunstaged\n").unwrap();
    fs::write(r.local.join("scratch"), "untracked work").unwrap();
    git(&r.local, &["status", "--porcelain"])
}

fn saved_ref(r: &Repo) -> String {
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(r.journal()).unwrap()).unwrap();
    format!(
        "refs/gw/sync-work/{}",
        journal["work"]["token"].as_str().unwrap()
    )
}

#[test]
fn keep_work_restacks_and_preserves_index_untracked_and_existing_stashes() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    fs::write(r.local.join("initial"), "previous stash").unwrap();
    git(&r.local, &["stash", "push", "-m", "user stash"]);
    let stash = git(&r.local, &["rev-parse", "refs/stash"]);
    let before = local_work(&r);
    fs::write(r.local.join(".git/info/exclude"), "ignored\n").unwrap();
    fs::write(r.local.join("ignored"), "ignored work").unwrap();
    failure(r.gw(&["sync"]), "gw sync --keep-work");
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    success(r.gw(&["sync", "--keep-work"]));
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    assert_eq!(git(&r.local, &["rev-parse", "refs/stash"]), stash);
    assert_eq!(git(&r.local, &["stash", "list"]).lines().count(), 1);
    assert_eq!(
        fs::read_to_string(r.local.join("scratch")).unwrap(),
        "untracked work"
    );
    assert_eq!(
        fs::read_to_string(r.local.join("ignored")).unwrap(),
        "ignored work"
    );
    assert_eq!(git(&r.remote, &["show", "child:initial"]), "initial");
    assert_eq!(
        git(&r.remote, &["log", "main..child", "--format=%s"]),
        "child-change"
    );
    assert!(!r.journal().exists());
    assert!(git(&r.local, &["for-each-ref", "refs/gw/sync-work/"]).is_empty());
}

#[test]
fn keep_work_survives_push_failure_and_stash_changes_then_resumes() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    let before = local_work(&r);
    fs::write(r.mock.join("fail-push"), "").unwrap();
    failure(r.gw(&["sync", "--keep-work"]), "injected push failure");
    let backup = saved_ref(&r);
    assert!(!git(&r.local, &["rev-parse", &backup]).is_empty());
    assert!(git(&r.local, &["status", "--porcelain"]).is_empty());
    // Another tool/worktree can change the shared stash list while sync is paused.
    fs::write(r.local.join("initial"), "someone else's stash").unwrap();
    git(&r.local, &["stash", "push", "-m", "other work"]);
    let other = git(&r.local, &["rev-parse", "refs/stash"]);
    fs::remove_file(r.mock.join("fail-push")).unwrap();
    success(r.gw(&["sync"]));
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    assert_eq!(git(&r.local, &["rev-parse", "refs/stash"]), other);
    assert!(!r.journal().exists());
}

#[test]
fn keep_work_abort_restores_original_head_and_staging() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    let head = r.head();
    let before = local_work(&r);
    fs::write(r.mock.join("fail-push"), "").unwrap();
    failure(r.gw(&["sync", "--keep-work"]), "injected push failure");
    success(r.gw(&["sync", "--abort"]));
    assert_eq!(r.head(), head);
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    assert_eq!(git(&r.local, &["config", "branch.child.gwBase"]), "parent");
    assert!(!r.journal().exists());
}

#[test]
fn restore_conflict_is_failure_and_requires_explicit_acknowledgment() {
    let r = Repo::new();
    git(&r.local, &["checkout", "-b", "feature"]);
    r.commit("feature-change");
    git(&r.local, &["push", "-u", "origin", "feature"]);
    git(&r.local, &["checkout", "main"]);
    fs::write(r.local.join("initial"), "upstream edit\n").unwrap();
    git(&r.local, &["commit", "-am", "upstream edit"]);
    git(&r.local, &["push", "origin", "main"]);
    git(&r.local, &["checkout", "feature"]);
    fs::write(r.local.join("initial"), "local edit\n").unwrap();
    failure(
        r.gw(&["sync", "--keep-work"]),
        "local work restoration failed",
    );
    let backup = saved_ref(&r);
    assert_eq!(git(&r.remote, &["rev-parse", "feature"]), r.head());
    assert!(!r.local.join(".git/rebase-merge").exists());
    let contents = fs::read_to_string(r.local.join("initial")).unwrap();
    failure(r.gw(&["sync"]), "gw sync --finish-work");
    assert_eq!(
        fs::read_to_string(r.local.join("initial")).unwrap(),
        contents
    );
    failure(r.gw(&["sync", "--finish-work"]), "Unresolved files remain");
    let status = r.gw(&["status"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("gw sync --finish-work"));
    fs::write(r.local.join("initial"), "resolved local edit\n").unwrap();
    git(&r.local, &["add", "initial"]);
    success(r.gw(&["sync", "--finish-work"]));
    assert_eq!(
        fs::read_to_string(r.local.join("initial")).unwrap(),
        "resolved local edit\n"
    );
    assert!(!r.journal().exists());
    assert!(git(&r.local, &["for-each-ref", &backup]).is_empty());
}

#[test]
fn keep_work_handles_home_fast_forward_and_untracked_path_collision() {
    let r = Repo::new();
    git(&r.local, &["branch", "old-home"]);
    r.commit("scratch");
    git(&r.local, &["push", "origin", "main"]);
    git(&r.local, &["reset", "--hard", "old-home"]);
    fs::write(r.local.join("scratch"), "local untracked").unwrap();
    failure(
        r.gw(&["sync", "--keep-work"]),
        "local work restoration failed",
    );
    assert_eq!(r.head(), git(&r.remote, &["rev-parse", "main"]));
    let reference = saved_ref(&r);
    assert_eq!(
        git(&r.local, &["show", &format!("{reference}^3:scratch")]),
        "local untracked"
    );
    // The user explicitly reconciles the collision before finishing.
    fs::write(r.local.join("scratch"), "reconciled work").unwrap();
    success(r.gw(&["sync", "--finish-work"]));
    assert_eq!(
        fs::read_to_string(r.local.join("scratch")).unwrap(),
        "reconciled work"
    );
}

#[test]
fn keep_work_rejects_resume_from_another_worktree() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    local_work(&r);
    fs::write(r.mock.join("fail-push"), "").unwrap();
    failure(r.gw(&["sync", "--keep-work"]), "injected push failure");
    let other = r._root.path().join("other");
    git(
        &r.local,
        &[
            "worktree",
            "add",
            "-b",
            "other",
            other.to_str().unwrap(),
            "main",
        ],
    );
    failure(r.command(&other, &["sync"]), "Unfinished sync belongs");
    failure(r.command(&other, &["sync", "--abort"]), "original worktree");
    assert!(r.journal().exists());
}

#[test]
fn keep_work_preflight_failures_leave_work_untouched_and_flags_validate() {
    let r = Repo::new();
    r.stack();
    let before = local_work(&r);
    fs::write(r.mock.join("fail-query"), "").unwrap();
    failure(r.gw(&["sync", "--keep-work"]), "API unavailable");
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    assert!(!r.journal().exists());
    for flags in [
        ["--abort", "--keep-work"],
        ["--abort", "--finish-work"],
        ["--keep-work", "--finish-work"],
    ] {
        let out = r.gw(&["sync", flags[0], flags[1]]);
        assert_eq!(out.status.code(), Some(2));
    }
    failure(
        r.gw(&["sync", "--finish-work"]),
        "No interrupted work restoration",
    );
}

#[test]
fn keep_work_noop_does_not_create_snapshot_or_change_staging() {
    let r = Repo::new();
    git(&r.local, &["checkout", "-b", "feature"]);
    let before = local_work(&r);
    let out = r.gw(&["sync", "--keep-work"]);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Saving staged"));
    success(out);
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    assert!(!r.journal().exists());
    assert!(git(&r.local, &["stash", "list"]).is_empty());
}

#[test]
fn keep_work_recovers_capture_and_interrupted_restoration_without_reapplying() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    local_work(&r);
    fs::write(r.mock.join("fail-push"), "").unwrap();
    failure(r.gw(&["sync", "--keep-work"]), "injected push failure");
    let reference = saved_ref(&r);
    let mut journal: serde_json::Value =
        serde_json::from_slice(&fs::read(r.journal()).unwrap()).unwrap();
    let oid = journal["work"]["oid"].as_str().unwrap().to_string();
    // Simulate interruption after stash creation, before pin/journal capture.
    git(
        &r.local,
        &[
            "stash",
            "store",
            "-m",
            &format!("On child: {}", journal["work"]["token"].as_str().unwrap()),
            &oid,
        ],
    );
    git(&r.local, &["update-ref", "-d", &reference]);
    journal["work"]["oid"] = serde_json::Value::Null;
    fs::write(r.journal(), serde_json::to_vec(&journal).unwrap()).unwrap();
    failure(r.gw(&["sync"]), "injected push failure");
    assert_eq!(git(&r.local, &["rev-parse", &reference]), oid);
    assert!(git(&r.local, &["stash", "list"]).is_empty());
    // Simulate a crash after applying saved work but before recording success.
    git(&r.local, &["stash", "apply", "--index", &reference]);
    journal = serde_json::from_slice(&fs::read(r.journal()).unwrap()).unwrap();
    journal["work"]["restoring"] = true.into();
    fs::write(r.journal(), serde_json::to_vec(&journal).unwrap()).unwrap();
    let before = git(&r.local, &["status", "--porcelain"]);
    fs::remove_file(r.mock.join("fail-push")).unwrap();
    failure(r.gw(&["sync"]), "gw sync --finish-work");
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    success(r.gw(&["sync", "--finish-work"]));
    assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
    assert!(!r.journal().exists());
}

#[test]
fn dirty_status_suggests_keep_work_when_parent_merged() {
    let r = Repo::new();
    r.stack();
    r.merged_parent("main");
    local_work(&r);
    let out = r.gw(&["status"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("gw sync --keep-work"));
    success(out);
}

#[test]
fn keep_work_rebase_conflict_can_abort_or_continue_without_losing_local_work() {
    for abort in [true, false] {
        let r = Repo::new();
        r.stack();
        git(&r.local, &["checkout", "parent"]);
        fs::write(r.local.join("initial"), "parent edit").unwrap();
        git(&r.local, &["commit", "-am", "parent edit"]);
        git(&r.local, &["push", "origin", "parent"]);
        git(&r.local, &["checkout", "child"]);
        fs::write(r.local.join("initial"), "child edit").unwrap();
        git(&r.local, &["commit", "-am", "child edit"]);
        let original = r.head();
        fs::write(r.local.join("child-change"), "staged work\n").unwrap();
        git(&r.local, &["add", "child-change"]);
        fs::write(r.local.join("child-change"), "staged work\nunstaged work\n").unwrap();
        fs::write(r.local.join("scratch"), "untracked").unwrap();
        let before = git(&r.local, &["status", "--porcelain"]);
        failure(r.gw(&["sync", "--keep-work"]), "recorded for recovery");
        assert!(!r.local.join("scratch").exists());
        if abort {
            git(&r.local, &["rebase", "--abort"]);
            success(r.gw(&["sync", "--abort"]));
            assert_eq!(r.head(), original);
        } else {
            fs::write(r.local.join("initial"), "resolved committed edit").unwrap();
            git(&r.local, &["add", "initial"]);
            git(
                &r.local,
                &["-c", "core.editor=true", "rebase", "--continue"],
            );
            success(r.gw(&["sync"]));
            assert_ne!(r.head(), original);
            assert_eq!(git(&r.remote, &["rev-parse", "child"]), r.head());
        }
        assert_eq!(git(&r.local, &["status", "--porcelain"]), before);
        assert_eq!(
            fs::read_to_string(r.local.join("scratch")).unwrap(),
            "untracked"
        );
        assert!(!r.journal().exists());
    }
}
