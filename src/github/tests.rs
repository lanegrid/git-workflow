//! Comprehensive tests for GitHub integration
//!
//! Tests cover:
//! - PR state detection (open, merged, closed)
//! - Merge method detection (merge, squash, rebase)
//! - Error handling (auth errors, network errors)
//! - Edge cases (Japanese titles, special characters)

use super::client::GitHubClient;
use super::mock::{MockScenarioBuilder, fixtures};
use super::types::{MergeMethod, PrState};

// =============================================================================
// gh CLI availability tests
// =============================================================================

#[test]
fn test_gh_available() {
    let executor = MockScenarioBuilder::new().gh_available().build();
    let client = GitHubClient::with_executor(executor);

    assert!(client.is_available());
    assert!(client.is_authenticated());
}

#[test]
fn test_gh_not_available() {
    let executor = MockScenarioBuilder::new().gh_not_available().build();
    let client = GitHubClient::with_executor(executor);

    assert!(!client.is_available());
}

#[test]
fn test_gh_not_authenticated() {
    let executor = MockScenarioBuilder::new().gh_not_authenticated().build();
    let client = GitHubClient::with_executor(executor);

    assert!(client.is_available());
    assert!(!client.is_authenticated());
}

// =============================================================================
// PR state detection tests
// =============================================================================

#[test]
fn test_get_open_pr() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/test", &fixtures::open_pr(42, "feature/test"))
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client.get_pr_for_branch("feature/test").unwrap().unwrap();

    assert_eq!(pr.number, 42);
    assert!(pr.state.is_open());
    assert!(!pr.state.is_merged());
    assert!(!pr.state.is_closed());
    assert_eq!(pr.base_branch, "main");
}

#[test]
fn test_get_merged_pr_with_squash() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/test", &fixtures::merged_pr(42, "abc123"))
        .with_merge_commit("abc123", 1) // 1 parent = squash
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client.get_pr_for_branch("feature/test").unwrap().unwrap();

    assert_eq!(pr.number, 42);
    assert!(pr.state.is_merged());
    match &pr.state {
        PrState::Merged {
            method,
            merge_commit,
        } => {
            assert_eq!(*method, MergeMethod::Squash);
            assert_eq!(merge_commit, &Some("abc123".to_string()));
        }
        _ => panic!("Expected Merged state"),
    }
}

#[test]
fn test_get_merged_pr_with_merge() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/test", &fixtures::merged_pr(42, "def456"))
        .with_merge_commit("def456", 2) // 2 parents = regular merge
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client.get_pr_for_branch("feature/test").unwrap().unwrap();

    assert!(pr.state.is_merged());
    match &pr.state {
        PrState::Merged { method, .. } => {
            assert_eq!(*method, MergeMethod::Merge);
        }
        _ => panic!("Expected Merged state"),
    }
}

#[test]
fn test_get_merged_pr_with_rebase() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/test", &fixtures::merged_pr_rebase(42))
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client.get_pr_for_branch("feature/test").unwrap().unwrap();

    assert!(pr.state.is_merged());
    match &pr.state {
        PrState::Merged {
            method,
            merge_commit,
        } => {
            assert_eq!(*method, MergeMethod::Rebase);
            assert!(merge_commit.is_none());
        }
        _ => panic!("Expected Merged state"),
    }
}

#[test]
fn test_get_closed_pr() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/abandoned", &fixtures::closed_pr(99))
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client
        .get_pr_for_branch("feature/abandoned")
        .unwrap()
        .unwrap();

    assert_eq!(pr.number, 99);
    assert!(pr.state.is_closed());
    assert!(!pr.state.is_merged());
    assert!(!pr.state.is_open());
}

#[test]
fn test_no_pr_found() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_no_pr("feature/no-pr")
        .build();
    let client = GitHubClient::with_executor(executor);

    let result = client.get_pr_for_branch("feature/no-pr").unwrap();

    assert!(result.is_none());
}

// =============================================================================
// Error handling tests
// =============================================================================

#[test]
fn test_auth_error() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_auth_error("feature/test")
        .build();
    let client = GitHubClient::with_executor(executor);

    let result = client.get_pr_for_branch("feature/test");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not authenticated"));
}

// =============================================================================
// Remote branch deletion tests
// =============================================================================

#[test]
fn test_delete_remote_branch_success() {
    let executor = MockScenarioBuilder::new()
        .with_remote_delete_success("feature/test")
        .build();
    let client = GitHubClient::with_executor(executor.clone());

    let result = client.delete_remote_branch("feature/test");

    assert!(result.is_ok());
    assert!(executor.was_called("git", &["push", "origin", "--delete", "feature/test"]));
}

