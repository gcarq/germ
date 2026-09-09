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
    pub fn roots(&self) -> ExpressionChildren<'a, T> {
        ExpressionChildren::new(*self, self.arena.root_range())
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
    AllOf(ExpressionChildren<'a, T>),     // ( a b )
    AnyOf(ExpressionChildren<'a, T>),     // || ( a b )
    OneOf(ExpressionChildren<'a, T>),     // ^^ ( a b )
    OnlyOneOf(ExpressionChildren<'a, T>), // ?? ( a b )
    Use {
        flag: &'a UseFlag,
        negated: bool,
        children: ExpressionChildren<'a, T>,
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
            ArenaEntry::AllOf(children) => {
                Expression::AllOf(ExpressionChildren::new(self.tree, children))
            }
            ArenaEntry::AnyOf(children) => {
                Expression::AnyOf(ExpressionChildren::new(self.tree, children))
            }
            ArenaEntry::OneOf(children) => {
                Expression::OneOf(ExpressionChildren::new(self.tree, children))
            }
            ArenaEntry::OnlyOneOf(children) => {
                Expression::OnlyOneOf(ExpressionChildren::new(self.tree, children))
            }
            ArenaEntry::Use {
                flag,
                negated,
                children,
            } => Expression::Use {
                flag,
                negated: *negated,
                children: ExpressionChildren::new(self.tree, children),
            },
            ArenaEntry::Not(child) => Expression::Not(Self {
                tree: self.tree,
                id: *child,
            }),
            ArenaEntry::Forbidden(child) => Expression::Forbidden(Self {
                tree: self.tree,
                id: *child,
            }),
        }
    }
}

/// An iterator over the children of an expression.
pub struct ExpressionChildren<'a, T: ExpressionItem> {
    tree: ExpressionTree<'a, T>,
    children: slice::Iter<'a, ExpressionId>,
}

impl<'a, T: ExpressionItem> ExpressionChildren<'a, T> {
    fn new(tree: ExpressionTree<'a, T>, range: &Range<u32>) -> Self {
        let children = tree.arena.get_children(range).iter();
        Self { tree, children }
    }
}

impl<'a, T: ExpressionItem> Iterator for ExpressionChildren<'a, T> {
    type Item = ExpressionNode<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(ExpressionNode {
            tree: self.tree,
            id: self.children.next().copied()?,
        })
    }
}

impl<T: ExpressionItem> ExpressionArena<T> {
    /// Returns a view of the arena.
    pub const fn view(&self) -> ExpressionTree<'_, T> {
        ExpressionTree { arena: self }
    }
}
