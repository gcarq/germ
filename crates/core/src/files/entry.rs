use crate::deps::atom::Atom;
use crate::deps::useflag::UseFlag;
use anyhow::{Result, bail};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::str::FromStr;

/// This trait abstracts a single item in a line-based file, such as `package.mask` or `use.mask`.
pub trait FileEntry:
    FromStr<Err = anyhow::Error> + Eq + PartialEq + Ord + PartialOrd + Hash + Clone
{
}

impl FileEntry for SysAtom {}
impl FileEntry for Atom {}
impl FileEntry for UseFlag {}

#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Copy, Clone, Debug)]
pub enum Operation {
    Set,
    Unset,
}

impl Operation {
    pub const fn as_bool(&self) -> bool {
        match self {
            Self::Set => true,
            Self::Unset => false,
        }
    }
}

/// Defines the precedence in inheritance chains for resolving package masks.
#[derive(Eq, Copy, Clone, Debug)]
pub enum Precedence {
    Profile(usize),
    Repository,
    User,
}

impl Precedence {
    pub const fn ordinal(&self) -> usize {
        match self {
            Self::Profile(ord) => *ord,
            Self::Repository => 65536,
            Self::User => usize::MAX,
        }
    }
}

impl PartialEq for Precedence {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for Precedence {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ordinal().cmp(&other.ordinal())
    }
}

impl PartialOrd for Precedence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Precedence {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ordinal().hash(state);
    }
}

/// This wraps a [`FileEntry`], usually a [`Atom`] or USE flags in line based files.
///
/// Values prefixed with a hyphen are considered [`Operation::Unset`] and clear all previous entries
/// with the same inner value.
/// The [`Precedence`] holds the original inheritance chain necessary for lookups.
#[derive(Eq, Ord, PartialOrd, PartialEq, Hash, Clone, Debug)]
pub struct Entry<T: FileEntry> {
    pub prec: Precedence,
    pub op: Operation,
    inner: T,
}

impl<T: FileEntry> Entry<T> {
    pub fn from_str(value: &str, prec: Precedence) -> Result<Self> {
        let (op, inner) = match value.strip_prefix('-') {
            Some(value) => (Operation::Unset, value.parse()?),
            None => (Operation::Set, value.parse()?),
        };
        Ok(Self { prec, op, inner })
    }

    pub const fn inner(&self) -> &T {
        &self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: FileEntry> Deref for Entry<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Holds a system package, which is expressed as [`Atom`] that makes up the base system profile.
/// Only atoms prefixed with `*` are considered system packages.
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone)]
#[cfg_attr(test, derive(Debug))]
pub struct SysAtom(Atom);

impl FromStr for SysAtom {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.strip_prefix('*') {
            Some(atom) => Ok(Self(atom.parse()?)),
            None => bail!("invalid system package syntax: {value}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordered_from_str_ok() -> Result<()> {
        assert_eq!(
            Entry::from_str("foo", Precedence::User)?,
            Entry {
                prec: Precedence::User,
                op: Operation::Set,
                inner: UseFlag::from_str("foo")?
            }
        );
        assert_eq!(
            Entry::from_str("-bar", Precedence::Repository)?,
            Entry {
                prec: Precedence::Repository,
                op: Operation::Unset,
                inner: UseFlag::from_str("bar")?
            }
        );
        Ok(())
    }

    #[test]
    fn test_sysatom_from_str_ok() {
        let atom = SysAtom::from_str("*sys-libs/glibc");
        assert!(atom.is_ok());
    }
}
