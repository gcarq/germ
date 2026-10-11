use std::fmt;
use std::str::FromStr;

use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};

use crate::atom::{Atom, BlockerStrength};
use crate::useflag::UseFlag;

/// This trait defines an item that can be used in a dependency expression,
/// such as [`RequiredUseFlag`] and [`DepAtom`].
pub trait ExprItem: FromStr<Err = anyhow::Error> + fmt::Display {
    /// Validates if the item is valid in a dependency expression.
    ///
    /// Returns `Err` if the item is invalid.
    fn validate(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Extends an [`Atom`] with an optional blocker, e.g.:
/// `!<app-misc/foo-1.3` or `!!app-misc/foo`.
///
/// [`BlockerStrength`] only makes sense when used a dependency, and is not part of the atom itself.
#[derive(Archive, Serialize, Deserialize, Default, Clone, PartialEq, Eq, Hash, Debug)]
pub struct DepAtom {
    atom: Atom,
    blocker: Option<BlockerStrength>,
}

impl DepAtom {
    /// Returns the inner [`Atom`].
    pub const fn inner(&self) -> &Atom {
        &self.atom
    }

    /// Returns the blocker, if any.
    pub const fn blocker(&self) -> Option<BlockerStrength> {
        self.blocker
    }
}

impl ExprItem for DepAtom {
    fn validate(&self) -> anyhow::Result<()> {
        if self.atom.category().is_none()
            || self.atom.package().is_none()
            || self.atom.repo().is_some()
        {
            bail!("{self} is not valid in dependency expression");
        }
        Ok(())
    }
}

impl FromStr for DepAtom {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        let (blocker, atom) = match value.strip_prefix('!') {
            Some(atom) => match atom.strip_prefix("!") {
                Some(atom) => (Some(BlockerStrength::Strong), atom),
                None => (Some(BlockerStrength::Weak), atom),
            },
            None => (None, value),
        };
        let atom = Atom::new(atom)?;
        Ok(Self { blocker, atom })
    }
}

impl fmt::Display for DepAtom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(blocker) = &self.blocker {
            write!(f, "{blocker}")?;
        }
        write!(f, "{}", self.atom)
    }
}

/// Represents a required USE flag in a dependency expression.
///
/// Additional to the [`UseFlag`] it has the ability to be disabled.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct RequiredUseFlag {
    flag: UseFlag,
    negated: bool,
}

impl RequiredUseFlag {
    /// Returns the inner [`UseFlag`].
    pub const fn inner(&self) -> &UseFlag {
        &self.flag
    }

    /// Returns `true` if this required USE flag is negated.
    pub const fn is_negated(&self) -> bool {
        self.negated
    }
}

impl ExprItem for RequiredUseFlag {}

impl FromStr for RequiredUseFlag {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        let (flag, negated) = match value.strip_prefix('!') {
            Some(flag) => (flag, true),
            None => (value, false),
        };
        let flag = UseFlag::new(flag)?;
        Ok(Self { flag, negated })
    }
}

impl fmt::Display for RequiredUseFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negated {
            f.write_str("!")?;
        }
        self.flag.fmt(f)
    }
}
