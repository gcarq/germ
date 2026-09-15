use crate::grammar::SLOT;
use anyhow::bail;
use fancy_regex::Regex;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

static SLOT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\A{SLOT}\z")).unwrap());

/// Holds a validated package slot or sub-slot name.
///
/// See PMS 3.1.3.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct SlotName(Box<str>);

impl SlotName {
    /// Creates a new [`SlotName`] from the given `name`.
    pub fn new(name: &str) -> anyhow::Result<Self> {
        match SLOT_RE.is_match(name)? {
            true => Ok(Self(name.into())),
            false => bail!("invalid slot name: '{name}'"),
        }
    }

    /// Returns the slot name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SlotName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for SlotName {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> anyhow::Result<Self> {
        Self::new(name)
    }
}

impl fmt::Display for SlotName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Holds the slot and an optional sub-slot from package metadata.
///
/// If the sub-slot is not provided, the slot is used as the effective sub-slot.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct PackageSlot {
    slot: SlotName,
    sub_slot: Option<SlotName>,
}

impl PackageSlot {
    /// Creates a new [`PackageSlot`] from the given `name`.
    pub fn new(name: &str) -> anyhow::Result<Self> {
        match name.split_once('/') {
            Some((slot, sub_slot)) => Ok(Self {
                slot: slot.parse()?,
                sub_slot: Some(sub_slot.parse()?),
            }),
            None => Ok(Self {
                slot: name.parse()?,
                sub_slot: None,
            }),
        }
    }

    /// Returns the package's slot.
    pub const fn slot(&self) -> &SlotName {
        &self.slot
    }

    /// Returns the effective sub-slot.
    pub fn sub_slot(&self) -> &SlotName {
        self.sub_slot.as_ref().unwrap_or(&self.slot)
    }
}

impl Default for PackageSlot {
    fn default() -> Self {
        Self {
            slot: SlotName("0".into()),
            sub_slot: None,
        }
    }
}

impl FromStr for PackageSlot {
    type Err = anyhow::Error;

    fn from_str(slot_str: &str) -> anyhow::Result<Self> {
        Self::new(slot_str)
    }
}

impl fmt::Display for PackageSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slot.as_str())?;
        if let Some(sub_slot) = &self.sub_slot {
            write!(f, "/{sub_slot}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_name_parse() {
        for name in ["0", "2", "foo", "2.30"] {
            assert_eq!(SlotName::new(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn test_slot_name_rejects_invalid() {
        for name in ["", "*", "=", "2/3", "2="] {
            assert!(SlotName::new(name).is_err());
        }
    }

    #[test]
    fn test_slot_parse() {
        for slot in ["0", "2", "2/3", "foo/bar"] {
            assert!(PackageSlot::new(slot).is_ok());
        }
    }

    #[test]
    fn test_sub_slot() {
        let slot = PackageSlot::new("2").unwrap();

        assert_eq!(slot.slot().as_str(), "2");
        assert_eq!(slot.sub_slot().as_str(), "2");
        assert_eq!(slot.to_string(), "2");

        let slot = PackageSlot::new("2/2").unwrap();
        assert_eq!(slot.sub_slot().as_str(), "2");
        assert_eq!(slot.to_string(), "2/2");
    }

    #[test]
    fn test_slot_display_sub_slot() {
        assert_eq!(PackageSlot::new("2/2.30").unwrap().to_string(), "2/2.30");
    }

    #[test]
    fn test_slot_rejects_constraint() {
        for slot in ["*", "=", "2=", "2/3="] {
            assert!(PackageSlot::new(slot).is_err());
        }
    }
}
