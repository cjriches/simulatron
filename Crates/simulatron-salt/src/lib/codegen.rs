#![allow(clippy::len_zero)]

#[macro_use]
mod instruction_macros;

use itertools::Itertools;
use log::{debug, error, info, trace, warn};
use simulatron_utils::{hexprint, write_be::WriteBE};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::io::Write;
use std::ops::Range;

use crate::ast::{self, ArrayLength, AstNode, LiteralValue, OperandValue, RegisterType};
use crate::error::{SaltError, SaltResult};

// The following constants are used to provide guesses for initial vector
// capacities. Thus, they are important for performance but not correctness.

/// A rough estimate, assuming equal distribution of all instructions and
/// addressing modes.
const AVG_INSTRUCTION_LEN: usize = 4;
/// A very rough guesstimate, considering scalars and vectors.
const AVG_DATA_LEN: usize = 16;
/// A complete guess.
const AVG_SYMBOL_REFERENCES: usize = 8;
/// A rough estimate based on 3 sections.
const AVG_HEADER_OVERHEAD: usize = 32;
/// A complete guess.
const AVG_EXTERNAL_REFERENCES: usize = 32;

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
#[derive(Debug)]
struct SymbolTableStats {
    num_entries: usize,
    size: usize,           // Total size.
    readonly_size: usize,  // Size of all read-only data items.
    readwrite_size: usize, // Size of all read-write data items.
}

impl SymbolTable {
    //noinspection RsSelfConvention
    fn with_capacity(cap: usize) -> Self {
        Self {
            table: HashMap::with_capacity(cap),
        }
    }

    /// Iterate mutably through only labels.
    fn iter_labels(&mut self) -> impl Iterator<Item = (&String, &mut Label)> {
        self.table.iter_mut().filter_map(|(name, entry)| {
            if let SymbolTableEntry::L(label) = entry {
                Some((name, label))
            } else {
                None
            }
        })
    }

