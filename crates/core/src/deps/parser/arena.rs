use crate::deps::{ExpressionItem, ExpressionKind};
use crate::useflag::UseFlag;
use anyhow::{Context, bail};
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::ops::Range;

/// Represents an arena entry, this can be an `Item` (USE Flag, Atom, URI, ...),
/// an expression group or other variants defined in PMS 8.2.
///
/// [`Range`] is used to reference the child expressions in the flat [`Vec`].
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
pub enum ArenaEntry<T: ExpressionItem> {
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

    Not(ExpressionId),
    Forbidden(ExpressionId),
}

impl<T: ExpressionItem> ArenaEntry<T> {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Item(_) => "item",
            Self::AllOf(_) => "all-of",
            Self::AnyOf(_) => "any-of",
            Self::ExactlyOneOf(_) => "exactly-one-of",
            Self::AtMostOneOf(_) => "at-most-one-of",
            Self::Use { .. } => "USE-conditional",
            Self::Not(_) => "negation",
            Self::Forbidden(_) => "strong-blocker",
        }
    }
}

/// This is a basic wrapper around `u32` that distinguishes between expression ids
/// and children indices.
#[derive(Archive, Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Debug)]
pub struct ExpressionId(u32);

/// Holds the entire expression in a flat arena representation.
///
/// All expressions are saved in `expressions`.
///
/// `self.children` holds indices to look up [`ArenaEntry`] in `self.expressions`,
/// this allows children to be expressed as [`Range`] into `self.children`.
///
/// The "root" of the expression can also consist of multiple expressions,
/// which is why `self.roots` is also a [`Range`] into `children`.
#[derive(Archive, Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
pub struct ExpressionArena<T: ExpressionItem> {
    expressions: Vec<ArenaEntry<T>>,
    children: Vec<ExpressionId>,
    roots: Range<u32>,
}

impl<T: ExpressionItem> ExpressionArena<T> {
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
    pub fn push_children(&mut self, ids: &[ExpressionId]) -> anyhow::Result<Range<u32>> {
        let start = u32::try_from(self.children.len()).context("expression doesn't fit in u32")?;
        self.children.extend(ids);
        let end = u32::try_from(self.children.len()).context("expression doesn't fit in u32")?;
        Ok(start..end)
    }

    /// Pushes the given `expr` into the arena and returns its [`ExpressionId`].
    pub fn push_expression(&mut self, expr: ArenaEntry<T>) -> anyhow::Result<ExpressionId> {
        let id = ExpressionId(
            u32::try_from(self.expressions.len()).context("expression doesn't fit in u32")?,
        );
        self.expressions.push(expr);
        Ok(id)
    }

    /// Returns the expression for the given `id`.
    pub fn get_expression(&self, id: &ExpressionId) -> &ArenaEntry<T> {
        &self.expressions[id.0 as usize]
    }

    /// Returns the children for the given `range`.
    pub fn get_children(&self, range: &Range<u32>) -> &[ExpressionId] {
        &self.children[range.start as usize..range.end as usize]
    }

    /// Validates the expression against the given `kind`.
    ///
    /// Returns `Err` when a group, negation, blocker, or required-use operator is not valid
    /// for the selected EAPI and expression context.
    pub fn validate(&self, kind: ExpressionKind) -> anyhow::Result<()> {
        self.validate_range(&self.roots, kind)
    }

    fn validate_range(&self, range: &Range<u32>, kind: ExpressionKind) -> anyhow::Result<()> {
        for node in self.get_children(range) {
            self.validate_expression(*node, kind)?;
        }
        Ok(())
    }

    fn validate_expression(&self, id: ExpressionId, kind: ExpressionKind) -> anyhow::Result<()> {
        let expression = self.get_expression(&id);
        if !kind.supports_expression(expression) {
            bail!("{} is not valid in {kind} expressions", expression.name());
        }

        match expression {
            ArenaEntry::Item(_) | ArenaEntry::Forbidden(_) => Ok(()),
            ArenaEntry::AllOf(nodes)
            | ArenaEntry::AnyOf(nodes)
            | ArenaEntry::ExactlyOneOf(nodes)
            | ArenaEntry::AtMostOneOf(nodes)
            | ArenaEntry::Use { nodes, .. } => self.validate_range(nodes, kind),
            ArenaEntry::Not(node) => self.validate_negation(*node, kind),
        }
    }

    fn validate_negation(&self, node: ExpressionId, kind: ExpressionKind) -> anyhow::Result<()> {
        match self.get_expression(&node) {
            ArenaEntry::Item(_) => match kind {
                ExpressionKind::Dependency | ExpressionKind::RequiredUse => Ok(()),
                _ => bail!("negation is not valid in {kind} expressions"),
            },
            _ => bail!("negation must apply to an item"),
        }
    }

    fn fmt_expression(&self, expr: &ArenaEntry<T>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match expr {
            ArenaEntry::Item(item) => item.fmt(f),
            ArenaEntry::Not(node) => {
                f.write_str("!")?;
                self.fmt_expression(self.get_expression(node), f)
            }
            ArenaEntry::Forbidden(node) => {
                f.write_str("!!")?;
                self.fmt_expression(self.get_expression(node), f)
            }
            ArenaEntry::Use {
                flag,
                negated,
                nodes,
            } => {
                if *negated {
                    f.write_str("!")?;
                }
                write!(f, "{flag}? ( ")?;
                self.fmt_children(nodes, f)?;
                f.write_str(" )")
            }
            ArenaEntry::AllOf(nodes) => {
                f.write_str("( ")?;
                self.fmt_children(nodes, f)?;
                f.write_str(" )")
            }
            ArenaEntry::AnyOf(nodes) => {
                f.write_str("|| ( ")?;
                self.fmt_children(nodes, f)?;
                f.write_str(" )")
            }
            ArenaEntry::ExactlyOneOf(nodes) => {
                f.write_str("^^ ( ")?;
                self.fmt_children(nodes, f)?;
                f.write_str(" )")
            }
            ArenaEntry::AtMostOneOf(nodes) => {
                f.write_str("?? ( ")?;
                self.fmt_children(nodes, f)?;
                f.write_str(" )")
            }
        }
    }

    fn fmt_children(&self, range: &Range<u32>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, id) in self.get_children(range).iter().enumerate() {
            let node = self.get_expression(id);
            if i > 0 {
                f.write_str(" ")?;
            }
            self.fmt_expression(node, f)?;
        }
        Ok(())
    }
}

impl<T: ExpressionItem> fmt::Display for ExpressionArena<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_children(&self.roots, f)
    }
}

impl<T: ExpressionItem> Default for ExpressionArena<T> {
    fn default() -> Self {
        Self {
            expressions: Vec::default(),
            children: Vec::default(),
            roots: Range::default(),
        }
    }
}
