//! Repository flow (`gw.flow`): how changes reach the default branch

use std::str::FromStr;

use crate::error::{GwError, Result};
use crate::git;

/// How changes reach the default branch, read from `git config gw.flow`.
///
/// The setting is plain git config, so it resolves through git's usual scopes
/// (repo-local, `includeIf`, global). Unset means [`Flow::Pr`], which keeps the
/// behavior gw has always had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flow {
    /// Every change goes through a feature branch and a PR.
    #[default]
    Pr,
    /// Changes are committed on the default branch and pushed directly.
    /// `gw new` still works for a change that should go through a PR.
    Trunk,
}

impl Flow {
    /// The flow set by `gw.flow`, or `None` when unset.
    pub fn configured() -> Result<Option<Self>> {
        git::gw_flow().map(|value| value.parse()).transpose()
    }

    /// The effective flow: `gw.flow`, defaulting to [`Flow::Pr`].
    pub fn detect() -> Result<Self> {
        Ok(Self::configured()?.unwrap_or_default())
    }

    /// The config value for this flow (`pr` / `trunk`).
    pub fn as_str(self) -> &'static str {
        match self {
            Flow::Pr => "pr",
            Flow::Trunk => "trunk",
        }
    }
}

impl FromStr for Flow {
    type Err = GwError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pr" => Ok(Flow::Pr),
            "trunk" => Ok(Flow::Trunk),
            _ => Err(GwError::InvalidFlow(s.trim().to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_known_values() {
        assert_eq!("pr".parse::<Flow>().unwrap(), Flow::Pr);
        assert_eq!("trunk".parse::<Flow>().unwrap(), Flow::Trunk);
    }

    #[test]
    fn test_parse_ignores_case_and_surrounding_whitespace() {
        assert_eq!(" Trunk ".parse::<Flow>().unwrap(), Flow::Trunk);
        assert_eq!("PR".parse::<Flow>().unwrap(), Flow::Pr);
    }

    #[test]
    fn test_parse_rejects_unknown_values() {
        let err = "direct".parse::<Flow>().unwrap_err();
        assert!(matches!(err, GwError::InvalidFlow(ref v) if v == "direct"));
    }

    #[test]
    fn test_default_is_pr() {
        assert_eq!(Flow::default(), Flow::Pr);
    }
}
