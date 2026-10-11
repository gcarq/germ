mod arena;
mod item;
mod kind;
mod parser;
mod tree;

use std::fmt;

use rkyv::{Archive, Deserialize, Serialize};

use self::arena::ExprArena;
pub use self::item::{AtomDep, ExprItem, RequiredUseFlag};
pub use self::kind::{BDepend, Depend, ExprKind, IDepend, PDepend, RDepend, RequiredUse};
use self::parser::ExprParser;
pub use self::tree::{Expr, ExprEval, ExprNodes, ExprTree};
use crate::MetaVar;

/// Holds a metadata field expression parsed into an expression arena.
/// See PMS 8.2 for the dependency specification format.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct DepExpr<F: ExprKind> {
    arena: ExprArena<F::Item>,
}

impl<F: ExprKind> DepExpr<F> {
    /// Parses `input` as an expression of the field `F`.
    ///
    /// # Errors
    ///
    /// Returns an error when `input` is syntactically invalid or when the parsed expression
    /// violates the field restrictions.
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }

        let arena = ExprParser::<F::Item>::parse(input)?;
        F::validate(arena.view())?;
        Ok(Self { arena })
    }

    /// Returns a view of the expression.
    pub const fn view(&self) -> ExprTree<'_, F::Item> {
        self.arena.view()
    }

    /// Returns the metadata variable.
    pub const fn var(&self) -> MetaVar {
        F::VAR
    }
}

impl<F: ExprKind> fmt::Display for DepExpr<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.view().fmt(f)
    }
}

impl<F: ExprKind> Default for DepExpr<F> {
    fn default() -> Self {
        Self {
            arena: ExprArena::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_dep_parse() {
        for (input, atom) in [
            ("dev-lang/rust", "dev-lang/rust"),
            ("!dev-lang/rust", "dev-lang/rust"),
            ("!!dev-lang/rust", "dev-lang/rust"),
            ("!<dev-perl/Mail-Box-3", "<dev-perl/Mail-Box-3"),
            ("!!=sys-libs/db-5*:5", "=sys-libs/db-5*:5"),
            ("!cat/pkg[foo]", "cat/pkg[foo]"),
            ("!cat/pkg::repo-", "cat/pkg::repo-"),
            ("!cat/pkg:slot", "cat/pkg:slot"),
        ] {
            let parsed = input.parse::<AtomDep>().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.inner().to_string(), atom);
        }
    }

    #[test]
    fn test_atom_dep_parse_error() {
        for input in ["!", "!!", "!!!cat/pkg", "!(", "!||", "!^^", "! cat/pkg"] {
            assert!(
                input.parse::<AtomDep>().is_err(),
                "expected atom dependency error for '{input}'"
            );
        }
    }

    #[test]
    fn test_required_use_flag() {
        for (input, flag, negated) in [("foo", "foo", false), ("!foo", "foo", true)] {
            let parsed = input.parse::<RequiredUseFlag>().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.inner().to_string(), flag);
            assert_eq!(parsed.is_negated(), negated);
        }

        for input in ["", "!", "!!foo", "foo!"] {
            assert!(
                input.parse::<RequiredUseFlag>().is_err(),
                "expected error for '{input}'"
            );
        }
    }

    #[test]
    fn test_parse_empty() {
        let expression = DepExpr::<Depend>::parse(" \t").unwrap();
        assert_eq!(expression.to_string(), "");
    }

    #[test]
    fn test_parse_kinds() {
        for (input, valid) in [
            ("!cat/pkg", true),
            ("!!cat/pkg", true),
            ("^^ ( cat/pkg cat/other )", false),
        ] {
            let result = DepExpr::<Depend>::parse(input);
            assert_eq!(result.is_ok(), valid, "dependency: {input}");
        }

        for (input, valid) in [
            ("^^ ( foo bar )", true),
            ("?? ( foo bar )", true),
            ("!foo", true),
            ("!!foo", false),
        ] {
            let result = DepExpr::<RequiredUse>::parse(input);
            assert_eq!(result.is_ok(), valid, "REQUIRED_USE: {input}");
        }
    }

    #[test]
    fn test_atom_dep_validate_ok() {
        let cases = [
            "dev-lang/rust",
            "=dev-lang/rust-1.70.0:1.70",
            "dev-lang/rust[foo(-)]",
            "!dev-lang/rust",
            "!!dev-lang/rust",
            "=dev-lang/rust-1*",
        ];

        for input in cases {
            let result = DepExpr::<Depend>::parse(input);
            assert!(result.is_ok(), "expected valid dependency: {input}");
        }
    }

    #[test]
    fn test_atom_dep_validate_err() {
        for input in ["*/*", "dev-lang/*", "*/rust", "dev-lang/rust::gentoo"] {
            let result = DepExpr::<Depend>::parse(input);
            assert!(result.is_err(), "expected invalid dependency: {input}");
        }
    }

    #[test]
    fn test_slot_operator_placement() {
        assert!(DepExpr::<Depend>::parse("dev-lang/rust:=").is_ok());
        assert!(DepExpr::<Depend>::parse("|| ( dev-lang/rust:= dev-lang/go )").is_err());
        assert!(DepExpr::<PDepend>::parse("dev-lang/rust:=").is_err());
        assert!(DepExpr::<PDepend>::parse("dev-lang/rust:0").is_ok());
        assert!(DepExpr::<Depend>::parse("dev-lang/rust:0/1=").is_ok());
        assert!(DepExpr::<PDepend>::parse("|| ( dev-lang/rust:0/1= dev-lang/go )").is_err());
    }
}
