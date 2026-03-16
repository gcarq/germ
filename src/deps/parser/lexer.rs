use std::fmt;
use std::fmt::Write;
use std::iter::Peekable;
use std::str::CharIndices;

#[derive(PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum Token<'a> {
    Bang,
    Forbidden,      // syntax: !!
    OneOf,          // syntax: ^^
    AnyOf,          // syntax: ||
    OnlyOneOf,      // syntax: ??
    Use(&'a str),   // syntax: foo? - holds a USE flag
    Ident(&'a str), // holds an atom or USE flag
    LParen,
    RParen,
    Illegal(char),
}

impl<'a> fmt::Display for Token<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Bang => f.write_char('!'),
            Token::Forbidden => f.write_str("!!"),
            Token::OneOf => f.write_str("^^"),
            Token::AnyOf => f.write_str("||"),
            Token::OnlyOneOf => f.write_str("??"),
            Token::Use(ident) | Token::Ident(ident) => f.write_str(ident),
            Token::LParen => f.write_char('('),
            Token::RParen => f.write_char(')'),
            Token::Illegal(chr) => f.write_char(*chr),
        }
    }
}

/// A basic lexer to provides [`Token`] iterator for parsing dependency expressions.
pub struct Lexer<'a> {
    input: &'a str,
    chars: Peekable<CharIndices<'a>>,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer with the given `input` string.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.char_indices().peekable(),
        }
    }

    /// Returns the next char without consuming it.
    fn peek_char(&mut self) -> Option<&char> {
        self.chars.peek().map(|(_, char)| char)
    }

    /// Parses and returns the next token from `self.input`.
    fn consume_token(&mut self) -> Option<Token<'a>> {
        self.skip_whitespaces();

        let (start, char) = self.chars.next()?;
        let token = match char {
            '!' => match self.peek_char() {
                Some('!') => {
                    self.chars.next();
                    Token::Forbidden
                }
                _ => Token::Bang,
            },
            '^' if *self.peek_char()? == '^' => {
                self.chars.next();
                Token::OneOf
            }
            '|' if *self.peek_char()? == '|' => {
                self.chars.next();
                Token::AnyOf
            }
            '?' if *self.peek_char()? == '?' => {
                self.chars.next();
                Token::OnlyOneOf
            }
            '(' => Token::LParen,
            ')' => Token::RParen,
            c if Lexer::is_ident_char(c) => self.consume_identifier(start, c),
            c => Token::Illegal(c),
        };
        Some(token)
    }

    /// Consumes characters beginning at position `start` to create
    /// either [`Token::Condition`] or [`Token::Ident`].
    ///
    /// The `first` character needs to be passed to calculate the correct utf-8 offset.
    fn consume_identifier(&mut self, start: usize, first: char) -> Token<'a> {
        let mut end = start + first.len_utf8();
        while let Some((index, char)) = self.chars.next() {
            if char == '?' && self.peek_char().is_some_and(|c| c.is_whitespace()) {
                return Token::Use(&self.input[start..end]);
            }
            if !Lexer::is_ident_char(char) {
                break;
            }
            end = index + char.len_utf8();
        }
        Token::Ident(&self.input[start..end])
    }

    /// Checks if the given character is a valid identifier character.
    const fn is_ident_char(char: char) -> bool {
        !char.is_whitespace() && char != '^' && char != '|'
    }

    /// Skips whitespace characters in `self.input`.
    /// Does not consume the character that is not a whitespace.
    fn skip_whitespaces(&mut self) {
        while let Some(char) = self.peek_char() {
            if !char.is_whitespace() {
                break;
            }
            self.chars.next();
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.consume_token()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_depend_syntax() {
        let input = r"
            sys-libs/db[foo]
            bar? ( sys-libs/db[baz] )
            || (
                =sys-libs/db-5*:5
                dev-lang/python-exec[python_targets_python3_14(-)]
            )
            !foo? ( !app-misc/foo )
            !!<dev-perl/Mail-Box-3
        ";
        let lexer = Lexer::new(input);
        let tokens = lexer.collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                Token::Ident("sys-libs/db[foo]"),
                Token::Use("bar"),
                Token::LParen,
                Token::Ident("sys-libs/db[baz]"),
                Token::RParen,
                Token::AnyOf,
                Token::LParen,
                Token::Ident("=sys-libs/db-5*:5"),
                Token::Ident("dev-lang/python-exec[python_targets_python3_14(-)]"),
                Token::RParen,
                Token::Bang,
                Token::Use("foo"),
                Token::LParen,
                Token::Bang,
                Token::Ident("app-misc/foo"),
                Token::RParen,
                Token::Forbidden,
                Token::Ident("<dev-perl/Mail-Box-3"),
            ]
        );
    }

    #[test]
    fn test_lexer_required_use_syntax() {
        let input = r"
            || ( wayland X )
            ^^ ( gnutls openssl )
            ?? ( mysql mariadb )
            ssh? ( || ( rdp ( vnc X ) ) )
        ";
        let lexer = Lexer::new(input);
        let tokens = lexer.collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                Token::AnyOf,
                Token::LParen,
                Token::Ident("wayland"),
                Token::Ident("X"),
                Token::RParen,
                Token::OneOf,
                Token::LParen,
                Token::Ident("gnutls"),
                Token::Ident("openssl"),
                Token::RParen,
                Token::OnlyOneOf,
                Token::LParen,
                Token::Ident("mysql"),
                Token::Ident("mariadb"),
                Token::RParen,
                Token::Use("ssh"),
                Token::LParen,
                Token::AnyOf,
                Token::LParen,
                Token::Ident("rdp"),
                Token::LParen,
                Token::Ident("vnc"),
                Token::Ident("X"),
                Token::RParen,
                Token::RParen,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn test_lexer_bogus_input() {
        let bogus_data = ["| ( )", "^ ( )", "? ( )", "foo|"];
        for data in bogus_data {
            let mut lexer = Lexer::new(data);
            lexer.any(|token| matches!(token, Token::Illegal(_)));
        }
    }
}
