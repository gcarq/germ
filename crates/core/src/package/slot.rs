use crate::grammar::SLOT;
use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::fmt::Write;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::LazyLock;

static SLOT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\A{SLOT}\z")).unwrap());

/// Represents all possible slot definitions a [`Package`] can have.
///
/// See <https://devmanual.gentoo.org/general-concepts/dependencies/index.html#slot-dependencies>
/// or `man 5 ebuild` for more details.
/// TODO: To implement the equals slot operators = and slot=, the package manager will need to
///   store the slot/sub-slot pair of the best installed version of the matching package.
///   This syntax is only for package manager use and must not be used by ebuilds.
#[derive(Archive, Serialize, Deserialize, Eq, Clone, Default, Debug)]
pub enum PackageSlot {
    /// `:=` - Any slot is acceptable
    #[default]
    Any,
    /// `:*` - Any slot is acceptable, but the package should be rebuilt if the slot changes
    AnyRebuild,
    /// `:SLOT=` - The slot must match, but the package should be rebuilt if the sub-slot changes
    EqRebuild(Box<str>),
    /// `:SLOT/SUBSLOT=` - Same as `EqRebuild`, but this is for internal use only
    /// TODO: restrict this syntax from ebuilds
    EqSubSlotRebuild(Box<str>, Box<str>),
    /// `:SLOT` - The slot must match
    Eq(Box<str>),
    /// `:SLOT/SUBSLOT` - The slot and sub-slot must match
    EqSubSlot(Box<str>, Box<str>),
}

impl PackageSlot {
    /// Creates a new [`Slot`] from the given `slot` string.
    fn new(slot_str: &str) -> anyhow::Result<Self> {
        match slot_str {
            "*" => return Ok(Self::Any),
            "=" => return Ok(Self::AnyRebuild),
            _ => (),
        };

        let (slot, rebuild) = match slot_str.strip_suffix('=') {
            Some(slot) => (slot, true),
            None => (slot_str, false),
        };

        match slot.split_once('/') {
            Some((slot, sub_slot)) if SLOT_RE.is_match(slot)? && SLOT_RE.is_match(sub_slot)? => {
                if rebuild {
                    Ok(Self::EqSubSlotRebuild(slot.into(), sub_slot.into()))
                } else {
                    Ok(Self::EqSubSlot(slot.into(), sub_slot.into()))
                }
            }
            None if SLOT_RE.is_match(slot)? => {
                if rebuild {
                    Ok(Self::EqRebuild(slot.into()))
                } else {
                    Ok(Self::Eq(slot.into()))
                }
            }
            _ => bail!("invalid slot '{slot_str}'"),
        }
    }
}

impl FromStr for PackageSlot {
    type Err = anyhow::Error;

    fn from_str(slot_str: &str) -> anyhow::Result<Self> {
        Self::new(slot_str)
    }
}

impl PartialEq<Self> for PackageSlot {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for PackageSlot {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Any | Self::AnyRebuild, _) | (_, Self::Any | Self::AnyRebuild) => {
                Ordering::Equal
            }
            (
                Self::Eq(s1) | Self::EqRebuild(s1) | Self::EqSubSlotRebuild(s1, _),
                Self::Eq(s2) | Self::EqRebuild(s2) | Self::EqSubSlotRebuild(s2, _),
            ) => s1.cmp(s2),
            (
                Self::EqSubSlot(s1, ss1) | Self::EqSubSlotRebuild(s1, ss1),
                Self::EqSubSlot(s2, ss2) | Self::EqSubSlotRebuild(s2, ss2),
            ) => s1.cmp(s2).then(ss1.cmp(ss2)),
            _ => Ordering::Less,
        }
    }
}

impl PartialOrd for PackageSlot {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for PackageSlot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Any | Self::AnyRebuild => {
                state.write_u8(0);
            }
            Self::Eq(slot) | Self::EqRebuild(slot) => {
                state.write_u8(1);
                slot.hash(state);
            }
            Self::EqSubSlot(slot, sub_slot) | Self::EqSubSlotRebuild(slot, sub_slot) => {
                state.write_u8(2);
                slot.hash(state);
                sub_slot.hash(state);
            }
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
            Self::EqSubSlotRebuild(slot, sub_slot) => write!(f, "{slot}/{sub_slot}="),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_from_str_ok() {
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
            PackageSlot::from_str("3/4=").unwrap(),
            PackageSlot::EqSubSlotRebuild("3".into(), "4".into())
        );
        assert_eq!(
            PackageSlot::from_str("2/2.30").unwrap(),
            PackageSlot::EqSubSlot("2".into(), "2.30".into())
        );
    }

    #[test]
    fn test_slot_from_str_err() {
        assert!(PackageSlot::from_str("").is_err());
        assert!(PackageSlot::from_str("3/").is_err());
        assert!(PackageSlot::from_str("/3").is_err());
    }

    #[test]
    fn test_slot_eq() {
        let slot = PackageSlot::EqSubSlotRebuild("2".into(), "2.30".into());

        assert_eq!(slot, slot);
        assert_eq!(slot, PackageSlot::Any);
        assert_eq!(slot, PackageSlot::AnyRebuild);
        assert_eq!(slot, PackageSlot::Eq("2".into()));
        assert_eq!(slot, PackageSlot::EqRebuild("2".into()));
        assert_eq!(slot, PackageSlot::EqSubSlot("2".into(), "2.30".into()));
    }
}
