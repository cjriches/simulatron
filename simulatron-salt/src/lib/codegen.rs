use itertools::Itertools;
use std::collections::HashMap;
use std::convert::{TryInto, TryFrom};
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
/// A complete guess.
const AVG_SYMBOL_REFERENCES: usize = 8;
/// A rough estimate based on 3 sections.
const AVG_HEADER_OVERHEAD: usize = 32;

// SimObj object code constants.
const MAGIC_HEADER: &[u8; 6] = b"SIMOBJ";
const ABI_VERSION: u16 = 0x0001;
const SYMBOL_TYPE_INTERNAL: u8 = b'I';
const SYMBOL_TYPE_PUBLIC: u8 = b'P';
const SYMBOL_TYPE_EXTERNAL: u8 = b'E';
const FLAG_ENTRYPOINT: u8 = 0x01;
const FLAG_READ: u8 = 0x04;
const FLAG_WRITE: u8 = 0x08;
const FLAG_EXECUTE: u8 = 0x10;
/// The size of the file header.
const SIMOBJ_HEADER_LEN: usize = 16;
/// The size of the non-variable-length portion of a symbol table entry.
const SYMBOL_HEADER_LEN: usize = 10;
/// A full section header.
const SECTION_HEADER_LEN: usize = 5;

/// Intermediate Representation Symbol Table.
struct SymbolTable {
    table: HashMap<String, SymbolTableEntry>,
}

/// Info about the SimObj representation of a SymbolTable.
struct SymbolTableStats {
    num_entries: usize,
    size: usize,            // Total size.
    readonly_size: usize,   // Size of all read-only data items.
    readwrite_size: usize,  // Size of all read-write data items.
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

