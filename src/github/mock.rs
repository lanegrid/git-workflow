//! Mock implementations for testing GitHub integration
//!
//! Provides configurable mock responses for testing without actual `gh` CLI.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::Result;

use super::client::{CommandExecutor, CommandOutput};

/// A recorded command call for verification
#[derive(Debug, Clone)]
pub struct CommandCall {
    pub program: String,
    pub args: Vec<String>,
    pub dir: Option<String>,
}

/// Mock command executor for testing
///
/// Allows configuring responses for specific commands and verifying calls.
#[derive(Debug, Clone)]
pub struct MockCommandExecutor {
    /// Configured responses: (program, args_pattern) -> output
    responses: Arc<Mutex<HashMap<String, CommandOutput>>>,
    /// Recorded calls for verification
    calls: Arc<Mutex<Vec<CommandCall>>>,
    /// Default response for unconfigured commands
    default_response: CommandOutput,
}

impl Default for MockCommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCommandExecutor {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
            default_response: CommandOutput::failure("command not configured"),
        }
    }

    /// Set default response for unconfigured commands
    pub fn with_default_response(mut self, output: CommandOutput) -> Self {
        self.default_response = output;
        self
    }

    /// Configure a response for a specific command pattern
    ///
    /// The key is generated from `program args[0] args[1] ...`
    pub fn on_command(&self, program: &str, args: &[&str], output: CommandOutput) {
        let key = self.make_key(program, args);
        self.responses.lock().unwrap().insert(key, output);
    }

    /// Get all recorded calls
    pub fn calls(&self) -> Vec<CommandCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Check if a specific command was called
    pub fn was_called(&self, program: &str, args: &[&str]) -> bool {
        let calls = self.calls.lock().unwrap();
        calls.iter().any(|c| {
            c.program == program && c.args.iter().map(|s| s.as_str()).collect::<Vec<_>>() == args
        })
    }

    /// Get the number of times a command was called
    pub fn call_count(&self, program: &str) -> usize {
        let calls = self.calls.lock().unwrap();
        calls.iter().filter(|c| c.program == program).count()
    }

    /// Clear all recorded calls
    pub fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }

    fn make_key(&self, program: &str, args: &[&str]) -> String {
        std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn record_call(&self, program: &str, args: &[&str], dir: Option<&str>) {
        self.calls.lock().unwrap().push(CommandCall {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            dir: dir.map(|s| s.to_string()),
        });
    }
}

impl CommandExecutor for MockCommandExecutor {
    fn execute(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        self.record_call(program, args, None);

        let key = self.make_key(program, args);
        let responses = self.responses.lock().unwrap();

        Ok(responses
            .get(&key)
            .cloned()
            .unwrap_or(self.default_response.clone()))
    }

    fn execute_in_dir(&self, program: &str, args: &[&str], dir: &str) -> Result<CommandOutput> {
        self.record_call(program, args, Some(dir));

        let key = self.make_key(program, args);
        let responses = self.responses.lock().unwrap();

        Ok(responses
            .get(&key)
            .cloned()
            .unwrap_or(self.default_response.clone()))
    }
}

/// Builder for creating mock GitHub scenarios
pub struct MockScenarioBuilder {
    executor: MockCommandExecutor,
}

impl Default for MockScenarioBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MockScenarioBuilder {
    pub fn new() -> Self {
        Self {
            executor: MockCommandExecutor::new(),
        }
    }

    /// gh CLI is available and authenticated
    pub fn gh_available(self) -> Self {
        self.executor.on_command(
            "gh",
            &["--version"],
            CommandOutput::success("gh version 2.40.0"),
        );
        self.executor.on_command(
            "gh",
            &["auth", "status"],
            CommandOutput::success("Logged in"),
        );
        self
    }

    /// gh CLI is not available
    pub fn gh_not_available(self) -> Self {
        self.executor.on_command(
            "gh",
            &["--version"],
            CommandOutput::failure("command not found: gh"),
        );
        self
    }

    /// gh CLI is available but not authenticated
    pub fn gh_not_authenticated(self) -> Self {
        self.executor.on_command(
            "gh",
            &["--version"],
            CommandOutput::success("gh version 2.40.0"),
        );
        self.executor.on_command(
            "gh",
            &["auth", "status"],
            CommandOutput::failure("You are not logged into any GitHub hosts. Run gh auth login"),
        );
        self
    }

    /// Configure a PR response for a branch
    pub fn with_pr(self, branch: &str, json: &str) -> Self {
        self.executor.on_command(
            "gh",
            &[
                "pr",
                "view",
                branch,
                "--json",
                "number,title,url,state,baseRefName,headRefName,mergeCommit,mergedAt",
            ],
            CommandOutput::success(json),
        );
        self
    }

