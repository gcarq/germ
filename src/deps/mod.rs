pub mod atom;
pub mod expr;
mod parser;

use anyhow::{Result, anyhow};
use expr::ExpressionItem;
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

/// A USE flag may be prefixed with `+` or `-` to indicate whether it is enabled or disabled.
#[derive(Archive, Serialize, Deserialize, Eq, Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub enum UseFlagPrefix {
    #[default]
    None,
    Enable,
    Disable,
}

impl UseFlagPrefix {
    const fn ordinal(&self) -> u8 {
        match self {
            UseFlagPrefix::Enable => 0,
            UseFlagPrefix::None => 1,
            UseFlagPrefix::Disable => 2,
        }
    }
}

impl Ord for UseFlagPrefix {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ordinal().cmp(&other.ordinal())
    }
}

impl PartialOrd for UseFlagPrefix {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for UseFlagPrefix {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
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
#[derive(Archive, Serialize, Deserialize, Eq, Clone)]
#[cfg_attr(test, derive(Default, Debug))]
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

impl Ord for PrefixedUseFlag {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.prefix.cmp(&other.prefix) {
            Ordering::Equal => self.flag.cmp(&other.flag),
            ord => ord,
        }
    }
}

impl PartialEq for PrefixedUseFlag {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialEq<PrefixedUseFlag> for UseFlag {
    fn eq(&self, other: &PrefixedUseFlag) -> bool {
        self.eq(&other.flag)
    }
}

impl PartialOrd for PrefixedUseFlag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for PrefixedUseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.prefix, self.flag)
    }
}

/// Represents a USE flag
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(test, derive(Default, Debug))]
pub struct UseFlag(Box<str>);

impl UseFlag {
    // TODO: implement name validation
    pub fn new(flag: &str) -> Result<Self> {
        if flag.is_empty() {
            return Err(anyhow!("use flag cannot be empty"));
        }
        Ok(UseFlag(flag.into()))
    }
}

impl ExpressionItem for UseFlag {}

impl FromStr for UseFlag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl fmt::Display for UseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Deref for UseFlag {
    type Target = Box<str>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_flag_eq() {
        let test_cases = ["foo", "+foo", "-foo"];
        let flag = "foo".parse::<UseFlag>().unwrap();
        for prefixed in test_cases {
            let prefixed_flag = prefixed.parse::<PrefixedUseFlag>().unwrap();
            assert_eq!(flag, prefixed_flag);
        }

        assert_ne!(
            "foo".parse::<UseFlag>().unwrap(),
            "bar".parse::<UseFlag>().unwrap()
        );

        assert_ne!(
            "foo".parse::<UseFlag>().unwrap(),
            "-bar".parse::<PrefixedUseFlag>().unwrap()
        );
    }

    #[test]
    fn test_use_flag_ord() {
        let none_foo = "foo".parse::<PrefixedUseFlag>().unwrap();
        let enable_foo = "+foo".parse::<PrefixedUseFlag>().unwrap();
        let disable_foo = "-foo".parse::<PrefixedUseFlag>().unwrap();
        assert!(disable_foo > none_foo);
        assert!(none_foo > enable_foo);
    }

    #[test]
    fn test_use_flag_display() {
        assert_eq!("foo".parse::<UseFlag>().unwrap().to_string(), "foo");
        assert_eq!(
            "-foo".parse::<PrefixedUseFlag>().unwrap().to_string(),
            "-foo"
        );
    }
}
