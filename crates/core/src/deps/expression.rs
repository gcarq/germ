use super::ExpressionItem;
use super::parser::arena::{ArenaEntry, ExpressionArena, ExpressionId};
use crate::useflag::UseFlag;
use std::ops::Range;
use std::{fmt, slice};

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

impl<T: ExpressionItem> fmt::Display for ExpressionTree<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.roots().fmt(f)
    }
}

/// A borrowed expression from [`ExpressionNode`].
pub enum Expression<'a, T: ExpressionItem> {
    Item(&'a T),
    AllOf(ExpressionNodes<'a, T>),        // ( a b )
    AnyOf(ExpressionNodes<'a, T>),        // || ( a b )
    ExactlyOneOf(ExpressionNodes<'a, T>), // ^^ ( a b )
    AtMostOneOf(ExpressionNodes<'a, T>),  // ?? ( a b )
    Use {
        flag: &'a UseFlag,
        negated: bool,
        nodes: ExpressionNodes<'a, T>,
    },
    Not(ExpressionNode<'a, T>),
    Forbidden(ExpressionNode<'a, T>),
}

impl<T: ExpressionItem> fmt::Display for Expression<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expression::Item(item) => item.fmt(f),
            Expression::AllOf(nodes) => write!(f, "( {nodes} )"),
            Expression::AnyOf(nodes) => write!(f, "|| ( {nodes} )"),
            Expression::ExactlyOneOf(nodes) => write!(f, "^^ ( {nodes} )"),
            Expression::AtMostOneOf(nodes) => write!(f, "?? ( {nodes} )"),
            Expression::Use {
                flag,
                negated,
                nodes,
            } => match *negated {
                true => write!(f, "!{flag}? ( {nodes} )"),
                false => write!(f, "{flag}? ( {nodes} )"),
            },
            Expression::Not(node) => write!(f, "!{node}"),
            Expression::Forbidden(node) => write!(f, "!!{node}"),
        }
    }
}

/// Represents a handle to one node in an [`ExpressionTree`].
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
            ArenaEntry::ExactlyOneOf(nodes) => {
                Expression::ExactlyOneOf(ExpressionNodes::new(self.tree, nodes))
            }
            ArenaEntry::AtMostOneOf(nodes) => {
                Expression::AtMostOneOf(ExpressionNodes::new(self.tree, nodes))
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
            ArenaEntry::Not(node) => Expression::Not(Self::new(self.tree, *node)),
            ArenaEntry::Forbidden(node) => Expression::Forbidden(Self::new(self.tree, *node)),
        }
    }

    const fn new(tree: ExpressionTree<'a, T>, id: ExpressionId) -> Self {
        Self { tree, id }
    }
}

impl<T: ExpressionItem> Copy for ExpressionNode<'_, T> {}

impl<T: ExpressionItem> Clone for ExpressionNode<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ExpressionItem> fmt::Display for ExpressionNode<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.expression().fmt(f)
    }
}

/// An iterator over the nodes of an expression.
pub struct ExpressionNodes<'a, T: ExpressionItem> {
    tree: ExpressionTree<'a, T>,
    nodes: slice::Iter<'a, ExpressionId>,
}

impl<T: ExpressionItem> fmt::Display for ExpressionNodes<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, id) in self.nodes.as_slice().iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            ExpressionNode::new(self.tree, *id).fmt(f)?;
        }
        Ok(())
    }
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
        Some(ExpressionNode::new(self.tree, self.nodes.next().copied()?))
    }
}

impl<T: ExpressionItem> ExpressionArena<T> {
    /// Returns a view of the arena.
    pub const fn view(&self) -> ExpressionTree<'_, T> {
        ExpressionTree { arena: self }
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::ExpressionParser;
    use crate::useflag::UseFlag;

    #[test]
    fn test_display_nested() {
        let input = "|| ( cli gui ) gui? ( ^^ ( X wayland ) X? ( ?? ( gles2 opengl ) !minimal? ( || ( vulkan vaapi ) ) ) )";
        let arena = ExpressionParser::<UseFlag>::parse(input).unwrap();

        assert_eq!(arena.view().to_string(), input);
    }
}
