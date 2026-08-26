pub mod arena;
mod lexer;
#[cfg(test)]
mod test_support;

use self::arena::{Expression, ExpressionArena, ExpressionId};
use self::lexer::{Lexer, Token};
use crate::deps::ExpressionItem;
use crate::useflag::UseFlag;
use anyhow::{anyhow, bail};
use std::ops::Range;

/// A parser for ebuild dependency expressions commonly found in `DEPEND`, `REQUIRED_USE`, etc..
/// For more information see PMS 8.2.
pub struct ExpressionParser<'a, T: ExpressionItem> {
    lexer: Lexer<'a>,
    arena: ExpressionArena<T>,
}

impl<'a, T: ExpressionItem> ExpressionParser<'a, T> {
    /// Parses the `input` string and constructs an [`ExpressionArena`].
    ///
    /// Returns `Err` if the input is not a valid expression.
    pub fn parse(input: &'a str) -> anyhow::Result<ExpressionArena<T>> {
        let mut parser = Self {
            lexer: Lexer::new(input),
            arena: ExpressionArena::default(),
        };

        parser.parse_root()?;
        Ok(parser.arena)
    }

    /// Parses the whole expression and initializes the arena.
    fn parse_root(&mut self) -> anyhow::Result<()> {
        let mut buffer = Vec::with_capacity(64);
        while let Some(token) = self.lexer.next() {
            if token == Token::Whitespace {
                continue;
            }

            buffer.push(self.parse_expression(token)?);
            match self.lexer.next() {
                Some(Token::Whitespace) => {}
                Some(token) => bail!("expected whitespace between expressions, got '{token}'"),
                None => break,
            }
        }

        let root = self.arena.push_children(&buffer)?;
        self.arena.set_roots(root);
        Ok(())
    }

    /// Parses an expression based on given [`Token`].
    fn parse_expression(&mut self, token: Token) -> anyhow::Result<ExpressionId> {
        let node = match token {
            Token::Ident(ident) => Expression::Item(T::parse(ident)?),
            Token::LParen => Expression::AllOf(self.parse_group()?),
            Token::OneOf => {
                self.expect_separated(Token::LParen)?;
                Expression::OneOf(self.parse_group()?)
            }
            Token::AnyOf => {
                self.expect_separated(Token::LParen)?;
                Expression::AnyOf(self.parse_group()?)
            }
            Token::OnlyOneOf => {
                self.expect_separated(Token::LParen)?;
                Expression::OnlyOneOf(self.parse_group()?)
            }
            Token::UseConditional(flag) => self.parse_use_conditional(flag, false)?,
            Token::Bang => match self.lexer.next().ok_or_else(|| anyhow!("unexpected EOF"))? {
                Token::Whitespace => bail!("expected an adjacent operand after '!'"),
                Token::Ident(name) => Expression::Not(self.parse_expression(Token::Ident(name))?),
                Token::UseConditional(flag) => self.parse_use_conditional(flag, true)?,
                Token::Bang => match self.lexer.next() {
                    Some(Token::Ident(name)) => {
                        Expression::Forbidden(self.parse_expression(Token::Ident(name))?)
                    }
                    Some(t) => bail!("expected identifier, got '{t}'"),
                    None => bail!("expected identifier, got EOF"),
                },
                token => {
                    bail!("expected identifier or USE conditional after '!', got '{token}'")
                }
            },
            Token::Whitespace | Token::RParen | Token::Illegal(_) => {
                bail!("unexpected token '{token}'")
            }
        };
        self.arena.push_expression(node)
    }

    /// Parses a USE conditional expression, e.g. `foo? ( bar )` or `!foo? ( bar )`.
    fn parse_use_conditional(
        &mut self,
        flag: &str,
        negated: bool,
    ) -> anyhow::Result<Expression<T>> {
        self.expect_separated(Token::LParen)?;
        Ok(Expression::Use {
            flag: UseFlag::parse(flag)?,
            negated,
            children: self.parse_group()?,
        })
    }

