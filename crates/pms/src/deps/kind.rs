use anyhow::bail;
use rkyv::{Archive, Deserialize, Serialize};

use super::item::{DepAtom, ExprItem, RequiredUseFlag};
use super::tree::{Expr, ExprNodes, ExprTree};
use crate::MetaVar;

/// Pins the item type and allowed expression syntax of one metadata field.
/// See PMS 8.2 for the dependency specification formats.
pub trait ExprKind {
    /// The value type of a single expression item.
    type Item: ExprItem;

    /// The metadata variable this kind models.
    const VAR: MetaVar;

    /// Validates the given [`Expr`] in the context of [`Self::VAR`];
    /// `in_any_of` indicates whether the expression is inside an any-of group.
    ///
    /// Returns an `Err` if the expression is not valid.
    ///
    /// NOTE: The implementation should be called from [`Self::validate`] and
    /// the implementation should just assert the given [`Expr`] and not recurse further.
    fn validate_expr(expr: &Expr<'_, Self::Item>, in_any_of: bool) -> anyhow::Result<()>;

    /// Validates the given [`ExprTree`] item syntax and field specific rules.
    ///
    /// Returns an `Err` if the expression violates the field rules.
    fn validate(tree: ExprTree<'_, Self::Item>) -> anyhow::Result<()> {
        validate_nodes::<Self>(tree.roots(), false)
    }
}

/// The `DEPEND` metadata field.
#[derive(Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]
pub struct Depend;

impl ExprKind for Depend {
    type Item = DepAtom;
    const VAR: MetaVar = MetaVar::Depend;

    fn validate_expr(expr: &Expr<'_, Self::Item>, in_any_of: bool) -> anyhow::Result<()> {
        match expr {
            Expr::Item(atom) if in_any_of && atom.inner().has_rebuild_slot() => {
                bail!("equals slot operators are not allowed in {}", Self::VAR);
            }
            Expr::ExactlyOneOf(_) | Expr::AtMostOneOf(_) => {
                bail!("{} is not valid in {}", expr.name(), Self::VAR);
            }
            _ => Ok(()),
        }
    }
}

/// The `BDEPEND` metadata field.
#[derive(Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]
pub struct BDepend;

impl ExprKind for BDepend {
    type Item = DepAtom;
    const VAR: MetaVar = MetaVar::BDepend;

    fn validate_expr(expr: &Expr<'_, Self::Item>, in_any_of: bool) -> anyhow::Result<()> {
        match expr {
            Expr::Item(atom) if in_any_of && atom.inner().has_rebuild_slot() => {
                bail!("equals slot operators are not allowed in {}", Self::VAR);
            }
            Expr::ExactlyOneOf(_) | Expr::AtMostOneOf(_) => {
                bail!("{} is not valid in {}", expr.name(), Self::VAR);
            }
            _ => Ok(()),
        }
    }
}

/// The `IDEPEND` metadata field.
#[derive(Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]
pub struct IDepend;

impl ExprKind for IDepend {
    type Item = DepAtom;
    const VAR: MetaVar = MetaVar::IDepend;

    fn validate_expr(expr: &Expr<'_, Self::Item>, in_any_of: bool) -> anyhow::Result<()> {
        match expr {
            Expr::Item(atom) if in_any_of && atom.inner().has_rebuild_slot() => {
                bail!("equals slot operator not allowed in {}", Self::VAR);
            }
            Expr::ExactlyOneOf(_) | Expr::AtMostOneOf(_) => {
                bail!("{} is not valid in {}", expr.name(), Self::VAR);
            }
            _ => Ok(()),
        }
    }
}

/// The `RDEPEND` metadata field.
#[derive(Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]
pub struct RDepend;

impl ExprKind for RDepend {
    type Item = DepAtom;
    const VAR: MetaVar = MetaVar::RDepend;

    fn validate_expr(expr: &Expr<'_, Self::Item>, in_any_of: bool) -> anyhow::Result<()> {
        match expr {
            Expr::Item(atom) if in_any_of && atom.inner().has_rebuild_slot() => {
                bail!("equals slot operator not allowed in {}", Self::VAR);
            }
            Expr::ExactlyOneOf(_) | Expr::AtMostOneOf(_) => {
                bail!("{} is not valid in {}", expr.name(), Self::VAR);
            }
            _ => Ok(()),
        }
    }
}

/// The `PDEPEND` metadata field.
#[derive(Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]
pub struct PDepend;

impl ExprKind for PDepend {
    type Item = DepAtom;
    const VAR: MetaVar = MetaVar::PDepend;

    fn validate_expr(expr: &Expr<'_, Self::Item>, _in_any_of: bool) -> anyhow::Result<()> {
        match expr {
            Expr::Item(atom) if atom.inner().has_rebuild_slot() => {
                bail!("equals slot operator not allowed in {}", Self::VAR);
            }
            Expr::ExactlyOneOf(_) | Expr::AtMostOneOf(_) => {
                bail!("{} is not valid in {}", expr.name(), Self::VAR);
            }
            _ => Ok(()),
        }
    }
}

/// The `REQUIRED_USE` metadata field.
#[derive(Archive, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Debug)]
pub struct RequiredUse;

impl ExprKind for RequiredUse {
    type Item = RequiredUseFlag;
    const VAR: MetaVar = MetaVar::RequiredUse;

    fn validate_expr(_expr: &Expr<'_, Self::Item>, _in_any_of: bool) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Validates each `node` against the field `F`, recursing into groups.
///
/// Equals slot operators (`:=`, `:SLOT=`) are invalid inside an any-of group and, when rejected
/// for every node (i.e. for `PDEPEND`), anywhere in the expression. See PMS 8.3.3.
fn validate_nodes<K: ExprKind + ?Sized>(
    nodes: ExprNodes<'_, K::Item>,
    in_any_of: bool,
) -> anyhow::Result<()> {
    for expr in nodes {
        K::validate_expr(&expr, in_any_of)?;
        match expr {
            Expr::Item(item) => item.validate()?,
            Expr::AnyOf(nodes) => validate_nodes::<K>(nodes, true)?,
            Expr::AllOf(nodes)
            | Expr::ExactlyOneOf(nodes)
            | Expr::AtMostOneOf(nodes)
            | Expr::Use { nodes, .. } => validate_nodes::<K>(nodes, in_any_of)?,
        }
    }
    Ok(())
}
