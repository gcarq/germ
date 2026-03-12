use crate::deps::UseFlag;
use crate::deps::expr::ExpressionItem;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Holds a dependency expression over a generic [`ExpressionItem`],
/// which can be an [`Atom`] or [`UseFlag`].
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum Expression<T: ExpressionItem> {
    Item(T),
    Group(Grouped<T>),
    Negation(Box<Expression<T>>),
    Forbidden(Box<Expression<T>>),
}

impl<T: ExpressionItem> fmt::Display for Expression<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expression::Item(atom) => write!(f, "{atom}"),
            Expression::Group(group) => write!(f, "{group}"),
            Expression::Negation(expr) => write!(f, "!{expr}"),
            Expression::Forbidden(expr) => write!(f, "!!{expr}"),
        }
    }
}

/// Holds a grouped dependency expressions, which can be one of the following:
/// - `OneOff`:       `^^ ( ... )` - exactly one of the items must be satisfied
/// - `AllOff`:       `   ( ... )` - all items must be satisfied
/// - `AnyOff`:       `|| ( ... )` - at least one of the items must be satisfied
/// - `AtMostOneOff`: `?? ( ... )` - at most one of the items can be satisfied
/// - `Condition`:    `foo? ( ... )` - the items are only relevant if the USE flag `foo` is enabled
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum Grouped<T: ExpressionItem> {
    OneOff(Box<[Expression<T>]>),
    AllOff(Box<[Expression<T>]>),
    AnyOff(Box<[Expression<T>]>),
    AtMostOneOff(Box<[Expression<T>]>),
    Condition(UseFlag, Box<[Expression<T>]>),
}

impl<T: ExpressionItem> fmt::Display for Grouped<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (prefix, items) = match self {
            Grouped::OneOff(items) => ("^^ ", items),
            Grouped::AllOff(items) => ("", items),
            Grouped::AnyOff(items) => ("|| ", items),
            Grouped::AtMostOneOff(items) => ("?? ", items),
            Grouped::Condition(use_flag, items) => {
                f.write_str(use_flag)?;
                f.write_str("? (")?;
                for item in items {
                    write!(f, " {item}")?;
                }
                return f.write_str(" )");
            }
        };
        write!(f, "{prefix}(")?;
        for item in items {
            write!(f, " {item}")?;
        }
        f.write_str(" )")
    }
}
