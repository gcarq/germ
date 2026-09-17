pub mod expression;
mod parser;

use crate::atom::{Atom, AtomBlocker};
use crate::deps::expression::ExpressionTree;
use crate::deps::parser::ExpressionParser;
use crate::deps::parser::arena::{ArenaEntry, ExpressionArena};
use crate::eapi::Eapi;
use crate::useflag::UseFlag;

use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Selects the expression context for validation.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ExpressionKind {
    Dependency,
    RequiredUse,
    Homepage,
    SrcUri,
    License,
    Properties,
    Restrict,
}

impl ExpressionKind {
    /// Returns `true` if the given `expression` is valid for this kind.
    const fn supports_expression<T: ExpressionItem>(self, expression: &ArenaEntry<T>) -> bool {
        match expression {
            ArenaEntry::Item(_) | ArenaEntry::AllOf(_) => true,
            ArenaEntry::Use { .. } => true,
            ArenaEntry::AnyOf(_) => {
                matches!(self, Self::Dependency | Self::License | Self::RequiredUse)
            }
            ArenaEntry::ExactlyOneOf(_) | ArenaEntry::AtMostOneOf(_) => {
                matches!(self, Self::RequiredUse)
            }
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Dependency => "dependency",
            Self::RequiredUse => "REQUIRED_USE",
            Self::Homepage => "HOMEPAGE",
            Self::SrcUri => "SRC_URI",
            Self::License => "LICENSE",
            Self::Properties => "PROPERTIES",
            Self::Restrict => "RESTRICT",
        }
    }
}

impl fmt::Display for ExpressionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// This trait defines an item that can be used in a dependency expression,
/// such as [`RequiredUseFlag`] and [`AtomDep`].
pub trait ExpressionItem: FromStr<Err = anyhow::Error> + fmt::Display {
    fn parse(input: &str) -> anyhow::Result<Self> {
        Self::from_str(input)
    }
}

/// Extends an [`Atom`] with an optional blocker, e.g.:
/// `!<app-misc/foo-1.3` or `!!app-misc/foo`.
///
/// [`AtomBlocker`] only makes sense when used a dependency, and is not part of the atom itself.
#[derive(
    Archive, Serialize, Deserialize, Default, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, Debug,
)]
pub struct AtomDep {
    atom: Atom,
    blocker: Option<AtomBlocker>,
}

impl AtomDep {
    /// Returns the inner [`Atom`].
    pub const fn inner(&self) -> &Atom {
        &self.atom
    }

    /// Returns the blocker, if any.
    pub const fn blocker(&self) -> Option<AtomBlocker> {
        self.blocker
    }
}

impl ExpressionItem for AtomDep {}

impl FromStr for AtomDep {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        let (blocker, atom) = match value.strip_prefix('!') {
            Some(atom) => match atom.strip_prefix("!") {
                Some(atom) => (Some(AtomBlocker::Strong), atom),
                None => (Some(AtomBlocker::Weak), atom),
            },
            None => (None, value),
        };
        let atom = Atom::new(atom)?;
        Ok(Self { blocker, atom })
    }
}

impl fmt::Display for AtomDep {
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

impl ExpressionItem for RequiredUseFlag {}

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

/// Holds a dependency expression parsed into an expression arena.
/// See PMS 8.2 for the dependency specification format.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct DepExpression<T: ExpressionItem> {
    arena: ExpressionArena<T>,
}

impl<T: ExpressionItem> DepExpression<T> {
    /// Parses the given `input` using the EAPI and metadata expression context.
    ///
    /// # Errors
    ///
    /// Returns an error when the EAPI is not supported, when the input is syntactically invalid,
    /// or when the parsed expression violates the context restrictions.
    pub fn parse(eapi: Eapi, kind: ExpressionKind, input: &str) -> anyhow::Result<Self> {
        if !eapi.is_supported_for_ebuilds() {
            bail!("EAPI {eapi} is not supported for dependency expressions");
        }

        if input.trim().is_empty() {
            return Ok(Self::default());
        }

        let arena = ExpressionParser::parse(input)?;
        arena.validate(kind)?;
        Ok(Self { arena })
    }

    /// Returns a view of the expression.
    pub const fn view(&self) -> ExpressionTree<'_, T> {
        self.arena.view()
    }
}

impl<T: ExpressionItem> fmt::Display for DepExpression<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.view().fmt(f)
    }
}

impl<T: ExpressionItem> Default for DepExpression<T> {
    fn default() -> Self {
        Self {
            arena: ExpressionArena::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_dep_parse() {
        for (input, atom) in [
            ("dev-lang/rust", "dev-lang/rust"),
            ("!dev-lang/rust", "dev-lang/rust"),
            ("!!dev-lang/rust", "dev-lang/rust"),
            ("!<dev-perl/Mail-Box-3", "<dev-perl/Mail-Box-3"),
            ("!!=sys-libs/db-5*:5", "=sys-libs/db-5*:5"),
            ("!cat/pkg[foo]", "cat/pkg[foo]"),
            ("!cat/pkg::repo-", "cat/pkg::repo-"),
            ("!cat/pkg:slot", "cat/pkg:slot"),
        ] {
            let parsed = input.parse::<AtomDep>().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.inner().to_string(), atom);
        }
    }

    #[test]
    fn test_atom_dep_parse_error() {
        for input in ["!", "!!", "!!!cat/pkg", "!(", "!||", "!^^", "! cat/pkg"] {
            assert!(
                input.parse::<AtomDep>().is_err(),
                "expected atom dependency error for '{input}'"
            );
        }
    }

    #[test]
    fn test_required_use_flag() {
        for (input, flag, negated) in [("foo", "foo", false), ("!foo", "foo", true)] {
            let parsed = RequiredUseFlag::from_str(input).unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.inner().to_string(), flag);
            assert_eq!(parsed.is_negated(), negated);
        }

        for input in ["", "!", "!!foo", "foo!"] {
            assert!(
                RequiredUseFlag::from_str(input).is_err(),
                "expected error for '{input}'"
            );
        }
    }

    #[test]
    fn test_parse_empty() {
        let expression =
            DepExpression::<AtomDep>::parse(Eapi::Seven, ExpressionKind::Dependency, " \t")
                .unwrap();
        assert_eq!(expression.to_string(), "");
    }

    #[test]
    fn test_parse_kinds() {
        let eapi = Eapi::Eight;
        for (input, valid) in [
            ("!cat/pkg", true),
            ("!!cat/pkg", true),
            ("^^ ( cat/pkg cat/other )", false),
        ] {
            let result = DepExpression::<AtomDep>::parse(eapi, ExpressionKind::Dependency, input);
            assert_eq!(result.is_ok(), valid, "dependency: {input}");
        }

        for (kind, input, valid) in [
            (ExpressionKind::RequiredUse, "^^ ( foo bar )", true),
            (ExpressionKind::RequiredUse, "?? ( foo bar )", true),
            (ExpressionKind::RequiredUse, "!foo", true),
            (ExpressionKind::RequiredUse, "!!foo", false),
            (ExpressionKind::License, "|| ( GPL-2 MIT )", true),
            (ExpressionKind::Restrict, "( fetch mirror )", true),
            (ExpressionKind::Restrict, "|| ( fetch mirror )", false),
        ] {
            let result = DepExpression::<RequiredUseFlag>::parse(eapi, kind, input);
            assert_eq!(result.is_ok(), valid, "{kind}: {input}");
        }
    }
}
