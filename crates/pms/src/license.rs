use std::borrow::Borrow;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};

use crate::deps::ExprItem;
use crate::grammar::LICENSE;

static LICENSE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A{LICENSE}\z")).unwrap());

/// Holds a validated license name.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct LicenseName(Box<str>);

impl LicenseName {
    /// Creates a new [`LicenseName`] from the given `name`.
    pub fn new(name: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let name = name.into();
        if !LICENSE_RE.is_match(name.as_ref())? {
            bail!("invalid license name: '{name}'");
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for LicenseName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for LicenseName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for LicenseName {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl ExprItem for LicenseName {}

impl fmt::Display for LicenseName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_license_name_ok() {
        let names = [
            "GPL-2",
            "GPL-2+",
            "Apache-2.0",
            "BSD",
            "public-domain",
            "0bsd",
        ];
        for name in names {
            let parsed = LicenseName::new(name).unwrap();
            assert_eq!(parsed.as_str(), name);
        }
    }

    #[test]
    fn test_license_name_err() {
        for name in ["", "-invalid", ".invalid", "+invalid", "@group", "foo bar"] {
            let result = LicenseName::new(name);
            assert!(result.is_err(), "{name:?} should be invalid");
        }
    }
}