    /// Parses a group of expressions, see [Expression]`.
    ///
    /// This function expects that [`Token::LParen`] has already been consumed.
    /// Returns a [`Range`] that can be used for slicing `expression.children`.
    fn parse_group(&mut self) -> anyhow::Result<Range<u32>> {
        let mut buffer = Vec::with_capacity(16);
        let first = match self.lexer.next() {
            Some(Token::RParen) => bail!("empty groups are not supported"),
            Some(Token::Whitespace) => match self.lexer.next() {
                Some(Token::RParen) => bail!("empty groups are not supported"),
                Some(token) => token,
                None => bail!("unexpected EOF while parsing group"),
            },
            Some(token) => bail!("expected whitespace after '(', got '{token}'"),
            None => bail!("unexpected EOF while parsing group"),
        };
        buffer.push(self.parse_expression(first)?);

        loop {
            match self.lexer.next() {
                Some(Token::Whitespace) => match self.lexer.next() {
                    Some(Token::RParen) => {
                        return self.arena.push_children(&buffer);
                    }
                    Some(token) => buffer.push(self.parse_expression(token)?),
                    None => bail!("unexpected EOF while parsing group"),
                },
                Some(Token::RParen) => {
                    bail!("expected whitespace before ')', got ')'")
                }
                Some(token) => {
                    bail!("expected whitespace between group items, got '{token}'")
                }
                None => bail!("unexpected EOF while parsing group"),
            }
        }
    }

    /// Expects the next [`Token`] to be the given `token`.
    ///
    /// Returns `Err` if the token doesn't match.
    fn expect_next(&mut self, token: Token) -> anyhow::Result<()> {
        match self.lexer.next() {
            Some(next) if next == token => Ok(()),
            Some(next) => bail!("expected '{token}', got '{next}'"),
            None => bail!("expected '{token}', got EOF"),
        }
    }

