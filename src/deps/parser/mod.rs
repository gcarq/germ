pub mod arena;
mod lexer;

use crate::deps::ExpressionItem;
use crate::deps::parser::arena::{Expression, ExpressionArena, ExpressionId};
use crate::deps::parser::lexer::{Lexer, Token};
use crate::deps::useflag::UseFlag;
use anyhow::{Result, anyhow};
use std::ops::Range;

/// A parser for ebuild dependency expressions commonly found in `DEPEND`, `REQUIRED_USE`, etc..
/// For more information see PMS 8.2.
pub struct ExpressionParser<'a, T: ExpressionItem> {
    lexer: Lexer<'a>,
    expression: ExpressionArena<T>,
}

impl<'a, T: ExpressionItem> ExpressionParser<'a, T> {
    /// Parses the `input` string and constructs an [`ExpressionArena`].
    ///
    /// Returns `Err` if the input is not a valid expression.
    pub fn parse(input: &'a str) -> Result<ExpressionArena<T>> {
        let mut parser = Self {
            lexer: Lexer::new(input),
            expression: ExpressionArena::new(),
        };

        let mut buffer = Vec::with_capacity(64);
        while let Some(token) = parser.lexer.next() {
            buffer.push(parser.parse_expression(token)?);
        }
        let root = parser.expression.push_children(&buffer);
        parser.expression.set_root(root);
        Ok(parser.expression)
    }

    /// Parses an expression based on given [`Token`].
    fn parse_expression(&mut self, token: Token) -> Result<ExpressionId> {
        let node = match token {
            Token::Ident(ident) => Expression::Item(T::parse(ident)?),
            Token::LParen => Expression::AllOf(self.parse_group()?),
            Token::OneOf => {
                self.expect_next(Token::LParen)?;
                Expression::OneOf(self.parse_group()?)
            }
            Token::AnyOf => {
                self.expect_next(Token::LParen)?;
                Expression::AnyOf(self.parse_group()?)
            }
            Token::OnlyOneOf => {
                self.expect_next(Token::LParen)?;
                Expression::OnlyOneOf(self.parse_group()?)
            }
            Token::Use(flag) => {
                self.expect_next(Token::LParen)?;
                Expression::Use {
                    flag: UseFlag::parse(flag)?,
                    children: self.parse_group()?,
                }
            }
            Token::Bang => {
                let next = self.lexer.next().ok_or_else(|| anyhow!("unexpected EOF"))?;
                Expression::Not(self.parse_expression(next)?)
            }
            Token::Forbidden => match self.lexer.next() {
                Some(Token::Ident(name)) => {
                    Expression::Forbidden(self.parse_expression(Token::Ident(name))?)
                }
                Some(t) => Err(anyhow!("expected identifier, got '{t}'"))?,
                None => Err(anyhow!("expected identifier, got EOF"))?,
            },
            Token::RParen | Token::Illegal(_) => Err(anyhow!("unexpected token '{token}'"))?,
        };
        Ok(self.expression.push_expression(node))
    }

    /// Parses a group of expressions enclosed in parentheses.
    ///
    /// This function expects that the [`Token::LParen`] has already been consumed.
    /// Returns a [`Range`] that can be used for slicing `expression.children`.
    fn parse_group(&mut self) -> Result<Range<u16>> {
        let mut buffer = Vec::with_capacity(16);
        while let Some(token) = self.lexer.next() {
            if token == Token::RParen {
                return Ok(self.expression.push_children(&buffer));
            }
            buffer.push(self.parse_expression(token)?);
        }
        Err(anyhow!("unexpected EOF while parsing group"))
    }

    /// Expects the next [`Token`] to be the given `token`.
    ///
    /// Returns `Err` if the token doesn't match.
    fn expect_next(&mut self, token: Token) -> Result<()> {
        match self.lexer.next() {
            Some(next) if next == token => Ok(()),
            Some(next) => Err(anyhow!("expected '{token}', got '{next}'")),
            None => Err(anyhow!("expected '{token}', got EOF")),
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
    fn test_parser_utf8() {
        let input = "schlüsselwort? ( || ( ä ( ö ü ) ) )";
        let expr = ExpressionParser::<UseFlag>::parse(input).unwrap();
        // assert_eq!(
        //     expr.as_slice(),
        //     &[Item(Atom::parse("schlüsselwort").unwrap())]
        // );
        assert_eq!(expr.to_string(), "schlüsselwort? ( || ( ä ( ö ü ) ) )");
    }

    #[test]
    fn test_parser_errors() {
        // (input, expected err)
        let test_data = [
            ("bar? sys-libs/db", "expected '(', got 'sys-libs/db'"),
            ("|| sys-libs/db", "expected '(', got 'sys-libs/db'"),
            ("sys-libs/db)", "'sys-libs/db)' is not a valid package atom"),
            ("(sys-libs/db", "unexpected EOF while parsing group"),
            ("bar? ( sys-libs/db", "unexpected EOF while parsing group"),
            ("bar? sys-libs/db )", "expected '(', got 'sys-libs/db'"),
            ("!! ( sys-libs/db ) ", "expected identifier, got '('"),
        ];
        for (input, expected_err) in test_data {
            let err = ExpressionParser::<Atom>::parse(input).unwrap_err();
            assert_eq!(err.to_string(), expected_err, "failure for input: {input}");
        }
    }
}
