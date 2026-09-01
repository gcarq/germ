pub mod atom;
pub mod expression;
mod parser;

use crate::deps::atom::Atom;
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
            ArenaEntry::Item(_) | ArenaEntry::AllOf(_) | ArenaEntry::Not(_) => true,
            ArenaEntry::Use { .. } => true,
            ArenaEntry::AnyOf(_) => {
                matches!(self, Self::Dependency | Self::License | Self::RequiredUse)
            }
            ArenaEntry::OneOf(_) | ArenaEntry::OnlyOneOf(_) => matches!(self, Self::RequiredUse),
            ArenaEntry::Forbidden(_) => matches!(self, Self::Dependency),
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
/// such as [`UseFlag`] and [`Atom`].
pub trait ExpressionItem: FromStr<Err = anyhow::Error> + fmt::Display {
    fn parse(input: &str) -> anyhow::Result<Self> {
        Self::from_str(input)
    }
}

impl ExpressionItem for Atom {}
impl ExpressionItem for UseFlag {}

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
    #[allow(dead_code)]
    pub const fn view(&self) -> ExpressionTree<'_, T> {
        self.arena.view()
    }
}

impl<T: ExpressionItem + fmt::Display> fmt::Display for DepExpression<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.arena.fmt(f)
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
    fn test_parse_empty() {
        let expression =
            DepExpression::<Atom>::parse(Eapi::Seven, ExpressionKind::Dependency, " \t").unwrap();
        assert_eq!(expression.to_string(), "");
    }

    #[test]
    fn test_expression_view() {
        let expression =
            DepExpression::<Atom>::parse(Eapi::Eight, ExpressionKind::Dependency, "cat/pkg")
                .unwrap();
        assert_eq!(expression.view().roots().count(), 1);
    }

    #[test]
    fn test_parse_valid_expressions() {
        let eapi = Eapi::Eight;
        for input in [
            "( cat/pkg )",
            "|| ( cat/pkg cat/other )",
            "foo? ( cat/pkg )",
            "!cat/pkg",
            "!!cat/pkg",
            "!foo? ( cat/pkg )",
        ] {
            let result = DepExpression::<Atom>::parse(eapi, ExpressionKind::Dependency, input);
            assert!(result.is_ok());
        }

        for (kind, input) in [
            (ExpressionKind::RequiredUse, "^^ ( foo bar )"),
            (ExpressionKind::RequiredUse, "?? ( foo bar )"),
            (ExpressionKind::RequiredUse, "!foo"),
            (ExpressionKind::RequiredUse, "!foo? ( bar )"),
            (ExpressionKind::License, "|| ( GPL-2 MIT )"),
            (ExpressionKind::Restrict, "( fetch mirror )"),
            (ExpressionKind::Restrict, "!foo? ( fetch )"),
        ] {
            let result = DepExpression::<UseFlag>::parse(eapi, kind, input);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_parse_invalid_expressions() {
        let eapi = Eapi::Eight;
        for input in ["^^ ( cat/pkg cat/other )", "!( cat/pkg )"] {
            let result = DepExpression::<Atom>::parse(eapi, ExpressionKind::Dependency, input);
            assert!(result.is_err());
        }

        for (kind, input) in [
            (ExpressionKind::RequiredUse, "!!foo"),
            (ExpressionKind::Restrict, "|| ( fetch mirror )"),
            (ExpressionKind::Restrict, "foo? ( || ( fetch mirror ) )"),
            (ExpressionKind::Restrict, "!fetch"),
        ] {
            let result = DepExpression::<UseFlag>::parse(eapi, kind, input);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_parse_empty_groups() {
        let eapi = Eapi::Eight;
        for input in ["()", "|| ()", "^^ ()", "?? ()", "foo? ()", "foo? ( || () )"] {
            let result = DepExpression::<Atom>::parse(eapi, ExpressionKind::Dependency, input);
            assert!(result.is_err());
        }
    }
}
