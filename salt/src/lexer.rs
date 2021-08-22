use logos::Logos;

#[derive(Logos, Debug, PartialEq, Copy, Clone)]
pub enum TokenType {
    #[token(r"yeet")]
    Yeet,

    #[regex(r"\s+")]
    Whitespace,

    #[regex(r"[a-zA-Z]+")]
    Word,

    #[error]
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::TokenType::*;

    use logos::Lexer;

    #[derive(Debug, PartialEq, Copy, Clone)]
    struct Token<'a> {
        token_type: TokenType,
        lexeme: &'a str,
    }

    fn assert_tokens(input: &str, tokens: &[Token]) {
        let mut lex: Lexer<TokenType> = TokenType::lexer(input);
        let mut pos: usize = 0;
        for tok in tokens {
            let end_pos: usize = pos + tok.lexeme.len();
            assert_eq!(lex.next(), Some(tok.token_type));
            assert_eq!(lex.span(), pos..end_pos);
            assert_eq!(lex.slice(), tok.lexeme);
            pos = end_pos;
        }
    }

    #[test]
    fn test_simple() {
        let input = "This is a sentence with the word yeet in it";
        let space = Token {token_type: Whitespace, lexeme: " "};
        let expected = [
            Token {token_type: Word, lexeme: "This"},
            space,
            Token {token_type: Word, lexeme: "is"},
            space,
            Token {token_type: Word, lexeme: "a"},
            space,
            Token {token_type: Word, lexeme: "sentence"},
            space,
            Token {token_type: Word, lexeme: "with"},
            space,
            Token {token_type: Word, lexeme: "the"},
            space,
            Token {token_type: Word, lexeme: "word"},
            space,
            Token {token_type: Yeet, lexeme: "yeet"},
            space,
            Token {token_type: Word, lexeme: "in"},
            space,
            Token {token_type: Word, lexeme: "it"},
        ];
        assert_tokens(input, &expected);
    }
}
