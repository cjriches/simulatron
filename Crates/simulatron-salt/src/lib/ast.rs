use ast_sourcegen::{derive_ast_nodes, derive_token_casts};
use std::convert::TryInto;
use std::ops::Range;
use std::str::FromStr;

use crate::error::{SaltError, SaltResult};
use crate::language::{SyntaxElement, SyntaxKind, SyntaxNode};

/// A thin strongly-typed layer over the weakly-typed SyntaxNode.
pub trait AstNode {
    fn cast(syntax: SyntaxNode) -> Option<Self>
    where
        Self: Sized;
    fn syntax(&self) -> &SyntaxNode;
}

// Proc macro invocation to derive boilerplate structs and AstNode
// implementations for each AST node type.
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

// Proc macro invocation to derive boilerplate token casts.
// These are snake_case like `int_literal_cast`.
derive_token_casts! {
    Identifier,
    IntLiteral,
    FloatLiteral,
    CharLiteral,
    StringLiteral,
}

/// Possible types of a register.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RegisterType {
    Byte,
    Half,
    Word,
    Float,
}

/// Array lengths may be either a literal, or inferred from the initialiser.
#[derive(Debug)]
pub enum ArrayLength {
    Literal(usize),
    Inferred,
}

/// An enum for Operands, which can be either Identifiers or Literals.
#[derive(Debug)]
pub enum OperandValue {
    Ident(String),
    Lit(LiteralValue),
}

/// The value of a literal.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum LiteralValue {
    Lit {
        value: u32,
        min_reg_type: RegisterType,
    },
    Sizeof {
        ident: String,
    },
}