    /// Iterate immutably through only private symbols.
    fn iter_private(&self) -> impl Iterator<Item = (&String, &SymbolTableEntry)> {
        self.table.iter().filter(|(_, entry)| {
            match entry {
                SymbolTableEntry::C(const_) => !const_.public,
                SymbolTableEntry::D(data) => !data.public,
                SymbolTableEntry::L(label) => !label.public,
                SymbolTableEntry::E(_) => false, // Always public.
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
                continue; // Ignore constants.
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
                }
                SymbolTableEntry::L(label) => {
                    size += label.references.len() * 4;
                }
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

/// Constant symbol table entry.
#[derive(Debug)]
struct Constant {
    public: bool,
    value: LiteralValue,
    span: Range<usize>,
    used: bool,
}

/// Static data symbol table entry.
#[derive(Debug)]
struct Data {
    public: bool,
    mutable: bool,
    size: usize,
    initialiser: Vec<u8>,
    span: Range<usize>,
    references: Vec<u32>,
}

/// The location a label points to.
#[derive(Debug)]
enum LabelLocation {
    Reference(ast::Instruction), // Not yet resolved to a code offset.
    Offset(usize),               // Resolved to a code offset.
}

impl LabelLocation {
    /// Assume this `LabelLocation` is an `Offset`, and return the offset.
    fn unwrap_offset(&self) -> u32 {
        match self {
            LabelLocation::Reference(_) => panic!(),
            LabelLocation::Offset(off) => (*off).try_into().unwrap(),
        }
    }
}

/// Label symbol table entry.
#[derive(Debug)]
struct Label {
    public: bool,
    location: LabelLocation,
    span: Range<usize>,
    references: Vec<u32>,
}

/// External symbol symbol table entry.
#[derive(Debug)]
struct External {
    references: Vec<u32>,
    span: Range<usize>,
}

/// Possible types of a register reference.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum RegRef {
    Any,
    Int,
    Byte,
    Half,
    Word,
    Float,
}

/// Does the given register type match the given reference type?
fn register_type_matches(reg: RegisterType, ref_: RegRef) -> bool {
    match reg {
        RegisterType::Byte => ref_ == RegRef::Any || ref_ == RegRef::Int || ref_ == RegRef::Byte,
        RegisterType::Half => ref_ == RegRef::Any || ref_ == RegRef::Int || ref_ == RegRef::Half,
        RegisterType::Word => ref_ == RegRef::Any || ref_ == RegRef::Int || ref_ == RegRef::Word,
        RegisterType::Float => ref_ == RegRef::Any || ref_ == RegRef::Float,
    }
}

/// The result of resolving an operand.
#[derive(Debug)]
enum ResolvedOperand {
    RegRef(u8, RegisterType),
    Literal(LiteralValue),
    SymbolReference,
}

/// The result of successful codegen: object code and warnings.
#[derive(Debug)]
pub struct CodegenSuccess {
    pub simobj: Vec<u8>,
    pub warnings: Vec<SaltError>,
}

/// The result of unsuccessful codegen: errors and warnings.
#[derive(Debug)]
pub struct CodegenFailure {
    pub errors: Vec<SaltError>,
    pub warnings: Vec<SaltError>,
}

/// An object code generator.
pub struct CodeGenerator {
    symbol_table: SymbolTable,
    code: Vec<u8>, // Binary generated from the instructions.
    errors: Vec<SaltError>,
    warnings: Vec<SaltError>,
    instructions: Option<Vec<ast::Instruction>>,
}

/// Unwrap the given SaltResult or add it as an error and continue the current loop.
macro_rules! ok_or_continue {
    ($self:ident, $result:expr) => {{
        match $result {
            Ok(value) => value,
            Err(e) => {
                $self.error(e);
                continue;
            }
        }
    }};
}

impl CodeGenerator {
    /// Create a new CodeGenerator with the given AST and list of extra constants.
    pub fn new(
        program: ast::Program,
        extra_consts: &Vec<ast::ConstDecl>,
    ) -> Result<Self, CodegenFailure> {
        // Extract program components.
        let mut consts = program.const_decls();
        consts.reserve_exact(extra_consts.len());
        for extra in extra_consts.iter() {
            // A ConstDecl is just a typed SyntaxNode, so they're cheap to clone.
            consts.push(extra.clone());
        }
        let data = program.data_decls();
        let labels = program.labels();
        let instructions = program.instructions();

        // Allocate data structures with estimates for the capacity.
        let symbol_table = SymbolTable::with_capacity(
            consts.len() + data.len() + labels.len() + AVG_EXTERNAL_REFERENCES,
        );
        let code: Vec<u8> = Vec::with_capacity(
            data.len() * AVG_DATA_LEN + instructions.len() * AVG_INSTRUCTION_LEN,
        );
        let mut generator = Self {
            symbol_table,
            code,
            errors: Vec::new(),
            warnings: Vec::new(),
            instructions: Some(instructions),
        };

        // Populate symbol table.
        generator.add_constants(&consts);
        generator.add_data(&data);
        generator.add_labels(&labels);

        // Check for errors.
        if generator.errors.is_empty() {
            Ok(generator)
        } else {
            Err(CodegenFailure {
                errors: generator.errors,
                warnings: generator.warnings,
            })
        }
    }

    /// Add an error.
    fn error(&mut self, e: SaltError) {
        error!("Producing error: {}", e.message.as_ref());
        self.errors.push(e);
    }

    /// Add a warning.
    fn warning(&mut self, w: SaltError) {
        warn!("Producing warning: {}", w.message.as_ref());
        self.warnings.push(w);
    }

    /// Run the code generator, consuming it.
    pub fn run(mut self, entrypoint: bool) -> Result<CodegenSuccess, CodegenFailure> {
        let simobj = self.codegen(entrypoint);
        info!(
            "Code generated:\n{}",
            hexprint::pretty_print_hex_block_zero(&simobj)
        );
        if self.errors.is_empty() {
            Ok(CodegenSuccess {
                simobj,
                warnings: self.warnings,
            })
        } else {
            Err(CodegenFailure {
                errors: self.errors,
                warnings: self.warnings,
            })
        }
    }

    fn codegen(&mut self, entrypoint: bool) -> Vec<u8> {
        // Process all instructions, taking temporary ownership of the
        // vector to satisfy the borrow checker.
        let instructions = self.instructions.take().unwrap();
        for instruction in instructions.iter() {
            // Resolve any labels pointing here.
            self.resolve_labels(instruction);
            // Codegen the instruction.
            ok_or_continue!(self, self.codegen_instruction(instruction));
        }
        self.instructions = Some(instructions);

        // Generate warnings for any unused private symbols.
        let mut unused_warnings = Vec::new();
        for (_, entry) in self.symbol_table.iter_private() {
            let (used, span) = match entry {
                SymbolTableEntry::C(const_) => (const_.used, const_.span.clone()),
                SymbolTableEntry::D(data) => (!data.references.is_empty(), data.span.clone()),
                SymbolTableEntry::L(label) => (!label.references.is_empty(), label.span.clone()),
                SymbolTableEntry::E(_) => unreachable!(),
            };
            if !used {
                unused_warnings.push(SaltError {
                    span,
                    message: "Unused private symbol.".into(),
                });
            }
        }
        // Sort by appearance in file.
        for w in unused_warnings.into_iter().sorted_by_key(|w| w.span.start) {
            self.warning(w);
        }

        // Generate object code.
        // Size is instructions plus symbol table plus headers.
        let st_stats = self.symbol_table.stats();
        info!("Symbol table stats: {:#?}", st_stats);
        let mut simobj: Vec<u8> =
            Vec::with_capacity(self.code.len() + st_stats.size + AVG_HEADER_OVERHEAD);
        let mut readonly_data: Vec<u8> = Vec::with_capacity(st_stats.readonly_size);
        let mut readwrite_data: Vec<u8> = Vec::with_capacity(st_stats.readwrite_size);

        // Write header and version.
        simobj.write_all(MAGIC_HEADER).unwrap();
        simobj.write_be_u16(ABI_VERSION).unwrap();

        // Write the number of symbols and sections. We'll use up to three
        // sections: instructions, read-only data, and read-write data.
        let num_sections = {
            (if self.code.len() > 0 {
                info!("Producing a code section.");
                1
            } else {
                info!("Code section empty: skipping.");
                0
            }) + (if st_stats.readonly_size > 0 {
                info!("Producing a read-only section.");
                1
            } else {
                info!("Read-only section empty: skipping.");
                0
            }) + (if st_stats.readwrite_size > 0 {
                info!("Producing a read-write section.");
                1
            } else {
                info!("Read-write section empty: skipping.");
                0
            })
        };

        if num_sections == 0 {
            self.error(SaltError {
                span: 0..0,
                message: "Cannot compile an empty file.".into(),
            });
            return simobj;
        }

        simobj
            .write_be_u32(st_stats.num_entries.try_into().unwrap())
            .unwrap();
        simobj.write_be_u32(num_sections).unwrap();

        // Calculate the start offset of each section.
        let instruction_base = SIMOBJ_HEADER_LEN
            + st_stats.size
            + SECTION_HEADER_LEN * usize::try_from(num_sections).unwrap();
        let readonly_base = instruction_base + self.code.len();
        let readwrite_base = readonly_base + st_stats.readonly_size;

        // Helper macro for symbol names.
        macro_rules! symbol_name_len {
            ($name:expr, $span:expr) => {{
                match $name.len().try_into() {
                    Ok(val) => val,
                    Err(_) => {
                        // Using `self.error` here causes overlapping mutable
                        // references. To avoid an awkward workaround, do the
                        // log and push manually here.
                        let e = SaltError {
                            span: $span,
                            message: "Symbol name too long (max 255 chars).".into(),
                        };
                        error!("Producing error: {}", e.message.as_ref());
                        self.errors.push(e);
                        continue;
                    }
                }
            }};
        }

        // Write symbol table.
        let instruction_base: u32 = instruction_base.try_into().unwrap();
        let mut next_readonly: u32 = readonly_base.try_into().unwrap();
        let mut next_readwrite: u32 = readwrite_base.try_into().unwrap();
        debug!("Instruction base: {:#X}", instruction_base);
        debug!("Read-only base: {:#X}", next_readonly);
        debug!("Read-write base: {:#X}", next_readwrite);
        // Iterate through sorted by key, so the results are deterministic.
        for (name, entry) in self
            .symbol_table
            .table
            .iter_mut()
            .sorted_by_key(|(k, _)| *k)
        {
            if let SymbolTableEntry::C(_) = entry {
                continue; // Ignore constants.
            }
            match entry {
                SymbolTableEntry::D(data) => {
                    // Write symbol type.
                    let type_ = if data.public {
                        SYMBOL_TYPE_PUBLIC
                    } else {
                        SYMBOL_TYPE_INTERNAL
                    };
                    simobj.write_u8(type_).unwrap();
                    // Write symbol value.
                    let value = if data.mutable {
                        simobj.write_be_u32(next_readwrite).unwrap();
                        let value = next_readwrite;
                        next_readwrite += u32::try_from(data.size).unwrap();
                        readwrite_data.append(&mut data.initialiser);
                        value
                    } else {
                        simobj.write_be_u32(next_readonly).unwrap();
                        let value = next_readonly;
                        next_readonly += u32::try_from(data.size).unwrap();
                        readonly_data.append(&mut data.initialiser);
                        value
                    };
                    // Write symbol name length.
                    let name_len = symbol_name_len!(name, data.span.clone());
                    simobj.write_u8(name_len).unwrap();
                    // Write symbol name.
                    simobj.write_all(name.as_bytes()).unwrap();
                    // Write number of references.
                    simobj
                        .write_be_u32(data.references.len().try_into().unwrap())
                        .unwrap();
                    // Write references.
                    for reference in data.references.iter() {
                        simobj.write_be_u32(instruction_base + reference).unwrap();
                    }
                    trace!(
                        "Writing {} data symbol {} with type '{}', value \
                           {:#X} with {} references.",
                        if data.mutable { "mutable" } else { "immutable" },
                        name,
                        char::from(type_),
                        value,
                        data.references.len()
                    );
                }
                SymbolTableEntry::L(label) => {
                    // Write symbol type.
                    let type_ = if label.public {
                        SYMBOL_TYPE_PUBLIC
                    } else {
                        SYMBOL_TYPE_INTERNAL
                    };
                    simobj.write_u8(type_).unwrap();
                    // Write symbol value.
                    let value = instruction_base + label.location.unwrap_offset();
                    simobj.write_be_u32(value).unwrap();
                    // Write symbol name length.
                    let name_len = symbol_name_len!(name, label.span.clone());
                    simobj.write_u8(name_len).unwrap();
                    // Write symbol name.
                    simobj.write_all(name.as_bytes()).unwrap();
                    // Write number of references.
                    simobj
                        .write_be_u32(label.references.len().try_into().unwrap())
                        .unwrap();
                    // Write references.
                    for reference in label.references.iter() {
                        simobj.write_be_u32(instruction_base + reference).unwrap();
                    }
                    trace!(
                        "Writing label {} with type '{}', value \
                           {:#X} with {} references.",
                        name,
                        char::from(type_),
                        value,
                        label.references.len()
                    );
                }
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
                    simobj
                        .write_be_u32(external.references.len().try_into().unwrap())
                        .unwrap();
                    // Write references.
                    for reference in external.references.iter() {
                        simobj.write_be_u32(instruction_base + reference).unwrap();
                    }
                    trace!(
                        "Writing symbol {} with type '{}', with {} references.",
                        name,
                        char::from(SYMBOL_TYPE_EXTERNAL),
                        external.references.len()
                    );
                }
                SymbolTableEntry::C(_) => unreachable!(),
            }
        }
        debug!("Written symbol table.");

        // Write section headers.
        if self.code.len() > 0 {
            let flags = FLAG_EXECUTE | if entrypoint { FLAG_ENTRYPOINT } else { 0 };
            simobj.write_u8(flags).unwrap();
            simobj
                .write_be_u32(self.code.len().try_into().unwrap())
                .unwrap();
            debug!("Written code section header.");
        }
        if st_stats.readonly_size > 0 {
            simobj.write_u8(FLAG_READ).unwrap();
            simobj
                .write_be_u32(readonly_data.len().try_into().unwrap())
                .unwrap();
            debug!("Written read-only section header.");
        }
        if st_stats.readwrite_size > 0 {
            simobj.write_u8(FLAG_READ | FLAG_WRITE).unwrap();
            simobj
                .write_be_u32(readwrite_data.len().try_into().unwrap())
                .unwrap();
            debug!("Written read-write section header.");
        }

        // Sanity check.
        assert_eq!(readonly_data.len(), st_stats.readonly_size);
        assert_eq!(readwrite_data.len(), st_stats.readwrite_size);
        assert_eq!(simobj.len(), usize::try_from(instruction_base).unwrap());

        // Write code section.
        if self.code.len() > 0 {
            simobj.write_all(self.code.as_slice()).unwrap();
            debug!("Written code section.");
        }

        // Sanity check.
        assert_eq!(simobj.len(), readonly_base);

        // Write read-only data section.
        if st_stats.readonly_size > 0 {
            simobj.write_all(readonly_data.as_slice()).unwrap();
            debug!("Written read-only section.");
        }

        // Sanity check.
        assert_eq!(simobj.len(), readwrite_base);

        // Write read-write data section.
        if st_stats.readwrite_size > 0 {
            simobj.write_all(readwrite_data.as_slice()).unwrap();
            debug!("Written read-write section.");
        }

        simobj
    }

    fn add_constants(&mut self, consts: &[ast::ConstDecl]) {
        for const_ in consts.iter() {
            let name = const_.name();
            let public = const_.public();
            let value = ok_or_continue!(self, const_.value());

            if !is_uppercase(&name) {
                self.warning(SaltError {
                    span: const_.name_span(),
                    message: "Constant names are expected to be \
                              UPPER_SNAKE_CASE."
                        .into(),
                });
            }

            let existing = self.symbol_table.table.insert(
                name,
                SymbolTableEntry::C(Constant {
                    public,
                    value,
                    span: const_.name_span(),
                    used: false,
                }),
            );
            if existing.is_some() {
                self.error(SaltError {
                    span: const_.name_span(),
                    message: "Name already in use.".into(),
                });
            }
        }
    }

    fn add_data(&mut self, data_decls: &[ast::DataDecl]) {
        for data in data_decls.iter() {
            let name = data.name();
            let public = data.public();
            let mutable = data.mutable();
            let type_ = data.type_();
            let base_size = type_.base_size();
            let type_dims = ok_or_continue!(self, type_.dimensions());
            let (initialiser, mut init_dims) = ok_or_continue!(self, data.initialiser());

            if !is_lowercase(&name) {
                self.warning(SaltError {
                    span: data.name_span(),
                    message: "Data names are expected to be \
                              lower_snake_case."
                        .into(),
                });
            }

            // Check the number of dimensions matches.
            if type_dims.len() != init_dims.len() {
                self.error(SaltError {
                    span: type_.span(),
                    message: format!(
                        "Dimensions mismatch. Type specifies {} dimensions but \
                         initialiser has {}.",
                        type_dims.len(),
                        init_dims.len()
                    )
                    .into(),
                });
                continue;
            }
            // Unify the type dimensions with the initialiser dimensions.
            let mut okay = true;
            for i in 0..type_dims.len() {
                if let ArrayLength::Literal(d) = type_dims[i] {
                    if d < init_dims[i] {
                        self.error(SaltError {
                            span: data.init_span(),
                            message: "Initialiser too long for \
                                      stated dimensions."
                                .into(),
                        });
                        okay = false;
                        break;
                    }
                    // If the initialiser is shorter than the type, we want
                    // to preserve the empty space.
                    init_dims[i] = d;
                }
            }
            if !okay {
                continue;
            }

            // Calculate the total size.
            let mut size = base_size;
            for dim in init_dims {
                size = match size.checked_mul(dim) {
                    Some(val) => val,
                    None => {
                        self.error(SaltError {
                            span: type_.span(),
                            message: "Array size is out of range.".into(),
                        });
                        okay = false;
                        break;
                    }
                };
            }
            if !okay {
                continue;
            }

            // Calculate the full initialiser.
            let initialiser = {
                let mut buf = Vec::with_capacity(size);
                for literal in initialiser.iter() {
                    let bytes = match base_size {
                        1 => self.value_as_byte(literal, data.init_span()),
                        2 => self.value_as_half(literal, data.init_span()),
                        4 => self.value_as_word_or_float(literal, data.init_span()),
                        _ => unreachable!(),
                    };
                    let mut bytes = match bytes {
                        Some(bytes) => bytes,
                        None => {
                            self.error(SaltError {
                                span: data.name_span(),
                                message: "Initialiser too big for type.".into(),
                            });
                            break;
                        }
                    };
                    buf.append(&mut bytes);
                }
                if buf.len() > size {
                    self.error(SaltError {
                        span: data.name_span(),
                        message: "Initialiser too big for type.".into(),
                    });
                    continue;
                }
                buf.resize(size, 0);
                buf
            };

            let existing = self.symbol_table.table.insert(
                name,
                SymbolTableEntry::D(Data {
                    public,
                    mutable,
                    size,
                    initialiser,
                    span: data.name_span(),
                    references: Vec::with_capacity(AVG_SYMBOL_REFERENCES),
                }),
            );
            if existing.is_some() {
                self.error(SaltError {
                    span: data.name_span(),
                    message: "Name already in use.".into(),
                });
            }
        }
    }

    fn add_labels(&mut self, labels: &[ast::Label]) {
        for label in labels.iter() {
            let name = label.name();
            let public = label.public();
            let instruction = ok_or_continue!(self, label.instruction());
            let span: Range<usize> = label.syntax().text_range().into();

            let existing = self.symbol_table.table.insert(
                name,
                SymbolTableEntry::L(Label {
                    public,
                    location: LabelLocation::Reference(instruction),
                    span: span.clone(),
                    references: Vec::with_capacity(AVG_SYMBOL_REFERENCES),
                }),
            );
            if existing.is_some() {
                self.error(SaltError {
                    span,
                    message: "Name already in use.".into(),
                });
            }
        }
    }

    /// Resolve any labels pointing to the given instruction, making them point
    /// to an offset of the current code buffer length.
    fn resolve_labels(&mut self, instruction: &ast::Instruction) {
        for (name, label) in self.symbol_table.iter_labels() {
            if let LabelLocation::Reference(referenced_instruction) = &label.location {
                if instruction == referenced_instruction {
                    let offset = self.code.len();
                    debug!("Resolved label {} to code offset {:#X}", name, offset);
                    label.location = LabelLocation::Offset(offset);
                }
            }
        }
    }

    /// Perform codegen for a single instruction.
    fn codegen_instruction(&mut self, instruction: &ast::Instruction) -> SaltResult<()> {
        let span: Range<usize> = instruction.syntax().text_range().into();

        // Shortcut macro.
        macro_rules! def {
            ($name:expr, $addr_mode:ident, $opcodes:expr) => {{
                debug!("Generating code for {}.", $name);
                $addr_mode!(self, $opcodes, instruction.operands(), span)
            }};
        }

        let (opcode, op_span) = instruction.opcode();
        match opcode.as_str() {
            "halt" => def!("halt", i_none, 0x00),
            "pause" => def!("pause", i_none, 0x01),
            "timer" => def!("timer", i_w, (0x02, 0x03)),
            "usermode" => def!("usermode", i_none, 0x04),
            "ireturn" => def!("ireturn", i_none, 0x05),
            "load" => def!("load", i_BHWF_a, (0x06, 0x07)),
            "store" => def!("store", i_a_BHWF, (0x08, 0x09)),
            "copy" => def!("copy", i_BHWF_bhwf, (0x0A, 0x0B)),
            "swap" => def!("swap", i_BHWF_a, (0x0C, 0x0D)),
            "push" => def!("push", i_BHWF, 0x0E),
            "pop" => def!("pop", i_BHWF, 0x0F),
            "blockcopy" => def!(
                "blockcopy",
                i_w_a_a,
                (0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17)
            ),
            "blockset" => def!(
                "blockset",
                i_w_a_b,
                (0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F)
            ),
            "negate" => def!("negate", i_BHWF, 0x20),
            "add" => def!("add", i_BHWF_bhwf, (0x21, 0x22)),
            "addcarry" => def!("addcarry", i_BHW_bhw, (0x23, 0x24)),
            "sub" => def!("sub", i_BHWF_bhwf, (0x25, 0x26)),
            "subborrow" => def!("subborrow", i_BHW_bhw, (0x27, 0x28)),
            "mult" => def!("mult", i_BHWF_bhwf, (0x29, 0x2A)),
            "sdiv" => def!("sdiv", i_BHWF_bhwf, (0x2B, 0x2C)),
            "udiv" => def!("udiv", i_BHW_bhw, (0x2D, 0x2E)),
            "srem" => def!("srem", i_BHWF_bhwf, (0x2F, 0x30)),
            "urem" => def!("urem", i_BHW_bhw, (0x31, 0x32)),
            "not" => def!("not", i_BHW, 0x33),
            "and" => def!("and", i_BHW_bhw, (0x34, 0x35)),
            "or" => def!("or", i_BHW_bhw, (0x36, 0x37)),
            "xor" => def!("xor", i_BHW_bhw, (0x38, 0x39)),
            "lshift" => def!("lshift", i_BHW_b, (0x3A, 0x3B)),
            "srshift" => def!("srshift", i_BHW_b, (0x3C, 0x3D)),
            "urshift" => def!("urshift", i_BHW_b, (0x3E, 0x3F)),
            "lrot" => def!("lrot", i_BHW_b, (0x40, 0x41)),
            "rrot" => def!("rrot", i_BHW_b, (0x42, 0x43)),
            "lrotcarry" => def!("lrotcarry", i_BHW_b, (0x44, 0x45)),
            "rrotcarry" => def!("rrotcarry", i_BHW_b, (0x46, 0x47)),
            "jump" => def!("jump", i_a, (0x48, 0x49)),
            "compare" => def!("compare", i_BHWF_bhwf, (0x4A, 0x4B)),
            "blockcmp" => def!(
                "blockcmp",
                i_w_a_a,
                (0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x53)
            ),
            "jequal" => def!("jequal", i_a, (0x54, 0x55)),
            "jnotequal" => def!("jnotequal", i_a, (0x56, 0x57)),
            "sjgreater" => def!("sjgreater", i_a, (0x58, 0x59)),
            "sjgreatereq" => def!("sjgreatereq", i_a, (0x5A, 0x5B)),
            "ujgreater" => def!("ujgreater", i_a, (0x5C, 0x5D)),
            "ujgreatereq" => def!("ujgreatereq", i_a, (0x5E, 0x5F)),
            "sjlesser" => def!("sjlesser", i_a, (0x60, 0x61)),
            "sjlessereq" => def!("sjlessereq", i_a, (0x62, 0x63)),
            "ujlesser" => def!("ujlesser", i_a, (0x64, 0x65)),
            "ujlessereq" => def!("ujlessereq", i_a, (0x66, 0x67)),
            "call" => def!("call", i_a, (0x68, 0x69)),
            "return" => def!("return", i_none, 0x6A),
            "syscall" => def!("syscall", i_none, 0x6B),
            "sconvert" => def!("sconvert", i_WF_WF, 0x6C),
            "uconvert" => def!("uconvert", i_WF_WF, 0x6D),
            _ => Err(SaltError {
                span: op_span,
                message: "Unrecognised opcode.".into(),
            }),
        }
    }

    /// Resolve an operand. Literals are returned directly, register references
    /// are recognised, known constants are substituted, known data and labels
    /// add a reference to the symbol table and push a zero placeholder to the
    /// code, and unknown identifiers implicitly declare an external symbol.
    ///
    /// If there is an uppercase unknown identifier, this generates a warning as
    /// it looks like a missing constant, which can't be resolved at link time.
    ///
    /// This is called from the various i_* macros in `instruction_macros`.
    fn resolve_operand(
        &mut self,
        operand: &ast::Operand,
    ) -> SaltResult<(ResolvedOperand, Range<usize>)> {
        let span: Range<usize> = operand.syntax().text_range().into();

        // Directly resolve a literal, or extract an identifier.
        let ident = match operand.value()? {
            OperandValue::Ident(ident) => ident,
            OperandValue::Lit(literal) => {
                trace!("Operand resolved to {:?}", literal);
                return Ok((ResolvedOperand::Literal(literal), span));
            }
        };

        // Try and resolve as a register reference.
        if let Some((reg_ref, reg_type)) = get_reg_ref(&ident) {
            trace!("Operand resolved to register reference {}", ident);
            return Ok((ResolvedOperand::RegRef(reg_ref, reg_type), span));
        }

        // Try and resolve as symbol.
        if let Some(entry) = self.symbol_table.table.get_mut(&ident) {
            return Ok((
                match entry {
                    SymbolTableEntry::C(constant) => {
                        trace!("Operand resolved to constant {}", ident);
                        constant.used = true;
                        ResolvedOperand::Literal(constant.value.clone())
                    }
                    SymbolTableEntry::D(data) => {
                        trace!("Operand resolved to static data {}", ident);
                        data.references.push(self.code.len().try_into().unwrap());
                        self.code.write_be_u32(0).unwrap();
                        ResolvedOperand::SymbolReference
                    }
                    SymbolTableEntry::L(label) => {
                        trace!("Operand resolved to label {}", ident);
                        label.references.push(self.code.len().try_into().unwrap());
                        self.code.write_be_u32(0).unwrap();
                        ResolvedOperand::SymbolReference
                    }
                    SymbolTableEntry::E(external) => {
                        trace!("Operand resolved to external symbol {}", ident);
                        external
                            .references
                            .push(self.code.len().try_into().unwrap());
                        self.code.write_be_u32(0).unwrap();
                        ResolvedOperand::SymbolReference
                    }
                },
                span,
            ));
        }

        // Unresolved: create a new external symbol.
        // Warn if it looks like a constant.
        if is_uppercase(&ident) {
            self.warning(SaltError {
                span: span.clone(),
                message: "Unresolved symbol creates an external data reference, \
                          but this looks like a constant."
                    .into(),
            });
        }
        let external = External {
            references: vec![self.code.len().try_into().unwrap()],
            span: span.clone(),
        };
        trace!("Operand created new external symbol {}", ident);
        self.symbol_table
            .table
            .insert(ident, SymbolTableEntry::E(external));
        self.code.write_be_u32(0).unwrap();
        Ok((ResolvedOperand::SymbolReference, span))
    }

    fn push_value_as_reg_type(
        &mut self,
        val: &LiteralValue,
        reg_type: RegisterType,
        span: Range<usize>,
    ) -> SaltResult<()> {
        match reg_type {
            RegisterType::Byte => self.value_as_byte(val, span.clone()),
            RegisterType::Half => self.value_as_half(val, span.clone()),
            RegisterType::Word => self.value_as_word(val, span.clone()),
            RegisterType::Float => self.value_as_float(val, span.clone()),
        }
        .map(|mut bytes| self.code.append(&mut bytes))
        .ok_or_else(|| SaltError {
            span,
            message: "Literal too big for register.".into(),
        })
    }

    fn value_as_byte(&mut self, val: &LiteralValue, span: Range<usize>) -> Option<Vec<u8>> {
        let (value, min_reg_type) = self.resolve_literal(val, span)?;
        if let RegisterType::Byte = min_reg_type {
            Some(vec![value as u8])
        } else {
            None
        }
    }

    fn value_as_half(&mut self, val: &LiteralValue, span: Range<usize>) -> Option<Vec<u8>> {
        let (value, min_reg_type) = self.resolve_literal(val, span)?;
        if let RegisterType::Byte | RegisterType::Half = min_reg_type {
            Some((value as u16).to_be_bytes().to_vec())
        } else {
            None
        }
    }

    fn value_as_word(&mut self, val: &LiteralValue, span: Range<usize>) -> Option<Vec<u8>> {
        let (value, min_reg_type) = self.resolve_literal(val, span.clone())?;
        if let RegisterType::Float = min_reg_type {
            self.warning(SaltError {
                span,
                message: "Float literal being used as an integer.".into(),
            });
        }

        Some(value.to_be_bytes().to_vec())
    }

    fn value_as_float(&mut self, val: &LiteralValue, span: Range<usize>) -> Option<Vec<u8>> {
        let (value, min_reg_type) = self.resolve_literal(val, span.clone())?;
        if let RegisterType::Float = min_reg_type { /* no-op */
        } else {
            self.warning(SaltError {
                span,
                message: "Integer literal being used as a float.".into(),
            });
        }

        Some(value.to_be_bytes().to_vec())
    }

    fn value_as_word_or_float(
        &mut self,
        val: &LiteralValue,
        span: Range<usize>,
    ) -> Option<Vec<u8>> {
        let (value, _) = self.resolve_literal(val, span)?;
        Some(value.to_be_bytes().to_vec())
    }

    /// Resolve the given literal, either returning the actual literal inside,
    /// or finding the value of a sizeof.
    fn resolve_literal(
        &mut self,
        val: &LiteralValue,
        span: Range<usize>,
    ) -> Option<(u32, RegisterType)> {
        match *val {
            LiteralValue::Lit {
                value,
                min_reg_type,
            } => Some((value, min_reg_type)),
            LiteralValue::Sizeof { ref ident } => {
                if let Some(SymbolTableEntry::D(data)) = self.symbol_table.table.get(ident) {
                    let size: u32 = data.size.try_into().unwrap();
                    let min_reg_type = ast::minimum_reg_type(size as i64);
                    Some((size, min_reg_type))
                } else {
                    self.error(SaltError {
                        span,
                        message: "No corresponding data declaration found \
                                  in this file."
                            .into(),
                    });
                    None
                }
            }
        }
    }
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
    true
}

/// Check if a string is in lowercase.
fn is_lowercase(string: &str) -> bool {
    for c in string.chars() {
        if c.is_ascii_uppercase() {
            return false;
        }
    }
    true
}
