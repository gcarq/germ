use super::UseFlag;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents the optional default state on an IUSE entry.
#[derive(
    Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug,
)]
pub enum IUseState {
    Enabled,
    Disabled,
}

impl IUseState {
    /// Returns the default USE state.
    pub const fn as_bool(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// Represents a package IUSE entry, see PMS 7.3 for more information.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct IUseEntry {
    flag: UseFlag,
    default: Option<IUseState>,
}

impl IUseEntry {
    /// Returns the bare USE flag in this IUSE entry.
    pub const fn flag(&self) -> &UseFlag {
        &self.flag
    }

    /// Returns the optional default USE state.
    pub const fn state(&self) -> Option<IUseState> {
        self.default
    }
}

impl FromStr for IUseEntry {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> anyhow::Result<Self> {
        let (default, flag) = match input.chars().next() {
            Some('+') => (Some(IUseState::Enabled), &input[1..]),
            Some('-') => (Some(IUseState::Disabled), &input[1..]),
            _ => (None, input),
        };

        Ok(Self {
            flag: flag.parse()?,
            default,
        })
    }
}

impl fmt::Display for IUseEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(default) = self.default {
            f.write_str(match default {
                IUseState::Enabled => "+",
                IUseState::Disabled => "-",
            })?;
        }
        self.flag.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iuse_entry_parse() {
        let test_cases = [
            ("foo", None),
            ("+foo", Some(IUseState::Enabled)),
            ("-foo", Some(IUseState::Disabled)),
        ];

        for (input, default) in test_cases {
            let entry = input.parse::<IUseEntry>().unwrap();
            assert_eq!(entry.flag, "foo".parse().unwrap());
            assert_eq!(entry.default, default);
        }
    }

    #[test]
    fn test_iuse_entry_invalid() {
        for input in ["++foo", "--foo"] {
            assert!(
                input.parse::<IUseEntry>().is_err(),
                "{input:?} should be invalid"
            );
        }
    }

    #[test]
    fn test_iuse_entry_display() {
        for input in ["foo", "+foo", "-foo"] {
            let entry = input.parse::<IUseEntry>().unwrap();
            assert_eq!(entry.to_string(), input);
        }
    }
}
