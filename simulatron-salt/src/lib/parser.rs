mod node_builder;

use log::{debug, info, error};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::ops::Range;

use crate::error::SaltError;
use crate::language::{SyntaxKind::{self, *}, SyntaxNode};
use crate::lexer::{Lexer, Token, TokenType};
use node_builder::{NodeGuard, SafeNodeBuilder};

/// A failure due to token mismatch or EOF.
enum Failure {
    WrongToken,
    EOF,
}
type ParseResult<'a, T> = Result<T, Failure>;

/// Return codes for sequences that may terminate.
enum SequenceResult {
    GoAgain,
    GracefulEnd,
}

/// A mostly LL(1) recursive descent parser for SimAsm, with bits of iterative
/// Pratt-like parsing to deal with left recursion, and the occasional LL(2)
/// lookahead.
pub struct Parser<'a> {
    builder: SafeNodeBuilder,
    tokens: Lexer<'a>,
    buffer: VecDeque<Token<'a>>,
    last_span: Range<usize>,
    errors: Vec<SaltError>,
}

/// A non-destructive iterator over a parser's token stream, yielding token types.
struct TokenTypeIter<'a, 'b> {
    parser: &'b mut Parser<'a>,
    pos: usize,
}

impl<'a, 'b> Iterator for TokenTypeIter<'a, 'b> {
    type Item = TokenType;

    fn next(&mut self) -> Option<Self::Item> {
        // Ensure the buffer is full enough.
        for _ in self.parser.buffer.len()..(self.pos+1) {
            let token = self.parser.tokens.next()?;
            self.parser.buffer.push_back(token);
        }
        // Return the next token type.
        let ret = Some(self.parser.buffer.get(self.pos).unwrap().tt);
        self.pos += 1;
        ret
    }
}

impl<'a> Parser<'a> {
    /// Construct a new parser from the given token stream.
    pub fn new(tokens: Lexer<'a>) -> Self {
        Self {
            builder: SafeNodeBuilder::new(),
            tokens,
            buffer: VecDeque::with_capacity(8),
            last_span: 0..0,
            errors: Vec::new(),
        }
    }

    /// Run the parser, producing either a SyntaxNode tree or a vector of errors.
    pub fn run(mut self) -> Result<SyntaxNode, Vec<SaltError>> {
        self.parse_program();

        if self.errors.is_empty() {
            Ok(SyntaxNode::new_root(self.builder.finish()))
        } else {
            Err(self.errors)
        }
    }

    /// Wrapper for `builder.start_node`.
    fn start_node(&mut self, kind: SyntaxKind) -> NodeGuard {
        self.builder.start_node(kind)
    }

    /// Wrapper for `builder.add_token`.
    fn add_token(&mut self, t: Token) {
        self.builder.add_token(t)
    }

