use log::trace;
use logos::Logos;
use std::collections::VecDeque;

#[derive(Logos, Debug, PartialEq, Eq, Copy, Clone)]
pub enum TokenType {
    // Keywords
    #[token("const")]
    Const,
    #[token("static")]
    Static,
    #[token("mut")]
    Mut,
    #[token("byte")]
    Byte,
    #[token("half")]
    Half,
    #[token("word")]
    Word,

    // Punctuation
    #[token("[")]
    OpenSquare,
    #[token("]")]
    CloseSquare,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,

    // Literal components
    #[regex(r"-?([0-9]+(e-?[0-9]+)?|0b[01]+|0x[A-Fa-f0-9]+)")]
    IntLiteral,
    #[regex(r"-?[0-9]+\.[0-9]+(e-?[0-9]+)?")]
    FloatLiteral,
    #[regex(r"'(\\[^\n]|[^\n\\'])'")]
    CharLiteral,
    #[regex(r#""(\\[^\n]|[^\n\\"])*""#)]
    StringLiteral,

    // Identifiers
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Identifier,

    // Comments
    #[regex(r"//[^\r\n]*")]
    Comment,

    // Whitespace
    #[regex(r"\r|\n|\r\n")]
    Newline,
    #[regex(r"[^\S\n\r]+")]
    Whitespace,

    // Unrecognised tokens.
    #[error]
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub tt: TokenType,
    pub span: logos::Span,
    pub slice: &'a str,
}

/// Wrap the Logos implementation with some extra buffering.
pub struct Lexer<'a> {
    inner: logos::Lexer<'a, TokenType>,
    buffer: VecDeque<Token<'a>>,
}

impl<'a> Lexer<'a> {
    /// Create a new token stream from the given source.
    pub fn new(source: &'a str) -> Self {
        Self {
            inner: TokenType::lexer(source),
            buffer: VecDeque::with_capacity(1),
        }
    }

    /// Check the type of the next token.
    pub fn peek(&mut self) -> Option<TokenType> {
        if self.buffer.is_empty() {
            self.read_next()
        } else {
            Some(self.buffer.front().unwrap().tt)
        }
    }

    /// Consume the next token.
    pub fn consume(&mut self) -> Option<Token<'a>> {
        if self.buffer.is_empty() {
            self.read_next()?;
        }
        self.buffer.pop_front()
    }

    /// Read the next token from the lexer, place it in the buffer, and
    /// return its type.
    fn read_next(&mut self) -> Option<TokenType> {
        let tt = self.inner.next()?;
        let span = self.inner.span();
        let slice = self.inner.slice();
        let token = Token { tt, span, slice };
        trace!("Lexer produced {:?}", token);
        self.buffer.push_back(token);
        Some(tt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_logging;

    use insta::assert_snapshot;

    fn assert_tokens_snapshot(path: &str) {
        use std::fmt::Write;

        init_test_logging();

        let input = std::fs::read_to_string(path).unwrap();
        let mut lexer = Lexer::new(&input);
        let mut output = String::new();
        while let Some(t) = lexer.consume() {
            if let TokenType::Newline = t.tt {
                writeln!(output, "{:?}", t.tt).unwrap();
            } else {
                writeln!(output, "{:?} `{}`", t.tt, t.slice).unwrap();
            }
        }
        assert_snapshot!(output);
    }

    /// Test the simplest possible input: a single instruction.
    #[test]
    fn test_minimal() {
        assert_tokens_snapshot("examples/minimal.simasm");
    }

    /// Test a small instruction block.
    #[test]
    fn test_instruction_block() {
        assert_tokens_snapshot("examples/instruction-block.simasm");
    }

    /// Test character literals.
    #[test]
    fn test_char_literal() {
        assert_tokens_snapshot("examples/char-literal.simasm");
    }

    /// Test string literals.
    #[test]
    fn test_string_literal() {
        assert_tokens_snapshot("examples/string-literal.simasm");
    }

    /// Test numeric literals
    #[test]
    fn test_numeric_literals() {
        assert_tokens_snapshot("examples/numeric-literals.simasm");
    }

    /// Test comments.
    #[test]
    fn test_comments() {
        assert_tokens_snapshot("examples/comments.simasm");
    }

    /// Test a simple hello world program.
    #[test]
    fn test_hello_world() {
        assert_tokens_snapshot("examples/hello-world.simasm");
    }

    /// Test a program full of invalid tokens.
    #[test]
    fn test_bad_tokens() {
        assert_tokens_snapshot("examples/bad-tokens.simasm");
    }
}
