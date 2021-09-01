use logos::Logos;

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
    #[regex(r"//[^\n]*")]
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
    pushed_back: Vec<Token<'a>>,
}

impl<'a> Lexer<'a> {
    /// Create a new token stream from the given source.
    pub fn new(source: &'a str) -> Self {
        Self {
            inner: TokenType::lexer(source),
            pushed_back: Vec::with_capacity(1),
        }
    }

    /// Push a token back onto the front of the stream.
    pub fn push_back(&mut self, token: Token<'a>) {
        self.pushed_back.push(token)
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    /// Get the next token.
    fn next(&mut self) -> Option<Self::Item> {
        // Use the pushed back tokens first.
        if !self.pushed_back.is_empty() {
            return self.pushed_back.pop();
        }

        // Otherwise, get from the lexer.
        return match self.inner.next() {
            Some(tt) => {
                Some(Token {
                    tt,
                    span: self.inner.span(),
                    slice: self.inner.slice(),
                })
            },
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use insta::assert_snapshot;
    use std::fmt::Write;

    fn assert_tokens_snapshot(path: &str) {
        let input = std::fs::read_to_string(path).unwrap();
        let mut lexer = Lexer::new(&input);
        let mut output = String::new();
        while let Some(t) = lexer.next() {
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
