use std::fmt::Debug;

use crate::deps::ExpressionItem;
use crate::deps::expression::{Expression, ExpressionChildren, ExpressionTree};
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

/// Asserts the given `tree` against `expected`.
pub fn assert_expr<T: ExpressionItem + Debug + PartialEq>(
    tree: ExpressionTree<'_, T>,
    expected: &[TestExpression<T>],
) {
    assert_children(tree.roots(), expected);
}

/// Creates a [`TestExpression`] from the given `input`.
pub fn item<T: ExpressionItem>(input: &str) -> TestExpression<T> {
    Item(T::parse(input).unwrap())
}

fn assert_expression<T: ExpressionItem + Debug + PartialEq>(
    actual: Expression<'_, T>,
    expected: &TestExpression<T>,
) {
    match (actual, expected) {
        (Expression::Item(actual), Item(expected)) => assert_eq!(actual, expected),
        (Expression::AllOf(actual), AllOf(expected)) => assert_children(actual, expected),
        (Expression::AnyOf(actual), AnyOf(expected)) => assert_children(actual, expected),
        (Expression::OneOf(actual), OneOf(expected)) => assert_children(actual, expected),
        (Expression::OnlyOneOf(actual), OnlyOneOf(expected)) => assert_children(actual, expected),
        (
            Expression::Use {
                flag: actual_flag,
                negated: actual_negated,
                children: actual_children,
            },
            Use {
                flag: expected_flag,
                negated: expected_negated,
                children: expected_children,
            },
        ) => {
            assert_eq!(actual_flag, expected_flag);
            assert_eq!(actual_negated, *expected_negated);
            assert_children(actual_children, expected_children);
        }
        (Expression::Not(actual), Not(expected)) => {
            assert_expression(actual.expression(), expected);
        }
        (Expression::Forbidden(actual), Forbidden(expected)) => {
            assert_expression(actual.expression(), expected);
        }
        _ => panic!("expression variants differ"),
    }
}

fn assert_children<T: ExpressionItem + Debug + PartialEq>(
    mut actual: ExpressionChildren<'_, T>,
    expected: &[TestExpression<T>],
) {
    for expected in expected {
        let actual = actual.next().expect("missing expression");
        assert_expression(actual.expression(), expected);
    }
    assert!(actual.next().is_none(), "extra expression");
}
