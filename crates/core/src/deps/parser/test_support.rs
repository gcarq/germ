use std::fmt::Debug;

use super::arena::{Expression, ExpressionArena, ExpressionId};
use crate::deps::ExpressionItem;
use crate::useflag::UseFlag;
use TestExpression::{AllOf, AnyOf, Forbidden, Item, Not, OneOf, OnlyOneOf, Use};

#[derive(Debug, Eq, PartialEq)]
pub enum TestExpression<T> {
    Item(T),
    AllOf(Vec<Self>),
    AnyOf(Vec<Self>),
    OneOf(Vec<Self>),
    OnlyOneOf(Vec<Self>),
    Use {
        flag: UseFlag,
        negated: bool,
        children: Vec<Self>,
    },
    Not(Box<Self>),
    Forbidden(Box<Self>),
}

/// Asserts the given `expr` against `expected`.
pub fn assert_expr<T: ExpressionItem + Clone + Debug + PartialEq>(
    expr: &ExpressionArena<T>,
    expected: &[TestExpression<T>],
) {
    assert_eq!(collect_children(expr, expr.roots()), expected);
}

/// Creates a `TestExpression` from the given `input`.
pub fn item<T: ExpressionItem>(input: &str) -> TestExpression<T> {
    Item(T::parse(input).unwrap())
}

fn collect_expression<T: ExpressionItem + Clone>(
    expr: &ExpressionArena<T>,
    id: ExpressionId,
) -> TestExpression<T> {
    match expr.get_expression(&id) {
        Expression::Item(item) => Item(item.clone()),
        Expression::AllOf(children) => AllOf(collect_children(expr, expr.get_children(children))),
        Expression::AnyOf(children) => AnyOf(collect_children(expr, expr.get_children(children))),
        Expression::OneOf(children) => OneOf(collect_children(expr, expr.get_children(children))),
        Expression::OnlyOneOf(children) => {
            OnlyOneOf(collect_children(expr, expr.get_children(children)))
        }
        Expression::Use {
            flag,
            negated,
            children,
        } => Use {
            flag: flag.clone(),
            negated: *negated,
            children: collect_children(expr, expr.get_children(children)),
        },
        Expression::Not(child) => Not(collect_expression(expr, *child).into()),
        Expression::Forbidden(child) => Forbidden(collect_expression(expr, *child).into()),
    }
}

fn collect_children<T: ExpressionItem + Clone>(
    expr: &ExpressionArena<T>,
    children: &[ExpressionId],
) -> Vec<TestExpression<T>> {
    children
        .iter()
        .map(|id| collect_expression(expr, *id))
        .collect()
}