    /// Iterate non-destructively through the token stream.
    fn iter_token_types<'b>(&'b mut self) -> TokenTypeIter<'a, 'b> {
        TokenTypeIter {
            parser: self,
            pos: 0,
        }
    }

    /// Peek at the type of the next non-whitespace token.
    fn peek(&mut self) -> ParseResult<TokenType> {
        for tt in self.iter_token_types() {
            if tt != TokenType::Whitespace {
                return Ok(tt);
            }
        }
        Err(Failure::EOF)
    }

    /// Double lookahead, skipping whitespace.
    fn double_lookahead(&mut self) -> ParseResult<TokenType> {
        let mut seen: usize = 0;
        for tt in self.iter_token_types() {
            if tt != TokenType::Whitespace {
                seen += 1;
                if seen == 2 {
                    return Ok(tt);
                }
            }
        }
        Err(Failure::EOF)
    }

    /// Consume the next non-whitespace token and all whitespace before it,
    /// adding them to the current position.
    fn consume(&mut self) -> ParseResult<()> {
        // Helper function for DRY.
        fn eat(self_: &mut Parser) -> TokenType {
            let token = self_.buffer.pop_front().unwrap();
            debug!("Consuming {:?}", token);
            let tt = token.tt;
            self_.last_span = token.span.clone();
            self_.add_token(token);
            tt
        }

        // Ensure the buffer has a non-whitespace item.
        match self.peek() {
            Ok(_) => {
                // Eat up to and including the first non-whitespace token.
                loop {
                    let tt = eat(self);
                    if tt != TokenType::Whitespace {
                        return Ok(());
                    }
                }
            },
            Err(Failure::EOF) => {
                // Eat any trailing whitespace.
                for _ in 0..self.buffer.len() {
                    eat(self);
                }
                return Err(Failure::EOF);
            },
            Err(Failure::WrongToken) => unreachable!(),
        }
    }

    /// Try and consume the specified token. If the token is wrong, it will
    /// not be consumed.
    fn try_consume_exact(&mut self, target: TokenType) -> ParseResult<()> {
        debug!("Trying to consume {:?}.", target);
        if self.peek()? == target {
            self.consume()?;
            Ok(())
        } else {
            Err(Failure::WrongToken)
        }
    }

    /// Try and consume the specified token. If the token is wrong, the given
    /// error will be generated and the token consumed.
    fn consume_exact<M>(&mut self, target: TokenType, msg: M) -> ParseResult<()>
        where M: Into<Cow<'static, str>>
    {
        debug!("Needing to consume {:?}.", target);
        if self.peek()? == target {
            self.consume()?;
            Ok(())
        } else {
            self.error_consume(msg);
            Err(Failure::WrongToken)
        }
    }

    /// Consume everything up to and including a newline.
    fn consume_till_nl(&mut self) -> ParseResult<()> {
        debug!("Consuming till the next newline.");
        loop {
            match self.try_consume_exact(TokenType::Newline) {
                Ok(()) => return Ok(()),
                Err(Failure::WrongToken) => self.consume()?,
                Err(Failure::EOF) => return Err(Failure::EOF),
            }
        }
    }

    /// Consume the next non-whitespace token (and all whitespace before it),
    /// generating the given error for the token. If no token can be found due
    /// to EOF, `self.last_span` will be used for the error.
    fn error_consume<M>(&mut self, message: M)
        where M: Into<Cow<'static, str>>
    {
        let message = message.into();
        error!("Generating error: {}", message);
        let _ = self.consume();  // We don't care about EOF.
        self.errors.push(SaltError {span: self.last_span.clone(), message});
    }

    /// Program non-terminal.
    fn parse_program(&mut self) {
        let _guard = self.start_node(Program);
        info!("Parsing Program...");

        // Parse the next line until EOF.
        loop {
            match self.parse_line() {
                Ok(SequenceResult::GoAgain) => {},
                Ok(SequenceResult::GracefulEnd) => break,
                Err(Failure::EOF) => {
                    debug!("Unexpected EOF.");
                    self.error_consume("Unexpected EOF");
                    break;
                },
                Err(_) => panic!("Invalid return from parse_line()"),
            }
        }

        // We must be at the end of the file now.
        assert!(self.tokens.next().is_none(), "Reached end of PROGRAM before EOF.");

        info!("...Finished Program.");
    }

    /// Line non-terminal.
    fn parse_line(&mut self) -> ParseResult<SequenceResult> {
        let _guard = self.start_node(Line);
        info!("Parsing Line...");

        // We might have gracefully reached the end of the file.
        if let Err(Failure::EOF) = self.peek() {
            info!("...Finished line with EOF.");
            return Ok(SequenceResult::GracefulEnd);
        }

        // Lookahead.
        let line_result = match self.peek()? {
            TokenType::Const => {
                // Constant declaration.
                self.parse_const_decl()
            },
            TokenType::Static => {
                // Data declaration.
                self.parse_data_decl()
            },
            TokenType::Identifier => {
                // Label or instruction: we need a second lookahead.
                if let Ok(TokenType::Colon) = self.double_lookahead() {
                    self.parse_label()
                } else {
                    self.parse_instruction()
                }
            },
            TokenType::Comment => {
                self.consume()
            },
            TokenType::Newline => {
                // Empty line.
                Ok(())
            },
            _ => {
                // Invalid token.
                self.error_consume("Unexpected token at start of line: expected \
                                   const declaration, data declaration, label, \
                                   instruction, or comment.");
                Err(Failure::WrongToken)
            }
        };

        // Handle possible failures.
        match line_result {
            Ok(()) => {},
            Err(Failure::WrongToken) => {
                // Eat the rest of the line and carry on parsing.
                self.consume_till_nl()?;
                info!("...Finished Line with error.");
                return Ok(SequenceResult::GoAgain);
            },
            Err(Failure::EOF) => return Err(Failure::EOF),
        }

        // We may have reached the end of the file.
        if let Err(Failure::EOF) = self.peek() {
            info!("...Finished line with EOF.");
            return Ok(SequenceResult::GracefulEnd);
        }

        // There may be a comment after the line.
        match self.peek()? {
            TokenType::Comment => {
                // Consume the comment and the following newline.
                self.consume()?;
                if let Err(Failure::WrongToken) =
                        self.try_consume_exact(TokenType::Newline) {
                    panic!("Comment didn't end with a newline!");
                }
                // If the newline fails due to EOF, this will fall through and
                // be caught at the start of the next `parse_line`.
            }
            TokenType::Newline => {
                self.consume()?;
            }
            _ => {
                // Report the error and eat the rest of the line.
                self.error_consume("Unexpected token after end \
                                   of line; expected newline.");
                self.consume_till_nl()?;
                info!("...Finished Line with error.");
                return Ok(SequenceResult::GoAgain);
            }
        }

        info!("...Finished Line.");
        Ok(SequenceResult::GoAgain)
    }

    /// ConstDecl non-terminal.
    fn parse_const_decl(&mut self) -> ParseResult<()> {
        let _guard = self.start_node(ConstDecl);
        info!("Parsing ConstDecl...");

        // Const keyword.
        self.consume_exact(TokenType::Const, "Expected const keyword.")?;

        // Identifier name.
        self.consume_exact(TokenType::Identifier, "Expected constant name.")?;

        // Literal value.
        self.parse_literal()?;

        info!("...Finished ConstDecl.");
        Ok(())
    }

    /// DataDecl non-terminal.
    fn parse_data_decl(&mut self) -> ParseResult<()> {
        let _guard = self.start_node(DataDecl);
        info!("Parsing DataDecl...");

        // Static keyword.
        self.consume_exact(TokenType::Static, "Expected static keyword.")?;

        // Optional mut.
        if let TokenType::Mut = self.peek()? {
            self.consume()?;
        }

        // Data type.
        self.parse_data_type()?;

        // Identifier name.
        self.consume_exact(TokenType::Identifier, "Expected data name.")?;

        // (array) literal value.
        self.parse_array_literal()?;

        info!("...Finished DataDecl.");
        Ok(())
    }

    /// DataType non-terminal.
    fn parse_data_type(&mut self) -> ParseResult<()> {
        let _guard = self.start_node(DataType);
        info!("Parsing DataType...");

        // Byte, Half, or Word.
        match self.peek()? {
            TokenType::Byte
            | TokenType::Half
            | TokenType::Word => {
                self.consume()?;
            },
            _ => {
                self.error_consume("Expected data type.");
                info!("...Finished DataType with error.");
                return Err(Failure::WrongToken);
            }
        }

        // Optional sequence of array length specifiers.
        while let TokenType::OpenSquare = self.peek()? {
            self.consume()?;
            self.consume_exact(TokenType::IntLiteral,
                               "Expected array length literal.")?;
            self.consume_exact(TokenType::CloseSquare, "Expected ']'.")?;
        }

        info!("...Finished DataType.");
        Ok(())
    }

    /// Label non-terminal.
    fn parse_label(&mut self) -> ParseResult<()> {
        let _guard = self.start_node(Label);
        info!("Parsing Label...");

        // Label identifier.
        self.consume_exact(TokenType::Identifier, "Expected label name.")?;

        // Colon.
        self.consume_exact(TokenType::Colon, "Expected ':'")?;

        info!("...Finished Label.");
        Ok(())
    }

    /// Instruction non-terminal.
    fn parse_instruction(&mut self) -> ParseResult<()> {
        let _guard = self.start_node(Instruction);
        info!("Parsing Instruction...");

        // Opcode identifier.
        self.consume_exact(TokenType::Identifier, "Expected opcode.")?;

        // Zero or more operands.
        loop {
            if let SequenceResult::GracefulEnd = self.parse_operand()? {
                break;
            }
        }

        info!("...Finished Instruction.");
        Ok(())
    }

    /// Operand non-terminal.
    fn parse_operand(&mut self) -> ParseResult<SequenceResult> {
        info!("Parsing Operand...");
        // Since operand lists have no terminator, we must be aware of
        // potential EOFs.
        let tt = self.peek();
        match tt {
            Ok(TokenType::Identifier)
            | Ok(TokenType::IntLiteral)
            | Ok(TokenType::FloatLiteral)
            | Ok(TokenType::CharLiteral) => {
                let _guard = self.start_node(Operand);
                if let Ok(TokenType::Identifier) = tt {
                    self.consume()?;
                } else {
                    self.parse_literal()?;
                }
                info!("...Finished Operand.");
                Ok(SequenceResult::GoAgain)
            },
            _ => {
                // No more operands.
                info!("...Finished last Operand.");
                Ok(SequenceResult::GracefulEnd)
            }
        }
    }

    /// ArrayLiteral non-terminal.
    fn parse_array_literal(&mut self) -> ParseResult<()> {
        let _guard = self.start_node(ArrayLiteral);
        info!("Parsing ArrayLiteral...");

        // Lookahead.
        match self.peek()? {
            TokenType::IntLiteral
            | TokenType::FloatLiteral
            | TokenType::CharLiteral => {
                // Scalar literal.
                self.parse_literal()?;
            },
            TokenType::StringLiteral => {
                // String literal.
                self.consume()?;
            },
            TokenType::OpenSquare => {
                // Full array literal.
                self.consume()?;

                // Array might be empty.
                if self.peek()? != TokenType::CloseSquare {
                    loop {
                        // Expect an element, which is also an ArrayLiteral.
                        self.parse_array_literal()?;
                        // Must be either a comma or a close bracket next.
                        match self.peek()? {
                            TokenType::Comma => {
                                self.consume()?;
                            },
                            TokenType::CloseSquare => {
                                break;
                            },
                            _ => {
                                self.error_consume("Expected ',' or ']'");
                                info!("...Finishing ArrayLiteral with error.");
                                return Err(Failure::WrongToken);
                            }
                        }
                    }
                }
                self.consume()?;  // Eat the close bracket.
            }
            _ => {
                self.error_consume("Expected literal.");
                info!("...Finishing ArrayLiteral with error.");
                return Err(Failure::WrongToken);
            }
        }

        info!("...Finished ArrayLiteral.");
        Ok(())
    }

    /// Literal non-terminal.
    fn parse_literal(&mut self) -> ParseResult<()> {
        let _guard = self.start_node(Literal);
        info!("Parsing Literal...");

        match self.peek()? {
            TokenType::IntLiteral
            | TokenType::FloatLiteral
            | TokenType::CharLiteral => {
                self.consume()?;
                info!("...Finished Literal.");
                Ok(())
            },
            _ => {
                self.error_consume("Expected integer, float, or character literal.");
                info!("...Finished literal with error.");
                Err(Failure::WrongToken)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_logging;

    use insta::assert_debug_snapshot;

    fn assert_syntax_tree_snapshot(path: &str) {
        init_test_logging();

        let input = std::fs::read_to_string(path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        let output = parser.run().unwrap();
        assert_debug_snapshot!(output);
    }

    fn assert_error_snapshot(path: &str) {
        init_test_logging();

        let input = std::fs::read_to_string(path).unwrap();
        let mut parser = Parser::new(Lexer::new(&input));
        parser.parse_program();
        let tree = SyntaxNode::new_root(parser.builder.finish());
        let errors = parser.errors;
        assert!(!errors.is_empty());
        assert_debug_snapshot!(tree);
        assert_debug_snapshot!(errors);
    }

    #[test]
    fn test_empty() {
        assert_syntax_tree_snapshot("examples/empty-file.simasm");
    }

    #[test]
    fn test_comments() {
        assert_syntax_tree_snapshot("examples/comments-only.simasm");
    }

    #[test]
    fn test_consts() {
        assert_syntax_tree_snapshot("examples/consts-only.simasm");
    }

    #[test]
    fn test_statics() {
        assert_syntax_tree_snapshot("examples/statics-only.simasm");
    }

    #[test]
    fn test_arrays() {
        assert_syntax_tree_snapshot("examples/array-literals.simasm");
    }

    #[test]
    fn test_hello_world() {
        assert_syntax_tree_snapshot("examples/hello-world.simasm");
    }

    #[test]
    fn test_error_recovery() {
        assert_error_snapshot("examples/first-line-bad.simasm");
    }

    #[test]
    fn test_bad_tokens() {
        assert_error_snapshot("examples/bad-tokens.simasm");
    }
}
