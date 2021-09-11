use itertools::Itertools;
use simulatron_utils::hexprint::pretty_print_hex_block;
use std::collections::HashMap;
use std::convert::TryInto;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};

use crate::error::{OFError, OFResult};

// Symbol type constants.
pub const SYMBOL_TYPE_INTERNAL: u8 = b'I';
pub const SYMBOL_TYPE_PUBLIC: u8 = b'P';
pub const SYMBOL_TYPE_EXTERNAL: u8 = b'E';

pub fn symbol_type_name(symbol_type: u8) -> OFResult<&'static str> {
    match symbol_type {
        SYMBOL_TYPE_INTERNAL => Ok("Internal"),
        SYMBOL_TYPE_PUBLIC => Ok("Public"),
        SYMBOL_TYPE_EXTERNAL => Ok("External"),
        _ => Err(OFError::new("Invalid symbol type.")),
    }
}

// Section header flags.
pub const FLAG_ENTRYPOINT: u8 = 0x01;
pub const FLAG_READ: u8 = 0x04;
pub const FLAG_WRITE: u8 = 0x08;
pub const FLAG_EXECUTE: u8 = 0x10;
pub const INVALID_FLAGS: u8 = !(FLAG_ENTRYPOINT | FLAG_READ
                              | FLAG_WRITE | FLAG_EXECUTE);

// Simulatron-specific constants.
pub const ROM_SIZE: usize = 512;
pub const DISK_ALIGN: usize = 4096;
pub const ROM_BASE: u32 = 0x40;
pub const DISK_BASE: u32 = 0x4000;

/// An object code section.
#[derive(Debug)]
pub struct Section {
    pub flags: u8,
    pub start: u32,
    pub length: u32,
    pub data: Vec<u8>,
}

impl Display for Section {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Write flags.
        writeln!(f, "flags: {:08b} start: {:#010X} length: {:#010X}",
                 self.flags, self.start, self.length)?;
        // Write data.
        write!(f, "{}", pretty_print_hex_block(&self.data,
                                               self.start.try_into().unwrap()))
    }
}

impl Section {
    /// Compare the given section's range against the given address.
    fn compare_address(&self, index: u32) -> Ordering {
        if index < self.start {
            Ordering::Greater  // The section is greater than the index.
        } else if index < self.start + self.length {
            Ordering::Equal    // The section contains the index.
        } else {
            Ordering::Less     // The section is lesser than the index.
        }
    }

    /// Find the section containing the given address within its range, and
    /// return a reference
    pub fn find(sections: &Vec<Section>, address: u32) -> Option<&Section> {
        sections.binary_search_by(|sec| {
            sec.compare_address(address)
        }).ok().map(|i| &sections[i])
    }

    /// Find the section containing the given address within its range, and
    /// return a mutable reference.
    pub fn find_mut(sections: &mut Vec<Section>, address: u32) -> Option<&mut Section> {
        sections.binary_search_by(|sec| {
            sec.compare_address(address)
        }).ok().map(move |i| &mut sections[i])
    }
}

/// A symbol table entry.
#[derive(Debug)]
pub struct SymbolTableEntry {
    pub symbol_type: u8,
    pub value: Option<u32>,
    pub references: Vec<u32>,
}

/// A symbol table.
pub type SymbolTable = HashMap<String, SymbolTableEntry>;

/// A whole parsed object file. Can be combined with others, and then processed
/// into a specific target.
#[derive(Debug)]
pub struct ObjectFile {
    pub(crate) symbols: SymbolTable,    // We want to expose the fields to this
    pub(crate) sections: Vec<Section>,  // crate, but not beyond.
}

impl Display for ObjectFile {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "---Symbols---")?;
        // Sort the hashmap keys so the order is deterministic.
        for (name, symbol) in self.symbols.iter()
                .sorted_by_key(|(k, _)| *k) {
            let value_str = match symbol.value {
                None => String::new(),
                Some(val) => format!(" {:#010X} ", val),
            };
            writeln!(f, "{} {}{}", name, char::from(symbol.symbol_type), value_str)?;
            for reference in symbol.references.iter() {
                writeln!(f, "  {:#010X}", reference)?;
            }
        }
        writeln!(f, "---Sections---")?;
        for (i, section) in self.sections.iter().enumerate() {
            writeln!(f, "Section {} {}", i, section)?;
        }

        Ok(())
    }
}
