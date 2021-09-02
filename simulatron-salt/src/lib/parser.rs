use log::{trace, debug, info};
use rowan::{GreenNodeBuilder, Language};
use std::borrow::Cow;
use std::convert::{TryFrom, TryInto};
use std::ops::Range;

use crate::error::SaltError;
use crate::language::{SimAsmLanguage, SyntaxKind::{self, *}, SyntaxNode};
use crate::lexer::{Lexer, Token, TokenType};

/// A failure due to token mismatch or EOF.
enum Failure<'a> {
    WrongToken(Token<'a>),
    EOF,
}
type ParseResult<'a, T> = Result<T, Failure<'a>>;

/// A failure due just to EOF.
struct EOF;
type EOFResult<T> = Result<T, EOF>;

impl<'a> From<EOF> for Failure<'a> {
    fn from(_: EOF) -> Self {
        Failure::EOF
    }
}

impl<'a> TryFrom<Failure<'a>> for EOF {
    type Error = ();

    fn try_from(value: Failure<'a>) -> Result<Self, Self::Error> {
        if let Failure::EOF = value {
            Ok(EOF)
        } else {
            Err(())
        }
    }
}

/// Return codes from parsing a single line.
enum LineResult {
    GoAgain,
    GracefulEOF,
}

/// A recursive descent parser for SimAsm.
pub struct Parser<'a> {
    builder: GreenNodeBuilder<'static>,
    tokens: Lexer<'a>,
    last_span: Range<usize>,
    errors: Vec<SaltError>,
}

/// Unwrap a ParseResult. On WrongToken, produce the given error, end the
/// node, and return Ok(()). On EOF, return EOF.
macro_rules! unwrap_or_err {
    ($self:ident, $result:expr, $msg:expr) => {{
        match $result {
            Ok(t) => t,
            Err(Failure::WrongToken(t)) => {
                $self.error(t, $msg.into());
                $self.finish_node();
                return Ok(());
            },
            Err(Failure::EOF) => return Err(EOF),
        }
    }}
}

/// Shortcut for required whitespace.
macro_rules! after_ws {
    ($self:ident) => {{
        unwrap_or_err!($self, $self.eat_ws(), "Expected whitespace.")
    }}
}

/// Ensure a token is of the correct type; if not, the given error will be
/// produced, the node ended, and Ok(()) returned.
macro_rules! check_tt {
    ($self:ident, $token:expr, $target:ident, $msg:expr) => {{
        if let TokenType::$target = $token.tt {
            // no-op
        } else {
            $self.error($token, $msg.into());
            $self.finish_node();
            return Ok(());
        }
    }}
}

impl<'a> Parser<'a> {
    /// Construct a new parser from the given token stream.
    pub fn new(tokens: Lexer<'a>) -> Self {
        Self {
            builder: GreenNodeBuilder::new(),
            tokens,
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
    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(SimAsmLanguage::kind_to_raw(kind))
    }

    /// Wrapper for `builder.token`.
    fn add_token(&mut self, t: Token) {
        self.builder.token(SimAsmLanguage::kind_to_raw(t.tt.into()), t.slice)
    }

    /// Wrapper for `builder.finish_node`.
    fn finish_node(&mut self) {
        self.builder.finish_node()
    }

    /// Wrapper for `tokens.push_back`.
    fn push_back(&mut self, token: Token<'a>) {
        self.tokens.push_back(token)
    }

    /// Consume the next token from the stream.
    fn eat(&mut self) -> EOFResult<Token<'a>> {
        debug!("Eating any token.");
        match self.tokens.next() {
            Some(t) => {
                debug!("Ate {:?}", t);
                self.last_span = t.span.clone();
                Ok(t)
            },
            None => Err(EOF),
        }
    }

