use super::ExprItem;
use super::parser::arena::{ExprArena, ExprEntry, ExprId};
use crate::useflag::UseFlag;
use std::ops::Range;
use std::{fmt, slice};

/// A borrowed view of [`ExprArena`].
pub struct ExprTree<'a, T: ExprItem> {
    arena: &'a ExprArena<T>,
}

impl<'a, T: ExprItem> ExprTree<'a, T> {
    /// Returns the root expressions in source order.
    pub fn roots(&self) -> ExprNodes<'a, T> {
        ExprNodes::new(*self, self.arena.root_range())
    }

    /// Evaluates the expression with the given [`ExprEval`] for [`ExprItem`]s.
    pub fn eval<E>(self, evaluator: &mut E) -> anyhow::Result<bool>
    where
        E: ExprEval<T>,
    {
        eval_all(evaluator, self.roots())
    }
}

impl<'a, T: ExprItem> Copy for ExprTree<'a, T> {}

impl<'a, T: ExprItem> Clone for ExprTree<'a, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ExprItem> fmt::Display for ExprTree<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.roots().fmt(f)
    }
}

/// Supplies item and conditional USE decisions to [`ExprTree::eval`].
pub trait ExprEval<T: ExprItem> {
    /// Evaluates one [`ExprItem`].
    fn eval_item(&mut self, item: &T) -> anyhow::Result<bool>;

    /// Returns whether a USE flag is enabled.
    fn is_use_enabled(&self, flag: &UseFlag) -> anyhow::Result<bool>;
}

