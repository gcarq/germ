use anyhow::bail;
use std::{borrow::Borrow, fmt, str::FromStr};

/// Holds a validated make variable name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EnvVarName(Box<str>);

impl EnvVarName {
    /// Creates a new [`EnvVarName`] from the given `name`.
    ///
    /// Returns `Err` if the given `name` is not a valid make variable name.
    pub fn new(name: impl Into<Box<str>>) -> anyhow::Result<Self> {
        let name = name.into();
        if !is_valid(name.as_ref()) {
            bail!("invalid make variable name: '{name}'");
        }
        Ok(Self(name))
    }

    /// Returns this name as a string slice.
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for EnvVarName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for EnvVarName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for EnvVarName {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::new(name)
    }
}

impl fmt::Display for EnvVarName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Checks if the given `name` is a valid variable name as per PMS 5.2.4.
fn is_valid(name: &str) -> bool {
    match name.as_bytes().split_first() {
        Some((first, rest)) if first.is_ascii_alphabetic() => {
            rest.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_name_valid() {
        for name in ["ARCH", "USE", "enable_year2038", "USE_EXPAND_VALUES_ARCH"] {
            let parsed = EnvVarName::new(name).unwrap();
            assert_eq!(parsed.as_str(), name);
        }
    }

    #[test]
    fn test_env_name_invalid() {
        for name in ["", "-VAR", "1VAR", "/VAR1", "VAR-1", "VAR.1"] {
            assert!(EnvVarName::new(name).is_err(), "{name:?} should be invalid");
        }
    }
}
