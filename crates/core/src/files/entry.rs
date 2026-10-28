use std::fmt;
use std::hash::Hash;
use std::ops::Deref;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

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

/// The file and line an entry was read from.
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct LineOrigin {
    path: Arc<Path>,
    lineno: u32,
}

impl LineOrigin {
    pub const fn new(path: Arc<Path>, lineno: u32) -> Self {
        Self { path, lineno }
    }
}

impl fmt::Display for LineOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.path.display(), self.lineno)
    }
}

/// This wraps a [`EntryValue`], usually a [`Atom`] or USE flags in line based files.
///
/// Values prefixed with a hyphen are considered [`Operation::Unset`] and clear all previous entries
/// with the same inner value.
/// The [`Precedence`] holds the original inheritance chain necessary for lookups,
/// and the [`LineOrigin`] records where the entry was read from.
#[derive(Eq, Ord, PartialOrd, PartialEq, Hash, Clone, Debug)]
pub struct Entry<T: EntryValue> {
    pub prec: Precedence,
    pub op: Operation,
    pub origin: LineOrigin,
    inner: T,
}

impl<T: EntryValue> Entry<T> {
    pub fn new(value: &str, prec: Precedence, origin: LineOrigin) -> anyhow::Result<Self> {
        let (op, inner) = match value.strip_prefix('-') {
            Some(value) => (Operation::Unset, value.parse()?),
            None => (Operation::Set, value.parse()?),
        };
        Ok(Self {
            prec,
            op,
            origin,
            inner,
        })
    }

    /// Replaces the inner value while preserving its operation, precedence and origin.
    pub fn replace_inner<U: EntryValue>(self, inner: U) -> Entry<U> {
        Entry {
            prec: self.prec,
            op: self.op,
            origin: self.origin,
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
    use crate::files::test_support::{entry, origin};

    #[test]
    fn test_new() -> anyhow::Result<()> {
        assert_eq!(
            entry("foo", Precedence::User, 1),
            Entry {
                prec: Precedence::User,
                op: Operation::Set,
                origin: origin(1),
                inner: UseFlag::from_str("foo")?
            }
        );
        assert_eq!(
            entry("-bar", Precedence::Repository, 2),
            Entry {
                prec: Precedence::Repository,
                op: Operation::Unset,
                origin: origin(2),
                inner: UseFlag::from_str("bar")?
            }
        );
        Ok(())
    }
}
