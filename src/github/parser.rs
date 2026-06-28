//! JSON parsing for GitHub CLI output, backed by `serde_json`.
//!
//! `gh pr view --json ...` returns a single JSON object. We deserialize it with
//! serde rather than hand-rolled string scanning so that whitespace, field
//! order, nulls, nested objects, and future fields are all handled correctly.

use serde::Deserialize;

use crate::error::{GwError, Result};

use super::types::RawPrData;

/// Shape of the JSON returned by `gh pr view --json number,title,url,state,...`.
///
/// Only the fields we request are modeled; serde ignores any extras, so adding
/// more `--json` fields later won't break parsing.
#[derive(Debug, Deserialize)]
struct GhPrJson {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    state: String,
    #[serde(default = "default_base_ref", rename = "baseRefName")]
    base_ref_name: String,
    #[serde(default, rename = "headRefName")]
    head_ref_name: String,
    #[serde(default, rename = "mergeCommit")]
    merge_commit: Option<MergeCommit>,
}

/// `mergeCommit` is `null` for non-merged PRs, or `{ "oid": "<sha>" }`.
#[derive(Debug, Deserialize)]
struct MergeCommit {
    #[serde(default)]
    oid: Option<String>,
}

fn default_base_ref() -> String {
    "main".to_string()
}

/// Parse PR JSON from `gh pr view --json` output.
///
/// Expected format:
/// ```json
/// {
///   "number": 123,
///   "title": "...",
///   "url": "...",
///   "state": "MERGED",
///   "baseRefName": "main",
///   "headRefName": "feature/x",
///   "mergeCommit": {"oid": "..."},
///   "mergedAt": "..."
/// }
/// ```
pub fn parse_pr_json(json: &str) -> Result<RawPrData> {
    let parsed: GhPrJson = serde_json::from_str(json)
        .map_err(|e| GwError::Other(format!("Failed to parse PR JSON: {e}. Raw: {json}")))?;

    Ok(RawPrData {
        number: parsed.number,
        title: parsed.title,
        url: parsed.url,
        state: parsed.state,
        base_branch: parsed.base_ref_name,
        head_branch: parsed.head_ref_name,
        merge_commit: parsed.merge_commit.and_then(|m| m.oid),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pr_json_merged() {
        let json = r#"{"number":42,"title":"feat: add feature","url":"https://github.com/owner/repo/pull/42","state":"MERGED","baseRefName":"main","mergeCommit":{"oid":"abc123"},"mergedAt":"2024-01-01T00:00:00Z"}"#;

        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "feat: add feature");
        assert_eq!(pr.state, "MERGED");
        assert_eq!(pr.base_branch, "main");
        assert_eq!(pr.merge_commit, Some("abc123".to_string()));
    }

    #[test]
    fn test_parse_pr_json_open() {
        let json = r#"{"number":43,"title":"fix: bug","url":"https://github.com/owner/repo/pull/43","state":"OPEN","baseRefName":"main","mergeCommit":null,"mergedAt":null}"#;

        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.number, 43);
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.merge_commit, None);
    }

    #[test]
    fn test_parse_pr_json_closed() {
        let json = r#"{"number":44,"title":"wip: abandoned","url":"https://github.com/owner/repo/pull/44","state":"CLOSED","baseRefName":"develop","mergeCommit":null,"mergedAt":null}"#;

        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.number, 44);
        assert_eq!(pr.state, "CLOSED");
        assert_eq!(pr.base_branch, "develop");
    }

    #[test]
    fn test_parse_pr_json_missing_number() {
        let json = r#"{"title":"no number"}"#;
        assert!(parse_pr_json(json).is_err());
    }

    #[test]
    fn test_parse_pr_json_japanese_title() {
        let json = r#"{"number":45,"title":"feat: 日本語タイトル","url":"https://github.com/owner/repo/pull/45","state":"OPEN","baseRefName":"main"}"#;

        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.title, "feat: 日本語タイトル");
    }

    #[test]
    fn test_parse_pr_json_special_characters_in_title() {
        let json = r#"{"number":46,"title":"fix: handle \"edge case\" & <special> chars","url":"https://github.com/owner/repo/pull/46","state":"OPEN","baseRefName":"main"}"#;

        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.title, "fix: handle \"edge case\" & <special> chars");
    }

    // --- Robustness cases the previous hand-rolled parser got wrong ---

    #[test]
    fn test_parse_pr_json_pretty_printed_with_spaces() {
        // gh can emit whitespace ("title": "...") — the old substring parser,
        // which searched for `"title":"`, would miss the value entirely.
        let json = r#"{
            "number": 47,
            "title": "feat: spaced out",
            "url": "https://github.com/owner/repo/pull/47",
            "state": "OPEN",
            "baseRefName": "main",
            "mergeCommit": null
        }"#;

        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.number, 47);
        assert_eq!(pr.title, "feat: spaced out");
        assert_eq!(pr.base_branch, "main");
        assert_eq!(pr.merge_commit, None);
    }

    #[test]
    fn test_parse_pr_json_field_order_independent() {
        let json = r#"{"state":"OPEN","baseRefName":"develop","title":"reordered","number":48}"#;
        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.number, 48);
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.base_branch, "develop");
    }

    #[test]
    fn test_parse_pr_json_ignores_unknown_fields() {
        let json = r#"{"number":49,"title":"t","state":"OPEN","baseRefName":"main","author":{"login":"x"},"labels":[{"name":"bug"}]}"#;
        let pr = parse_pr_json(json).unwrap();
        assert_eq!(pr.number, 49);
        assert_eq!(pr.title, "t");
    }
}
