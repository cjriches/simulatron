use ast_gen::derive_ast_nodes;
use std::convert::TryInto;
use std::str::FromStr;

use crate::error::{SaltError, SaltResult};
use crate::language::{SyntaxKind, SyntaxNode};

/// A thin strongly-typed layer over the weakly-typed SyntaxNode.
pub trait AstNode {
    fn cast(syntax: SyntaxNode) -> Option<Self> where Self: Sized;
    fn syntax(&self) -> &SyntaxNode;
}

// Proc macro invocation to derive boilerplate AstNode implementations for
// each AST node type.
derive_ast_nodes! {
    // Non-terminals.
    Program,
    Line,
    ConstDecl,
    DataDecl,
    DataType,
    Label,
    Instruction,
    Operand,
    ArrayLiteral,
    Literal,
    // It's also convenient to include identifier terminals.
    Identifier,
}

/// An enum for Operands, which can be either Identifiers or Literals.
pub enum OperandType {
    Ident(Identifier),
    Lit(Literal),
}

/// The value of a literal.
pub struct LiteralValue {
    value: u32,
    min_size: usize,
}

/// Programs contain Const Declarations, Data Declarations, Labels,
/// and Instructions.
impl Program {
    pub fn const_decls(&self) -> Vec<ConstDecl> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_const())
            .collect()
    }

    pub fn data_decls(&self) -> Vec<DataDecl> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_data())
            .collect()
    }

    pub fn labels(&self) -> Vec<Label> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_label())
            .collect()
    }

    pub fn instructions(&self) -> Vec<Instruction> {
        self.syntax.children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_instruction())
            .collect()
    }
}

/// Lines can be Const Declarations, Data Declarations, Labels, or Instructions.
impl Line {
    pub fn as_const(&self) -> Option<ConstDecl> {
        self.syntax.children().find_map(ConstDecl::cast)
    }

    pub fn as_data(&self) -> Option<DataDecl> {
        self.syntax.children().find_map(DataDecl::cast)
    }

    pub fn as_label(&self) -> Option<Label> {
        self.syntax.children().find_map(Label::cast)
    }

    pub fn as_instruction(&self) -> Option<Instruction> {
        self.syntax.children().find_map(Instruction::cast)
    }
}

/// ConstDecls have a name and a value.
impl ConstDecl {
    pub fn name(&self) -> Identifier {
        self.syntax.children().find_map(Identifier::cast).unwrap()
    }

    pub fn value(&self) -> Literal {
        self.syntax.children().find_map(Literal::cast).unwrap()
    }
}

/// DataDecls have a name, value, type, and mutability.
impl DataDecl {
    pub fn name(&self) -> Identifier {
        self.syntax.children().find_map(Identifier::cast).unwrap()
    }

    pub fn value(&self) -> ArrayLiteral {
        self.syntax.children().find_map(ArrayLiteral::cast).unwrap()
    }

    pub fn type_(&self) -> DataType {
        self.syntax.children().find_map(DataType::cast).unwrap()
    }

    pub fn mutable(&self) -> bool {
        node_contains_kind(&self.syntax, SyntaxKind::KwMut)
    }
}

/// DataTypes have a total size.
impl DataType {
    pub fn size(&self) -> SaltResult<usize> {
        // Get the size of the base data type.
        let base_size: usize = if node_contains_kind(&self.syntax, SyntaxKind::KwByte) {
            1
        } else if node_contains_kind(&self.syntax, SyntaxKind::KwHalf) {
            2
        } else if node_contains_kind(&self.syntax, SyntaxKind::KwWord) {
            4
        } else { unreachable!() };
        // Find all the array lengths and multiply by them.
        let mut size = base_size;
        for child in self.syntax.children() {
            if child.kind() == SyntaxKind::IntLiteral {
                // Parse the integer value.
                let value = int_literal_value(&child)?
                    .try_into().unwrap();  // TODO handle negatives
                size = match size.checked_mul(value) {
                    Some(val) => val,
                    None => {
                        return Err(SaltError {
                            span: child.text_range().into(),
                            message: "Array size is out of range.".into(),
                        });
                    }
                };
            }
        }
        Ok(size)
    }
}

