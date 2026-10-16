mod arena;
mod item;
mod kind;
mod parser;
mod tree;
mod types;

use std::fmt;

use rkyv::{Archive, Deserialize, Serialize};

use self::arena::ExprArena;
pub use self::item::{DepAtom, DistfileName, ExprItem, RequiredUseFlag, SrcUriItem};
pub use self::kind::{
    BDepend, Depend, ExprKind, Homepage, IDepend, License, PDepend, Properties, RDepend,
    RequiredUse, Restrict, SrcUri,
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
        let cases = [
            ("dev-lang/rust", "dev-lang/rust"),
            ("!dev-lang/rust", "dev-lang/rust"),
            ("!!dev-lang/rust", "dev-lang/rust"),
            ("!<dev-perl/Mail-Box-3", "<dev-perl/Mail-Box-3"),
            ("!!=sys-libs/db-5*:5", "=sys-libs/db-5*:5"),
            ("!cat/pkg[foo]", "cat/pkg[foo]"),
            ("!cat/pkg::repo-", "cat/pkg::repo-"),
            ("!cat/pkg:slot", "cat/pkg:slot"),
        ];
        for (input, atom) in cases {
            let parsed = input.parse::<DepAtom>().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.inner().to_string(), atom);
        }
    }

    #[test]
    fn test_dep_atom_parse_error() {
        let cases = ["!", "!!", "!!!cat/pkg", "!(", "!||", "!^^", "! cat/pkg"];
        for input in cases {
            assert!(
                input.parse::<DepAtom>().is_err(),
                "expected atom dependency error for '{input}'"
            );
        }
    }

    #[test]
    fn test_required_use_flag() {
        let valid = [("foo", "foo", false), ("!foo", "foo", true)];
        for (input, flag, negated) in valid {
            let parsed = input.parse::<RequiredUseFlag>().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.inner().to_string(), flag);
            assert_eq!(parsed.is_negated(), negated);
        }

        let invalid = ["", "!", "!!foo", "foo!"];
        for input in invalid {
            assert!(
                input.parse::<RequiredUseFlag>().is_err(),
                "expected error for '{input}'"
            );
        }
    }

    #[test]
    fn test_parse_kinds() {
        let depend_cases = [
            ("!cat/pkg", true),
            ("!!cat/pkg", true),
            ("^^ ( cat/pkg cat/other )", false),
        ];
        for (input, valid) in depend_cases {
            let result = ParsedExpr::<Depend>::parse(input);
            assert_eq!(result.is_ok(), valid, "dependency: {input}");
        }

        let required_use_cases = [
            ("^^ ( foo bar )", true),
            ("?? ( foo bar )", true),
            ("!foo", true),
            ("!!foo", false),
        ];
        for (input, valid) in required_use_cases {
            let result = ParsedExpr::<RequiredUse>::parse(input);
            assert_eq!(result.is_ok(), valid, "REQUIRED_USE: {input}");
        }

        let restrict_cases = [
            ("mirror test", true),
            ("( mirror test )", true),
            ("mirror? ( fetch )", true),
            ("|| ( mirror fetch )", false),
            ("^^ ( mirror fetch )", false),
            ("?? ( mirror fetch )", false),
        ];
        for (input, valid) in restrict_cases {
            let result = ParsedExpr::<Restrict>::parse(input);
            assert_eq!(result.is_ok(), valid, "RESTRICT: {input}");
        }

        let properties_cases = [
            ("live test_network", true),
            ("( live test_network )", true),
            ("test? ( live )", true),
            ("|| ( live test )", false),
            ("^^ ( live test )", false),
            ("?? ( live test )", false),
        ];
        for (input, valid) in properties_cases {
            let result = ParsedExpr::<Properties>::parse(input);
            assert_eq!(result.is_ok(), valid, "PROPERTIES: {input}");
        }

        let license_cases = [
            ("GPL-3", true),
            ("( GPL-3 MIT )", true),
            ("|| ( GPL-3 MIT )", true),
            ("static? ( GPL-3 )", true),
            ("!static? ( GPL-3 )", true),
            ("^^ ( GPL-3 MIT )", false),
            ("?? ( GPL-3 MIT )", false),
            ("@group", false),
            ("-invalid", false),
        ];
        for (input, valid) in license_cases {
            let result = ParsedExpr::<License>::parse(input);
            assert_eq!(result.is_ok(), valid, "LICENSE: {input}");
        }

        let homepage_cases = [
            ("https://example.org", true),
            ("( https://example.org https://example.org/b )", true),
            ("examples? ( https://example.org )", true),
            ("https://example.org -> file", false),
            ("|| ( https://example.org )", false),
            ("^^ ( https://example.org )", false),
            ("?? ( https://example.org )", false),
        ];
        for (input, valid) in homepage_cases {
            let result = ParsedExpr::<Homepage>::parse(input);
            assert_eq!(result.is_ok(), valid, "HOMEPAGE: {input}");
        }

        let src_uri_cases = [
            ("https://example.org", true),
            ("a.tar.gz", true),
            ("https://example.org/latest -> a.tar.gz", true),
            ("( a.tar.gz https://example.org/b )", true),
            ("examples? ( https://example.org/latest -> a.tar.gz )", true),
            ("|| ( https://example.org )", false),
            ("^^ ( https://example.org )", false),
            ("?? ( https://example.org )", false),
        ];
        for (input, valid) in src_uri_cases {
            let result = ParsedExpr::<SrcUri>::parse(input);
            assert_eq!(result.is_ok(), valid, "SRC_URI: {input}");
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
        let cases = ["*/*", "dev-lang/*", "*/rust", "dev-lang/rust::gentoo"];
        for input in cases {
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