            let existing = self.table.insert(name, SymbolTableEntry::C(Constant {
                public,
                value,
                span: span.clone(),
            }));
            if let Some(_) = existing {
                return Err(SaltError {
                    span,
                    message: "Name already in use.".into(),
                });
            }
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
                buf.resize(size, 0);
                buf
            };
            assert_eq!(initialiser.len(), size);

            let existing = self.table.insert(name, SymbolTableEntry::D(Data {
                public,
                mutable,
                size,
                initialiser,
                span: span.clone(),
                references: Vec::with_capacity(AVG_SYMBOL_REFERENCES),
            }));
            if let Some(_) = existing {
                return Err(SaltError {
                    span,
                    message: "Name already in use.".into(),
                });
            }
        }
        Ok(())
    }

    fn add_labels(&mut self, labels: &Vec<ast::Label>) -> SaltResult<()> {
        for label in labels.iter() {
            let name = label.name();
            let public = label.public();
            let instruction = label.instruction()?;
            let span: Range<usize> = label.syntax().text_range().into();

            let existing = self.table.insert(name, SymbolTableEntry::L(Label {
                public,
                location: LabelLocation::Reference(instruction),
                span: span.clone(),
                references: Vec::with_capacity(AVG_SYMBOL_REFERENCES),
            }));
            if let Some(_) = existing {
                return Err(SaltError {
                    span,
                    message: "Name already in use.".into(),
                });
            }
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

    fn stats(&self) -> SymbolTableStats {
        let mut num_entries = 0;
        let mut size = 0;
        let mut readonly_size = 0;
        let mut readwrite_size = 0;
        for (name, entry) in self.table.iter() {
            if let SymbolTableEntry::C(_) = entry {
                continue;  // Ignore constants.
            }
            num_entries += 1;
            size += SYMBOL_HEADER_LEN;
            size += name.len();
            match entry {
                SymbolTableEntry::D(data) => {
                    size += data.references.len() * 4;
                    if data.mutable {
                        readwrite_size += data.size;
                    } else {
                        readonly_size += data.size;
                    }
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

        SymbolTableStats {
            num_entries,
            size,
            readonly_size,
            readwrite_size,
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

impl LabelLocation {
    fn unwrap_offset(&self) -> u32 {
        match self {
            LabelLocation::Reference(_) => panic!(),
            LabelLocation::Offset(off) => (*off).try_into().unwrap(),
        }
    }
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
    span: Range<usize>,
}

/// Possible types of a register reference.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum RegRefType {
    RegRefAny,
    RegRefInt,
    RegRefWord,
    RegRefHalf,
    RegRefByte,
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

/// Does the given register type match the given reference type?
fn register_type_matches(reg: RegisterType, ref_: RegRefType) -> bool {
    use RegRefType::*;

    match reg {
        RegisterType::Byte => {
            ref_ == RegRefAny || ref_ == RegRefInt || ref_ == RegRefByte
        },
        RegisterType::Half => {
            ref_ == RegRefAny || ref_ == RegRefInt || ref_ == RegRefHalf
        },
        RegisterType::Word => {
            ref_ == RegRefAny || ref_ == RegRefInt || ref_ == RegRefWord
        },
        RegisterType::Float => {
            ref_ == RegRefAny || ref_ == RegRefFloat
        },
    }
}

/// The result of resolving an identifier.
#[derive(Debug)]
enum ResolvedOperand {
    RegRef(u8, RegisterType),
    Literal(LiteralValue),
    SymbolReference,
}

/// Shortcut for checking number of operands.
macro_rules! num_operands {
    ($num:expr, $ops:expr, $span:expr) => {{
        if $ops.len() != $num {
            return Err(SaltError {
                span: $span,
                message: format!("Expected {} operands, but found {}.",
                                    $num, $ops.len()).into(),
            });
        }
    }}
}

/// Shortcut for disallowing SymbolReference resolutions.
macro_rules! no_literals {
    ($span:expr) => {{
        return Err(SaltError {
            span: $span,
            message: "Cannot use a literal here.".into(),
        });
    }}
}

/// Shortcut for disallowing SymbolReference resolutions.
macro_rules! no_symbols {
    ($span:expr) => {{
        return Err(SaltError {
            span: $span,
            message: "Symbol references resolve to addresses, which can't be \
                      used here.".into(),
        });
    }}
}

/// Shortcut for an operand that must be any register reference.
macro_rules! reg_ref_any {
    ($self:ident, $resolved:expr) => {{
        match $resolved.0 {
            ResolvedOperand::Literal(_) => no_literals!($resolved.1),
            ResolvedOperand::RegRef(reg_ref, _) => $self.code.push(reg_ref),
            ResolvedOperand::SymbolReference => no_symbols!($resolved.1),
        }
    }}
}

/// The description of a specific operand.
#[derive(Debug)]
struct OperandDesc {
    op_type: RegRefType,
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
    pub code: Vec<u8>,
    pub warnings: Vec<SaltError>,
}

/// An object code generator.
pub struct CodeGenerator {
    symbol_table: SymbolTable,
    code: Vec<u8>,
    warnings: Vec<SaltError>,
    instructions: Option<Vec<ast::Instruction>>,
}

impl CodeGenerator {
    pub fn new(program: ast::Program,
               extra_consts: &Vec<ast::ConstDecl>) -> SaltResult<Self> {
        // Extract program components.
        let mut consts = program.const_decls();
        consts.reserve_exact(extra_consts.len());
        for extra in extra_consts.iter() {
            consts.push(extra.clone());
        }
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
    pub fn codegen(mut self, entrypoint: bool) -> SaltResult<ObjectCode> {
        // Process all instructions.
        for instruction in self.instructions.take().unwrap().iter() {
            // Resolve any labels pointing here.
            self.resolve_labels(instruction);
            // Codegen the instruction.
            self.codegen_instruction(instruction)?;
        }

        // Generate object code.
        // Size is instructions plus symbol table plus headers.
        let st_stats = self.symbol_table.stats();
        let mut simobj: Vec<u8> = Vec::with_capacity(
              self.code.len()
            + st_stats.size
            + AVG_HEADER_OVERHEAD
        );
        let mut readonly_data: Vec<u8> = Vec::with_capacity(st_stats.readonly_size);
        let mut readwrite_data: Vec<u8> = Vec::with_capacity(st_stats.readwrite_size);

        // Write header and version.
        simobj.write_all(MAGIC_HEADER).unwrap();
        simobj.write_be_u16(ABI_VERSION).unwrap();

        // Write the number of symbols and sections. We'll use up to three
        // sections: instructions, read-only data, and read-write data.
        let num_sections =
              if self.code.len() > 0 {1} else {0}
            + if st_stats.readonly_size > 0 {1} else {0}
            + if st_stats.readwrite_size > 0 {1} else {0};

        if num_sections == 0 {
            return Err(SaltError {
                span: 0..0,
                message: "Cannot compile an empty file.".into(),
            });
        }

        simobj.write_be_u32(st_stats.num_entries.try_into().unwrap()).unwrap();
        simobj.write_be_u32(num_sections).unwrap();

        // Calculate the start offset of each section.
        let instruction_base = SIMOBJ_HEADER_LEN
            + st_stats.size        // Symbol table.
            + SECTION_HEADER_LEN;  // Instruction section header.
        let readonly_base = instruction_base
            - if self.code.len() > 0 {0} else {SECTION_HEADER_LEN}  // Instructions might be missing.
            + self.code.len()      // Instruction section.
            + SECTION_HEADER_LEN;  // Readonly section header.
        let readwrite_base = readonly_base
            - if st_stats.readonly_size > 0 {0} else {SECTION_HEADER_LEN}  // Readonly might be missing.
            + st_stats.readonly_size  // Readonly section.
            + SECTION_HEADER_LEN;     // Readwrite section header.

        // Helper macro for symbol names.
        macro_rules! symbol_name_len {
            ($name:expr, $span:expr) => {{
                match $name.len().try_into() {
                    Ok(val) => val,
                    Err(_) => return Err(SaltError {
                        span: $span,
                        message: "Symbol name too long (max 255 chars).".into(),
                    })
                }
            }}
        }

        // Write symbol table. Iterate through sorted by key, so the results
        // are deterministic.
        let instruction_base: u32 = instruction_base.try_into().unwrap();
        let mut next_readonly: u32 = readonly_base.try_into().unwrap();
        let mut next_readwrite: u32 = readwrite_base.try_into().unwrap();
        for (name, entry) in self.symbol_table.table
                .iter_mut()
                .sorted_by_key(|(k, _)| *k) {
            if let SymbolTableEntry::C(_) = entry {
                continue;  // Ignore constants.
            }
            match entry {
                SymbolTableEntry::D(data) => {
                    // Write symbol type.
                    let type_ = if data.public {SYMBOL_TYPE_PUBLIC} else {SYMBOL_TYPE_INTERNAL};
                    simobj.write_u8(type_).unwrap();
                    // Write symbol value.
                    if data.mutable {
                        simobj.write_be_u32(next_readwrite).unwrap();
                        next_readwrite += u32::try_from(data.size).unwrap();
                        readwrite_data.append(&mut data.initialiser);
                    } else {
                        simobj.write_be_u32(next_readonly).unwrap();
                        next_readonly += u32::try_from(data.size).unwrap();
                        readonly_data.append(&mut data.initialiser);
                    }
                    // Write symbol name length.
                    let name_len = symbol_name_len!(name, data.span.clone());
                    simobj.write_u8(name_len).unwrap();
                    // Write symbol name.
                    simobj.write_all(name.as_bytes()).unwrap();
                    // Write number of references.
                    simobj.write_be_u32(data.references.len().try_into().unwrap()).unwrap();
                    // Write references.
                    for reference in data.references.iter() {
                        simobj.write_be_u32(instruction_base + reference).unwrap();
                    }
                },
                SymbolTableEntry::L(label) => {
                    // Write symbol type.
                    let type_ = if label.public {SYMBOL_TYPE_PUBLIC} else {SYMBOL_TYPE_INTERNAL};
                    simobj.write_u8(type_).unwrap();
                    // Write symbol value.
                    simobj.write_be_u32(instruction_base + label.location.unwrap_offset()).unwrap();
                    // Write symbol name length.
                    let name_len = symbol_name_len!(name, label.span.clone());
                    simobj.write_u8(name_len).unwrap();
                    // Write symbol name.
                    simobj.write_all(name.as_bytes()).unwrap();
                    // Write number of references.
                    simobj.write_be_u32(label.references.len().try_into().unwrap()).unwrap();
                    // Write references.
                    for reference in label.references.iter() {
                        simobj.write_be_u32(instruction_base + reference).unwrap();
                    }
                },
                SymbolTableEntry::E(external) => {
                    // Write symbol type.
                    simobj.write_u8(SYMBOL_TYPE_EXTERNAL).unwrap();
                    // Write symbol value.
                    simobj.write_be_u32(0).unwrap();
                    // Write symbol name length.
                    let name_len = symbol_name_len!(name, external.span.clone());
                    simobj.write_u8(name_len).unwrap();
                    // Write symbol name.
                    simobj.write_all(name.as_bytes()).unwrap();
                    // Write number of references.
                    simobj.write_be_u32(external.references.len().try_into().unwrap()).unwrap();
                    // Write references.
                    for reference in external.references.iter() {
                        simobj.write_be_u32(instruction_base + reference).unwrap();
                    }
                },
                SymbolTableEntry::C(_) => unreachable!(),
            }
        }

        // Sanity check.
        assert_eq!(readonly_data.len(), st_stats.readonly_size);
        assert_eq!(readwrite_data.len(), st_stats.readwrite_size);
        assert_eq!(simobj.len(),
                   usize::try_from(instruction_base).unwrap() - SECTION_HEADER_LEN);

        // Write code section.
        if self.code.len() > 0 {
            let flags = FLAG_EXECUTE | if entrypoint {FLAG_ENTRYPOINT} else {0};
            simobj.write_u8(flags).unwrap();
            simobj.write_be_u32(self.code.len().try_into().unwrap()).unwrap();
            simobj.write_all(self.code.as_slice()).unwrap();
        }

        // Sanity check.
        assert_eq!(simobj.len(), readonly_base - SECTION_HEADER_LEN);

        // Write read-only data section.
        if st_stats.readonly_size > 0 {
            simobj.write_u8(FLAG_READ).unwrap();
            simobj.write_be_u32(readonly_data.len().try_into().unwrap()).unwrap();
            simobj.write_all(readonly_data.as_slice()).unwrap();
        }

        // Sanity check.
        assert_eq!(simobj.len(), readwrite_base - SECTION_HEADER_LEN);

        // Write read-write data section.
        if st_stats.readwrite_size > 0 {
            simobj.write_u8(FLAG_READ | FLAG_WRITE).unwrap();
            simobj.write_be_u32(readwrite_data.len().try_into().unwrap()).unwrap();
            simobj.write_all(readwrite_data.as_slice()).unwrap();
        }

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
        let span: Range<usize> = instruction.syntax().text_range().into();
        let gen_func = match instruction.opcode().as_str() {
            "halt" => Self::gen_halt,
            "pause" => Self::gen_pause,
            "timer" => Self::gen_timer,
            "usermode" => Self::gen_usermode,
            "ireturn" => Self::gen_ireturn,
            "load" => Self::gen_load,
            "store" => Self::gen_store,
            // TODO more
            _ => return Err(SaltError {
                span: instruction.syntax().text_range().into(),
                message: "Unrecognised opcode.".into(),
            }),
        };
        gen_func(self, instruction.operands(), span)
    }

    /// Codegen for the HALT instruction.
    fn gen_halt(&mut self, operands: Vec<ast::Operand>,
                span: Range<usize>) -> SaltResult<()> {
        num_operands!(0, operands, span);
        self.code.push(0x00);
        Ok(())
    }

    /// Codegen for the PAUSE instruction.
    fn gen_pause(&mut self, operands: Vec<ast::Operand>,
                 span: Range<usize>) -> SaltResult<()> {
        num_operands!(0, operands, span);
        self.code.push(0x01);
        Ok(())
    }

    /// Codegen for the TIMER instruction.
    fn gen_timer(&mut self, operands: Vec<ast::Operand>,
                 span: Range<usize>) -> SaltResult<()> {
        num_operands!(1, operands, span);

        let (resolved, op_span) = self.resolve_operand(&operands[0])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                // TIMER LiteralWord.
                self.code.push(0x02);
                self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                // TIMER RegRefWord.
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "TIMER requires a word register reference.".into(),
                    });
                }
                self.code.push(0x03);
                self.code.push(reg_ref);
            }
            ResolvedOperand::SymbolReference => no_symbols!(op_span),
        }

        Ok(())
    }

    /// Codegen for the USERMODE instruction.
    fn gen_usermode(&mut self, operands: Vec<ast::Operand>,
                span: Range<usize>) -> SaltResult<()> {
        num_operands!(0, operands, span);
        self.code.push(0x04);
        Ok(())
    }

    /// Codegen for the IRETURN instruction.
    fn gen_ireturn(&mut self, operands: Vec<ast::Operand>,
                 span: Range<usize>) -> SaltResult<()> {
        num_operands!(0, operands, span);
        self.code.push(0x05);
        Ok(())
    }

    /// Codegen for the LOAD instruction.
    fn gen_load(&mut self, operands: Vec<ast::Operand>,
                 span: Range<usize>) -> SaltResult<()> {
        num_operands!(2, operands, span);

        // Push placeholder opcode.
        let opcode_pos = self.code.len();
        self.code.push(0);

        // First operand: RegRefAny.
        let resolved = self.resolve_operand(&operands[0])?;
        reg_ref_any!(self, resolved);

        // Second operand: address.
        let (resolved, op_span) = self.resolve_operand(&operands[1])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                // LOAD Ref LitAddr
                self.code[opcode_pos] = 0x06;
                self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                // LOAD Ref RefAddr
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "LOAD requires an address (word) \
                                  register reference.".into(),
                    });
                }
                self.code[opcode_pos] = 0x07;
                self.code.push(reg_ref);
            },
            ResolvedOperand::SymbolReference => {
                // LOAD Ref LitAddr
                self.code[opcode_pos] = 0x06;
            }
        }

        Ok(())
    }

    /// Codegen for the STORE instruction.
    fn gen_store(&mut self, operands: Vec<ast::Operand>,
                span: Range<usize>) -> SaltResult<()> {
        num_operands!(2, operands, span);

        // Push placeholder opcode.
        let opcode_pos = self.code.len();
        self.code.push(0);

        // First operand: address.
        let (resolved, op_span) = self.resolve_operand(&operands[0])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                // STORE LitAddr Ref
                self.code[opcode_pos] = 0x08;
                self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                // Store RefAddr Ref
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "STORE requires an address (word) \
                                  register reference.".into(),
                    });
                }
                self.code[opcode_pos] = 0x09;
                self.code.push(reg_ref);
            },
            ResolvedOperand::SymbolReference => {
                // STORE LitAddr Ref
                self.code[opcode_pos] = 0x08;
            }
        }

        // Second operand: RegRefAny.
        let resolved = self.resolve_operand(&operands[1])?;
        reg_ref_any!(self, resolved);

        Ok(())
    }

    /// Resolve an operand. Literals are returned directly, register references
    /// are recognised, known constants are substituted, known data and labels
    /// add a reference to the symbol table and push a zero placeholder to the
    /// code, and unknown identifiers implicitly declare an external symbol.
    ///
    /// If there is an uppercase unknown identifier, this generates a warning as
    /// it looks like a missing constant, which can't be resolved at link time.
    fn resolve_operand(&mut self, operand: &ast::Operand)
                       -> SaltResult<(ResolvedOperand, Range<usize>)> {
        let span: Range<usize> = operand.syntax().text_range().into();

        // Directly resolve a literal, or extract an identifier.
        let ident = match operand.value()? {
            OperandValue::Ident(ident) => ident,
            OperandValue::Lit(literal) =>
                return Ok((ResolvedOperand::Literal(literal), span)),
        };

        // Try and resolve as a register reference.
        if let Some((reg_ref, reg_type)) = get_reg_ref(&ident) {
            return Ok((ResolvedOperand::RegRef(reg_ref, reg_type), span));
        }

        // Try and resolve as symbol.
        if let Some(entry) = self.symbol_table.table.get_mut(&ident) {
            return Ok((match entry {
                SymbolTableEntry::C(constant) => {
                    ResolvedOperand::Literal(constant.value)
                },
                SymbolTableEntry::D(data) => {
                    data.references.push(self.code.len().try_into().unwrap());
                    self.code.write_be_u32(0).unwrap();
                    ResolvedOperand::SymbolReference
                },
                SymbolTableEntry::L(label) => {
                    label.references.push(self.code.len().try_into().unwrap());
                    self.code.write_be_u32(0).unwrap();
                    ResolvedOperand::SymbolReference
                },
                SymbolTableEntry::E(external) => {
                    external.references.push(self.code.len().try_into().unwrap());
                    self.code.write_be_u32(0).unwrap();
                    ResolvedOperand::SymbolReference
                },
            }, span));
        }

        // Unresolved: create a new external symbol.
        // Warn if it looks like a constant.
        if is_uppercase(&ident) {
            self.warnings.push(SaltError {
                span: span.clone(),
                message: "Unresolved symbol creates an external data reference, \
                          but this looks like a constant.".into(),
            });
        }
        let external = External {
            references: vec![self.code.len().try_into().unwrap()],
            span: span.clone(),
        };
        self.symbol_table.table.insert(ident, SymbolTableEntry::E(external));
        self.code.write_be_u32(0).unwrap();
        Ok((ResolvedOperand::SymbolReference, span))
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

/// Check if a string is in uppercase.
fn is_uppercase(string: &str) -> bool {
    for c in string.chars() {
        if c.is_ascii_lowercase() {
            return false;
        }
    }
    return true;
}
