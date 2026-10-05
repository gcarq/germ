use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};

use crate::grammar::ARCH;

/// Regex for architecture name validation.
static ARCH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\A{ARCH}\z")).unwrap());

/// Represents a single architecture e.g.: `amd64`.
#[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Arch(Box<str>);

impl Arch {
    /// Creates a new [`Arch`].
    ///
    /// Returns `Err` if the given `arch` is invalid.
    pub fn new(arch: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let arch = arch.into();
        if !ARCH_RE.is_match(arch.as_ref())? {
            bail!("invalid arch: '{arch}'");
        }
        Ok(Self(arch))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Arch {
    type Err = anyhow::Error;

    fn from_str(arch: &str) -> Result<Self, Self::Err> {
        Self::new(arch)
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_valid() {
        for arch in ["amd64", "arm64", "x86", "riscv"] {
            let parsed = Arch::new(arch).unwrap();
            assert_eq!(parsed.as_str(), arch);
        }
    }

    #[test]
    fn test_arch_invalid() {
        for arch in ["", "-foo", "+foo", ".foo", "foo.bar"] {
            assert!(Arch::new(arch).is_err(), "{arch:?} should be invalid");
        }
    }
}
