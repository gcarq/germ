mod lexer;
#[cfg(test)]
mod test_support;

use std::ops::Range;

use anyhow::bail;

use self::lexer::{Lexer, Token};
use super::arena::{ExprArena, ExprEntry, ExprId};
use super::item::ARROW;
use crate::deps::ExprItem;
use crate::useflag::UseFlag;

/// A parser for ebuild dependency expressions commonly found in `DEPEND`, `REQUIRED_USE`, etc..
/// For more information see PMS 8.2.
pub struct ExprParser<'a, T: ExprItem> {
    lexer: Lexer<'a>,
    arena: ExprArena<T>,
    buffer: Vec<ExprId>,
}

#[expect(clippy::needless_pass_by_value)]
impl<'a, T: ExprItem> ExprParser<'a, T> {
    /// Parses the `input` string and constructs an [`ExprArena`].
    ///
    /// Returns `Err` if the input is not a valid expression.
    pub fn parse(input: &'a str) -> anyhow::Result<ExprArena<T>> {
        let mut parser = Self {
            lexer: Lexer::new(input),
            arena: ExprArena::default(),
            buffer: Vec::with_capacity(16),
        };

        parser.parse_root()?;
        Ok(parser.arena)
    }

    /// Parses the whole expression and initializes the arena.
    fn parse_root(&mut self) -> anyhow::Result<()> {
        let base = self.buffer.len();
        while let Some(token) = self.lexer.next() {
            if token == Token::Whitespace {
                continue;
            }

            let child = self.parse_expression(token)?;
            self.buffer.push(child);
            match self.lexer.next() {
                Some(Token::Whitespace) => {}
                Some(token) => bail!("expected whitespace between expressions, got '{token}'"),
                None => break,
            }
        }

        let root = self.commit_children(base)?;
        self.arena.set_roots(root);
        Ok(())
    }

    /// Moves the children collected in `buffer` since `base` into the arena.
    ///
    /// Returns a [`Range`] for future referencing in `arena.children` and
    /// restores `buffer` to its state at `base`.
    fn commit_children(&mut self, base: usize) -> anyhow::Result<Range<u32>> {
        let range = self.arena.push_children(&self.buffer[base..])?;
        self.buffer.truncate(base);
        Ok(range)
    }

    /// Parses an expression based on given [`Token`].
    fn parse_expression(&mut self, token: Token<'a>) -> anyhow::Result<ExprId> {
        let node = match token {
            Token::Ident(ident) => ExprEntry::Item(self.parse_item(ident)?),
            Token::LParen => ExprEntry::AllOf(self.parse_group()?),
            Token::ExactlyOneOf => {
                self.expect_separated(Token::LParen)?;
                ExprEntry::ExactlyOneOf(self.parse_group()?)
            }
            Token::AnyOf => {
                self.expect_separated(Token::LParen)?;
                ExprEntry::AnyOf(self.parse_group()?)
            }
            Token::AtMostOneOf => {
                self.expect_separated(Token::LParen)?;
                ExprEntry::AtMostOneOf(self.parse_group()?)
            }
            Token::UseConditional(flag) => self.parse_use_conditional(flag)?,
            Token::Whitespace | Token::RParen | Token::Illegal(_) => {
                bail!("unexpected token '{token}'")
            }
        };
        self.arena.push_expr(node)
    }

    /// Parses an expression item, consuming a trailing [`lexer::ARROW`]
    /// if the item type supports it.
    fn parse_item(&mut self, ident: &'a str) -> anyhow::Result<T> {
        if T::SUPPORTS_ARROW
            && let Some(target) = self.consume_arrow()?
        {
            return self.lexer.span(ident, target).parse();
        }
        ident.parse()
    }

