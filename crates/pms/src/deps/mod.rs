pub mod expr;
mod parser;

use std::fmt;
use std::str::FromStr;

pub use expr::{Expr, ExprEval, ExprNodes, ExprTree};
use rkyv::{Archive, Deserialize, Serialize};

use crate::atom::{Atom, BlockerStrength};
use crate::deps::parser::ExprParser;
use crate::deps::parser::arena::{ExprArena, ExprEntry};
use crate::useflag::UseFlag;

/// Selects the expression context for validation.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ExprKind {
    Dependency,
    RequiredUse,
    Homepage,
    SrcUri,
    License,
    Properties,
    Restrict,
}

impl ExprKind {
    /// Returns `true` if the given `expression` is valid for this kind.
    const fn supports_expr<T: ExprItem>(self, expression: &ExprEntry<T>) -> bool {
        match expression {
            ExprEntry::Item(_) | ExprEntry::AllOf(_) => true,
            ExprEntry::Use { .. } => true,
            ExprEntry::AnyOf(_) => {
                matches!(self, Self::Dependency | Self::License | Self::RequiredUse)
            }
            ExprEntry::ExactlyOneOf(_) | ExprEntry::AtMostOneOf(_) => {
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

impl fmt::Display for ExprKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// This trait defines an item that can be used in a dependency expression,
/// such as [`RequiredUseFlag`] and [`AtomDep`].
pub trait ExprItem: FromStr<Err = anyhow::Error> + fmt::Display {
    fn parse(input: &str) -> anyhow::Result<Self> {
        Self::from_str(input)
    }
}

/// Extends an [`Atom`] with an optional blocker, e.g.:
/// `!<app-misc/foo-1.3` or `!!app-misc/foo`.
///
/// [`BlockerStrength`] only makes sense when used a dependency, and is not part of the atom itself.
#[derive(Archive, Serialize, Deserialize, Default, Clone, PartialEq, Eq, Hash, Debug)]
pub struct AtomDep {
    atom: Atom,
    blocker: Option<BlockerStrength>,
}

impl AtomDep {
    /// Returns the inner [`Atom`].
    pub const fn inner(&self) -> &Atom {
        &self.atom
    }

    /// Returns the blocker, if any.
    pub const fn blocker(&self) -> Option<BlockerStrength> {
        self.blocker
    }
}

impl ExprItem for AtomDep {}

impl FromStr for AtomDep {
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

/// Holds a dependency expression parsed into an expression arena.
/// See PMS 8.2 for the dependency specification format.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct DepExpr<T: ExprItem> {
    arena: ExprArena<T>,
}

impl<T: ExprItem> DepExpr<T> {
    /// Parses `input` as an expression of the given [`ExprKind`].
    ///
    /// # Errors
    ///
    /// Returns an error when `input` is syntactically invalid or when the parsed expression
    /// violates the context restrictions.
    pub fn parse(kind: ExprKind, input: &str) -> anyhow::Result<Self> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }

        let arena = ExprParser::parse(input)?;
        arena.validate(kind)?;
        Ok(Self { arena })
    }

    /// Returns a view of the expression.
    pub const fn view(&self) -> ExprTree<'_, T> {
        self.arena.view()
    }
}

impl<T: ExprItem> fmt::Display for DepExpr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.view().fmt(f)
    }
}

impl<T: ExprItem> Default for DepExpr<T> {
    fn default() -> Self {
        Self {
            arena: ExprArena::default(),
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
        let expression = DepExpr::<AtomDep>::parse(ExprKind::Dependency, " \t").unwrap();
        assert_eq!(expression.to_string(), "");
    }

    #[test]
    fn test_parse_kinds() {
        for (input, valid) in [
            ("!cat/pkg", true),
            ("!!cat/pkg", true),
            ("^^ ( cat/pkg cat/other )", false),
        ] {
            let result = DepExpr::<AtomDep>::parse(ExprKind::Dependency, input);
            assert_eq!(result.is_ok(), valid, "dependency: {input}");
        }

        for (kind, input, valid) in [
            (ExprKind::RequiredUse, "^^ ( foo bar )", true),
            (ExprKind::RequiredUse, "?? ( foo bar )", true),
            (ExprKind::RequiredUse, "!foo", true),
            (ExprKind::RequiredUse, "!!foo", false),
            (ExprKind::License, "|| ( GPL-2 MIT )", true),
            (ExprKind::Restrict, "( fetch mirror )", true),
            (ExprKind::Restrict, "|| ( fetch mirror )", false),
        ] {
            let result = DepExpr::<RequiredUseFlag>::parse(kind, input);
            assert_eq!(result.is_ok(), valid, "{kind}: {input}");
        }
    }
}
