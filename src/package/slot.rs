use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents all possible slot definitions a [`Package`] can have.
///
/// See `man 5 ebuild` for more details.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
pub enum PackageSlot {
    Any,
    Equals,
    Simple(String),
    EqualsSimple(String),
    WithSubSlot(String, String),
    EqualsWithSubSlot(String, String),
}

impl FromStr for PackageSlot {
    type Err = anyhow::Error;

    /// Creates a new [`Slot`] from the given `slot` string.
    fn from_str(slot: &str) -> Result<Self> {
        match slot {
            "*" => return Ok(Self::Any),
            "=" => return Ok(Self::Equals),
            _ => (),
        };

        let slot = match slot.split_once('/') {
            Some((slot, sub_slot)) => {
                if slot.is_empty() || sub_slot.is_empty() {
                    Err(anyhow!("invalid slot '{slot}'"))?;
                }
                match sub_slot.strip_suffix('=') {
                    Some(sub_slot) => Self::EqualsWithSubSlot(slot.into(), sub_slot.into()),
                    None => Self::WithSubSlot(slot.into(), sub_slot.into()),
                }
            }
            None if slot.ends_with('=') => Self::EqualsSimple(slot[..slot.len() - 1].into()),
            None if !slot.is_empty() && !slot.contains('*') => Self::Simple(slot.into()),
            None => Err(anyhow!("invalid slot '{slot}'"))?,
        };
        Ok(slot)
    }
}

impl fmt::Display for PackageSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "*"),
            Self::Equals => write!(f, "="),
            Self::Simple(slot) => write!(f, "{slot}"),
            Self::EqualsSimple(slot) => write!(f, "{slot}="),
            Self::WithSubSlot(slot, sub_slot) => write!(f, "{slot}/{sub_slot}"),
            Self::EqualsWithSubSlot(slot, sub_slot) => write!(f, "{slot}/{sub_slot}="),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_from_str_ok() {
        assert_eq!(PackageSlot::from_str("*").unwrap(), PackageSlot::Any);
        assert_eq!(PackageSlot::from_str("=").unwrap(), PackageSlot::Equals);
        assert_eq!(
            PackageSlot::from_str("3").unwrap(),
            PackageSlot::Simple("3".to_owned())
        );
        assert_eq!(
            PackageSlot::from_str("2=").unwrap(),
            PackageSlot::EqualsSimple("2".to_owned())
        );
        assert_eq!(
            PackageSlot::from_str("2/2.30").unwrap(),
            PackageSlot::WithSubSlot("2".to_owned(), "2.30".to_owned())
        );
        assert_eq!(
            PackageSlot::from_str("6/6.23=").unwrap(),
            PackageSlot::EqualsWithSubSlot("6".to_owned(), "6.23".to_owned())
        );
    }

    #[test]
    fn test_atom_from_str_err() {
        assert!(PackageSlot::from_str("").is_err());
        assert!(PackageSlot::from_str("3*").is_err());
        assert!(PackageSlot::from_str("3/").is_err());
        assert!(PackageSlot::from_str("/3").is_err());
    }
}
