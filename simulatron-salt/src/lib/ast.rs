use ast_gen::{derive_ast_nodes, derive_token_casts};
use std::convert::TryInto;
use std::ops::Range;
use std::str::FromStr;

use crate::error::{SaltError, SaltResult};
use crate::language::{SyntaxElement, SyntaxKind, SyntaxNode};

/// A thin strongly-typed layer over the weakly-typed SyntaxNode.
pub trait AstNode {
    fn cast(syntax: SyntaxNode) -> Option<Self> where Self: Sized;
    fn syntax(&self) -> &SyntaxNode;
}

// Proc macro invocation to derive boilerplate AstNode implementations for
// each AST node type.
derive_ast_nodes! {
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
}

// Similar to derive boilerplate for token casts.
// These look like `int_literal_cast`.
derive_token_casts! {
    Identifier,
    IntLiteral,
    FloatLiteral,
    CharLiteral,
    StringLiteral,
}

/// An enum for Operands, which can be either Identifiers or Literals.
#[derive(Debug)]
pub enum OperandType {
    Ident((String, Range<usize>)),
    Lit(LiteralValue),
}

/// The value of a literal.
#[derive(Debug, PartialEq, Eq)]
pub struct LiteralValue {
    value: u32,
    min_size: usize,  // Minimum number of bytes needed to represent the value.
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
    pub fn name(&self) -> String {
        self.syntax.children_with_tokens().find_map(identifier_cast).unwrap().0
    }

    pub fn value(&self) -> SaltResult<LiteralValue> {
        self.syntax.children().find_map(Literal::cast).unwrap().value()
    }
}

/// DataDecls have a name, mutability, size, and initialiser.
impl DataDecl {
    pub fn name(&self) -> String {
        self.syntax.children_with_tokens().find_map(identifier_cast).unwrap().0
    }

    pub fn mutable(&self) -> bool {
        node_contains_kind(&self.syntax, SyntaxKind::KwMut)
    }

    pub fn size(&self) -> SaltResult<usize> {
        self.syntax.children()
            .find_map(DataType::cast)
            .unwrap()
            .size()
    }

    pub fn initialiser(&self) -> SaltResult<Vec<LiteralValue>> {
        self.syntax.children()
            .find_map(ArrayLiteral::cast)
            .unwrap()
            .values()
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
        for child in self.syntax.children_with_tokens() {
            if let Some((text, span)) = int_literal_cast(child) {
                // Parse the integer value.
                let value = int_literal_value(&text, span.clone())?;
                // Ensure positive.
                let value = if value >= 0 {
                    value.try_into().unwrap()
                } else {
                    return Err(SaltError {
                        span,
                        message: "Array lengths cannot be negative.".into(),
                    });
                };
                // Multiply.
                size = match size.checked_mul(value) {
                    Some(val) => val,
                    None => {
                        return Err(SaltError {
                            span,
                            message: "Array size is out of range.".into(),
                        });
                    }
                };
            }
        }
        Ok(size)
    }
}

/// Labels have a name and a following instruction.
impl Label {
    pub fn name(&self) -> String {
        self.syntax.children_with_tokens().find_map(identifier_cast).unwrap().0
    }

    pub fn instruction(&self) -> SaltResult<Instruction> {
        // A label sits inside a line, so we need to look at the parent's
        // following siblings.
        self.syntax.parent()
            .unwrap()
            .siblings(rowan::Direction::Next)
            .filter_map(Line::cast)
            .find_map(|line| line.as_instruction())
            .ok_or_else(|| SaltError {
                span: self.syntax.text_range().into(),
                message: "Label without a following instruction.".into(),
            })
    }
}

/// Instructions have an opcode an a list of operands.
impl Instruction {
    pub fn opcode(&self) -> String {
        self.syntax.children_with_tokens().find_map(identifier_cast).unwrap().0
    }

    pub fn operands(&self) -> Vec<Operand> {
        self.syntax.children().filter_map(Operand::cast).collect()
    }
}