/// Programs contain Const Declarations, Data Declarations, Labels,
/// and Instructions.
impl Program {
    pub fn const_decls(&self) -> Vec<ConstDecl> {
        self.syntax
            .children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_const())
            .collect()
    }

    pub fn data_decls(&self) -> Vec<DataDecl> {
        self.syntax
            .children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_data())
            .collect()
    }

    pub fn labels(&self) -> Vec<Label> {
        self.syntax
            .children()
            .filter_map(Line::cast)
            .filter_map(|line| line.as_label())
            .collect()
    }

    pub fn instructions(&self) -> Vec<Instruction> {
        self.syntax
            .children()
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

/// ConstDecls have a name, publicity, and value.
impl ConstDecl {
    pub fn name(&self) -> String {
        self.syntax
            .children_with_tokens()
            .find_map(identifier_cast)
            .unwrap()
            .0
    }

    pub fn name_span(&self) -> Range<usize> {
        self.syntax
            .children_with_tokens()
            .find_map(identifier_cast)
            .unwrap()
            .1
    }

    pub fn public(&self) -> bool {
        node_contains_kind(&self.syntax, SyntaxKind::KwPub)
    }

    pub fn value(&self) -> SaltResult<LiteralValue> {
        self.syntax
            .children()
            .find_map(Literal::cast)
            .unwrap()
            .value()
    }
}

/// DataDecls have a name, publicity, mutability, type, and initialiser.
impl DataDecl {
    pub fn name(&self) -> String {
        self.syntax
            .children_with_tokens()
            .find_map(identifier_cast)
            .unwrap()
            .0
    }

    pub fn name_span(&self) -> Range<usize> {
        self.syntax
            .children_with_tokens()
            .find_map(identifier_cast)
            .unwrap()
            .1
    }

    pub fn init_span(&self) -> Range<usize> {
        self.syntax
            .children()
            .find_map(ArrayLiteral::cast)
            .unwrap()
            .syntax()
            .text_range()
            .into()
    }

    pub fn public(&self) -> bool {
        node_contains_kind(&self.syntax, SyntaxKind::KwPub)
    }

    pub fn mutable(&self) -> bool {
        node_contains_kind(&self.syntax, SyntaxKind::KwMut)
    }

    pub fn type_(&self) -> DataType {
        self.syntax.children().find_map(DataType::cast).unwrap()
    }

    pub fn initialiser(&self) -> SaltResult<(Vec<LiteralValue>, Vec<usize>)> {
        self.syntax
            .children()
            .find_map(ArrayLiteral::cast)
            .unwrap()
            .values()
    }
}

/// DataTypes have a base size and a set of dimensions.
impl DataType {
    pub fn base_size(&self) -> usize {
        if node_contains_kind(&self.syntax, SyntaxKind::KwByte) {
            1
        } else if node_contains_kind(&self.syntax, SyntaxKind::KwHalf) {
            2
        } else if node_contains_kind(&self.syntax, SyntaxKind::KwWord) {
            4
        } else {
            unreachable!()
        }
    }

    pub fn dimensions(&self) -> SaltResult<Vec<ArrayLength>> {
        let mut lengths = Vec::new();
        for child in self.syntax.children_with_tokens() {
            if let SyntaxKind::DoubleDot = child.kind() {
                lengths.push(ArrayLength::Inferred);
            } else if let Some((text, span)) = int_literal_cast(child) {
                // Parse the integer value.
                let value = int_literal_value(&text, span.clone())?;
                // Ensure positive.
                if value >= 0 {
                    lengths.push(ArrayLength::Literal(value.try_into().unwrap()));
                } else {
                    return Err(SaltError {
                        span,
                        message: "Array lengths cannot be negative.".into(),
                    });
                };
            }
        }

        // Count the base size.
        lengths.push(ArrayLength::Literal(1));

        Ok(lengths)
    }

    pub fn span(&self) -> Range<usize> {
        self.syntax.text_range().into()
    }
}

/// Labels have a name, publicity, and a following instruction.
impl Label {
    pub fn name(&self) -> String {
        self.syntax
            .children_with_tokens()
            .find_map(identifier_cast)
            .unwrap()
            .0
    }

    pub fn public(&self) -> bool {
        node_contains_kind(&self.syntax, SyntaxKind::KwPub)
    }

    pub fn instruction(&self) -> SaltResult<Instruction> {
        // A label sits inside a line, so we need to look at the parent's
        // following siblings.
        self.syntax
            .parent()
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

/// Instructions have an opcode and a list of operands.
impl Instruction {
    pub fn opcode(&self) -> (String, Range<usize>) {
        let mut opcode = self
            .syntax
            .children_with_tokens()
            .find_map(identifier_cast)
            .unwrap();
        opcode.0.make_ascii_lowercase();
        opcode
    }

    pub fn operands(&self) -> Vec<Operand> {
        self.syntax.children().filter_map(Operand::cast).collect()
    }
}

/// Operands have a value.
impl Operand {
    pub fn value(&self) -> SaltResult<OperandValue> {
        match self.syntax.children_with_tokens().find_map(identifier_cast) {
            Some(ident) => Ok(OperandValue::Ident(ident.0)),
            None => {
                let val = self
                    .syntax
                    .children()
                    .find_map(Literal::cast)
                    .unwrap()
                    .value()?;
                Ok(OperandValue::Lit(val))
            }
        }
    }
}

/// ArrayLiterals are a vector of literal values and a vector of dimensions.
impl ArrayLiteral {
    pub fn values(&self) -> SaltResult<(Vec<LiteralValue>, Vec<usize>)> {
        // Just a single literal.
        if let Some(lit) = self.syntax.children().find_map(Literal::cast) {
            Ok((vec![lit.value()?], vec![1]))
        } else if let Some((text, _)) = self
            .syntax
            .children_with_tokens()
            .find_map(string_literal_cast)
        {
            // A string literal. Convert character by character.
            let mut values = Vec::with_capacity(text.len() - 2);
            // Split into slices that look like character literals, so we
            // can use the same conversion function.
            let mut i = 1;
            while i < text.len() - 1 {
                // Include the character before to take the place of the opening
                // single quote, and include the character after in case this
                // is an escape sequence.
                let char_slice = &text[(i - 1)..=(i + 1)];
                let (value, escape) = char_literal_value(char_slice);
                values.push(LiteralValue::Lit {
                    value,
                    min_reg_type: RegisterType::Byte,
                });
                if escape {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            let len = values.len();
            Ok((values, vec![len, 1]))
        } else if self
            .syntax()
            .children_with_tokens()
            .any(|child| child.kind() == SyntaxKind::OpenSquare)
        {
            // A full array literal.
            let (child_values, child_dims): (Vec<_>, Vec<_>) = self
                .syntax
                .children()
                .filter_map(ArrayLiteral::cast) // Select the ArrayLiterals.
                .map(|arr_lit| arr_lit.values()) // Extract the values.
                .collect::<Result<Vec<_>, _>>()? // Merge the Results.
                .into_iter()
                .unzip(); // Separate the components.

            // Concatenate the child literals.
            let values = child_values.into_iter().flatten().collect();

            // Take the maximum of each child dimension.
            let num_dims = child_dims.iter().map(|c| c.len()).max().unwrap_or(1) + 1;
            let mut dims = Vec::with_capacity(num_dims);
            dims.push(child_dims.len());
            dims.resize(num_dims, 1);
            for child_dim in child_dims.iter() {
                #[allow(clippy::needless_range_loop)]
                for i in 1..num_dims {
                    let dim = *child_dim.get(i - 1).unwrap_or(&1);
                    if dim > dims[i] {
                        dims[i] = dim;
                    }
                }
            }

            Ok((values, dims))
        } else {
            unreachable!()
        }
    }
}

/// Literals have a value.
impl Literal {
    pub fn value(&self) -> SaltResult<LiteralValue> {
        if let Some((text, span)) = self
            .syntax
            .children_with_tokens()
            .find_map(int_literal_cast)
        {
            // Integer literal: parse and determine minimum size.
            let value = int_literal_value(&text, span)?;
            let min_reg_type = minimum_reg_type(value);
            // We know value is in the range of i32+u32, so just keep its
            // u32 bit-representation.
            let value = value as u32;
            Ok(LiteralValue::Lit {
                value,
                min_reg_type,
            })
        } else if let Some((text, _)) = self
            .syntax
            .children_with_tokens()
            .find_map(float_literal_cast)
        {
            // Float literal: parse, get bit representation as u32 and
            // size is always Word.
            let value = f32::from_str(&text).unwrap().to_bits();
            Ok(LiteralValue::Lit {
                value,
                min_reg_type: RegisterType::Float,
            })
        } else if let Some((text, _)) = self
            .syntax
            .children_with_tokens()
            .find_map(char_literal_cast)
        {
            // Character literal: parse, and size is always Byte.
            let (value, _) = char_literal_value(&text);
            Ok(LiteralValue::Lit {
                value,
                min_reg_type: RegisterType::Byte,
            })
        } else if let Some((text, _)) = self.syntax.children_with_tokens().find_map(identifier_cast)
        {
            // Sizeof literal: return the identifier.
            Ok(LiteralValue::Sizeof { ident: text })
        } else {
            unreachable!()
        }
    }
}

/// Does a SyntaxNode contain a child of the given SyntaxKind?
fn node_contains_kind(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .any(|child| child.kind() == kind)
}

/// Parse a string to get the value of an IntLiteral. We return this as an
/// i64 since it encompasses the range of both u32 and i32.
fn int_literal_value(text: &str, span: Range<usize>) -> SaltResult<i64> {
    // Shortcut for out-of-range error.
    macro_rules! out_of_range {
        () => {{
            return Err(SaltError {
                span,
                message: "Integer literal out of range.".into(),
            });
        }};
    }

    // Find where the number (with possible base prefix) begins.
    let number_start = if text.starts_with('-') { 1 } else { 0 };

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
    } else {
        None
    };
    let digits_end = exponent_start.unwrap_or(text.len());

    // Parse the digits.
    let mut value = match i64::from_str_radix(&text[digits_start..digits_end], base) {
        Ok(val) => val,
        Err(_) => out_of_range!(),
    };

    // Apply a possible exponent.
    if let Some(exponent_start) = exponent_start {
        let exponent = match text[(exponent_start + 1)..text.len()].parse::<i32>() {
            Ok(val) => {
                if val >= 0 {
                    val as u32
                } else {
                    return Err(SaltError {
                        span,
                        message: "Integer exponents cannot be negative.".into(),
                    });
                }
            }
            Err(_) => out_of_range!(),
        };
        value = match 10_i64
            .checked_pow(exponent)
            .and_then(|mul| value.checked_mul(mul))
        {
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

/// Parse a string to get the value of a character. Useful for both CharLiterals
/// and slices of StringLiterals. Also returns whether the character was an
/// escape sequence.
fn char_literal_value(text: &str) -> (u32, bool) {
    // The first character is a quote, and is ignored.
    // The second character is either the literal character itself or the start
    // of an escape sequence. Check which.
    let char1 = text.chars().nth(1).unwrap();
    match char1 {
        '\\' => {
            // Escape sequence. Third character determines value.
            let char2 = text.chars().nth(2).unwrap();
            let value = match char2 {
                'n' => 15,
                '\'' => 39,
                '"' => 34,
                '\\' => 92,
                _ => unreachable!(),
            };
            (value, true)
        }
        // Special cases where Simulatron instruction set diverges from ASCII.
        '??' => (31, false),
        '??' => (127, false),
        c => {
            // ASCII conversion.
            let value: u32 = c.into();
            assert!((32..=126).contains(&value));
            (value, false)
        }
    }
}

/// Calculate the minimum register size needed to store the given integer value.
/// The given value must be within the combined range of i32 and u32.
pub fn minimum_reg_type(value: i64) -> RegisterType {
    if value < i32::MIN.into() {
        panic!("Value of {} is out of range!", value);
    } else if value < i16::MIN.into() {
        RegisterType::Word
    } else if value < i8::MIN.into() {
        RegisterType::Half
    } else if value <= u8::MAX.into() {
        RegisterType::Byte
    } else if value <= u16::MAX.into() {
        RegisterType::Half
    } else if value <= u32::MAX.into() {
        RegisterType::Word
    } else {
        panic!("Value of {} is out of range!", value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{init_test_logging, lexer::Lexer, parser::Parser};

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
        let values: Vec<LiteralValue> = consts
            .iter()
            .map(ConstDecl::value)
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            values[0],
            LiteralValue::Lit {
                value: 0b01001,
                min_reg_type: RegisterType::Byte
            }
        );
        assert_eq!(
            values[1],
            LiteralValue::Lit {
                value: 42,
                min_reg_type: RegisterType::Byte
            }
        );
        assert_eq!(
            values[2],
            LiteralValue::Lit {
                value: 42_000_000,
                min_reg_type: RegisterType::Word
            }
        );
        assert_eq!(
            values[3],
            LiteralValue::Lit {
                value: 0xDEADB00F,
                min_reg_type: RegisterType::Word
            }
        );
        assert_eq!(
            values[4],
            LiteralValue::Lit {
                value: f32::to_bits(1.0),
                min_reg_type: RegisterType::Float
            }
        );
        assert_eq!(
            values[5],
            LiteralValue::Lit {
                value: f32::to_bits(42e-12),
                min_reg_type: RegisterType::Float
            }
        );
        assert_eq!(
            values[6],
            LiteralValue::Lit {
                value: (-5_i32) as u32,
                min_reg_type: RegisterType::Byte
            }
        );
        assert_eq!(
            values[7],
            LiteralValue::Lit {
                value: f32::to_bits(-9.9432),
                min_reg_type: RegisterType::Float
            }
        );
        assert_eq!(
            values[8],
            LiteralValue::Lit {
                value: 1000,
                min_reg_type: RegisterType::Half
            }
        );
    }

    #[test]
    fn test_data() {
        let ast = setup("examples/literal-ranges.simasm");
        let data = ast.data_decls();
        assert_eq!(data.len(), 8);

        macro_rules! test {
            ($decl: expr, $name: expr, $mutable: expr) => {{
                assert_eq!($decl.name(), $name);
                assert_eq!($decl.mutable(), $mutable);
                assert_debug_snapshot!($decl.type_().dimensions());
                assert_debug_snapshot!($decl.initialiser());
            }};
        }

        test!(&data[0], "arr", false);
        test!(&data[1], "primes_and_doubles", false);
        test!(&data[2], "empty", false);
        test!(&data[3], "zeros", true);
        test!(&data[4], "negative", false);
        test!(&data[5], "too_big", false);
        test!(&data[6], "init_too_big", false);
        test!(&data[7], "init_too_small", false);
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

        macro_rules! test {
            ($instruction: expr, $opcode: expr) => {{
                assert_eq!(&$instruction.opcode().0, $opcode);
                assert_debug_snapshot!($instruction
                    .operands()
                    .iter()
                    .map(Operand::value)
                    .map(Result::unwrap)
                    .collect::<Vec<_>>());
            }};
        }

        test!(&instructions[0], "copy");
        test!(&instructions[1], "mult");
        test!(&instructions[2], "add");
        test!(&instructions[3], "add");
        test!(&instructions[4], "blockcopy");
        test!(&instructions[5], "halt");
    }

    #[test]
    #[allow(clippy::bool_assert_comparison)]
    fn test_publics() {
        let ast = setup("examples/publics.simasm");
        let consts = ast.const_decls();
        let data = ast.data_decls();
        let labels = ast.labels();
        assert_eq!(consts.len(), 2);
        assert_eq!(data.len(), 2);
        assert_eq!(labels.len(), 2);

        assert_eq!(consts[0].public(), true);
        assert_eq!(consts[1].public(), false);

        assert_eq!(data[0].public(), false);
        assert_eq!(data[1].public(), true);

        assert_eq!(labels[0].public(), false);
        assert_eq!(labels[1].public(), true);
    }
}
