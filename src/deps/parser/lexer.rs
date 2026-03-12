use std::fmt;
use std::fmt::Write;
use std::iter::Peekable;
use std::str::Chars;

#[derive(PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum Token {
    Bang,
    Forbidden,         // syntax: !!
    OneOff,            // syntax: ^^
    AnyOff,            // syntax: ||
    AtMostOneOff,      // syntax: ??
    Condition(String), // syntax: foo? - holds a USE flag
    Ident(String),     // holds an atom or USE flag
    LParen,
    RParen,
    Illegal(char),
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Bang => f.write_char('!'),
            Token::Forbidden => f.write_str("!!"),
            Token::OneOff => f.write_str("^^"),
            Token::AnyOff => f.write_str("||"),
            Token::AtMostOneOff => f.write_str("??"),
            Token::Condition(ident) => write!(f, "{ident}?"),
            Token::Ident(ident) => f.write_str(ident),
            Token::LParen => f.write_char('('),
            Token::RParen => f.write_char(')'),
            Token::Illegal(chr) => f.write_char(*chr),
        }
    }
}

/// A basic lexer to provides [`Token`] iterator for parsing dependency expressions.
pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer with the given `input` string.
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable(),
        }
    }

    /// Parses and returns the next token from `self.input`.
    fn consume_token(&mut self) -> Option<Token> {
        self.skip_whitespaces();

        let cur_char = self.input.next()?;
        let token = match cur_char {
            '!' => match self.input.peek() {
                Some('!') => {
                    self.input.next();
                    Token::Forbidden
                }
                _ => Token::Bang,
            },
            '^' if *self.input.peek()? == '^' => {
                self.input.next();
                Token::OneOff
            }
            '|' if *self.input.peek()? == '|' => {
                self.input.next();
                Token::AnyOff
            }
            '?' if *self.input.peek()? == '?' => {
                self.input.next();
                Token::AtMostOneOff
            }
            '(' => Token::LParen,
            ')' => Token::RParen,
            c if Lexer::is_ident_char(c) => self.consume_identifier(c),
            c => Token::Illegal(c),
        };
        Some(token)
    }

    /// Consumes characters from `self.input` to create either a `Condition` or `Ident`.
    fn consume_identifier(&mut self, cur_char: char) -> Token {
        let mut literal = String::from(cur_char);
        while let Some(char) = self.input.next() {
            if char == '?' && self.input.peek().is_some_and(|c| c.is_whitespace()) {
                return Token::Condition(literal);
            }
            if !Lexer::is_ident_char(char) {
                break;
            }
            literal.push(char);
        }
        Token::Ident(literal)
    }

    /// Checks if the given character is a valid identifier character.
    const fn is_ident_char(char: char) -> bool {
        !char.is_whitespace() && char != '^' && char != '|'
    }

    /// Skips whitespace characters in `self.input`.
    /// Does not consume the character that is not a whitespace.
    fn skip_whitespaces(&mut self) {
        while let Some(char) = self.input.peek() {
            if !char.is_whitespace() {
                break;
            }
            self.input.next();
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

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
                Token::Ident("sys-libs/db[foo]".into()),
                Token::Condition("bar".into()),
                Token::LParen,
                Token::Ident("sys-libs/db[baz]".into()),
                Token::RParen,
                Token::AnyOff,
                Token::LParen,
                Token::Ident("=sys-libs/db-5*:5".into()),
                Token::Ident("dev-lang/python-exec[python_targets_python3_14(-)]".into()),
                Token::RParen,
                Token::Bang,
                Token::Condition("foo".into()),
                Token::LParen,
                Token::Bang,
                Token::Ident("app-misc/foo".into()),
                Token::RParen,
                Token::Forbidden,
                Token::Ident("<dev-perl/Mail-Box-3".into()),
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
                Token::AnyOff,
                Token::LParen,
                Token::Ident("wayland".into()),
                Token::Ident("X".into()),
                Token::RParen,
                Token::OneOff,
                Token::LParen,
                Token::Ident("gnutls".into()),
                Token::Ident("openssl".into()),
                Token::RParen,
                Token::AtMostOneOff,
                Token::LParen,
                Token::Ident("mysql".into()),
                Token::Ident("mariadb".into()),
                Token::RParen,
                Token::Condition("ssh".into()),
                Token::LParen,
                Token::AnyOff,
                Token::LParen,
                Token::Ident("rdp".into()),
                Token::LParen,
                Token::Ident("vnc".into()),
                Token::Ident("X".into()),
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
