use super::ExpressionItem;
use super::parser::arena::{ArenaEntry, ExpressionArena, ExpressionId};
use crate::useflag::UseFlag;
use std::ops::Range;
use std::slice;

/// A borrowed view of [`ExpressionArena`].
pub struct ExpressionTree<'a, T: ExpressionItem> {
    arena: &'a ExpressionArena<T>,
}

impl<'a, T: ExpressionItem> ExpressionTree<'a, T> {
    /// Returns the root nodes in source order.
    pub fn roots(&self) -> ExpressionNodes<'a, T> {
        ExpressionNodes::new(*self, self.arena.root_range())
    }
}

impl<'a, T: ExpressionItem> Copy for ExpressionTree<'a, T> {}

impl<'a, T: ExpressionItem> Clone for ExpressionTree<'a, T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// A borrowed expression from [`ExpressionNode`].
pub enum Expression<'a, T: ExpressionItem> {
    Item(&'a T),
    AllOf(ExpressionNodes<'a, T>),     // ( a b )
    AnyOf(ExpressionNodes<'a, T>),     // || ( a b )
    OneOf(ExpressionNodes<'a, T>),     // ^^ ( a b )
    OnlyOneOf(ExpressionNodes<'a, T>), // ?? ( a b )
    Use {
        flag: &'a UseFlag,
        negated: bool,
        nodes: ExpressionNodes<'a, T>,
    },
    Not(ExpressionNode<'a, T>),
    Forbidden(ExpressionNode<'a, T>),
}

/// Represents a handle to one node in an [`ExpressionTree`].
#[derive(Copy, Clone)]
pub struct ExpressionNode<'a, T: ExpressionItem> {
    tree: ExpressionTree<'a, T>,
    id: ExpressionId,
}

impl<'a, T: ExpressionItem> ExpressionNode<'a, T> {
    /// Returns the borrowed semantic expression represented by this node.
    pub fn expression(&self) -> Expression<'a, T> {
        match self.tree.arena.get_expression(&self.id) {
            ArenaEntry::Item(item) => Expression::Item(item),
            ArenaEntry::AllOf(nodes) => Expression::AllOf(ExpressionNodes::new(self.tree, nodes)),
            ArenaEntry::AnyOf(nodes) => Expression::AnyOf(ExpressionNodes::new(self.tree, nodes)),
            ArenaEntry::OneOf(nodes) => Expression::OneOf(ExpressionNodes::new(self.tree, nodes)),
            ArenaEntry::OnlyOneOf(nodes) => {
                Expression::OnlyOneOf(ExpressionNodes::new(self.tree, nodes))
            }
            ArenaEntry::Use {
                flag,
                negated,
                nodes,
            } => Expression::Use {
                flag,
                negated: *negated,
                nodes: ExpressionNodes::new(self.tree, nodes),
            },
            ArenaEntry::Not(node) => Expression::Not(Self {
                tree: self.tree,
                id: *node,
            }),
            ArenaEntry::Forbidden(node) => Expression::Forbidden(Self {
                tree: self.tree,
                id: *node,
            }),
        }
    }
}

/// An iterator over the nodes of an expression.
pub struct ExpressionNodes<'a, T: ExpressionItem> {
    tree: ExpressionTree<'a, T>,
    nodes: slice::Iter<'a, ExpressionId>,
}

impl<'a, T: ExpressionItem> ExpressionNodes<'a, T> {
    fn new(tree: ExpressionTree<'a, T>, range: &Range<u32>) -> Self {
        let nodes = tree.arena.get_children(range).iter();
        Self { tree, nodes }
    }
}

impl<'a, T: ExpressionItem> Iterator for ExpressionNodes<'a, T> {
    type Item = ExpressionNode<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(ExpressionNode {
            tree: self.tree,
            id: self.nodes.next().copied()?,
        })
    }
}

impl<T: ExpressionItem> ExpressionArena<T> {
    /// Returns a view of the arena.
    pub const fn view(&self) -> ExpressionTree<'_, T> {
        ExpressionTree { arena: self }
    }
}