    /// Configure no PR found for a branch
    pub fn with_no_pr(self, branch: &str) -> Self {
        self.executor.on_command(
            "gh",
            &[
                "pr",
                "view",
                branch,
                "--json",
                "number,title,url,state,baseRefName,headRefName,mergeCommit,mergedAt",
            ],
            CommandOutput::failure("no pull requests found for branch \"branch\""),
        );
        self
    }

    /// Configure PR view to fail with auth error
    pub fn with_auth_error(self, branch: &str) -> Self {
        self.executor.on_command(
            "gh",
            &[
                "pr",
                "view",
                branch,
                "--json",
                "number,title,url,state,baseRefName,headRefName,mergeCommit,mergedAt",
            ],
            CommandOutput::failure("To get started with GitHub CLI, please run: gh auth login"),
        );
        self
    }

    /// Configure git cat-file response for merge method detection
    pub fn with_merge_commit(self, sha: &str, parent_count: usize) -> Self {
        let parents = (0..parent_count)
            .map(|i| format!("parent abc{:03}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let output = format!(
            "tree def456\n{}\nauthor Test <test@example.com>\ncommitter Test <test@example.com>\n\nMerge commit",
            parents
        );
        self.executor.on_command(
            "git",
            &["cat-file", "-p", sha],
            CommandOutput::success(output),
        );
        self
    }

    /// Configure successful remote branch deletion
    pub fn with_remote_delete_success(self, branch: &str) -> Self {
        self.executor.on_command(
            "git",
            &["push", "origin", "--delete", branch],
            CommandOutput::success(""),
        );
        self
    }

    /// Configure remote branch already deleted
    pub fn with_remote_already_deleted(self, branch: &str) -> Self {
        self.executor.on_command(
            "git",
            &["push", "origin", "--delete", branch],
            CommandOutput::failure("error: unable to delete 'branch': remote ref does not exist"),
        );
        self
    }

    /// Configure remote branch deletion failure
    pub fn with_remote_delete_failure(self, branch: &str, error: &str) -> Self {
        self.executor.on_command(
            "git",
            &["push", "origin", "--delete", branch],
            CommandOutput::failure(error),
        );
        self
    }

    /// Build the mock executor
    pub fn build(self) -> MockCommandExecutor {
        self.executor
    }
}

/// Pre-built JSON responses for common PR scenarios
pub mod fixtures {
    /// Open PR JSON
    pub fn open_pr(number: u64, branch: &str) -> String {
        format!(
            r#"{{"number":{},"title":"feat: add feature","url":"https://github.com/owner/repo/pull/{}","state":"OPEN","baseRefName":"main","headRefName":"{}","mergeCommit":null,"mergedAt":null}}"#,
            number, number, branch
        )
    }

    /// Merged PR JSON (with merge commit)
    pub fn merged_pr(number: u64, merge_commit: &str) -> String {
        format!(
            r#"{{"number":{},"title":"feat: merged feature","url":"https://github.com/owner/repo/pull/{}","state":"MERGED","baseRefName":"main","headRefName":"feature/merged","mergeCommit":{{"oid":"{}"}},"mergedAt":"2024-01-01T00:00:00Z"}}"#,
            number, number, merge_commit
        )
    }

    /// Merged PR JSON (rebase, no merge commit)
    pub fn merged_pr_rebase(number: u64) -> String {
        format!(
            r#"{{"number":{},"title":"feat: rebased feature","url":"https://github.com/owner/repo/pull/{}","state":"MERGED","baseRefName":"main","headRefName":"feature/rebased","mergeCommit":null,"mergedAt":"2024-01-01T00:00:00Z"}}"#,
            number, number
        )
    }

    /// Closed PR JSON
    pub fn closed_pr(number: u64) -> String {
        format!(
            r#"{{"number":{},"title":"wip: abandoned","url":"https://github.com/owner/repo/pull/{}","state":"CLOSED","baseRefName":"main","headRefName":"feature/abandoned","mergeCommit":null,"mergedAt":null}}"#,
            number, number
        )
    }

    /// PR with Japanese title
    pub fn pr_with_japanese_title(number: u64) -> String {
        format!(
            r#"{{"number":{},"title":"feat: 日本語タイトル","url":"https://github.com/owner/repo/pull/{}","state":"OPEN","baseRefName":"main","headRefName":"feature/jp","mergeCommit":null,"mergedAt":null}}"#,
            number, number
        )
    }

    /// PR with develop base branch
    pub fn pr_with_develop_base(number: u64) -> String {
        format!(
            r#"{{"number":{},"title":"feat: develop branch","url":"https://github.com/owner/repo/pull/{}","state":"OPEN","baseRefName":"develop","headRefName":"feature/develop-based","mergeCommit":null,"mergedAt":null}}"#,
            number, number
        )
    }
}