/// Evaluates a single [`Expr`].
fn eval_expr<T, E>(evaluator: &mut E, expr: Expr<'_, T>) -> anyhow::Result<bool>
where
    T: ExprItem,
    E: ExprEval<T>,
{
    match expr {
        Expr::Item(item) => evaluator.eval_item(item),
        Expr::AllOf(nodes) => eval_all(evaluator, nodes),
        Expr::AnyOf(nodes) => eval_any(evaluator, nodes),
        Expr::ExactlyOneOf(nodes) => Ok(eval_up_to_two(evaluator, nodes)? == 1),
        Expr::AtMostOneOf(nodes) => Ok(eval_up_to_two(evaluator, nodes)? < 2),
        Expr::Use {
            flag,
            negated,
            nodes,
        } => match evaluator.is_use_enabled(flag)? == negated {
            true => Ok(true),
            false => eval_all(evaluator, nodes),
        },
    }
}

/// Evaluates an all-of group `( foo bar )`.
fn eval_all<T, E>(evaluator: &mut E, nodes: ExprNodes<'_, T>) -> anyhow::Result<bool>
where
    T: ExprItem,
    E: ExprEval<T>,
{
    for expr in nodes {
        if !eval_expr(evaluator, expr)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Evaluates an any-of group `|| ( foo bar )`.
fn eval_any<T, E>(evaluator: &mut E, nodes: ExprNodes<'_, T>) -> anyhow::Result<bool>
where
    T: ExprItem,
    E: ExprEval<T>,
{
    for expr in nodes {
        if eval_expr(evaluator, expr)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Counts the satisfied expressions, stopping at two matches.
fn eval_up_to_two<T, E>(evaluator: &mut E, nodes: ExprNodes<'_, T>) -> anyhow::Result<u8>
where
    T: ExprItem,
    E: ExprEval<T>,
{
    let mut count = 0;
    for expr in nodes {
        if eval_expr(evaluator, expr)? {
            count += 1;
            if count > 1 {
                break;
            }
        }
    }
    Ok(count)
}

/// A borrowed expression from an [`ExprTree`].
pub enum Expr<'a, T: ExprItem> {
    Item(&'a T),
    AllOf(ExprNodes<'a, T>),        // ( a b )
    AnyOf(ExprNodes<'a, T>),        // || ( a b )
    ExactlyOneOf(ExprNodes<'a, T>), // ^^ ( a b )
    AtMostOneOf(ExprNodes<'a, T>),  // ?? ( a b )
    Use {
        flag: &'a UseFlag,
        negated: bool,
        nodes: ExprNodes<'a, T>,
    },
}

impl<'a, T: ExprItem> Expr<'a, T> {
    /// Creates a new expression from the given `tree` and `id`.
    fn new(tree: ExprTree<'a, T>, id: &ExprId) -> Self {
        match tree.arena.get_expr(id) {
            ExprEntry::Item(item) => Self::Item(item),
            ExprEntry::AllOf(nodes) => Self::AllOf(ExprNodes::new(tree, nodes)),
            ExprEntry::AnyOf(nodes) => Self::AnyOf(ExprNodes::new(tree, nodes)),
            ExprEntry::ExactlyOneOf(nodes) => Self::ExactlyOneOf(ExprNodes::new(tree, nodes)),
            ExprEntry::AtMostOneOf(nodes) => Self::AtMostOneOf(ExprNodes::new(tree, nodes)),
            ExprEntry::Use {
                flag,
                negated,
                nodes,
            } => Self::Use {
                flag,
                negated: *negated,
                nodes: ExprNodes::new(tree, nodes),
            },
        }
    }
}

impl<T: ExprItem> fmt::Display for Expr<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Item(item) => item.fmt(f),
            Expr::AllOf(nodes) => write!(f, "( {nodes} )"),
            Expr::AnyOf(nodes) => write!(f, "|| ( {nodes} )"),
            Expr::ExactlyOneOf(nodes) => write!(f, "^^ ( {nodes} )"),
            Expr::AtMostOneOf(nodes) => write!(f, "?? ( {nodes} )"),
            Expr::Use {
                flag,
                negated,
                nodes,
            } => match *negated {
                true => write!(f, "!{flag}? ( {nodes} )"),
                false => write!(f, "{flag}? ( {nodes} )"),
            },
        }
    }
}

/// An iterator over the expressions of an [`ExprTree`].
pub struct ExprNodes<'a, T: ExprItem> {
    tree: ExprTree<'a, T>,
    nodes: slice::Iter<'a, ExprId>,
}

impl<T: ExprItem> fmt::Display for ExprNodes<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, id) in self.nodes.as_slice().iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            Expr::new(self.tree, id).fmt(f)?;
        }
        Ok(())
    }
}

impl<'a, T: ExprItem> ExprNodes<'a, T> {
    fn new(tree: ExprTree<'a, T>, range: &Range<u32>) -> Self {
        let nodes = tree.arena.get_children(range).iter();
        Self { tree, nodes }
    }
}

impl<'a, T: ExprItem> Iterator for ExprNodes<'a, T> {
    type Item = Expr<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(Expr::new(self.tree, self.nodes.next()?))
    }
}

impl<T: ExprItem> ExprArena<T> {
    /// Returns a view of the arena.
    pub const fn view(&self) -> ExprTree<'_, T> {
        ExprTree { arena: self }
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::ExprParser;
    use super::*;
    use crate::deps::{AtomDep, RequiredUseFlag};

    struct TestEvaluator {
        enabled: Vec<UseFlag>,
        evaluated: String,
    }

    impl ExprEval<RequiredUseFlag> for TestEvaluator {
        fn eval_item(&mut self, item: &RequiredUseFlag) -> anyhow::Result<bool> {
            let value = item.inner().as_str();
            self.evaluated.push_str(value);
            Ok(value == "t")
        }

        fn is_use_enabled(&self, flag: &UseFlag) -> anyhow::Result<bool> {
            Ok(self.enabled.contains(flag))
        }
    }

    #[test]
    fn test_display() {
        let input = "|| ( cli gui ) gui? ( ^^ ( X wayland ) X? ( ?? ( gles2 opengl ) !minimal? ( || ( vulkan vaapi ) ) ) )";
        let arena = ExprParser::<RequiredUseFlag>::parse(input).unwrap();
        assert_eq!(arena.view().to_string(), input);

        let input = "!foo !bar? ( !baz )";
        let arena = ExprParser::<RequiredUseFlag>::parse(input).unwrap();
        assert_eq!(arena.view().to_string(), input);

        let input = "!!cat/pkg !cat/other";
        let arena = ExprParser::<AtomDep>::parse(input).unwrap();
        assert_eq!(arena.view().to_string(), input);
    }

    #[test]
    fn test_evaluate() -> anyhow::Result<()> {
        // expression, enabled USE flags, expected result, evaluated items in source order
        let cases: &[(&str, &[&str], bool, &str)] = &[
            ("( t t )", &[], true, "tt"),
            ("( t f )", &[], false, "tf"),
            ("( f t )", &[], false, "f"),
            ("|| ( f t )", &[], true, "ft"),
            ("|| ( t f )", &[], true, "t"),
            ("|| ( f f )", &[], false, "ff"),
            ("^^ ( t f )", &[], true, "tf"),
            ("^^ ( f f )", &[], false, "ff"),
            ("^^ ( f t f )", &[], true, "ftf"),
            ("^^ ( t t f )", &[], false, "tt"),
            ("?? ( t f )", &[], true, "tf"),
            ("?? ( f f f )", &[], true, "fff"),
            ("?? ( t t f )", &[], false, "tt"),
            ("g? ( t )", &["g"], true, "t"),
            ("g? ( f )", &["g"], false, "f"),
            ("g? ( f )", &[], true, ""),
            ("!g? ( t )", &[], true, "t"),
            ("!g? ( t )", &["g"], true, ""),
            ("t t g? ( t )", &["g"], true, "ttt"),
            ("|| ( f t ) t", &[], true, "ftt"),
        ];

        for &(input, enabled, expected, evaluated) in cases {
            let expr = ExprParser::<RequiredUseFlag>::parse(input)?;
            let mut evaluator = TestEvaluator {
                enabled: enabled.iter().map(|flag| flag.parse().unwrap()).collect(),
                evaluated: String::new(),
            };
            let result = expr.view().eval(&mut evaluator)?;
            let actual = (result, evaluator.evaluated.as_str());
            assert_eq!(actual, (expected, evaluated), "{input}");
        }
        Ok(())
    }
}