    /// Expects the next token to be separated from the previous token by whitespace.
    fn expect_separated(&mut self, token: Token) -> anyhow::Result<()> {
        match self.lexer.next() {
            Some(Token::Whitespace) => self.expect_next(token),
            Some(next) => bail!("expected whitespace before '{token}', got '{next}'"),
            None => bail!("expected whitespace before '{token}', got EOF"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TestExpression::{
        AllOf, AnyOf, Forbidden, Not, OneOf, OnlyOneOf, Use,
    };
    use super::test_support::{assert_expr, item};
    use super::*;
    use crate::deps::atom::Atom;

    #[test]
    fn test_parser_group_one_of() {
        let input = "^^ ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[OneOf(vec![item("sys-libs/db"), item("app-misc/foo")])],
        );
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_parser_group_all_of() {
        let input = "( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[AllOf(vec![item("sys-libs/db"), item("app-misc/foo")])],
        );
        assert_eq!(expr.to_string(), "( sys-libs/db app-misc/foo )");
    }

    #[test]
    fn test_parser_group_any_of() {
        let input = "|| ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[AnyOf(vec![item("sys-libs/db"), item("app-misc/foo")])],
        );
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_parser_group_only_one_of() {
        let input = "?? ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[OnlyOneOf(vec![item("sys-libs/db"), item("app-misc/foo")])],
        );
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_parser_use_conditional() {
        let input = "bar? ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[Use {
                flag: "bar".parse().unwrap(),
                negated: false,
                children: vec![item("sys-libs/db"), item("app-misc/foo")],
            }],
        );
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_parser_negation() {
        let input = "!sys-libs/db";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(&expr, &[Not(item("sys-libs/db").into())]);
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_parser_forbidden() {
        let input = "!!sys-libs/db";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(&expr, &[Forbidden(item("sys-libs/db").into())]);
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_parser_item() {
        let input = "media-libs/mesa[gbm(+)] dev-lang/R";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[item("media-libs/mesa[gbm(+)]"), item("dev-lang/R")],
        );
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_parser_atoms() {
        let input = r"
            sys-libs/db
            bar? ( sys-libs/db )
            || (
                =sys-libs/db-5*:5
                =sys-libs/db-4*:4
            )
            !foo? ( !app-misc/foo )
            !!<dev-perl/Mail-Box-3
        ";

        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[
                item("sys-libs/db"),
                Use {
                    flag: "bar".parse().unwrap(),
                    negated: false,
                    children: vec![item("sys-libs/db")],
                },
                AnyOf(vec![item("=sys-libs/db-5*:5"), item("=sys-libs/db-4*:4")]),
                Use {
                    flag: "foo".parse().unwrap(),
                    negated: true,
                    children: vec![Not(item("app-misc/foo").into())],
                },
                Forbidden(item("<dev-perl/Mail-Box-3").into()),
            ],
        );
        assert_eq!(
            expr.to_string(),
            "sys-libs/db bar? ( sys-libs/db ) || ( =sys-libs/db-5*:5 =sys-libs/db-4*:4 ) !foo? ( !app-misc/foo ) !!<dev-perl/Mail-Box-3"
        );
    }

    #[test]
    fn test_parser_use_flags() {
        let input = r"
            || ( wayland X )
            ssh? ( || ( rdp ( vnc X ) ) )
        ";
        let expr = ExpressionParser::<UseFlag>::parse(input).unwrap();
        assert_expr(
            &expr,
            &[
                AnyOf(vec![item("wayland"), item("X")]),
                Use {
                    flag: "ssh".parse().unwrap(),
                    negated: false,
                    children: vec![AnyOf(vec![
                        item("rdp"),
                        AllOf(vec![item("vnc"), item("X")]),
                    ])],
                },
            ],
        );
        assert_eq!(
            expr.to_string(),
            "|| ( wayland X ) ssh? ( || ( rdp ( vnc X ) ) )"
        );
    }

    #[test]
    fn test_parser_boundaries() {
        for input in [
            "cat/pkg app-misc/foo",
            "( cat/pkg )",
            "|| ( cat/pkg )",
            "foo? ( cat/pkg )",
            "!foo? ( cat/pkg )",
            "!cat/pkg",
            "!!cat/pkg",
        ] {
            assert!(
                ExpressionParser::<Atom>::parse(input).is_ok(),
                "expected valid expression: {input}"
            );
        }

        for input in [
            "||( cat/pkg )",
            "foo||bar",
            "foo ||bar",
            "foo|| bar",
            "(cat/pkg )",
            "( cat/pkg)",
            "foo?( cat/pkg )",
            "cat/pkg(app-misc/foo)",
            "! cat/pkg",
            "!! cat/pkg",
        ] {
            assert!(
                ExpressionParser::<Atom>::parse(input).is_err(),
                "expected invalid expression: {input}"
            );
        }
    }

    #[test]
    fn test_parser_whitespace_only() {
        let expression = ExpressionParser::<Atom>::parse(" \t\n").unwrap();
        assert_eq!(expression.to_string(), "");
    }

    #[test]
    fn test_parser_errors() {
        // (input, expected err)
        let test_data = [
            ("bar? sys-libs/db", "expected '(', got 'sys-libs/db'"),
            ("|| sys-libs/db", "expected '(', got 'sys-libs/db'"),
            ("sys-libs/db)", "'sys-libs/db)' is not a valid atom"),
            (
                "(sys-libs/db",
                "expected whitespace after '(', got 'sys-libs/db'",
            ),
            ("bar? ( sys-libs/db", "unexpected EOF while parsing group"),
            ("()", "empty groups are not supported"),
            ("bar? sys-libs/db )", "expected '(', got 'sys-libs/db'"),
            ("!! ( sys-libs/db ) ", "expected identifier, got ' '"),
            (
                "!( sys-libs/db )",
                "expected identifier or USE conditional after '!', got '('",
            ),
            (
                "!|| ( sys-libs/db )",
                "expected identifier or USE conditional after '!', got '||'",
            ),
            (
                "!^^ ( sys-libs/db )",
                "expected identifier or USE conditional after '!', got '^^'",
            ),
            (
                "!?? ( sys-libs/db )",
                "expected identifier or USE conditional after '!', got '??'",
            ),
            ("! cat/pkg", "expected an adjacent operand after '!'"),
            ("!", "unexpected EOF"),
            ("!!foo? ( cat/pkg )", "expected identifier, got 'foo'"),
        ];
        for (input, expected_err) in test_data {
            let err = ExpressionParser::<Atom>::parse(input).unwrap_err();
            assert_eq!(err.to_string(), expected_err, "failure for input: {input}");
        }
    }
}
