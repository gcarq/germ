use crate::regex::SLOT;
use anyhow::{Result, anyhow};
use regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::fmt::Write;
use std::str::FromStr;
use std::sync::LazyLock;

pub static SLOT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^{SLOT}$")).unwrap());

/// Represents all possible slot definitions a [`Package`] can have.
///
/// See <https://devmanual.gentoo.org/general-concepts/dependencies/index.html#slot-dependencies>
/// or `man 5 ebuild` for more details.
/// TODO: To implement the equals slot operators = and slot=, the package manager will need to
///   store the slot/sub-slot pair of the best installed version of the matching package.
///   This syntax is only for package manager use and must not be used by ebuilds.
#[derive(Archive, Serialize, Deserialize, Eq, Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub enum PackageSlot {
    // `:=` - Any slot is acceptable
    #[default]
    Any,
    // `:*` - Any slot is acceptable, but the package should be rebuilt if the slot changes
    AnyRebuild,
    // `:SLOT=` The slot must match, but the package should be rebuilt if the sub-slot changes
    EqRebuild(Box<str>),
    // `:SLOT` - The slot must match
    Eq(Box<str>),
    // The slot and sub-slot must match
    EqSubSlot(Box<str>, Box<str>),
}

impl FromStr for PackageSlot {
    type Err = anyhow::Error;

    /// Creates a new [`Slot`] from the given `slot` string.
    fn from_str(slot_str: &str) -> Result<Self> {
        match slot_str {
            "*" => return Ok(Self::Any),
            "=" => return Ok(Self::AnyRebuild),
            _ => (),
        };

        let slot = match slot_str.split_once('/') {
            Some((slot, sub_slot)) if SLOT_RE.is_match(slot) && SLOT_RE.is_match(sub_slot) => {
                Self::EqSubSlot(slot.into(), sub_slot.into())
            }
            None => match slot_str.strip_suffix('=') {
                Some(slot) if SLOT_RE.is_match(slot) => Self::EqRebuild(slot.into()),
                None if SLOT_RE.is_match(slot_str) => Self::Eq(slot_str.into()),
                _ => Err(anyhow!("invalid slot '{slot_str}'"))?,
            },
            _ => Err(anyhow!("invalid slot '{slot_str}'"))?,
        };
        Ok(slot)
    }
}

impl PartialEq<Self> for PackageSlot {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Any | Self::AnyRebuild, _) | (_, Self::Any | Self::AnyRebuild) => true,
            (Self::Eq(s1) | Self::EqRebuild(s1), Self::Eq(s2) | Self::EqRebuild(s2)) => s1 == s2,
            (Self::EqSubSlot(s1, ss1), Self::EqSubSlot(s2, ss2)) => s1 == s2 && ss1 == ss2,
            _ => false,
        }
    }
}

impl fmt::Display for PackageSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_char('*'),
            Self::AnyRebuild => f.write_char('='),
            Self::Eq(slot) => f.write_str(slot),
            Self::EqSubSlot(slot, sub_slot) => write!(f, "{slot}/{sub_slot}"),
            Self::EqRebuild(slot) => write!(f, "{slot}="),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_from_str_ok() {
        assert_eq!(PackageSlot::from_str("*").unwrap(), PackageSlot::Any);
        assert_eq!(PackageSlot::from_str("=").unwrap(), PackageSlot::AnyRebuild);
        assert_eq!(
            PackageSlot::from_str("3").unwrap(),
            PackageSlot::Eq("3".into())
        );
        assert_eq!(
            PackageSlot::from_str("2=").unwrap(),
            PackageSlot::EqRebuild("2".into())
        );
        assert_eq!(
            PackageSlot::from_str("2/2.30").unwrap(),
            PackageSlot::EqSubSlot("2".into(), "2.30".into())
        );
    }

    #[test]
    fn test_atom_from_str_err() {
        assert!(PackageSlot::from_str("").is_err());
        assert!(PackageSlot::from_str("3/").is_err());
        assert!(PackageSlot::from_str("/3").is_err());
    }
}
