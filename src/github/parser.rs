//! JSON parsing utilities for GitHub CLI output
//!
//! Simple JSON parsing without external dependencies.

use crate::error::{GwError, Result};

use super::types::RawPrData;

/// Parse PR JSON from `gh pr view --json` output
///
/// Expected format:
/// ```json
/// {
///   "number": 123,
///   "title": "...",
///   "url": "...",
///   "state": "MERGED",
///   "baseRefName": "main",
///   "mergeCommit": {"oid": "..."},
///   "mergedAt": "..."
/// }
/// ```
pub fn parse_pr_json(json: &str) -> Result<RawPrData> {
    let number = extract_json_number(json, "number")
        .ok_or_else(|| GwError::Other(format!("Failed to parse PR number from: {json}")))?;

    let title = extract_json_string(json, "title").unwrap_or_default();
    let url = extract_json_string(json, "url").unwrap_or_default();
    let state = extract_json_string(json, "state").unwrap_or_default();
    let base_branch =
        extract_json_string(json, "baseRefName").unwrap_or_else(|| "main".to_string());
    let merge_commit = extract_json_nested_string(json, "mergeCommit", "oid");

    Ok(RawPrData {
        number,
        title,
        url,
        state,
        base_branch,
        merge_commit,
    })
}

/// Extract a string value from JSON
///
/// # Examples
/// ```ignore
/// let json = r#"{"name":"test"}"#;
/// assert_eq!(extract_json_string(json, "name"), Some("test".to_string()));
/// ```
pub fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":\"", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = find_string_end(rest)?;
    Some(unescape_json_string(&rest[..end]))
}

/// Extract a number value from JSON
///
/// # Examples
/// ```ignore
/// let json = r#"{"number":42}"#;
/// assert_eq!(extract_json_number(json, "number"), Some(42));
/// ```
pub fn extract_json_number(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Extract a nested string value from JSON
///
/// # Examples
/// ```ignore
/// let json = r#"{"outer":{"inner":"value"}}"#;
/// assert_eq!(extract_json_nested_string(json, "outer", "inner"), Some("value".to_string()));
/// ```
pub fn extract_json_nested_string(json: &str, outer_key: &str, inner_key: &str) -> Option<String> {
    let outer_pattern = format!("\"{}\":{{", outer_key);
    let start = json.find(&outer_pattern)?;
    let rest = &json[start..];
    let end = rest.find('}')?;
    let inner = &rest[..=end];
    extract_json_string(inner, inner_key)
}

/// Extract a boolean value from JSON
#[allow(dead_code)]
pub fn extract_json_bool(json: &str, key: &str) -> Option<bool> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();

    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Find the end of a JSON string, handling escape sequences
fn find_string_end(s: &str) -> Option<usize> {
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some(i),
            '\\' => {
                // Skip the next character (escaped)
                chars.next();
            }
            _ => {}
        }
    }
    None
}

/// Unescape a JSON string
fn unescape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('/') => result.push('/'),
                Some('u') => {
                    // Unicode escape: \uXXXX
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code_point) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(code_point) {
                            result.push(c);
                        }
                    }
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_string_simple() {
        let json = r#"{"name":"test","value":"123"}"#;
        assert_eq!(extract_json_string(json, "name"), Some("test".to_string()));
        assert_eq!(extract_json_string(json, "value"), Some("123".to_string()));
        assert_eq!(extract_json_string(json, "missing"), None);
    }

    #[test]
    fn test_extract_json_string_with_escapes() {
        let json = r#"{"title":"feat: add \"quoted\" feature"}"#;
        assert_eq!(
            extract_json_string(json, "title"),
            Some("feat: add \"quoted\" feature".to_string())
        );
    }

    #[test]
    fn test_extract_json_string_with_unicode() {
        let json = r#"{"title":"fix: \u65e5\u672c\u8a9e"}"#;
        assert_eq!(
            extract_json_string(json, "title"),
            Some("fix: 日本語".to_string())
        );
    }

    #[test]
    fn test_extract_json_string_with_newline() {
        let json = r#"{"body":"line1\nline2"}"#;
        assert_eq!(
            extract_json_string(json, "body"),
            Some("line1\nline2".to_string())
        );
    }

    #[test]
    fn test_extract_json_number() {
        let json = r#"{"number":42,"name":"test"}"#;
        assert_eq!(extract_json_number(json, "number"), Some(42));
        assert_eq!(extract_json_number(json, "missing"), None);
    }

    #[test]
    fn test_extract_json_number_with_spaces() {
        let json = r#"{"number": 42}"#;
        assert_eq!(extract_json_number(json, "number"), Some(42));
    }

    #[test]
    fn test_extract_json_nested_string() {
        let json = r#"{"mergeCommit":{"oid":"abc123"}}"#;
        assert_eq!(
            extract_json_nested_string(json, "mergeCommit", "oid"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_extract_json_nested_string_null() {
        let json = r#"{"mergeCommit":null}"#;
        assert_eq!(extract_json_nested_string(json, "mergeCommit", "oid"), None);
    }

    #[test]
    fn test_extract_json_bool() {
        let json = r#"{"draft":true,"merged":false}"#;
        assert_eq!(extract_json_bool(json, "draft"), Some(true));
        assert_eq!(extract_json_bool(json, "merged"), Some(false));
        assert_eq!(extract_json_bool(json, "missing"), None);
    }

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
}
