//! S-expression tokenizer.

use std::fmt;

/// Token types produced by the lexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Eof,
    LParen,
    RParen,
    Arrow,
    Keyword,
    Symbol,
    Str,
    Number,
    Guard,
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenType::Eof => write!(f, "EOF"),
            TokenType::LParen => write!(f, "("),
            TokenType::RParen => write!(f, ")"),
            TokenType::Arrow => write!(f, "->"),
            TokenType::Keyword => write!(f, "keyword"),
            TokenType::Symbol => write!(f, "symbol"),
            TokenType::Str => write!(f, "string"),
            TokenType::Number => write!(f, "number"),
            TokenType::Guard => write!(f, "guard"),
        }
    }
}

/// A single token from the lexer.
#[derive(Debug, Clone)]
pub struct Token {
    pub typ: TokenType,
    pub literal: String,
    pub pos: usize,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Token({}, {:?}, {})", self.typ, self.literal, self.pos)
    }
}

/// Tokenizes S-expression DSL input.
pub struct Lexer {
    input: Vec<u8>,
    pos: usize,
    read_pos: usize,
    ch: u8,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        let mut l = Self {
            input: input.as_bytes().to_vec(),
            pos: 0,
            read_pos: 0,
            ch: 0,
        };
        l.read_char();
        l
    }

    fn read_char(&mut self) {
        if self.read_pos >= self.input.len() {
            self.ch = 0;
        } else {
            self.ch = self.input[self.read_pos];
        }
        self.pos = self.read_pos;
        self.read_pos += 1;
    }

    fn peek_char(&self) -> u8 {
        if self.read_pos >= self.input.len() {
            0
        } else {
            self.input[self.read_pos]
        }
    }

    fn skip_whitespace(&mut self) {
        while self.ch == b' ' || self.ch == b'\t' || self.ch == b'\n' || self.ch == b'\r' {
            self.read_char();
        }
    }

    fn skip_comment(&mut self) {
        while self.ch != 0 && self.ch != b'\n' {
            self.read_char();
        }
    }

    pub fn next_token(&mut self) -> Token {
        loop {
            self.skip_whitespace();
            if self.ch == b';' {
                self.skip_comment();
                continue;
            }
            break;
        }

        let pos = self.pos;

        match self.ch {
            0 => Token {
                typ: TokenType::Eof,
                literal: String::new(),
                pos,
            },
            b'(' => {
                self.read_char();
                Token {
                    typ: TokenType::LParen,
                    literal: "(".into(),
                    pos,
                }
            }
            b')' => {
                self.read_char();
                Token {
                    typ: TokenType::RParen,
                    literal: ")".into(),
                    pos,
                }
            }
            b'-' => {
                if self.peek_char() == b'>' {
                    self.read_char();
                    self.read_char();
                    Token {
                        typ: TokenType::Arrow,
                        literal: "->".into(),
                        pos,
                    }
                } else if is_digit(self.peek_char()) {
                    self.read_char();
                    let num = self.read_number();
                    Token {
                        typ: TokenType::Number,
                        literal: format!("-{}", num),
                        pos,
                    }
                } else {
                    let sym = self.read_symbol();
                    Token {
                        typ: TokenType::Symbol,
                        literal: sym,
                        pos,
                    }
                }
            }
            b':' => {
                self.read_char();
                let kw = self.read_symbol();
                Token {
                    typ: TokenType::Keyword,
                    literal: format!(":{}", kw),
                    pos,
                }
            }
            b'"' => {
                self.read_char();
                let s = self.read_string(b'"');
                Token {
                    typ: TokenType::Str,
                    literal: s,
                    pos,
                }
            }
            b'{' => {
                self.read_char();
                let g = self.read_guard();
                Token {
                    typ: TokenType::Guard,
                    literal: g,
                    pos,
                }
            }
            ch if is_digit(ch) => {
                let num = self.read_number();
                Token {
                    typ: TokenType::Number,
                    literal: num,
                    pos,
                }
            }
            ch if is_symbol_start(ch) => {
                let sym = self.read_symbol();
                Token {
                    typ: TokenType::Symbol,
                    literal: sym,
                    pos,
                }
            }
            _ => {
                self.read_char();
                Token {
                    typ: TokenType::Eof,
                    literal: String::new(),
                    pos,
                }
            }
        }
    }

    fn read_symbol(&mut self) -> String {
        let start = self.pos;
        while is_symbol_char(self.ch) {
            self.read_char();
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    fn read_number(&mut self) -> String {
        let start = self.pos;
        while is_digit(self.ch) {
            self.read_char();
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    fn read_string(&mut self, quote: u8) -> String {
        let mut result = Vec::new();
        while self.ch != 0 && self.ch != quote {
            if self.ch == b'\\' {
                self.read_char();
                match self.ch {
                    b'n' => result.push(b'\n'),
                    b't' => result.push(b'\t'),
                    b'r' => result.push(b'\r'),
                    b'\\' => result.push(b'\\'),
                    b'"' => result.push(b'"'),
                    other => result.push(other),
                }
            } else {
                result.push(self.ch);
            }
            self.read_char();
        }
        if self.ch == quote {
            self.read_char();
        }
        String::from_utf8_lossy(&result).to_string()
    }

    fn read_guard(&mut self) -> String {
        let mut result = Vec::new();
        let mut depth = 1;
        while self.ch != 0 && depth > 0 {
            if self.ch == b'{' {
                depth += 1;
            } else if self.ch == b'}' {
                depth -= 1;
                if depth == 0 {
                    self.read_char();
                    break;
                }
            }
            result.push(self.ch);
            self.read_char();
        }
        String::from_utf8_lossy(&result).to_string()
    }
}

fn is_symbol_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_symbol_char(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-' || ch == b'[' || ch == b']' || ch == b'.'
}

fn is_digit(ch: u8) -> bool {
    ch.is_ascii_digit()
}

/// Tokenize the input into a list of tokens.
pub fn tokenize(input: &str) -> Vec<Token> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token();
        let is_eof = tok.typ == TokenType::Eof;
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let tokens = tokenize("(schema ERC-020)");
        assert_eq!(tokens[0].typ, TokenType::LParen);
        assert_eq!(tokens[1].typ, TokenType::Symbol);
        assert_eq!(tokens[1].literal, "schema");
        assert_eq!(tokens[2].typ, TokenType::Symbol);
        assert_eq!(tokens[2].literal, "ERC-020");
        assert_eq!(tokens[3].typ, TokenType::RParen);
    }

    #[test]
    fn test_keywords() {
        let tokens = tokenize(":type :guard :keys");
        assert_eq!(tokens[0].typ, TokenType::Keyword);
        assert_eq!(tokens[0].literal, ":type");
        assert_eq!(tokens[1].typ, TokenType::Keyword);
        assert_eq!(tokens[1].literal, ":guard");
    }

    #[test]
    fn test_arrow() {
        let tokens = tokenize("balances -> transfer");
        assert_eq!(tokens[0].typ, TokenType::Symbol);
        assert_eq!(tokens[1].typ, TokenType::Arrow);
        assert_eq!(tokens[2].typ, TokenType::Symbol);
    }

    #[test]
    fn test_guard() {
        let tokens = tokenize("{balances[from] >= amount}");
        assert_eq!(tokens[0].typ, TokenType::Guard);
        assert_eq!(tokens[0].literal, "balances[from] >= amount");
    }

    #[test]
    fn test_numbers() {
        let tokens = tokenize("123 -456");
        assert_eq!(tokens[0].typ, TokenType::Number);
        assert_eq!(tokens[0].literal, "123");
        assert_eq!(tokens[1].typ, TokenType::Number);
        assert_eq!(tokens[1].literal, "-456");
    }

    #[test]
    fn test_comments() {
        let tokens = tokenize("; this is a comment\n(schema test)");
        assert_eq!(tokens[0].typ, TokenType::LParen);
        assert_eq!(tokens[1].literal, "schema");
    }
}
