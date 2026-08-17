use super::UseFlag;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents the optional default annotation on an IUSE entry.
#[derive(
    Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug,
)]
pub enum IUseDefault {
    Enabled,
    Disabled,
}

/// Represents a package IUSE entry, see PMS 7.3 for more information.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct IUseEntry {
    flag: UseFlag,
    default: Option<IUseDefault>,
}

impl IUseEntry {
    /// Returns the bare USE flag in this IUSE entry.
    pub const fn flag(&self) -> &UseFlag {
        &self.flag
    }
}

impl FromStr for IUseEntry {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> anyhow::Result<Self> {
        let (default, flag) = match input.chars().next() {
            Some('+') => (Some(IUseDefault::Enabled), &input[1..]),
            Some('-') => (Some(IUseDefault::Disabled), &input[1..]),
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
                IUseDefault::Enabled => "+",
                IUseDefault::Disabled => "-",
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
            ("+foo", Some(IUseDefault::Enabled)),
            ("-foo", Some(IUseDefault::Disabled)),
        ];

        for (input, default) in test_cases {
            let entry = input.parse::<IUseEntry>().unwrap();
            assert_eq!(entry.flag, "foo".parse().unwrap());
            assert_eq!(entry.default, default);
        }
    }

    #[test]
    fn test_iuse_entry_invalid() {
        for input in [
            "", "++foo", "--foo", "!foo", "foo?", "foo=", "foo(+)", "foo bar",
        ] {
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
