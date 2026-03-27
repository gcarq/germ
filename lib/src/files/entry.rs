use crate::deps::atom::Atom;
use crate::deps::useflag::UseFlag;
use anyhow::{Result, anyhow};
use std::hash::{Hash, Hasher};
use std::str::FromStr;

/// This trait abstracts a single item in a line-based file, such as `package.mask` or `use.mask`.
pub trait FileEntry: FromStr<Err = anyhow::Error> + Eq + PartialEq + Hash + Clone {}

impl FileEntry for SysAtom {}
impl FileEntry for Atom {}
impl FileEntry for UseFlag {}

/// This wraps a value (usually a line) in inheritable files.
///
/// Values prefixed with a hyphen are considered [`Self::Unset`] and clear all previous entries
/// with the same inner value.
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum Prefixed<T: FileEntry> {
    Set(T),
    Unset(T),
}

impl<T: FileEntry> Prefixed<T> {
    /// Returns `true` if the entry should be set.
    pub const fn is_set(&self) -> bool {
        matches!(self, Prefixed::Set(_))
    }

    /// Consumes `self` and returns the inner value if the entry is set or `None` otherwise.
    pub fn into_value(self) -> Option<T> {
        match self {
            Prefixed::Set(value) => Some(value),
            Prefixed::Unset(_) => None,
        }
    }

    pub const fn inner(&self) -> &T {
        match self {
            Prefixed::Set(value) | Prefixed::Unset(value) => value,
        }
    }
}

impl<T: FileEntry> Hash for Prefixed<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner().hash(state);
    }
}

impl<T: FileEntry> FromStr for Prefixed<T> {
    type Err = anyhow::Error;

    fn from_str(prefixed: &str) -> Result<Self> {
        let entry = match prefixed.strip_prefix('-') {
            Some(value) => Self::Unset(value.parse()?),
            None => Self::Set(prefixed.parse()?),
        };
        Ok(entry)
    }
}

/// Holds a system package, which is expressed as [`Atom`] that makes up the base system profile.
/// Only atoms prefixed with `*` are considered system packages.
#[derive(Eq, PartialEq, Hash, Clone)]
#[cfg_attr(test, derive(Debug))]
pub struct SysAtom(Atom);

impl FromStr for SysAtom {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.strip_prefix('*') {
            Some(atom) => Ok(Self(atom.parse()?)),
            None => Err(anyhow!("invalid system package syntax: {value}")),
        }
    }
}
