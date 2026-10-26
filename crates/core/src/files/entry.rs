use std::hash::Hash;
use std::ops::Deref;
use std::str::FromStr;

use germ_pms::{Atom, UseFlag};

/// This trait abstracts a single value in a line-based file, such as `use.mask`.
pub trait EntryValue:
    FromStr<Err = anyhow::Error> + Eq + PartialEq + Ord + PartialOrd + Hash + Clone
{
}

impl EntryValue for Atom {}
impl EntryValue for UseFlag {}

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

/// Defines the precedence in inheritance chains for resolving
/// keywords and masks in the correct order.
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Copy, Clone, Debug)]
pub enum Precedence {
    Profile(usize),
    Repository,
    User,
}

/// This wraps a [`EntryValue`], usually a [`Atom`] or USE flags in line based files.
///
/// Values prefixed with a hyphen are considered [`Operation::Unset`] and clear all previous entries
/// with the same inner value.
/// The [`Precedence`] holds the original inheritance chain necessary for lookups.
#[derive(Eq, Ord, PartialOrd, PartialEq, Hash, Clone, Debug)]
pub struct Entry<T: EntryValue> {
    pub prec: Precedence,
    pub op: Operation,
    inner: T,
}

impl<T: EntryValue> Entry<T> {
    pub fn from_str(value: &str, prec: Precedence) -> anyhow::Result<Self> {
        let (op, inner) = match value.strip_prefix('-') {
            Some(value) => (Operation::Unset, value.parse()?),
            None => (Operation::Set, value.parse()?),
        };
        Ok(Self { prec, op, inner })
    }

    /// Replaces the inner value while preserving its operation and precedence.
    pub fn replace_inner<U: EntryValue>(self, inner: U) -> Entry<U> {
        Entry {
            prec: self.prec,
            op: self.op,
            inner,
        }
    }

    pub const fn inner(&self) -> &T {
        &self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: EntryValue> Deref for Entry<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordered_from_str_ok() -> anyhow::Result<()> {
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
}
