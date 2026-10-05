use std::fmt::Debug;

use TestExpression::{AllOf, AnyOf, AtMostOneOf, ExactlyOneOf, Item, Use};

use crate::deps::ExprItem;
use crate::deps::expr::{Expr, ExprNodes, ExprTree};
use crate::useflag::UseFlag;

#[derive(Debug, Eq, PartialEq)]
pub enum TestExpression<T> {
    Item(T),
    AllOf(Vec<Self>),
    AnyOf(Vec<Self>),
    ExactlyOneOf(Vec<Self>),
    AtMostOneOf(Vec<Self>),
    Use {
        flag: UseFlag,
        negated: bool,
        nodes: Vec<Self>,
    },
}

/// Asserts the given `tree` against `expected`.
pub fn assert_expr<T: ExprItem + Debug + PartialEq>(
    tree: ExprTree<'_, T>,
    expected: &[TestExpression<T>],
) {
    assert_nodes(tree.roots(), expected);
}

/// Creates a [`TestExpression`] from the given `input`.
pub fn item<T: ExprItem>(input: &str) -> TestExpression<T> {
    Item(T::parse(input).unwrap())
}

fn assert_expression<T: ExprItem + Debug + PartialEq>(
    actual: Expr<'_, T>,
    expected: &TestExpression<T>,
) {
    match (actual, expected) {
        (Expr::Item(actual), Item(expected)) => assert_eq!(actual, expected),
        (Expr::AllOf(actual), AllOf(expected)) => assert_nodes(actual, expected),
        (Expr::AnyOf(actual), AnyOf(expected)) => assert_nodes(actual, expected),
        (Expr::ExactlyOneOf(actual), ExactlyOneOf(expected)) => {
            assert_nodes(actual, expected);
        }
        (Expr::AtMostOneOf(actual), AtMostOneOf(expected)) => assert_nodes(actual, expected),
        (
            Expr::Use {
                flag: actual_flag,
                negated: actual_negated,
                nodes: actual_nodes,
            },
            Use {
                flag: expected_flag,
                negated: expected_negated,
                nodes: expected_nodes,
            },
        ) => {
            assert_eq!(actual_flag, expected_flag);
            assert_eq!(actual_negated, *expected_negated);
            assert_nodes(actual_nodes, expected_nodes);
        }
        _ => panic!("expression variants differ"),
    }
}

fn assert_nodes<T: ExprItem + Debug + PartialEq>(
    mut actual: ExprNodes<'_, T>,
    expected: &[TestExpression<T>],
) {
    for expected in expected {
        let actual = actual.next().expect("missing expression");
        assert_expression(actual, expected);
    }
    assert!(actual.next().is_none(), "extra expression");
}
