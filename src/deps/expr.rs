use crate::deps::parser::{DepExpressionParser, ast};
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

/// This trait defines an item that can be used in a dependency expression,
/// such as [`UseFlag`] and [`Atom`].
pub trait ExpressionItem: FromStr<Err = anyhow::Error> + fmt::Display {
    fn parse(input: &str) -> anyhow::Result<Self> {
        Self::from_str(input)
    }
}

/// Holds the entire dependency expression, which is a collection of [`ast::Expression`].
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Default)]
#[cfg_attr(test, derive(Debug))]
pub struct DepExpression<T: ExpressionItem> {
    expr: Vec<ast::Expression<T>>,
}

impl<T: ExpressionItem> DepExpression<T> {
    pub const fn new() -> Self {
        Self { expr: Vec::new() }
    }
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        DepExpressionParser::new(input)
            .parse::<T>()
            .with_context(|| anyhow!("failed to parse dependency expression: {input}"))
    }
}

impl<T: ExpressionItem> fmt::Display for DepExpression<T> {
    // TODO: get rid of allocation
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            &self
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

impl<T: ExpressionItem> Deref for DepExpression<T> {
    type Target = Vec<ast::Expression<T>>;

    fn deref(&self) -> &Self::Target {
        &self.expr
    }
}

impl<T: ExpressionItem> DerefMut for DepExpression<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.expr
    }
}
