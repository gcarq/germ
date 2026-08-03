pub mod atom;
mod parser;
pub mod useflag;

use crate::deps::atom::Atom;
use crate::deps::parser::ExpressionParser;
use crate::deps::parser::arena::ExpressionArena;
use crate::deps::useflag::UseFlag;
use anyhow::Result;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// This trait defines an item that can be used in a dependency expression,
/// such as [`UseFlag`] and [`Atom`].
pub trait ExpressionItem: FromStr<Err = anyhow::Error> + fmt::Display {
    fn parse(input: &str) -> Result<Self> {
        Self::from_str(input)
    }
}

impl ExpressionItem for Atom {}
impl ExpressionItem for UseFlag {}

/// Holds a dependency expression, which can be evaluated to check if all package requirements
/// are satisfied.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct DepExpression<T: ExpressionItem> {
    arena: ExpressionArena<T>,
}

impl<T: ExpressionItem> DepExpression<T> {
    /// Parses the given `input` string and returns a [`DepExpression`].
    pub fn parse(input: &str) -> Result<Self> {
        let arena = ExpressionParser::parse(input)?;
        Ok(Self { arena })
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
