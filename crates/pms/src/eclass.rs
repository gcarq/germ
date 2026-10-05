use std::borrow::Borrow;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};

/// Regex to validate eclass names.
static ECLASS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][a-zA-Z0-9_.-]*$").unwrap());

/// Holds a validated eclass name.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct EclassName(Box<str>);

impl EclassName {
    pub fn new(name: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let name = name.into();
        if !ECLASS_RE.is_match(name.as_ref())? || name.as_ref() == "default" {
            bail!("invalid eclass name: '{name}'");
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for EclassName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for EclassName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for EclassName {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::new(name)
    }
}

impl fmt::Display for EclassName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eclass_name_valid() {
        for name in ["apache-module", "autotools", "kernel-2", "python-utils-r1"] {
            let name = EclassName::new(name).unwrap();
            assert_eq!(name.as_str(), name.to_string());
        }
    }

    #[test]
    fn test_eclass_name_invalid() {
        for name in [
            "-invalid-eclass",
            ".hidden-eclass",
            "invalid eclass",
            "default",
            "",
        ] {
            assert!(EclassName::new(name).is_err());
        }
    }
}
