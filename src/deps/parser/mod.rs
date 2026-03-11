pub mod ast;
mod lexer;

use crate::deps::UseFlag;
use crate::deps::expr::{DepExpression, ExpressionItem};
use crate::deps::parser::ast::Expression;
use crate::deps::parser::ast::Expression::{Forbidden, Group, Item, Negation};
use crate::deps::parser::ast::Grouped::{AnyOff, AtMostOneOff, Condition, OneOff};
use crate::deps::parser::lexer::{Lexer, Token};
use anyhow::{Result, anyhow};
use std::str::FromStr;

/// A parser for ebuild dependency expressions commonly found in `DEPEND`, `REQUIRED_USE`, etc..
/// For more information see PMS 8.2.
pub struct DepExpressionParser<'a> {
    lexer: Lexer<'a>,
}

impl<'a> DepExpressionParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let lexer = Lexer::new(input);
        Self { lexer }
    }

    /// Parses the entire input and constructs a [`DepExpression`].
    pub fn parse<T>(&mut self) -> Result<DepExpression<T>>
    where
        T: ExpressionItem,
    {
        let mut deps = DepExpression::new();
        while let Some(token) = self.lexer.next() {
            let expr = self.parse_expression(token)?;
            deps.push(expr);
        }
        Ok(deps)
    }

    /// Parses an expression based on given `token`.
    fn parse_expression<T>(&mut self, token: Token) -> Result<Expression<T>>
    where
        T: ExpressionItem,
    {
        let expr = match token {
            Token::OneOff => Group(OneOff(self.parse_grouped()?)),
            Token::AnyOff => Group(AnyOff(self.parse_grouped()?)),
            Token::AtMostOneOff => Group(AtMostOneOff(self.parse_grouped()?)),
            Token::Condition(cond) => {
                Group(Condition(UseFlag::from_str(&cond)?, self.parse_grouped()?))
            }
            Token::Bang => {
                let t = self.lexer.next().ok_or_else(|| anyhow!("unexpected EOF"))?;
                Negation(Box::new(self.parse_expression(t)?))
            }
            Token::Ident(ident) => Item(T::parse(&ident)?),
            Token::Forbidden => {
                let t = self.lexer.next().ok_or_else(|| anyhow!("unexpected EOF"))?;
                Forbidden(Box::new(self.parse_expression(t)?))
            }
            Token::LParen | Token::RParen | Token::Illegal(_) => {
                Err(anyhow!("unexpected token {token}"))?
            }
        };
        Ok(expr)
    }

    /// Parses a group of expressions enclosed in parentheses.
    fn parse_grouped<T>(&mut self) -> Result<Box<[Expression<T>]>>
    where
        T: ExpressionItem,
    {
        let token = self.lexer.next().ok_or_else(|| anyhow!("unexpected EOF"))?;
        if token != Token::LParen {
            return Err(anyhow!("expected '(', got {token}"));
        }
        let mut expressions = Vec::new();
        loop {
            match self.lexer.next() {
                Some(Token::RParen) => break,
                Some(t) => expressions.push(self.parse_expression(t)?),
                None => return Err(anyhow!("unexpected EOF while parsing group")),
            }
        }
        Ok(expressions.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::atom::Atom;

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

        let expr = DepExpression::parse(input).unwrap();
        assert_eq!(
            expr.as_slice(),
            &[
                Item(Atom::parse("sys-libs/db").unwrap()),
                Group(Condition(
                    UseFlag::parse("bar").unwrap(),
                    Box::new([Item(Atom::parse("sys-libs/db").unwrap())]),
                )),
                Group(AnyOff(Box::new([
                    Item(Atom::parse("=sys-libs/db-5*:5").unwrap()),
                    Item(Atom::parse("=sys-libs/db-4*:4").unwrap()),
                ]))),
                Negation(Box::new(Group(Condition(
                    UseFlag::parse("foo").unwrap(),
                    Box::new([Negation(Box::new(Item(
                        Atom::parse("app-misc/foo").unwrap(),
                    )))]),
                )))),
                Forbidden(Box::new(Item(Atom::parse("<dev-perl/Mail-Box-3").unwrap()))),
            ]
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
            ^^ ( gnutls openssl )
            ?? ( mysql mariadb )
            ssh? ( || ( rdp vnc ) )
        ";
        let expr = DepExpression::parse(input).unwrap();
        assert_eq!(
            expr.as_slice(),
            &[
                Group(AnyOff(Box::new([
                    Item(UseFlag::parse("wayland").unwrap()),
                    Item(UseFlag::parse("X").unwrap()),
                ]))),
                Group(OneOff(Box::new([
                    Item(UseFlag::parse("gnutls").unwrap()),
                    Item(UseFlag::parse("openssl").unwrap()),
                ]))),
                Group(AtMostOneOff(Box::new([
                    Item(UseFlag::parse("mysql").unwrap()),
                    Item(UseFlag::parse("mariadb").unwrap()),
                ]))),
                Group(Condition(
                    UseFlag::parse("ssh").unwrap(),
                    Box::new([Group(AnyOff(Box::new([
                        Item(UseFlag::parse("rdp").unwrap()),
                        Item(UseFlag::parse("vnc").unwrap()),
                    ])))]),
                )),
            ]
        );
        assert_eq!(
            expr.to_string(),
            "|| ( wayland X ) ^^ ( gnutls openssl ) ?? ( mysql mariadb ) ssh? ( || ( rdp vnc ) )"
        );
    }

    #[test]
    fn test_parser_errors() {
        // (input, expected err)
        let test_data = [
            ("bar? sys-libs/db", "expected '(', got sys-libs/db"),
            ("|| sys-libs/db", "expected '(', got sys-libs/db"),
            ("sys-libs/db)", "unexpected token )"),
            ("(sys-libs/db", "unexpected token ("),
            ("bar? ( sys-libs/db", "unexpected EOF while parsing group"),
            ("bar? sys-libs/db )", "expected '(', got sys-libs/db"),
        ];
        for (input, expected_err) in test_data {
            let err = DepExpression::<Atom>::parse(input).unwrap_err();
            assert_eq!(
                err.chain().map(|e| format!("{e}")).skip(1).next(),
                Some(expected_err.into())
            );
        }
    }
}
