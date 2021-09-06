use std::collections::HashMap;
use std::convert::TryInto;
use std::io::Write;
use std::ops::Range;

use crate::ast::{self, AstNode, LiteralValue, OperandValue};
use crate::error::{SaltError, SaltResult};
use crate::write_be::WriteBE;

// The following constants are used to provide guesses for initial vector
// capacities. Thus, they are important for performance but not correctness.

/// A rough estimate, assuming equal distribution of all instructions and
/// addressing modes.
const AVG_INSTRUCTION_LEN: usize = 4;
/// A very rough guesstimate, considering scalars and vectors.
const AVG_DATA_LEN: usize = 8;
/// The size of the non-variable-length portion of a symbol table entry.
const SYMBOL_HEADER_LEN: usize = 10;
/// A complete guess.
const AVG_SYMBOL_REFERENCES: usize = 8;
/// A rough estimate based on 3 sections.
const AVG_HEADER_OVERHEAD: usize = 32;

// SimObj object code constants.
const MAGIC_HEADER: &[u8; 6] = b"SIMOBJ";
const ABI_VERSION: u16 = 0x0001;
pub const SYMBOL_TYPE_INTERNAL: u8 = b'I';
pub const SYMBOL_TYPE_PUBLIC: u8 = b'P';
pub const SYMBOL_TYPE_EXTERNAL: u8 = b'E';

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
                references: Vec::with_capacity(AVG_SYMBOL_REFERENCES),
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
                references: Vec::with_capacity(AVG_SYMBOL_REFERENCES),
            })).ok_or_else(|| {
                SaltError {
                    span,
                    message: "Name already in use.".into(),
                }
            })?;
        }
        Ok(())
    }

    fn iter_labels(&mut self) -> impl Iterator<Item = &mut Label> {
        self.table.values_mut().filter_map(|entry| {
            if let SymbolTableEntry::L(label) = entry {
                Some(label)
            } else {
                None
            }
        })
    }

    /// Calculate the number of entries and the byte size of this symbol table
    /// in the resulting object code.
    fn simobj_size(&self) -> (usize, usize) {
        let mut size = 0;
        let mut count = 0;
        for (name, entry) in self.table.iter() {
            if let SymbolTableEntry::C(_) = entry {
                continue;  // Ignore constants.
            }
            count += 1;
            size += SYMBOL_HEADER_LEN;
            size += name.len();
            match entry {
                SymbolTableEntry::D(data) => {
                    size += data.size;
                    size += data.references.len() * 4;
                },
                SymbolTableEntry::L(label) => {
                    size += label.references.len() * 4;
                },
                SymbolTableEntry::E(external) => {
                    size += external.references.len() * 4;
                }
                SymbolTableEntry::C(_) => unreachable!(),
            }
        }
        (count, size)
    }

    /// Write the symbol table as SimObj code into the given buffer.
    fn write_simobj(&self, buf: &mut Vec<u8>) {
        for (name, entry) in self.table.iter() {
            if let SymbolTableEntry::C(_) = entry {
                continue;  // Ignore constants.
            }
            match entry {
                SymbolTableEntry::D(data) => {
                    // Write symbol type.
                    let type_ = if data.public {SYMBOL_TYPE_PUBLIC} else {SYMBOL_TYPE_INTERNAL};
                    buf.write_u8(type_).unwrap();
                    // Write symbol value.
                    todo!()
                },
                SymbolTableEntry::L(label) => {
                    todo!()
                },
                SymbolTableEntry::E(external) => {
                    todo!()
                },
                SymbolTableEntry::C(_) => unreachable!(),
            }
        }
    }
}

#[derive(Debug)]
enum SymbolTableEntry {
    C(Constant),
    D(Data),
    L(Label),
    E(External),
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
    references: Vec<u32>,
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
    references: Vec<u32>,
}

#[derive(Debug)]
struct External {
    references: Vec<u32>,
}

/// Possible types of an operand.
/// VarLiteral and RegRefWordFloat only appear in variants, and get resolved to
/// more specific versions.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum OperandType {
    Literal(usize),
    VarLiteral,
    RegRefAny,
    RegRefInt,
    RegRefWord,
    RegRefHalf,
    RegRefByte,
    RegRefWordFloat,
    RegRefFloat,
}

/// Possible types of a register.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum RegisterType {
    Byte,
    Half,
    Word,
    Float,
}

/// Does the given register type match the given operand type?
fn register_type_matches(reg: RegisterType, op: OperandType) -> bool {
    use OperandType::*;

    match reg {
        RegisterType::Byte => {
            op == RegRefAny || op == RegRefInt || op == RegRefByte
        },
        RegisterType::Half => {
            op == RegRefAny || op == RegRefInt || op == RegRefHalf
        },
        RegisterType::Word => {
            op == RegRefAny || op == RegRefInt || op == RegRefWord
        },
        RegisterType::Float => {
            op == RegRefAny || op == RegRefFloat
        },
    }
}

