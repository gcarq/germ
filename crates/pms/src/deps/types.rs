use std::fmt;
use std::str::FromStr;

use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};

use super::item::ExprItem;

/// A non-empty string for metadata fields such as `RESTRICT` and `PROPERTIES`.
/// See PMS 8.2.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct SimpleString(Box<str>);

impl ExprItem for SimpleString {}

impl FromStr for SimpleString {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            bail!("empty string is not allowed");
        }
        Ok(Self(value.to_owned().into_boxed_str()))
    }
}

impl fmt::Display for SimpleString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_string() {
        for input in ["mirror", "test", "network-sandbox"] {
            let parsed = input.parse::<SimpleString>().unwrap();
            assert_eq!(parsed.to_string(), input);
        }

        assert!("".parse::<SimpleString>().is_err());
    }
}
