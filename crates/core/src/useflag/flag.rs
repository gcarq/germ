use crate::grammar::USE_FLAG;
use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

static USE_FLAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A{USE_FLAG}\z")).unwrap());

/// Represents a bare USE flag name, see PMS 3.1.4 for more information.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct UseFlag(Box<str>);

impl UseFlag {
    /// Creates a new [`UseFlag`] from the given `value`.
    ///
    /// Returns `Err` if `value` is not a valid USE flag name.
    pub fn new(value: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let value = value.into();
        if !USE_FLAG_RE.is_match(value.as_ref())? {
            bail!("invalid use flag: '{value}'");
        }
        Ok(Self(value))
    }

    /// Returns the bare USE flag name.
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for UseFlag {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        Self::new(value)
    }
}

impl fmt::Display for UseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_flag_valid() {
        for flag in ["foo", "123", "foo+bar", "foo_bar", "foo@bar", "foo-"] {
            assert!(flag.parse::<UseFlag>().is_ok(), "{flag:?} should be valid");
        }
    }

    #[test]
    fn test_use_flag_invalid() {
        for flag in [
            "", "-foo", "+foo", "!foo", "foo?", "foo=", "foo(+)", "_foo", "@foo", "foo.bar",
            "foo/bar", "foo bar",
        ] {
            assert!(
                flag.parse::<UseFlag>().is_err(),
                "{flag:?} should be invalid"
            );
        }
    }

    #[test]
    fn test_use_flag_display() {
        let flag = "foo".parse::<UseFlag>().unwrap();
        assert_eq!(flag.to_string(), "foo");
        assert_eq!(flag.as_str(), "foo");
    }
}