/// The description of a specific operand.
#[derive(Debug)]
struct OperandDesc {
    op_type: OperandType,
    err_msg: &'static str,
}

/// A specific variant of an instruction, with a single binary opcode and
/// set of operand types.
#[derive(Debug)]
struct InstructionVariant {
    opcode: u8,
    operands: Vec<OperandDesc>,
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
    instructions: Option<Vec<ast::Instruction>>,
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
            instructions: Some(instructions),
        })
    }

    /// Top-level codegen entrypoint.
    pub fn codegen(mut self) -> SaltResult<ObjectCode> {
        // Process all instructions.
        for instruction in self.instructions.take().unwrap().iter() {
            // Resolve any labels pointing here.
            self.resolve_labels(instruction);
            // Codegen the instruction.
            self.codegen_instruction(instruction)?;
        }

        // Generate object code.
        // Size is instructions plus symbol table plus headers.
        let (num_symbols, st_size) = self.symbol_table.simobj_size();
        let mut simobj: Vec<u8> = Vec::with_capacity(
              self.code.len()
            + st_size
            + AVG_HEADER_OVERHEAD
        );

        // Write header and version.
        simobj.write_all(MAGIC_HEADER).unwrap();
        simobj.write_be_u16(ABI_VERSION).unwrap();

        // Write the number of symbols and sections. We'll use three sections:
        // instructions, read-only data, and read-write data.
        // TODO skip data sections if empty.
        simobj.write_be_u32(num_symbols.try_into().unwrap()).unwrap();
        simobj.write_be_u32(3).unwrap();

        // Calculate data offsets.
        // TODO

        // Write symbol table.
        // TODO

        // Write code section.
        // TODO

        // Write read-only data section.
        // TODO

        // Write read-write data section.
        // TODO

        Ok(ObjectCode {
            code: simobj,
            warnings: self.warnings,
        })
    }

    /// Resolve any labels pointing to the given instruction, making them point
    /// to an offset of the current code buffer length.
    fn resolve_labels(&mut self, instruction: &ast::Instruction) {
        for label in self.symbol_table.iter_labels() {
            if let LabelLocation::Reference(referenced_instruction) = &label.location {
                if instruction == referenced_instruction {
                    label.location = LabelLocation::Offset(self.code.len());
                }
            }
        }
    }

    /// Perform codegen for a single instruction.
    fn codegen_instruction(&mut self, instruction: &ast::Instruction) -> SaltResult<()> {
        let gen_func = match instruction.opcode().as_str() {
            "halt" => gen_halt,
            // TODO more
            _ => return Err(SaltError {
                span: instruction.syntax().text_range().into(),
                message: "Unrecognised opcode.".into(),
            }),
        };
        gen_func(&mut self.code, instruction.operands())
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

/// Convert a register reference string into its binary reference and associated type.
fn get_reg_ref(reg_ref: &str) -> Option<(u8, RegisterType)> {
    use RegisterType::*;

    Some(match reg_ref {
        "r0" => (0x00, Word),
        "r1" => (0x01, Word),
        "r2" => (0x02, Word),
        "r3" => (0x03, Word),
        "r4" => (0x04, Word),
        "r5" => (0x05, Word),
        "r6" => (0x06, Word),
        "r7" => (0x07, Word),
        "r0h" => (0x08, Half),
        "r1h" => (0x09, Half),
        "r2h" => (0x0A, Half),
        "r3h" => (0x0B, Half),
        "r4h" => (0x0C, Half),
        "r5h" => (0x0D, Half),
        "r6h" => (0x0E, Half),
        "r7h" => (0x0F, Half),
        "r0b" => (0x10, Byte),
        "r1b" => (0x11, Byte),
        "r2b" => (0x12, Byte),
        "r3b" => (0x13, Byte),
        "r4b" => (0x14, Byte),
        "r5b" => (0x15, Byte),
        "r6b" => (0x16, Byte),
        "r7b" => (0x17, Byte),
        "f0" => (0x18, Float),
        "f1" => (0x19, Float),
        "f2" => (0x1A, Float),
        "f3" => (0x1B, Float),
        "f4" => (0x1C, Float),
        "f5" => (0x1D, Float),
        "f6" => (0x1E, Float),
        "f7" => (0x1F, Float),
        "flags" => (0x20, Half),
        "uspr" => (0x21, Word),
        "kspr" => (0x22, Word),
        "pdpr" => (0x23, Word),
        "imr" => (0x24, Half),
        "pfsr" => (0x25, Word),
        _ => return None,
    })
}

/// Codegen for the HALT instruction.
fn gen_halt(code: &mut Vec<u8>, operands: Vec<ast::Operand>) -> SaltResult<()> {
    if operands.len() == 0 {
        code.push(0x00);
        Ok(())
    } else {
        Err(SaltError {
            span: operands[0].syntax().text_range().into(),
            message: "HALT instructions do not take operands.".into(),
        })
    }
}
