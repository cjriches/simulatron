#[macro_use]
mod error;
mod read_be;

use std::collections::HashMap;
use std::convert::TryInto;
use std::io::{self, Seek, SeekFrom, Read};

use error::{OFError, OFResult};
use read_be::ReadBE;

// File header constants.
const MAGIC_HEADER: [u8; 6] = *b"SIMOBJ";
const ABI_VERSION: u16 = 0x0001;

// Symbol type constants.
const SYMBOL_TYPE_INTERNAL: u8 = b'I';
const SYMBOL_TYPE_PUBLIC: u8 = b'P';
const SYMBOL_TYPE_EXTERNAL: u8 = b'E';

// Section header flags.
const FLAG_ENTRYPOINT: u8 = 0x01;
const FLAG_READ: u8 = 0x04;
const FLAG_WRITE: u8 = 0x08;
const FLAG_EXECUTE: u8 = 0x10;

// Simulatron-specific constants.
pub const ROM_SIZE: usize = 512;

/// An object code section.
#[derive(Debug)]
struct Section {
    flags: u8,
    data: Vec<u8>,
}

/// A location within an object code section.
#[derive(Debug)]
struct Location {
    section_index: usize,
    section_offset: usize,
}

/// A symbol table entry.
#[derive(Debug)]
struct SymbolTableEntry {
    symbol_type: u8,
    value: Option<u32>,
    locations: Vec<Location>,
}

/// A symbol table.
#[derive(Debug)]
struct SymbolTable(HashMap<String, SymbolTableEntry>);

/// A whole parsed object file. Can be combined with others, and then processed
/// into a specific target.
#[derive(Debug)]
pub struct ObjectFile {
    symbols: SymbolTable,
    sections: Vec<Section>,
}

impl ObjectFile {
    /// Parse a new object file from a byte stream.
    pub fn new<S>(source: &mut S) -> OFResult<Self>
        where S: ReadBE + Seek
    {
        // Seek to beginning.
        source.seek(SeekFrom::Start(0))?;

        // Verify magic and version.
        let mut magic = [0; 6];
        source.read_exact(&mut magic)?;
        assert_or_error!(magic == MAGIC_HEADER, "Invalid magic header.");
        let version = source.read_be_u16()?;
        assert_or_error!(version == ABI_VERSION, "Unsupported ABI version.");

        // Read the rest of the header.
        let symbol_table_start = source.read_be_u32()?;
        let num_symbol_table_entries = source.read_be_u32()?.try_into().unwrap();
        let section_headers_start = source.read_be_u32()?;
        let num_section_headers = source.read_be_u32()?.try_into().unwrap();

        // Parse the sections.
        let sections = Self::parse_sections(source, section_headers_start,
                                            num_section_headers)?;

        // Parse the symbol table.
        let symbols = Self::parse_symbol_table(source, symbol_table_start,
                                               num_symbol_table_entries,
                                               &sections)?;

        // Return the result.
        Ok(ObjectFile {
            symbols,
            sections: sections.into_iter().map(|triple| triple.0).collect(),
        })
    }

    /// Parse the section headers and sections. Produces a vector of (Section,
    /// base, length) triples, where the base and length are used in
    /// `parse_symbol_table` to determine which section an offset belongs to.
    fn parse_sections<S>(source: &mut S, base: u32,
                         num_headers: usize) -> OFResult<Vec<(Section, u32, u32)>>
        where S: ReadBE + Seek
    {
        // Seek to the start of section headers.
        source.seek(SeekFrom::Start(base as u64))?;

        // Process each section.
        let mut sections = Vec::with_capacity(num_headers);
        for _ in 0..num_headers {
            // Read the flags.
            let flags = source.read_u8()?;
            // Skip the padding.
            source.seek(SeekFrom::Current(3))?;
            // Read the section location.
            let section_start = source.read_be_u32()?;
            let section_length = source.read_be_u32()?.try_into().unwrap();
            // Remember the current position and seek to the section.
            let current_pos = source.stream_position()?;
            source.seek(SeekFrom::Start(section_start as u64))?;
            // Read the section.
            let mut data = vec![0; section_length];
            source.read_exact(&mut data)?;
            // Restore the previous position.
            source.seek(SeekFrom::Start(current_pos))?;
            // Add the sector to the vector.
            sections.push((Section {
                flags,
                data,
            }, section_start, section_length as u32));
        }

        Ok(sections)
    }

    /// Parse the symbol table.
    fn parse_symbol_table<S>(source: &mut S, base: u32, num_entries: usize,
                             sections: &Vec<(Section, u32, u32)>)
                             -> OFResult<SymbolTable>
        where S: ReadBE + Seek
    {
        todo!()
    }

    /// Combine the symbols and sections of two object files.
    pub fn combine(self, other: Self) -> OFResult<Self> {
        todo!()
    }

    /// Process an object file into a ROM image.
    pub fn link_as_rom(self) -> OFResult<[u8; ROM_SIZE]> {
        todo!()
    }

    /// Process an object file into a disk image.
    pub fn link_as_disk(self) -> OFResult<Vec<u8>> {
        todo!()
    }
}