    /// Consumes a `->` arrow clause and returns its target token.
    ///
    /// Returns `Ok(None)` when the following tokens are not an arrow clause, leaving the
    /// lexer position unchanged. Returns an error when an arrow is present without a target.
    fn consume_arrow(&mut self) -> anyhow::Result<Option<&'a str>> {
        let mut lookahead = self.lexer.clone();
        match (lookahead.next(), lookahead.next()) {
            (Some(Token::Whitespace), Some(Token::Ident(ARROW))) => {
                match (lookahead.next(), lookahead.next()) {
                    (Some(Token::Whitespace), Some(Token::Ident(target))) => {
                        self.lexer = lookahead;
                        Ok(Some(target))
                    }
                    _ => bail!("expected a distfile name after '->'"),
                }
            }
            _ => Ok(None),
        }
    }

    /// Parses a USE conditional expression, e.g. `foo? ( bar )` or `!foo? ( bar )`.
    fn parse_use_conditional(&mut self, flag: &str) -> anyhow::Result<ExprEntry<T>> {
        self.expect_separated(Token::LParen)?;
        let (flag, negated) = match flag.strip_prefix('!') {
            Some(flag) => (flag, true),
            None => (flag, false),
        };
        Ok(ExprEntry::Use {
            flag: UseFlag::new(flag)?,
            negated,
            nodes: self.parse_group()?,
        })
    }

    /// Parses a group of expressions, see [`ExprEntry`].
    ///
    /// This function expects that [`Token::LParen`] has already been consumed.
    /// Returns a [`Range`] that can be used for slicing `expression.children`.
    fn parse_group(&mut self) -> anyhow::Result<Range<u32>> {
        let base = self.buffer.len();
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
        let first = self.parse_expression(first)?;
        self.buffer.push(first);

        loop {
            match self.lexer.next() {
                Some(Token::Whitespace) => match self.lexer.next() {
                    Some(Token::RParen) => {
                        return self.commit_children(base);
                    }
                    Some(token) => {
                        let child = self.parse_expression(token)?;
                        self.buffer.push(child);
                    }
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
    use super::test_support::TestExpression::{AllOf, AnyOf, AtMostOneOf, ExactlyOneOf, Use};
    use super::test_support::{TestExpression, assert_expr, item};
    use super::*;
    use crate::deps::{DepAtom, RequiredUseFlag, SrcUriItem};

    #[test]
    fn test_parser_structure() {
        let cases: [(&str, Vec<TestExpression<DepAtom>>); 8] = [
            (
                "( sys-libs/db app-misc/foo )",
                vec![AllOf(vec![item("sys-libs/db"), item("app-misc/foo")])],
            ),
            (
                "|| ( sys-libs/db app-misc/foo )",
                vec![AnyOf(vec![item("sys-libs/db"), item("app-misc/foo")])],
            ),
            (
                "^^ ( sys-libs/db app-misc/foo )",
                vec![ExactlyOneOf(vec![
                    item("sys-libs/db"),
                    item("app-misc/foo"),
                ])],
            ),
            (
                "?? ( sys-libs/db app-misc/foo )",
                vec![AtMostOneOf(vec![item("sys-libs/db"), item("app-misc/foo")])],
            ),
            (
                "bar? ( sys-libs/db app-misc/foo )",
                vec![Use {
                    flag: "bar".parse().unwrap(),
                    negated: false,
                    nodes: vec![item("sys-libs/db"), item("app-misc/foo")],
                }],
            ),
            (
                "media-libs/mesa[gbm(+)] dev-lang/R",
                vec![item("media-libs/mesa[gbm(+)]"), item("dev-lang/R")],
            ),
            (
                "!foo? ( !app-misc/foo )",
                vec![Use {
                    flag: "foo".parse().unwrap(),
                    negated: true,
                    nodes: vec![item("!app-misc/foo")],
                }],
            ),
            (
                "!!<dev-perl/Mail-Box-3",
                vec![item("!!<dev-perl/Mail-Box-3")],
            ),
        ];

        for (input, expected) in cases {
            let expr = ExprParser::<DepAtom>::parse(input).unwrap();
            assert_expr(expr.view(), &expected);
        }
    }

    #[test]
    fn test_parser_useflags() {
        let input = r"
            || ( wayland X )
            ssh? ( || ( rdp ( vnc X ) ) )
        ";
        let expr = ExprParser::<RequiredUseFlag>::parse(input).unwrap();
        assert_expr(
            expr.view(),
            &[
                AnyOf(vec![item("wayland"), item("X")]),
                Use {
                    flag: "ssh".parse().unwrap(),
                    negated: false,
                    nodes: vec![AnyOf(vec![
                        item("rdp"),
                        AllOf(vec![item("vnc"), item("X")]),
                    ])],
                },
            ],
        );
    }

    #[test]
    fn test_parser_src_uri() {
        let cases: [(&str, Vec<TestExpression<SrcUriItem>>); 4] = [
            (
                "https://example.org/a https://example.org/b",
                vec![item("https://example.org/a"), item("https://example.org/b")],
            ),
            (
                "https://example.org/latest -> a.tar.gz b.tar.gz",
                vec![
                    item("https://example.org/latest -> a.tar.gz"),
                    item("b.tar.gz"),
                ],
            ),
            (
                "examples? ( mirror://gentoo/example.tar.gz )",
                vec![Use {
                    flag: "examples".parse().unwrap(),
                    negated: false,
                    nodes: vec![item("mirror://gentoo/example.tar.gz")],
                }],
            ),
            (
                "https://example.org/a ->file",
                vec![item("https://example.org/a"), item("->file")],
            ),
        ];

        for (input, expected) in cases {
            let expr = ExprParser::<SrcUriItem>::parse(input).unwrap();
            assert_expr(expr.view(), &expected);
        }
    }

    #[test]
    fn test_parser_src_uri_errors() {
        let cases = ["https://example.org/a ->", "https://example.org/a -> ->"];
        for input in cases {
            assert!(
                ExprParser::<SrcUriItem>::parse(input).is_err(),
                "expected invalid source expression: {input}"
            );
        }
    }

    #[test]
    fn test_parser_boundaries() {
        let cases = [
            "||( cat/pkg )",
            "(cat/pkg )",
            "foo?( cat/pkg )",
            "! cat/pkg",
        ];
        for input in cases {
            assert!(
                ExprParser::<DepAtom>::parse(input).is_err(),
                "expected invalid expression: {input}"
            );
        }
    }

    #[test]
    fn test_parser_errors() {
        // (input, expected err)
        let test_data = [
            ("bar? sys-libs/db", "expected '(', got 'sys-libs/db'"),
            ("|| sys-libs/db", "expected '(', got 'sys-libs/db'"),
            (
                "(sys-libs/db",
                "expected whitespace after '(', got 'sys-libs/db'",
            ),
            ("bar? ( sys-libs/db", "unexpected EOF while parsing group"),
            ("()", "empty groups are not supported"),
            ("bar? sys-libs/db )", "expected '(', got 'sys-libs/db'"),
        ];
        for (input, expected_err) in test_data {
            let err = ExprParser::<DepAtom>::parse(input).unwrap_err();
            assert_eq!(err.to_string(), expected_err, "failure for input: {input}");
        }
    }
}
