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
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token("-")]
    Minus,
    #[token(".")]
    Period,

    // Literal components
    #[regex(r"e-?[0-9]+")]
    Exponent,
    #[regex(r"[0-9]+")]
    DecLiteral,
    #[regex(r"0b[01]+")]
    BinLiteral,
    #[regex(r"0x[A-Fa-f0-9]+")]
    HexLiteral,
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

    #[error]
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    use insta::assert_snapshot;
    use std::fmt::Write;

    fn assert_tokens_snapshot(path: &str) {
        let input = std::fs::read_to_string(path).unwrap();
        let mut lexer = TokenType::lexer(&input);
        let mut output = String::new();
        while let Some(t) = lexer.next() {
            if let TokenType::Newline = t {
                writeln!(output, "{:?}", t).unwrap();
            } else {
                writeln!(output, "{:?} `{}`", t, lexer.slice()).unwrap();
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
}
