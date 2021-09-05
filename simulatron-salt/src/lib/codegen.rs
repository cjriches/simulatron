use std::collections::HashMap;
use std::ops::Range;

use crate::ast::{self, AstNode, LiteralValue};
use crate::error::{SaltError, SaltResult};

/// A rough estimate, assuming equal distribution of all instructions and
/// addressing modes.
const AVG_INSTRUCTION_LEN: usize = 4;

/// A very rough guesstimate, considering scalars and vectors.
const AVG_DATA_LEN: usize = 8;

/// Intermediate Representation Symbol Table.
struct SymbolTable {
    table: HashMap<String, SymbolTableEntry>,
}

impl SymbolTable {
    //noinspection RsSelfConvention
    fn with_capacity(cap: usize) -> Self {
        Self {
            table: HashMap::with_capacity(cap),
        }
    }

    fn add_constants(&mut self, consts: &Vec<ast::ConstDecl>) -> SaltResult<()> {
        for const_ in consts.iter() {
            let name = const_.name();
            let public = const_.public();
            let value = const_.value()?;
            let span: Range<usize> = const_.syntax().text_range().into();

            self.table.insert(name, SymbolTableEntry::C(Constant {
                public,
                value,
                span: span.clone(),
            })).ok_or_else(|| {
                SaltError {
                    span,
                    message: "Name already in use.".into(),
                }
            })?;
        }
        Ok(())
    }

    fn add_data(&mut self, data_decls: &Vec<ast::DataDecl>) -> SaltResult<()> {
        for data in data_decls.iter() {
            let name = data.name();
            let public = data.public();
            let mutable = data.mutable();
            let type_ = data.type_();
            let initialiser = data.initialiser()?;
            let span: Range<usize> = data.syntax().text_range().into();

            // Calculate the full initialiser.
            let base_size = type_.base_size();
            let size = type_.total_size()?;
            let initialiser = {
                let mut buf = Vec::with_capacity(size);
                for literal in initialiser.iter() {
                    let mut bytes = match base_size {
                        1 => value_as_byte(literal),
                        2 => value_as_half(literal),
                        4 => value_as_word(literal),
                        _ => unreachable!(),
                    }.ok_or_else(|| SaltError {
                        span: span.clone(),
                        message: "Initialiser too big for type.".into(),
                    })?;
                    buf.append(&mut bytes);
                }
                buf
            };
            assert_eq!(initialiser.len(), size);

            self.table.insert(name, SymbolTableEntry::D(Data {
                public,
                mutable,
                size,
                initialiser,
                span: span.clone(),
            })).ok_or_else(|| {
                SaltError {
                    span,
                    message: "Name already in use.".into(),
                }
            })?;
        }
        Ok(())
    }

    fn add_labels(&mut self, labels: &Vec<ast::Label>) -> SaltResult<()> {
        for label in labels.iter() {
            let name = label.name();
            let public = label.public();
            let instruction = label.instruction()?;
            let span: Range<usize> = label.syntax().text_range().into();

            self.table.insert(name, SymbolTableEntry::L(Label {
                public,
                location: LabelLocation::Reference(instruction),
                span: span.clone(),
            })).ok_or_else(|| {
                SaltError {
                    span,
                    message: "Name already in use.".into(),
                }
            })?;
        }
        Ok(())
    }
}

#[derive(Debug)]
enum SymbolTableEntry {
    C(Constant),
    D(Data),
    L(Label),
}

#[derive(Debug)]
struct Constant {
    public: bool,
    value: LiteralValue,
    span: Range<usize>,
}

#[derive(Debug)]
struct Data {
    public: bool,
    mutable: bool,
    size: usize,
    initialiser: Vec<u8>,
    span: Range<usize>,
}

#[derive(Debug)]
enum LabelLocation {
    Reference(ast::Instruction),
    Offset(usize),
}

#[derive(Debug)]
struct Label {
    public: bool,
    location: LabelLocation,
    span: Range<usize>,
}

/// Possible types of an operand.
#[derive(Debug)]
enum OperandType {
    Literal(usize),
    VarLiteral,
    RegRefAny,
    RegRefInt,
    RegRefWord,
    RegRefByte,
    RegRefIntFloat,
}

/// A specific variant of an instruction, with a single binary opcode and
/// set of operand types.
#[derive(Debug)]
struct InstructionVariant {
    opcode: u8,
    operands: Vec<OperandType>,
}

/// The result of successful codegen.
#[derive(Debug)]
pub struct ObjectCode {
    code: Vec<u8>,
    warnings: Vec<SaltError>,
}

/// An object code generator.
pub struct CodeGenerator {
    symbol_table: SymbolTable,
    code: Vec<u8>,
    warnings: Vec<SaltError>,
    instructions: Vec<ast::Instruction>,
}

impl CodeGenerator {
    pub fn new(program: ast::Program) -> SaltResult<Self> {
        // Extract program components.
        let consts = program.const_decls();
        let data = program.data_decls();
        let labels = program.labels();
        let instructions = program.instructions();

        // Allocate data structures.
        let mut symbol_table = SymbolTable::with_capacity(
            consts.len() + data.len() + labels.len() + 32  // Extra space for external references.
        );
        let code: Vec<u8> = Vec::with_capacity(
            data.len() * AVG_DATA_LEN + instructions.len() * AVG_INSTRUCTION_LEN
        );
        let warnings: Vec<SaltError> = Vec::new();

        // Populate symbol table.
        symbol_table.add_constants(&consts)?;
        symbol_table.add_data(&data)?;
        symbol_table.add_labels(&labels)?;

        Ok(Self {
            symbol_table,
            code,
            warnings,
            instructions,
        })
    }

    pub fn codegen(mut self) -> SaltResult<ObjectCode> {
        for instruction in self.instructions.iter() {
            let variants = get_instruction_variants(instruction)?;
            todo!()
        }

        Ok(ObjectCode {
            code: self.code,
            warnings: self.warnings,
        })
    }
}

fn value_as_byte(val: &LiteralValue) -> Option<Vec<u8>> {
    if val.min_size == 1 {
        Some(vec![val.value as u8])
    } else {
        None
    }
}

fn value_as_half(val: &LiteralValue) -> Option<Vec<u8>> {
    if val.min_size <= 2 {
        Some((val.value as u16).to_be_bytes().to_vec())
    } else {
        None
    }
}

fn value_as_word(val: &LiteralValue) -> Option<Vec<u8>> {
    assert!(val.min_size <= 4);
    Some(val.value.to_be_bytes().to_vec())
}

/// Find the possible opcodes and operands for the given opcode string.
fn get_instruction_variants(instruction: &ast::Instruction)
        -> SaltResult<Vec<InstructionVariant>> {
    Ok(match instruction.opcode().as_str() {
        "halt" => vec![InstructionVariant { opcode: 0x00, operands: vec![] }],
        "pause" => vec![InstructionVariant { opcode: 0x01, operands: vec![] }],
        "timer" => vec![
            InstructionVariant { opcode: 0x02, operands: vec![OperandType::Literal(4)] },
            InstructionVariant { opcode: 0x03, operands: vec![OperandType::RegRefWord] },
        ],
        // TODO more
        _ => return Err(SaltError {
            span: instruction.syntax().text_range().into(),
            message: "Unrecognised opcode.".into(),
        })
    })
}
