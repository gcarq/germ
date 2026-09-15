use crate::package::slot::{PackageSlot, SlotName};
use crate::package::version::{PackageVersion, matches_wildcard};
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Defines how a versioned [`Atom`] matches a [`PackageVersion`].
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum VersionConstraint {
    Equal(PackageVersion),
    EqualWildcard(PackageVersion),
    Approximate(PackageVersion),
    Greater(PackageVersion),
    GreaterEqual(PackageVersion),
    Less(PackageVersion),
    LessEqual(PackageVersion),
}

impl VersionConstraint {
    /// Returns `true` if the given `candidate` version matches.
    pub fn matches(&self, candidate: &PackageVersion) -> bool {
        match self {
            Self::Equal(version) => candidate == version,
            Self::EqualWildcard(version) => matches_wildcard(version, candidate),
            Self::Approximate(version) => candidate.matches_approximate(version),
            Self::Greater(version) => candidate > version,
            Self::GreaterEqual(version) => candidate >= version,
            Self::Less(version) => candidate < version,
            Self::LessEqual(version) => candidate <= version,
        }
    }

    /// Returns the operator as `&str`.
    pub const fn operator(&self) -> &str {
        match self {
            Self::Equal(_) | Self::EqualWildcard(_) => "=",
            Self::Approximate(_) => "~",
            Self::Greater(_) => ">",
            Self::GreaterEqual(_) => ">=",
            Self::Less(_) => "<",
            Self::LessEqual(_) => "<=",
        }
    }

    /// Returns the inner [`PackageVersion`].
    pub const fn version(&self) -> &PackageVersion {
        match self {
            Self::Equal(version)
            | Self::EqualWildcard(version)
            | Self::Approximate(version)
            | Self::Greater(version)
            | Self::GreaterEqual(version)
            | Self::Less(version)
            | Self::LessEqual(version) => version,
        }
    }

    pub const fn is_wildcard(&self) -> bool {
        matches!(self, Self::EqualWildcard(_))
    }
}

/// Defines how an atom slot dependency matches a [`PackageSlot`].
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum SlotConstraint {
    /// `:*` - any package slot is acceptable.
    Any,
    /// `:=` - any package slot is acceptable with rebuild-sensitive semantics.
    AnyRebuild,
    /// `:SLOT` - the package slot must match.
    Slot(SlotName),
    /// `:SLOT=` - the package slot must match with rebuild-sensitive semantics.
    SlotRebuild(SlotName),
    /// `:SLOT/SUBSLOT` - the package slot and effective sub-slot must match.
    SlotSubSlot(SlotName, SlotName),
    /// `:SLOT/SUBSLOT=` - the package slot and effective sub-slot must match with rebuild-sensitive semantics.
    SlotSubSlotRebuild(SlotName, SlotName),
}

impl SlotConstraint {
    /// Returns `true` if the given package slot satisfies this constraint.
    pub fn matches(&self, candidate: &PackageSlot) -> bool {
        match self {
            Self::Any | Self::AnyRebuild => true,
            Self::Slot(slot) | Self::SlotRebuild(slot) => candidate.slot() == slot,
            Self::SlotSubSlot(slot, sub_slot) | Self::SlotSubSlotRebuild(slot, sub_slot) => {
                candidate.slot() == slot && candidate.sub_slot() == sub_slot
            }
        }
    }

    fn new(value: &str) -> anyhow::Result<Self> {
        match value {
            "*" => return Ok(Self::Any),
            "=" => return Ok(Self::AnyRebuild),
            _ => (),
        }

        let (slot, rebuild) = match value.strip_suffix('=') {
            Some(slot) => (slot, true),
            None => (value, false),
        };

        match slot.split_once('/') {
            Some((slot, sub_slot)) => match rebuild {
                true => Ok(Self::SlotSubSlotRebuild(slot.parse()?, sub_slot.parse()?)),
                false => Ok(Self::SlotSubSlot(slot.parse()?, sub_slot.parse()?)),
            },
            None => match rebuild {
                true => Ok(Self::SlotRebuild(slot.parse()?)),
                false => Ok(Self::Slot(slot.parse()?)),
            },
        }
    }
}

impl FromStr for SlotConstraint {
    type Err = anyhow::Error;

    fn from_str(slot_str: &str) -> anyhow::Result<Self> {
        Self::new(slot_str)
    }
}

impl fmt::Display for SlotConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_str("*"),
            Self::AnyRebuild => f.write_str("="),
            Self::Slot(slot) => f.write_str(slot.as_str()),
            Self::SlotRebuild(slot) => write!(f, "{slot}="),
            Self::SlotSubSlot(slot, sub_slot) => write!(f, "{slot}/{sub_slot}"),
            Self::SlotSubSlotRebuild(slot, sub_slot) => write!(f, "{slot}/{sub_slot}="),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_constraint_parse() {
        for (input, expected) in [
            ("*", SlotConstraint::Any),
            ("=", SlotConstraint::AnyRebuild),
            ("2", SlotConstraint::Slot("2".parse().unwrap())),
            ("2=", SlotConstraint::SlotRebuild("2".parse().unwrap())),
            (
                "2/3",
                SlotConstraint::SlotSubSlot("2".parse().unwrap(), "3".parse().unwrap()),
            ),
            (
                "2/3=",
                SlotConstraint::SlotSubSlotRebuild("2".parse().unwrap(), "3".parse().unwrap()),
            ),
        ] {
            let constraint = SlotConstraint::new(input).unwrap();
            assert_eq!(constraint, expected);
            assert_eq!(constraint.to_string(), input);
        }
    }

    #[test]
    fn test_slot_constraint_matches_implicit_sub_slot() {
        let slot: PackageSlot = "15".parse().unwrap();

        for constraint in ["15", "15=", "15/15", "15/15=", "*", "="] {
            assert!(SlotConstraint::new(constraint).unwrap().matches(&slot));
        }
        for constraint in ["14", "14=", "15/0", "15/0="] {
            assert!(!SlotConstraint::new(constraint).unwrap().matches(&slot));
        }
    }

    #[test]
    fn test_slot_constraint_matches_explicit_sub_slot() {
        let slot: PackageSlot = "15/16".parse().unwrap();

        for constraint in ["15", "15=", "15/16", "15/16="] {
            assert!(SlotConstraint::new(constraint).unwrap().matches(&slot));
        }
        assert!(!SlotConstraint::new("15/15").unwrap().matches(&slot));
    }

    #[test]
    fn test_slot_constraint_value_equality() {
        assert_ne!(
            SlotConstraint::Any,
            SlotConstraint::Slot("15".parse().unwrap())
        );
    }
}
