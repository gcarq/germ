use std::fmt;
use std::fmt::Write;
use std::iter::Peekable;
use std::str::CharIndices;

#[derive(PartialEq, Debug)]
pub enum Token<'a> {
    Whitespace,
    Bang,                    // syntax: !
    OneOf,                   // syntax: ^^
    AnyOf,                   // syntax: ||
    OnlyOneOf,               // syntax: ??
    UseConditional(&'a str), // syntax: foo? - holds a USE flag
    Ident(&'a str),          // holds an atom or USE flag
    LParen,
    RParen,
    Illegal(char),
}

impl<'a> fmt::Display for Token<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Whitespace => f.write_char(' '),
            Token::Bang => f.write_char('!'),
            Token::OneOf => f.write_str("^^"),
            Token::AnyOf => f.write_str("||"),
            Token::OnlyOneOf => f.write_str("??"),
            Token::UseConditional(ident) | Token::Ident(ident) => f.write_str(ident),
            Token::LParen => f.write_char('('),
            Token::RParen => f.write_char(')'),
            Token::Illegal(chr) => f.write_char(*chr),
        }
    }
}

/// A basic lexer that provides a [`Token`] iterator for parsing dependency expressions.
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
    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().map(|(_, char)| *char)
    }

    /// Parses and returns the next token from `self.input`.
    fn consume_token(&mut self) -> Option<Token<'a>> {
        let (start, first) = self.chars.next()?;

        if first.is_whitespace() {
            self.consume_whitespaces();
            return Some(Token::Whitespace);
        }

        let token = match first {
            '!' => Token::Bang,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '^' => self.consume_operator(first, Token::OneOf),
            '|' => self.consume_operator(first, Token::AnyOf),
            '?' => self.consume_operator(first, Token::OnlyOneOf),
            char if Self::is_ident_char(char) => self.consume_ident(start, char),
            char => Token::Illegal(char),
        };

        Some(token)
    }

    fn consume_operator(&mut self, first: char, token: Token<'a>) -> Token<'a> {
        match self.peek_char() {
            Some(char) if char == first => {
                self.chars.next();
                token
            }
            _ => Token::Illegal(first),
        }
    }

    /// Consumes whitespace delimited characters beginning at `start`
    /// to create either [`Token::UseConditional`] or [`Token::Ident`].
    fn consume_ident(&mut self, start: usize, first: char) -> Token<'a> {
        let mut end = start + first.len_utf8();

        while let Some((index, char)) = self.chars.peek().copied() {
            if char == '?' {
                self.chars.next();
                if self
                    .peek_char()
                    .is_some_and(|c| c.is_whitespace() || c == '(')
                {
                    return Token::UseConditional(&self.input[start..end]);
                }
                end = index + char.len_utf8();
                continue;
            }
            if !Self::is_ident_char(char) {
                break;
            }
            self.chars.next();
            end = index + char.len_utf8();
        }
        Token::Ident(&self.input[start..end])
    }

    /// Consumes all following whitespace characters.
    fn consume_whitespaces(&mut self) {
        while self.peek_char().is_some_and(char::is_whitespace) {
            self.chars.next();
        }
    }

    /// Checks if the given character is valid as port of an identifier.
    const fn is_ident_char(char: char) -> bool {
        !char.is_whitespace()
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
        let tokens = lexer
            .filter(|token| !matches!(token, &Token::Whitespace))
            .collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                Token::Ident("sys-libs/db[foo]"),
                Token::UseConditional("bar"),
                Token::LParen,
                Token::Ident("sys-libs/db[baz]"),
                Token::RParen,
                Token::AnyOf,
                Token::LParen,
                Token::Ident("=sys-libs/db-5*:5"),
                Token::Ident("dev-lang/python-exec[python_targets_python3_14(-)]"),
                Token::RParen,
                Token::Bang,
                Token::UseConditional("foo"),
                Token::LParen,
                Token::Bang,
                Token::Ident("app-misc/foo"),
                Token::RParen,
                Token::Bang,
                Token::Bang,
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
        let tokens = lexer
            .filter(|token| !matches!(token, &Token::Whitespace))
            .collect::<Vec<_>>();
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
                Token::UseConditional("ssh"),
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
    fn test_lexer_whitespace() {
        let input = " \tfoo  bar\n";
        let tokens = Lexer::new(input).collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                Token::Whitespace,
                Token::Ident("foo"),
                Token::Whitespace,
                Token::Ident("bar"),
                Token::Whitespace,
            ]
        );
    }

    #[test]
    fn test_lexer_item_url() {
        let input = "https://host/path?query!";
        assert_eq!(
            Lexer::new(input).collect::<Vec<_>>(),
            vec![Token::Ident(input)]
        );
    }

    #[test]
    fn test_lexer_operator_tokens() {
        let tokens = Lexer::new("foo || bar || ( baz )").collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                Token::Ident("foo"),
                Token::Whitespace,
                Token::AnyOf,
                Token::Whitespace,
                Token::Ident("bar"),
                Token::Whitespace,
                Token::AnyOf,
                Token::Whitespace,
                Token::LParen,
                Token::Whitespace,
                Token::Ident("baz"),
                Token::Whitespace,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn test_lexer_bogus_input() {
        let bogus_data = ["| ( )", "^ ( )", "? ( )"];
        for data in bogus_data {
            assert!(
                Lexer::new(data).any(|token| matches!(token, Token::Illegal(_))),
                "expected an illegal token for {data}"
            );
        }
    }
}
