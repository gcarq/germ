use anyhow::{Result, bail};
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

/// A USE flag may be prefixed with `+` or `-` to indicate whether it is enabled or disabled.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Default, Debug)]
pub enum UseFlagPrefix {
    #[default]
    None,
    Enable,
    Disable,
}

impl fmt::Display for UseFlagPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self {
            UseFlagPrefix::None => "",
            UseFlagPrefix::Enable => "+",
            UseFlagPrefix::Disable => "-",
        };
        f.write_str(prefix)
    }
}

/// A USE flag with an optional prefix.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct PrefixedUseFlag {
    prefix: UseFlagPrefix,
    flag: UseFlag,
}

impl PrefixedUseFlag {
    /// Parses a prefixed USE flag from the given `flag`, e.g.: `+foo`, `-foo`, or `foo`.
    pub fn new(flag: &str) -> Result<Self> {
        let (prefix, flag_str) = match flag.chars().next() {
            Some('+') => (UseFlagPrefix::Enable, &flag[1..]),
            Some('-') => (UseFlagPrefix::Disable, &flag[1..]),
            _ => (UseFlagPrefix::None, flag),
        };
        Ok(Self::from_parts(prefix, flag_str.parse()?))
    }

    pub const fn from_parts(prefix: UseFlagPrefix, flag: UseFlag) -> Self {
        Self { prefix, flag }
    }

    /// Returns the inner [`UseFlag`].
    pub const fn inner(&self) -> &UseFlag {
        &self.flag
    }
}

impl FromStr for PrefixedUseFlag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl fmt::Display for PrefixedUseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.prefix, self.flag)
    }
}

/// Represents a USE flag
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct UseFlag(String);

impl UseFlag {
    // TODO: implement name validation
    pub fn new(flag: String) -> Result<Self> {
        if flag.is_empty() {
            bail!("use flag cannot be empty");
        }
        Ok(UseFlag(flag))
    }
}

impl FromStr for UseFlag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s.into())
    }
}

impl fmt::Display for UseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Deref for UseFlag {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_flag_display() {
        assert_eq!("foo".parse::<UseFlag>().unwrap().to_string(), "foo");
        assert_eq!(
            "-foo".parse::<PrefixedUseFlag>().unwrap().to_string(),
            "-foo"
        );
    }
}