#[test]
fn test_delete_remote_branch_already_deleted() {
    let executor = MockScenarioBuilder::new()
        .with_remote_already_deleted("feature/test")
        .build();
    let client = GitHubClient::with_executor(executor);

    let result = client.delete_remote_branch("feature/test");

    // Should succeed even if branch doesn't exist
    assert!(result.is_ok());
}

#[test]
fn test_delete_remote_branch_failure() {
    let executor = MockScenarioBuilder::new()
        .with_remote_delete_failure("feature/test", "permission denied")
        .build();
    let client = GitHubClient::with_executor(executor);

    let result = client.delete_remote_branch("feature/test");

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("permission denied")
    );
}

// =============================================================================
// Edge case tests
// =============================================================================

#[test]
fn test_pr_with_japanese_title() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/ja", &fixtures::pr_with_japanese_title(50))
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client.get_pr_for_branch("feature/ja").unwrap().unwrap();

    assert_eq!(pr.title, "feat: 日本語タイトル");
}

#[test]
fn test_pr_with_non_main_base() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/dev", &fixtures::pr_with_develop_base(51))
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client.get_pr_for_branch("feature/dev").unwrap().unwrap();

    assert_eq!(pr.base_branch, "develop");
}

#[test]
fn test_pr_with_special_characters_in_title() {
    let json = r#"{"number":52,"title":"fix: handle \"edge case\" & <special>","url":"https://github.com/owner/repo/pull/52","state":"OPEN","baseRefName":"main","mergeCommit":null,"mergedAt":null}"#;

    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/special", json)
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client
        .get_pr_for_branch("feature/special")
        .unwrap()
        .unwrap();

    assert_eq!(pr.title, "fix: handle \"edge case\" & <special>");
}

// =============================================================================
// Command call verification tests
// =============================================================================

#[test]
fn test_commands_are_called_correctly() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/verify", &fixtures::open_pr(60, "feature/verify"))
        .build();
    let client = GitHubClient::with_executor(executor.clone());

    // Clear any calls from setup
    executor.clear_calls();

    // Make the call
    let _ = client.get_pr_for_branch("feature/verify");

    // Verify the correct command was called
    let calls = executor.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "gh");
    assert!(calls[0].args.contains(&"pr".to_string()));
    assert!(calls[0].args.contains(&"view".to_string()));
    assert!(calls[0].args.contains(&"feature/verify".to_string()));
}

#[test]
fn test_merge_method_detection_calls_git() {
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/merged", &fixtures::merged_pr(70, "sha123"))
        .with_merge_commit("sha123", 1)
        .build();
    let client = GitHubClient::with_executor(executor.clone());

    executor.clear_calls();

    let _ = client.get_pr_for_branch("feature/merged");

    // Should have called both gh and git
    assert!(executor.call_count("gh") >= 1);
    assert!(executor.call_count("git") >= 1);
    assert!(executor.was_called("git", &["cat-file", "-p", "sha123"]));
}

// =============================================================================
// Integration scenario tests
// =============================================================================

#[test]
fn test_cleanup_scenario_merged_pr() {
    // Simulates the cleanup command scenario where PR is merged
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/done", &fixtures::merged_pr(100, "merge123"))
        .with_merge_commit("merge123", 1) // squash
        .with_remote_delete_success("feature/done")
        .build();
    let client = GitHubClient::with_executor(executor);

    // Step 1: Check PR state
    let pr = client.get_pr_for_branch("feature/done").unwrap().unwrap();
    assert!(pr.state.is_merged());

    // Step 2: Delete remote branch (should succeed because PR is merged)
    let delete_result = client.delete_remote_branch("feature/done");
    assert!(delete_result.is_ok());
}

#[test]
fn test_cleanup_scenario_open_pr() {
    // Simulates checking an open PR before cleanup
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_pr("feature/wip", &fixtures::open_pr(101, "feature/wip"))
        .build();
    let client = GitHubClient::with_executor(executor);

    let pr = client.get_pr_for_branch("feature/wip").unwrap().unwrap();

    // PR is open - cleanup should warn about this
    assert!(pr.state.is_open());
    assert!(!pr.state.is_merged());
}

#[test]
fn test_cleanup_scenario_no_pr() {
    // Simulates cleanup when no PR exists
    let executor = MockScenarioBuilder::new()
        .gh_available()
        .with_no_pr("feature/local-only")
        .build();
    let client = GitHubClient::with_executor(executor);

    let result = client.get_pr_for_branch("feature/local-only").unwrap();

    // No PR - cleanup should handle this gracefully
    assert!(result.is_none());
}