/// Labels have a name.
impl Label {
    pub fn name(&self) -> Identifier {
        self.syntax.children().find_map(Identifier::cast).unwrap()
    }
}

/// Instructions have an opcode an a list of operands.
impl Instruction {
    pub fn opcode(&self) -> Identifier {
        self.syntax.children().find_map(Identifier::cast).unwrap()
    }

    pub fn operands(&self) -> Vec<Operand> {
        self.syntax.children().filter_map(Operand::cast).collect()
    }
}

/// Operands are either identifiers or literals.
impl Operand {
    pub fn value(&self) -> OperandType {
        match self.syntax.children().find_map(Identifier::cast) {
            Some(ident) => OperandType::Ident(ident),
            None => {
                let lit = self.syntax.children().find_map(Literal::cast);
                OperandType::Lit(lit.unwrap())
            }
        }
    }
}

/// ArrayLiterals are a vector of literal values.
impl ArrayLiteral {
    pub fn values(&self) -> SaltResult<Vec<LiteralValue>> {
        // Just a single literal.
        if let Some(lit) = self.syntax.children().find_map(Literal::cast) {
            Ok(vec![lit.value()?])
        } else if let Some(string) = self.syntax.children()
                .find(|child| child.kind() == SyntaxKind::StringLiteral) {
            // A string literal. Convert character by character.
            let text = string.text().to_string();
            let mut values = Vec::with_capacity(text.len() - 2);
            // Split into slices that look like character literals, so we
            // can use the same conversion function.
            let mut i = 1;
            while i < text.len() - 1 {
                // Include the character before to take the place of the opening
                // single quote, and include the character after in case this
                // is an escape sequence.
                let char_slice = &text[(i-1)..=(i+1)];
                let (value, escape) = char_literal_value(char_slice);
                values.push(LiteralValue {value, min_size: 1});
                if escape {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Ok(values)
        } else if let Some(_) = self.syntax().children()
                .find(|child| child.kind() == SyntaxKind::OpenSquare) {
            // A full array literal. Parse the internal array literals and
            // concatenate them together.
            Ok(self.syntax.children()
                .filter_map(ArrayLiteral::cast)   // Select the ArrayLiterals.
                .map(|arr_lit| arr_lit.values())  // Extract the values.
                .collect::<Result<Vec<_>, _>>()?  // Merge the Results.
                .into_iter()                      // Flatten the nested Vecs.
                .flatten()
                .collect())
        } else {
            unreachable!()
        }
    }
}

/// Literals have a value and a minimum size.
impl Literal {
    pub fn value(&self) -> SaltResult<LiteralValue> {
        if let Some(int) = self.syntax.children()
                .find(|child| child.kind() == SyntaxKind::IntLiteral) {
            // Integer literal: parse and determine minimum size.
            let value = int_literal_value(&int)?;
            let min_size = minimum_size(value);
            Ok(LiteralValue {value, min_size})
        } else if let Some(float) = self.syntax.children()
                .find(|child| child.kind() == SyntaxKind::FloatLiteral) {
            // Float literal: parse, transmute bit representation to u32, and
            // size is always 4 bytes.
            let value = f32::from_str(&float.text().to_string()).unwrap();
            let value = unsafe { std::mem::transmute::<f32, u32>(value) };
            Ok(LiteralValue {value, min_size: 4})
        } else if let Some(chr) = self.syntax.children()
                .find(|child| child.kind() == SyntaxKind::CharLiteral) {
            // Character literal: parse, and size is always 1 byte.
            let (value, _) = char_literal_value(&chr.text().to_string());
            Ok(LiteralValue {value, min_size: 1})
        } else {
            unreachable!()
        }
    }
}

/// Identifiers have a name.
impl Identifier {
    pub fn name(&self) -> String {
        self.syntax.text().to_string()
    }
}

/// Does a SyntaxNode contain a child of the given SyntaxKind?
fn node_contains_kind(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children()
        .find(|child| child.kind() == kind)
        .is_some()
}

/// Parse a SyntaxNode to get the value of an IntLiteral.
/// TODO calculate size and sign properly, return both here?
fn int_literal_value(syntax: &SyntaxNode) -> SaltResult<u32> {
    // Shortcut for out-of-range error.
    macro_rules! out_of_range {
        () => {{
            return Err(SaltError {
                span: syntax.text_range().into(),
                message: "Integer literal out of range.".into(),
            });
        }}
    }

    // Extract text from token.
    assert_eq!(syntax.kind(), SyntaxKind::IntLiteral);
    let text = syntax.text().to_string();

    // Find where the number (with possible base prefix) begins.
    let number_start = if text.chars().nth(0).unwrap() == '-' {1} else {0};

    // Find the base and the start index of the actual digits.
    let (base, digits_start) = if text.chars().nth(number_start).unwrap() == '0' {
        match text.chars().nth(number_start + 1) {
            Some('b') => (2, number_start + 2),
            Some('x') => (16, number_start + 2),
            _ => (10, number_start),
        }
    } else {
        (10, number_start)
    };

    // Check for a possible exponent suffix.
    let exponent_start = if base == 10 {
        text.chars().position(|c| c == 'e')
    } else { None };
    let digits_end = exponent_start.unwrap_or(text.len());

    // Parse the digits.
    let mut value = match u32::from_str_radix(
            &text[digits_start..digits_end], base) {
        Ok(val) => val,
        Err(_) => out_of_range!(),
    };

    // Apply a possible minus sign.
    if number_start == 1 {
        value = -(value as i32) as u32;  // TODO this seems dumb
    }

    // Apply a possible exponent.
    if let Some(exponent_start) = exponent_start {
        let exponent = match i32::from_str_radix(
                &text[exponent_start..text.len()], 10) {
            Ok(val) => {
                if val >= 0 {
                    val as u32
                } else {
                    return Err(SaltError {
                        span: syntax.text_range().into(),
                        message: "Integer exponents cannot be negative.".into(),
                    });
                }
            },
            Err(_) => out_of_range!(),
        };
        value = match 10_u32.checked_pow(exponent)
                .and_then(|mul| value.checked_mul(mul)) {
            Some(val) => val,
            None => out_of_range!(),
        };
    }

    Ok(value)
}

/// Parse a SyntaxNode to get the value of an character. Useful for both
/// CharLiterals and slices of StringLiterals. Also returns whether the character
/// was an escape sequence.
fn char_literal_value(text: &str) -> (u32, bool) {
    // The first character is a quote.
    // The second character is either the character itself or the start of
    // an escape sequence.
    let char1 = text.chars().nth(1).unwrap();
    match char1 {
        '\\' => {
            let char2 = text.chars().nth(2).unwrap();
            let value = match char2 {
                'n' => 15,
                '\'' => 39,
                '"' => 34,
                '\\' => 92,
                _ => unreachable!(),
            };
            (value, true)
        },
        '£' => (31, false),
        '¬' => (127, false),
        c => {
            let value: u32 = c.into();
            assert!(value >= 32 && value <= 126);
            (value, false)
        }
    }
}

/// Calculate the minimum number of bytes needed to store the given integer value.
fn minimum_size(value: u32) -> usize {
    // TODO this is wrong for re-encoded negative numbers.
    if value <= u8::MAX.into() {
        1
    } else if value <= u16::MAX.into() {
        2
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_logging;
    use crate::{lexer::Lexer, parser::Parser};

    use insta::assert_debug_snapshot;

    fn setup(path: &str) -> SyntaxNode {
        init_test_logging();
        let input = std::fs::read_to_string(path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        parser.run().unwrap()
    }

    #[test]
    fn test_program_components() {
        let cst = setup("examples/hello-world.simasm");
        let ast = Program::cast(cst).unwrap();
        assert_eq!(ast.const_decls().len(), 4);
        assert_eq!(ast.data_decls().len(), 1);
        assert_eq!(ast.labels().len(), 1);
        assert_eq!(ast.instructions().len(), 6);
    }
}
