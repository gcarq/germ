use crate::deps::ExpressionItem;
use crate::deps::useflag::UseFlag;
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::ops::Range;

/// Holds the entire [`Expression`], which is a flat representation of the expression tree.
///
/// To support multiple root expressions, `root` is defined as [`Range`] which holds the
/// index range in the `children` vector.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
pub struct ExpressionArena<T: ExpressionItem> {
    expressions: Vec<Expression<T>>,
    children: Vec<ExpressionId>,
    root: Range<u16>,
}

impl<T: ExpressionItem> ExpressionArena<T> {
    pub fn new() -> Self {
        Self {
            expressions: Vec::with_capacity(64),
            children: Vec::with_capacity(64),
            root: Range::default(),
        }
    }

    /// Consumes the given `ids` and pushes them as children.
    ///
    /// Returns a [`Range`] for future referencing in `self.children`.
    pub fn push_children(&mut self, ids: &[ExpressionId]) -> Range<u16> {
        let start = self.children.len() as u16;
        self.children.extend(ids);
        let end = self.children.len() as u16;
        start..end
    }

    /// Pushes the given `expr` into the arena and returns its [`ExpressionId`].
    pub fn push_expression(&mut self, expr: Expression<T>) -> ExpressionId {
        let id = ExpressionId(self.expressions.len() as u16);
        self.expressions.push(expr);
        id
    }

    pub fn get_expression(&self, id: &ExpressionId) -> &Expression<T> {
        &self.expressions[id.0 as usize]
    }

    pub fn get_children(&self, range: Range<u16>) -> impl Iterator<Item = &Expression<T>> {
        self.children[range.start as usize..range.end as usize]
            .iter()
            .map(|id| self.get_expression(id))
    }

    pub const fn set_root(&mut self, root: Range<u16>) {
        self.root = root;
    }

    fn fmt_expression(&self, expr: &Expression<T>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match expr {
            Expression::Item(item) => item.fmt(f),
            Expression::Not(child) => {
                f.write_str("!")?;
                self.fmt_expression(self.get_expression(child), f)
            }
            Expression::Forbidden(child) => {
                f.write_str("!!")?;
                self.fmt_expression(self.get_expression(child), f)
            }
            Expression::Use { flag, children } => {
                write!(f, "{flag}? ( ")?;
                self.fmt_children(children.clone(), f)?;
                f.write_str(" )")
            }
            Expression::AllOf(children) => {
                f.write_str("( ")?;
                self.fmt_children(children.clone(), f)?;
                f.write_str(" )")
            }
            Expression::AnyOf(children) => {
                f.write_str("|| ( ")?;
                self.fmt_children(children.clone(), f)?;
                f.write_str(" )")
            }
            Expression::OneOf(children) => {
                f.write_str("^^ ( ")?;
                self.fmt_children(children.clone(), f)?;
                f.write_str(" )")
            }
            Expression::OnlyOneOf(children) => {
                f.write_str("?? ( ")?;
                self.fmt_children(children.clone(), f)?;
                f.write_str(" )")
            }
        }
    }

    fn fmt_children(&self, range: Range<u16>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, child) in self.get_children(range).enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            self.fmt_expression(child, f)?;
        }
        Ok(())
    }
}

impl<T: ExpressionItem> fmt::Display for ExpressionArena<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_children(self.root.clone(), f)
    }
}

impl<T: ExpressionItem> Default for ExpressionArena<T> {
    fn default() -> Self {
        Self {
            expressions: Vec::default(),
            children: Vec::default(),
            root: Range::default(),
        }
    }
}

/// Represents an expression, this can be an `Item` (USE Flag or Atom), an expression group or
/// other variants defined in PMS 8.2.
///
/// [`Range`] is used to reference the child expressions in the flat [`Vec`].
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
pub enum Expression<T: ExpressionItem> {
    Item(T),

    AllOf(Range<u16>),     // ( a b )
    AnyOf(Range<u16>),     // || ( a b )
    OneOf(Range<u16>),     // ^^ ( a b )
    OnlyOneOf(Range<u16>), // ?? ( a b )

    Use { flag: UseFlag, children: Range<u16> },

    Not(ExpressionId),
    Forbidden(ExpressionId),
}

/// This is a basic wrapper around `u16` that distinguishes between expression ids
/// and children indices.
#[derive(Archive, Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct ExpressionId(u16);