/// Operands are either identifiers or literals.
impl Operand {
    pub fn value(&self) -> SaltResult<OperandType> {
        match self.syntax.children_with_tokens().find_map(identifier_cast) {
            Some(ident) => Ok(OperandType::Ident(ident)),
            None => {
                let val = self.syntax.children()
                    .find_map(Literal::cast)
                    .unwrap()
                    .value()?;
                Ok(OperandType::Lit(val))
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
        } else if let Some((text, _)) = self.syntax.children_with_tokens()
                .find_map(string_literal_cast) {
            // A string literal. Convert character by character.
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
        } else if let Some(_) = self.syntax().children_with_tokens()
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
        log::trace!("{:#?}", self.syntax.children());
        if let Some((text, span)) = self.syntax.children_with_tokens()
                .find_map(int_literal_cast) {
            // Integer literal: parse and determine minimum size.
            let value = int_literal_value(&text, span)?;
            let min_size = minimum_size(value);
            // We know value is in the range of i32+u32, so just keep its
            // u32 bit-representation.
            let value = value as u32;
            Ok(LiteralValue {value, min_size})
        } else if let Some((text, _)) = self.syntax.children_with_tokens()
            .find_map(float_literal_cast) {
            // Float literal: parse, transmute bit representation to u32, and
            // size is always 4 bytes.
            let value = f32::from_str(&text).unwrap();
            let value = unsafe { std::mem::transmute::<f32, u32>(value) };
            Ok(LiteralValue {value, min_size: 4})
        } else if let Some((text, _)) = self.syntax.children_with_tokens()
            .find_map(char_literal_cast) {
            // Character literal: parse, and size is always 1 byte.
            let (value, _) = char_literal_value(&text);
            Ok(LiteralValue {value, min_size: 1})
        } else {
            unreachable!()
        }
    }
}

/// Does a SyntaxNode contain a child of the given SyntaxKind?
fn node_contains_kind(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .find(|child| child.kind() == kind)
        .is_some()
}

/// Parse a SyntaxNode to get the value of an IntLiteral. We return this as an
/// i64 since it encompasses the range of both u32 and i32.
fn int_literal_value(text: &str, span: Range<usize>) -> SaltResult<i64> {
    // Shortcut for out-of-range error.
    macro_rules! out_of_range {
        () => {{
            return Err(SaltError {
                span,
                message: "Integer literal out of range.".into(),
            });
        }}
    }

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
    let mut value = match i64::from_str_radix(
            &text[digits_start..digits_end], base) {
        Ok(val) => val,
        Err(_) => out_of_range!(),
    };

    // Apply a possible exponent.
    if let Some(exponent_start) = exponent_start {
        let exponent = match i32::from_str_radix(
                &text[(exponent_start+1)..text.len()], 10) {
            Ok(val) => {
                if val >= 0 {
                    val as u32
                } else {
                    return Err(SaltError {
                        span,
                        message: "Integer exponents cannot be negative.".into(),
                    });
                }
            },
            Err(_) => out_of_range!(),
        };
        value = match 10_i64.checked_pow(exponent)
                .and_then(|mul| value.checked_mul(mul)) {
            Some(val) => val,
            None => out_of_range!(),
        };
    }

    // Apply a possible minus sign.
    if number_start == 1 {
        value = -value;
    }

    // Check bounds.
    if value > u32::MAX.into() || value < i32::MIN.into() {
        out_of_range!();
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
/// The given value must be within the combined range of i32 and u32.
fn minimum_size(value: i64) -> usize {
    if value < i32::MIN.into() {
        panic!("Value of {} is out of range!", value);
    } else if value < i16::MIN.into() {
        4
    } else if value < i8::MIN.into() {
        2
    } else if value <= u8::MAX.into() {
        1
    } else if value <= u16::MAX.into() {
        2
    } else if value <= u32::MAX.into() {
        4
    } else {
        panic!("Value of {} is out of range!", value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_logging;
    use crate::{lexer::Lexer, parser::Parser};

    use insta::assert_debug_snapshot;
    use log::info;

    fn setup(path: &str) -> Program {
        init_test_logging();
        let input = std::fs::read_to_string(path).unwrap();
        let parser = Parser::new(Lexer::new(&input));
        let cst = parser.run().unwrap();
        info!("{:#?}", cst);
        Program::cast(cst).unwrap()
    }

    #[test]
    fn test_program_components() {
        let ast = setup("examples/hello-world.simasm");
        assert_eq!(ast.const_decls().len(), 4);
        assert_eq!(ast.data_decls().len(), 1);
        assert_eq!(ast.labels().len(), 1);
        assert_eq!(ast.instructions().len(), 6);
    }

    #[test]
    fn test_consts() {
        let ast = setup("examples/consts-only.simasm");
        let consts = ast.const_decls();
        assert_eq!(consts.len(), 9);
        let values: Vec<LiteralValue> = consts.iter()
            .map(ConstDecl::value)
            .map(Result::unwrap)
            .collect();
        assert_eq!(values[0], LiteralValue {value: 0b01001, min_size: 1});
        assert_eq!(values[1], LiteralValue {value: 42, min_size: 1});
        assert_eq!(values[2], LiteralValue {value: 42_000_000, min_size: 4});
        assert_eq!(values[3], LiteralValue {value: 0xDEADB00F, min_size: 4});
        assert_eq!(values[4], LiteralValue {
            value: unsafe {std::mem::transmute::<f32,u32>(1.0)}, min_size: 4});
        assert_eq!(values[5], LiteralValue {
            value: unsafe {std::mem::transmute::<f32,u32>(42e-12)}, min_size: 4});
        assert_eq!(values[6], LiteralValue {value: (-5_i32) as u32, min_size: 1});
        assert_eq!(values[7], LiteralValue {
            value: unsafe {std::mem::transmute::<f32,u32>(-9.9432)}, min_size: 4});
        assert_eq!(values[8], LiteralValue {value: 1000, min_size: 2});
    }

    #[test]
    fn test_data() {
        let ast = setup("examples/literal-ranges.simasm");
        let data = ast.data_decls();
        assert_eq!(data.len(), 8);

        fn test(decl: &DataDecl, name: &str, mutable: bool) {
            assert_eq!(decl.name(), name);
            assert_eq!(decl.mutable(), mutable);
            assert_debug_snapshot!(decl.size());
            assert_debug_snapshot!(decl.initialiser());
        }

        test(&data[0], "arr", false);
        test(&data[1], "primes_and_doubles", false);
        test(&data[2], "empty", false);
        test(&data[3], "zeros", true);
        test(&data[4], "negative", false);
        test(&data[5], "too_big", false);
        test(&data[6], "init_too_big", false);
        test(&data[7], "init_too_small", false);
    }

    #[test]
    fn test_labels() {
        let ast = setup("examples/instruction-block.simasm");
        let first_instruction = &ast.instructions()[0];
        let label = &ast.labels()[0];

        assert_eq!(&label.name(), "somelabel");
        assert_eq!(&label.instruction().unwrap(), first_instruction);
    }

    #[test]
    fn test_instructions() {
        let ast = setup("examples/hello-world.simasm");
        let instructions = ast.instructions();
        assert_eq!(instructions.len(), 6);

        fn test(instruction: &Instruction, opcode: &str) {
            assert_eq!(&instruction.opcode(), opcode);
            assert_debug_snapshot!(instruction.operands()
                .iter()
                .map(Operand::value)
                .map(Result::unwrap)
                .collect::<Vec<_>>());
        }

        test(&instructions[0], "copy");
        test(&instructions[1], "mult");
        test(&instructions[2], "add");
        test(&instructions[3], "add");
        test(&instructions[4], "blockcopy");
        test(&instructions[5], "halt");
    }
}
