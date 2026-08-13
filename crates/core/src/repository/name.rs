use crate::grammar::{REPOSITORY, REVISION, VERSION, VERSION_SUFFIXES};
use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};
use std::{borrow::Borrow, fmt, str::FromStr, sync::LazyLock};

static REPO_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\A(?!.*-(?:{VERSION})(?:{VERSION_SUFFIXES})(?:-r{REVISION})?\z){REPOSITORY}\z"
    ))
    .unwrap()
});

/// Holds a validated PMS repository name.
#[derive(Archive, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RepoName(Box<str>);

impl RepoName {
    /// Creates a new [`RepoName`] from the given repository name.
    pub fn new(name: &str) -> anyhow::Result<Self> {
        match REPO_NAME_RE.is_match(name)? {
            true => Ok(Self(name.into())),
            false => bail!("invalid repository name: '{name}'"),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for RepoName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for RepoName {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
impl Default for RepoName {
    fn default() -> Self {
        Self::new("gentoo").unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_name_ok() {
        let valid_names = ["repo", "repo-", "repo--", "repo_1", "repo-r2"];
        for name in valid_names {
            let parsed = RepoName::new(name).unwrap();
            assert_eq!(parsed.as_str(), name);
        }
    }

    #[test]
    fn test_repository_name_err() {
        let invalid_names = [
            "",
            "-repo",
            "repo.foo",
            "repo/bar",
            "repo ",
            "repo*",
            "repo-1",
            "repo-1_alpha",
            "repo-1-r2",
        ];
        for name in invalid_names {
            assert!(RepoName::new(name).is_err(), "{name:?} should be invalid");
        }
    }
}
