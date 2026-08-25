pub mod arena;
mod lexer;

use crate::deps::ExpressionItem;
use crate::deps::parser::arena::{Expression, ExpressionArena, ExpressionId};
use crate::deps::parser::lexer::{Lexer, Token};
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
        self.arena.set_root(root);
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

// TODO: add snapshot tests for the parsed arena
#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::atom::Atom;

    #[test]
    fn test_parser_group_one_off() {
        let input = "^^ ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[OneOff(Box::new([
        //         Item(Atom::parse("sys-libs/db").unwrap()),
        //         Item(Atom::parse("app-misc/foo").unwrap()),
        //     ]))]
        // );
        assert_eq!(expr.to_string(), "^^ ( sys-libs/db app-misc/foo )");
    }

    #[test]
    fn test_parser_group_all_off() {
        let input = "( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[AllOff(Box::new([
        //         Item(Atom::parse("sys-libs/db").unwrap()),
        //         Item(Atom::parse("app-misc/foo").unwrap()),
        //     ]))]
        // );
        assert_eq!(expr.to_string(), "( sys-libs/db app-misc/foo )");
    }

    #[test]
    fn test_parser_group_any_off() {
        let input = "|| ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[AnyOff(Box::new([
        //         Item(Atom::parse("sys-libs/db").unwrap()),
        //         Item(Atom::parse("app-misc/foo").unwrap()),
        //     ]))]
        // );
        assert_eq!(expr.to_string(), "|| ( sys-libs/db app-misc/foo )");
    }

    #[test]
    fn test_parser_group_at_most_one_off() {
        let input = "?? ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[AtMostOneOff(Box::new([
        //         Item(Atom::parse("sys-libs/db").unwrap()),
        //         Item(Atom::parse("app-misc/foo").unwrap()),
        //     ]))]
        // );
        assert_eq!(expr.to_string(), "?? ( sys-libs/db app-misc/foo )");
    }

    #[test]
    fn test_parser_group_condition() {
        let input = "bar? ( sys-libs/db app-misc/foo )";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[Condition(
        //         UseFlag::parse("bar").unwrap(),
        //         Box::new([
        //             Item(Atom::parse("sys-libs/db").unwrap()),
        //             Item(Atom::parse("app-misc/foo").unwrap()),
        //         ]),
        //     )]
        // );
        assert_eq!(expr.to_string(), "bar? ( sys-libs/db app-misc/foo )");
    }

    #[test]
    fn test_parser_negation() {
        let input = "!sys-libs/db";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[Negation(Box::new(Item(
        //         Atom::parse("sys-libs/db").unwrap()
        //     )))]
        // );
        assert_eq!(expr.to_string(), "!sys-libs/db");
    }

    #[test]
    fn test_parser_forbidden() {
        let input = "!!sys-libs/db";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[Forbidden(Box::new(Item(
        //         Atom::parse("sys-libs/db").unwrap()
        //     )))]
        // );
        assert_eq!(expr.to_string(), "!!sys-libs/db");
    }

    #[test]
    fn test_parser_item() {
        let input = "media-libs/mesa[gbm(+)] dev-lang/R";
        let expr = ExpressionParser::<Atom>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[
        //         Item(Atom::parse("media-libs/mesa[gbm(+)]").unwrap()),
        //         Item(Atom::parse("dev-lang/R").unwrap()),
        //     ]
        // );
        assert_eq!(expr.to_string(), "media-libs/mesa[gbm(+)] dev-lang/R");
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
        // assert_eq!(
        //     expr.as_slice(),
        //     &[
        //         Item(Atom::parse("sys-libs/db").unwrap()),
        //         Condition(
        //             UseFlag::parse("bar").unwrap(),
        //             Box::new([Item(Atom::parse("sys-libs/db").unwrap())]),
        //         ),
        //         AnyOff(Box::new([
        //             Item(Atom::parse("=sys-libs/db-5*:5").unwrap()),
        //             Item(Atom::parse("=sys-libs/db-4*:4").unwrap()),
        //         ])),
        //         Negation(Box::new(Condition(
        //             UseFlag::parse("foo").unwrap(),
        //             Box::new([Negation(Box::new(Item(
        //                 Atom::parse("app-misc/foo").unwrap(),
        //             )))]),
        //         ))),
        //         Forbidden(Box::new(Item(Atom::parse("<dev-perl/Mail-Box-3").unwrap()))),
        //     ]
        // );
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
        // assert_eq!(
        //     expr.as_slice(),
        //     &[
        //         AnyOff(Box::new([
        //             Item(UseFlag::parse("wayland").unwrap()),
        //             Item(UseFlag::parse("X").unwrap()),
        //         ])),
        //         Condition(
        //             UseFlag::parse("ssh").unwrap(),
        //             Box::new([AnyOff(Box::new([
        //                 Item(UseFlag::parse("rdp").unwrap()),
        //                 AllOff(Box::new([
        //                     Item(UseFlag::parse("vnc").unwrap()),
        //                     Item(UseFlag::parse("X").unwrap()),
        //                 ]))
        //             ]))]),
        //         ),
        //     ]
        // );
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