    /// Consume the next token from the stream, ensuring it's of the given type.
    /// The token is automatically added to the tree at the current position
    /// if it was correct.
    fn eat_exact(&mut self, target_type: TokenType) -> ParseResult<'a, ()> {
        debug!("Trying to eat {:?}.", target_type);
        match self.tokens.next() {
            Some(t) => {
                if t.tt == target_type {
                    debug!("Ate (exact) {:?}", t);
                    self.last_span = t.span.clone();
                    self.add_token(t);
                    Ok(())
                } else {
                    debug!("Ate (wrong) {:?}", t);
                    Err(Failure::WrongToken(t))
                }
            },
            None => Err(Failure::EOF),
        }
    }

    /// Consume optional whitespace and return the next token after.
    fn eat_ows(&mut self) -> EOFResult<Token<'a>> {
        debug!("Eating optional whitespace.");
        let mut token = self.eat_exact(TokenType::Whitespace);
        while let Ok(()) = token {
            token = self.eat_exact(TokenType::Whitespace);
        }
        return match token {
            Err(Failure::WrongToken(t)) => Ok(t),
            Err(Failure::EOF) => Err(EOF),
            Ok(()) => unreachable!(),
        }
    }

    /// Consume required whitespace and return the next token after.
    fn eat_ws(&mut self) -> ParseResult<'a, Token<'a>> {
        debug!("Eating required whitespace.");
        let mut token = self.eat_exact(TokenType::Whitespace);
        if let Err(Failure::WrongToken(t)) = token {
            return Err(Failure::WrongToken(t));
        }
        while let Ok(()) = token {
            token = self.eat_exact(TokenType::Whitespace);
        }
        return match token {
            Err(Failure::WrongToken(t)) => Ok(t),
            Err(Failure::EOF) => Err(Failure::EOF),
            Ok(()) => unreachable!(),
        }
    }

    /// Consume everything till a newline, which also gets eaten.
    fn consume_till_nl(&mut self) -> EOFResult<()> {
        debug!("Consuming till the next newline.");
        let mut token = self.eat_exact(TokenType::Newline);
        while let Err(Failure::WrongToken(t)) = token {
            self.add_token(t);
            token = self.eat_exact(TokenType::Newline);
        }
        token.map_err(|e| e.try_into().unwrap())  // WrongToken is impossible.
    }

    /// Generate a parsing error.
    fn error(&mut self, token: Token<'a>, message: Cow<'static, str>) {
        debug!("Generating error: {}", message);
        self.errors.push(SaltError::new(
            token.span,
            message
        ))
    }

    /// Program non-terminal.
    fn parse_program(&mut self) {
        self.start_node(Program);
        info!("Parsing Program...");

        // Parse the next line until EOF.
        loop {
            match self.parse_line() {
                Ok(LineResult::GoAgain) => {},
                Ok(LineResult::GracefulEOF) => break,
                Err(_) => {  // Unexpected EOF.
                    debug!("Unexpected EOF.");
                    self.errors.push(SaltError::new(self.last_span.clone(),
                                                    "Unexpected EOF.".into()));
                    break;
                }
            }
        }

        // We must be at the end of the file now.
        assert!(self.tokens.next().is_none(), "Reached end of PROGRAM before EOF.");

        info!("...Finished Program.");
        self.finish_node();
    }

    /// Line non-terminal.
    fn parse_line(&mut self) -> EOFResult<LineResult> {
        self.start_node(Line);
        info!("Parsing Line...");

        // We might have reached the end of the file.
        let token = match self.eat_ows() {
            Ok(t) => t,
            Err(_) => {
                info!("...Finished line with EOF.");
                self.finish_node();
                return Ok(LineResult::GracefulEOF);
            }
        };
        // Branch based on next token.
        match token.tt {
            TokenType::Const => {
                // Constant declaration.
                self.parse_const_decl(token)?;
            },
            TokenType::Static => {
                // Data declaration.
                self.data_decl(token)?;
            },
            TokenType::Identifier => {
                // Label or instruction: currently ambiguous.
                self.label_or_instruction(token)?;
            },
            TokenType::Comment => {
                self.add_token(token);
            },
            TokenType::Newline => {
                self.add_token(token);
                info!("...Finished Line.");
                self.finish_node();
                return Ok(LineResult::GoAgain);
            },
            _ => {
                // Report the error and eat the rest of the line.
                self.error(token, "Unexpected token at start of line: expected \
                                   const declaration, data declaration, label, \
                                   instruction, or comment.".into());
                self.consume_till_nl()?;
                info!("...Finished Line with error.");
                self.finish_node();
                return Ok(LineResult::GoAgain);
            }
        }

        // There may be whitespace and/or a comment after the line.
        // We may have also reached the end of the file.
        let token = match self.eat_ows() {
            Ok(t) => t,
            Err(_) => {
                info!("...Finished Line with EOF.");
                return Ok(LineResult::GracefulEOF);
            },
        };
        match token.tt {
            TokenType::Comment => {
                // Consume the comment and the following newline.
                self.add_token(token);
                if let Err(Failure::WrongToken(_)) = self.eat_exact(TokenType::Newline) {
                    panic!("Comment didn't end with a newline!");
                }
            }
            TokenType::Newline => {
                self.add_token(token);
            }
            _ => {
                // Report the error and eat the rest of the line.
                self.error(token, "Unexpected token after end \
                                   of line; expected newline.".into());
                self.consume_till_nl()?;
            }
        }

        info!("...Finished Line.");
        self.finish_node();
        Ok(LineResult::GoAgain)
    }

    /// ConstDecl non-terminal.
    fn parse_const_decl(&mut self, const_tok: Token) -> EOFResult<()> {
        self.start_node(ConstDecl);
        info!("Parsing ConstDecl...");

        // Add the const keyword token.
        assert_eq!(const_tok.tt, TokenType::Const);
        self.add_token(const_tok);

        // Required whitespace followed by an identifier.
        let ident = after_ws!(self);
        check_tt!(self, ident, Identifier, "Expected constant name.");
        self.add_token(ident);

        // Required whitespace followed by a literal.
        let next = after_ws!(self);
        self.push_back(next);
        self.parse_literal()?;

        info!("...Finished ConstDecl.");
        Ok(())
    }

    /// DataDecl non-terminal.
    fn data_decl(&mut self, static_tok: Token) -> EOFResult<()> {
        self.start_node(DataDecl);
        info!("Parsing DataDecl...");

        // Add the static keyword token.
        assert_eq!(static_tok.tt, TokenType::Static);
        self.add_token(static_tok);

        // Optional mut.
        let mut next = after_ws!(self);
        if let TokenType::Mut = next.tt {
            // Add and eat the next whitespace.
            self.add_token(next);
            next = after_ws!(self);
        }
        self.push_back(next);

        // Required type.
        self.parse_data_type()?;

        // Required identifier.
        let ident = after_ws!(self);
        check_tt!(self, ident, Identifier, "Expected static data name.");
        self.add_token(ident);

        // Required (array) literal.
        let next = after_ws!(self);
        self.tokens.push_back(next);
        self.parse_array_literal()?;

        info!("...Finished DataDecl.");
        Ok(())
    }

    /// DataType non-terminal.
    fn parse_data_type(&mut self) -> EOFResult<()> {
        self.start_node(DataType);
        info!("Parsing DataType...");

        todo!();

        info!("...Finished DataType.");
        Ok(())
    }

    /// Either a label or an instruction.
    fn label_or_instruction(&mut self, ident_tok: Token) -> EOFResult<()> {
        todo!()
    }

    /// Label non-terminal.
    fn parse_label(&mut self) -> EOFResult<()> {
        self.start_node(Label);
        info!("Parsing Label...");

        todo!();

        info!("...Finished Label.");
        Ok(())
    }

    /// Instruction non-terminal.
    fn parse_instruction(&mut self) -> EOFResult<()> {
        self.start_node(Instruction);
        info!("Parsing Instruction...");

        todo!();

        info!("...Finished Instruction.");
        Ok(())
    }

    /// Operand non-terminal.
    fn parse_operand(&mut self) -> EOFResult<()> {
        self.start_node(Operand);
        info!("Parsing Operand...");

        todo!();

        info!("...Finished Operand.");
        Ok(())
    }

    /// ArrayLiteral non-terminal.
    fn parse_array_literal(&mut self) -> EOFResult<()> {
        self.start_node(ArrayLiteral);
        info!("Parsing ArrayLiteral...");

        todo!();

        info!("...Finished ArrayLiteral.");
        Ok(())
    }

    /// Literal non-terminal.
    fn parse_literal(&mut self) -> EOFResult<()> {
        self.start_node(Literal);
        info!("Parsing Literal...");

        todo!();

        info!("...Finished Literal.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use insta::assert_debug_snapshot;

    /// Initialise logging.
    pub fn init() {
        use std::io::Write;

        // The logger can only be initialised once, but we don't know the order of
        // tests. Therefore we use `try_init` and ignore the result.
        let _ = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("info"))
            .format(|out, record| {
                writeln!(out, "{:>7} {}", record.level(), record.args())
            })
            .is_test(true)
            .try_init();
    }

    fn assert_syntax_tree_snapshot(path: &str) {
        init();

        let input = std::fs::read_to_string(path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        let output = parser.run().unwrap();
        assert_debug_snapshot!(output);
    }

    #[test]
    fn test_empty() {
        assert_syntax_tree_snapshot("examples/empty-file.simasm");
    }

    #[test]
    fn test_comments() {
        assert_syntax_tree_snapshot("examples/comments-only.simasm");
    }
}
