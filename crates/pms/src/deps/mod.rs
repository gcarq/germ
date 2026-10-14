mod arena;
mod item;
mod kind;
mod parser;
mod tree;
mod types;

use std::fmt;

use rkyv::{Archive, Deserialize, Serialize};

use self::arena::ExprArena;
pub use self::item::{DepAtom, ExprItem, RequiredUseFlag};
pub use self::kind::{
    BDepend, Depend, ExprKind, IDepend, License, PDepend, Properties, RDepend, RequiredUse,
    Restrict,
};
use self::parser::ExprParser;
pub use self::tree::{Expr, ExprEval, ExprNodes, ExprTree};
pub use self::types::SimpleString;
use crate::MetaVar;

/// Holds a metadata field expression parsed into an expression arena.
/// See PMS 8.2 for the dependency specification format.
#[derive(Archive, Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct ParsedExpr<F: ExprKind> {
    arena: ExprArena<F::Item>,
}

impl<F: ExprKind> ParsedExpr<F> {
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

    /// Calls `visit` on every item in source order, stopping at the first error.
    pub fn try_for_each_item<E>(
        &self,
        visit: impl FnMut(&F::Item) -> Result<(), E>,
    ) -> Result<(), E> {
        self.view().try_for_each_item(visit)
    }

    /// Returns the metadata variable.
    pub const fn var(&self) -> MetaVar {
        F::VAR
    }
}

impl<F: ExprKind> fmt::Display for ParsedExpr<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.view().fmt(f)
    }
}

impl<F: ExprKind> Default for ParsedExpr<F> {
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
    fn test_dep_atom_parse() {
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
            let parsed = input.parse::<DepAtom>().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.inner().to_string(), atom);
        }
    }

    #[test]
    fn test_dep_atom_parse_error() {
        for input in ["!", "!!", "!!!cat/pkg", "!(", "!||", "!^^", "! cat/pkg"] {
            assert!(
                input.parse::<DepAtom>().is_err(),
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
        let expression = ParsedExpr::<Depend>::parse(" \t").unwrap();
        assert_eq!(expression.to_string(), "");
    }

    #[test]
    fn test_parse_kinds() {
        for (input, valid) in [
            ("!cat/pkg", true),
            ("!!cat/pkg", true),
            ("^^ ( cat/pkg cat/other )", false),
        ] {
            let result = ParsedExpr::<Depend>::parse(input);
            assert_eq!(result.is_ok(), valid, "dependency: {input}");
        }

        for (input, valid) in [
            ("^^ ( foo bar )", true),
            ("?? ( foo bar )", true),
            ("!foo", true),
            ("!!foo", false),
        ] {
            let result = ParsedExpr::<RequiredUse>::parse(input);
            assert_eq!(result.is_ok(), valid, "REQUIRED_USE: {input}");
        }

        for (input, valid) in [
            ("mirror test", true),
            ("( mirror test )", true),
            ("mirror? ( fetch )", true),
            ("|| ( mirror fetch )", false),
            ("^^ ( mirror fetch )", false),
            ("?? ( mirror fetch )", false),
        ] {
            let result = ParsedExpr::<Restrict>::parse(input);
            assert_eq!(result.is_ok(), valid, "RESTRICT: {input}");
        }

        for (input, valid) in [
            ("live test_network", true),
            ("( live test_network )", true),
            ("test? ( live )", true),
            ("|| ( live test )", false),
            ("^^ ( live test )", false),
            ("?? ( live test )", false),
        ] {
            let result = ParsedExpr::<Properties>::parse(input);
            assert_eq!(result.is_ok(), valid, "PROPERTIES: {input}");
        }

        for (input, valid) in [
            ("GPL-3", true),
            ("( GPL-3 MIT )", true),
            ("|| ( GPL-3 MIT )", true),
            ("static? ( GPL-3 )", true),
            ("!static? ( GPL-3 )", true),
            ("^^ ( GPL-3 MIT )", false),
            ("?? ( GPL-3 MIT )", false),
            ("@group", false),
            ("-invalid", false),
        ] {
            let result = ParsedExpr::<License>::parse(input);
            assert_eq!(result.is_ok(), valid, "LICENSE: {input}");
        }
    }

    #[test]
    fn test_dep_atom_validate_ok() {
        let cases = [
            "dev-lang/rust",
            "=dev-lang/rust-1.70.0:1.70",
            "dev-lang/rust[foo(-)]",
            "!dev-lang/rust",
            "!!dev-lang/rust",
            "=dev-lang/rust-1*",
        ];

        for input in cases {
            let result = ParsedExpr::<Depend>::parse(input);
            assert!(result.is_ok(), "expected valid dependency: {input}");
        }
    }

    #[test]
    fn test_dep_atom_validate_err() {
        for input in ["*/*", "dev-lang/*", "*/rust", "dev-lang/rust::gentoo"] {
            let result = ParsedExpr::<Depend>::parse(input);
            assert!(result.is_err(), "expected invalid dependency: {input}");
        }
    }

    #[test]
    fn test_slot_operator_placement() {
        assert!(ParsedExpr::<Depend>::parse("dev-lang/rust:=").is_ok());
        assert!(ParsedExpr::<Depend>::parse("|| ( dev-lang/rust:= dev-lang/go )").is_err());
        assert!(ParsedExpr::<PDepend>::parse("dev-lang/rust:=").is_err());
        assert!(ParsedExpr::<PDepend>::parse("dev-lang/rust:0").is_ok());
        assert!(ParsedExpr::<Depend>::parse("dev-lang/rust:0/1=").is_ok());
        assert!(ParsedExpr::<PDepend>::parse("|| ( dev-lang/rust:0/1= dev-lang/go )").is_err());
    }
}
