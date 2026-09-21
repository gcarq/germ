use crate::deps::{ExprItem, ExprKind};
use crate::useflag::UseFlag;
use anyhow::{Context, bail};
use rkyv::{Archive, Deserialize, Serialize};
use std::ops::Range;

/// Represents an arena entry, this can be an `Item` (USE Flag, Atom, URI, ...),
/// an expression group or other variants defined in PMS 8.2.
///
/// [`Range`] is used to reference the child expressions in the flat [`Vec`].
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
pub enum ExprEntry<T: ExprItem> {
    Item(T),

    AllOf(Range<u32>),        // ( a b )
    AnyOf(Range<u32>),        // || ( a b )
    ExactlyOneOf(Range<u32>), // ^^ ( a b )
    AtMostOneOf(Range<u32>),  // ?? ( a b )

    Use {
        flag: UseFlag,
        negated: bool,
        nodes: Range<u32>,
    },
}

impl<T: ExprItem> ExprEntry<T> {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Item(_) => "item",
            Self::AllOf(_) => "all-of",
            Self::AnyOf(_) => "any-of",
            Self::ExactlyOneOf(_) => "exactly-one-of",
            Self::AtMostOneOf(_) => "at-most-one-of",
            Self::Use { .. } => "USE-conditional",
        }
    }
}

/// This is a basic wrapper around `u32` that distinguishes between expression ids
/// and children indices.
#[derive(Archive, Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Debug)]
pub struct ExprId(u32);

/// Holds the entire expression in a flat arena representation.
///
/// All expressions are saved in `expressions`.
///
/// `self.children` holds indices to look up [`ExprEntry`] in `self.expressions`,
/// this allows children to be expressed as [`Range`] into `self.children`.
///
/// The "root" of the expression can also consist of multiple expressions,
/// which is why `self.roots` is also a [`Range`] into `children`.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
pub struct ExprArena<T: ExprItem> {
    expressions: Vec<ExprEntry<T>>,
    children: Vec<ExprId>,
    roots: Range<u32>,
}

impl<T: ExprItem> ExprArena<T> {
    /// Sets the root range for the expression arena.
    pub const fn set_roots(&mut self, root: Range<u32>) {
        self.roots = root;
    }

    /// Returns the root range.
    pub const fn root_range(&self) -> &Range<u32> {
        &self.roots
    }

    /// Consumes the given `ids` and pushes them as children.
    ///
    /// Returns a [`Range`] for future referencing in `self.children`.
    pub fn push_children(&mut self, ids: &[ExprId]) -> anyhow::Result<Range<u32>> {
        let start = u32::try_from(self.children.len()).context("expression doesn't fit in u32")?;
        self.children.extend(ids);
        let end = u32::try_from(self.children.len()).context("expression doesn't fit in u32")?;
        Ok(start..end)
    }

    /// Pushes the given `expr` into the arena and returns its [`ExprId`].
    pub fn push_expr(&mut self, expr: ExprEntry<T>) -> anyhow::Result<ExprId> {
        let id = u32::try_from(self.expressions.len()).context("expression doesn't fit in u32")?;
        self.expressions.push(expr);
        Ok(ExprId(id))
    }

    /// Returns the expression for the given `id`.
    pub fn get_expr(&self, id: &ExprId) -> &ExprEntry<T> {
        &self.expressions[id.0 as usize]
    }

    /// Returns the children for the given `range`.
    pub fn get_children(&self, range: &Range<u32>) -> &[ExprId] {
        &self.children[range.start as usize..range.end as usize]
    }

    /// Validates the expression against the given `kind`.
    ///
    /// Returns `Err` when a group, negation, blocker, or required-use operator is not valid
    /// for the selected EAPI and expression context.
    pub fn validate(&self, kind: ExprKind) -> anyhow::Result<()> {
        self.validate_range(&self.roots, kind)
    }

    fn validate_range(&self, range: &Range<u32>, kind: ExprKind) -> anyhow::Result<()> {
        for node in self.get_children(range) {
            self.validate_expr(*node, kind)?;
        }
        Ok(())
    }

    fn validate_expr(&self, id: ExprId, kind: ExprKind) -> anyhow::Result<()> {
        let expression = self.get_expr(&id);
        if !kind.supports_expr(expression) {
            bail!("{} is not valid in {kind} expressions", expression.name());
        }

        match expression {
            ExprEntry::Item(_) => Ok(()),
            ExprEntry::AllOf(nodes)
            | ExprEntry::AnyOf(nodes)
            | ExprEntry::ExactlyOneOf(nodes)
            | ExprEntry::AtMostOneOf(nodes)
            | ExprEntry::Use { nodes, .. } => self.validate_range(nodes, kind),
        }
    }
}

impl<T: ExprItem> Default for ExprArena<T> {
    fn default() -> Self {
        Self {
            expressions: Vec::default(),
            children: Vec::default(),
            roots: Range::default(),
        }
    }
}
