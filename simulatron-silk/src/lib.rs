#[macro_use]
mod error;
mod read_be;

use std::collections::HashMap;
use std::convert::TryInto;
use std::io::{Seek, SeekFrom};

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
#[derive(Debug, PartialEq, Eq)]
struct Section {
    flags: u8,
    data: Vec<u8>,
}

/// A location within an object code section.
#[derive(Debug, PartialEq, Eq)]
struct Location {
    section_index: usize,
    section_offset: usize,
}

/// A symbol table entry.
#[derive(Debug, PartialEq, Eq)]
struct SymbolTableEntry {
    symbol_type: u8,
    value: Option<u32>,
    references: Vec<Location>,
}

/// A symbol table.
#[derive(Debug, PartialEq, Eq)]
struct SymbolTable(HashMap<String, SymbolTableEntry>);

/// A whole parsed object file. Can be combined with others, and then processed
/// into a specific target.
#[derive(Debug, PartialEq, Eq)]
pub struct ObjectFile {
    symbols: SymbolTable,
    sections: Vec<Section>,
}

/// A section plus its location in an object file.
type SpannedSection = (Section, u32, u32);

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

    /// Parse the section headers and sections. Produces a vector of `(Section,
    /// base, length)` triples, where the base and length are used in
    /// `parse_symbol_table` to determine which section an offset belongs to.
    /// This vector is sorted by `base`.
    fn parse_sections<S>(source: &mut S, base: u32,
                         num_headers: usize) -> OFResult<Vec<SpannedSection>>
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

        // Sort sections by section_start.
        sections.sort_unstable_by_key(|section| section.1);

        Ok(sections)
    }

    /// Parse the symbol table.
    fn parse_symbol_table<S>(source: &mut S, base: u32, num_entries: usize,
                             sections: &Vec<SpannedSection>)
                             -> OFResult<SymbolTable>
        where S: ReadBE + Seek
    {
        // Seek to the start of the symbol table.
        source.seek(SeekFrom::Start(base as u64))?;

        // Process each section.
        let mut table = HashMap::with_capacity(num_entries);
        for _ in 0..num_entries {
            // Read the symbol type.
            let symbol_type = source.read_u8()?;
            // Read the symbol value.
            let value = source.read_be_u32()?;
            // Read the name.
            let name_len = source.read_u8()?.into();
            let mut name_buf = vec![0; name_len];
            source.read_exact(&mut name_buf)?;
            let name = Self::validate_symbol_name(name_buf)?;
            // Check the name is unique.
            assert_or_error!(!table.contains_key(&name),
                format!("Multiple definitions for symbol {}.", name));
            // Read the number of references.
            let num_refs = source.read_be_u32()?.try_into().unwrap();
            // Read the references.
            let references = Self::parse_references(source,
                                                    num_refs, sections)?;
            // Add the entry to the map.
            let value= if symbol_type == SYMBOL_TYPE_EXTERNAL {
                None
            } else {
                Some(value)
            };
            let entry = SymbolTableEntry {
                symbol_type,
                value,
                references,
            };
            let was_present = table.insert(name.clone(), entry);
            assert!(was_present.is_none());  // Sanity check.
        }

        Ok(SymbolTable(table))
    }

    /// Parse a list of symbol references. This validates that the reference
    /// points to a zero-filled location within a section.
    fn parse_references<S>(source: &mut S, num_refs: usize,
                           sections: &Vec<SpannedSection>)
                           -> OFResult<Vec<Location>>
        where S: ReadBE + Seek
    {
        // Allocate the vector.
        let mut refs = Vec::with_capacity(num_refs);

        // Read in all the offsets.
        let mut offsets = Vec::with_capacity(num_refs);
        for _ in 0..num_refs {
            offsets.push(source.read_be_u32()?);
        }

        // Remember the current file position.
        let current_pos = source.stream_position()?;

        // Check that each referenced location is currently zero, and turn it
        // into a Location.
        for offset in &offsets {
            source.seek(SeekFrom::Start(*offset as u64))?;
            let value = source.read_be_u32()?;
            assert_or_error!(value == 0, "Symbol reference was non-zero.");
            // Find the location from the file offset.
            let mut i = 0;
            let location = loop {
                // Check if the offset is within this base and length.
                let (_, base, length) = &sections[i];
                if *offset >= *base && *offset < *base + *length {
                    break Location {
                        section_index: i,
                        section_offset: (*offset - base)
                            .try_into().unwrap(),
                    };
                }
                i += 1;
                assert_or_error!(i < sections.len(),
                    "Symbol reference pointed outside of a section.");
            };
            // Add the location to the vector.
            refs.push(location);
        }

        // Restore the file position.
        source.seek(SeekFrom::Start(current_pos))?;

        Ok(refs)
    }

    /// Validate a symbol name, either returning it as a String, or returning
    /// an error.
    fn validate_symbol_name(name: Vec<u8>) -> OFResult<String> {
        // The length should be statically guaranteed by the object code format,
        // but do a sanity check anyway.
        assert!(name.len() < 256);

        // Valid bytes are in the inclusive ranges 48-57, 65-90, 95, or 97-122.
        for byte in &name {
            match byte {
                48..=57 | 65..=90 | 95 | 97..=122 => {},
                _ => return Err(OFError::new("Invalid symbol name.")),
            }
        }

        // Strings of this format are guaranteed to be valid UTF-8.
        Ok(String::from_utf8(name).unwrap())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_file(path: &str, expected: &ObjectFile) {
        let mut file = std::fs::File::open(path).unwrap();
        let parsed = ObjectFile::new(&mut file).unwrap();
        assert_eq!(parsed, *expected);
    }

    /// The simplest possible file: no symbols, one entrypoint section
    /// containing a single byte.
    #[test]
    fn test_minimal() {
        let section = Section {
            flags: FLAG_ENTRYPOINT | FLAG_EXECUTE,
            data: vec![0],
        };

        let symbols = SymbolTable(HashMap::new());

        let expected = ObjectFile {
            symbols,
            sections: vec![section],
        };

        parse_file("examples/minimal.simobj", &expected);
    }

    /// A file with a single symbol called foo, and a single entrypoint section.
    #[test]
    fn test_single_symbol() {
        let section = Section {
            flags: FLAG_ENTRYPOINT | FLAG_EXECUTE,
            data: vec![0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF,
                       0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00],
        };

        let mut symbols = SymbolTable(HashMap::with_capacity(1));
        symbols.0.insert(String::from("foo"), SymbolTableEntry {
            symbol_type: SYMBOL_TYPE_INTERNAL,
            value: Some(0x12345678),
            references: vec![Location { section_index: 0, section_offset: 0x02 },
                             Location { section_index: 0, section_offset: 0x0C },
            ],
        });

        let expected = ObjectFile {
            symbols,
            sections: vec![section],
        };

        parse_file("examples/single-symbol.simobj", &expected);
    }

    /// A file with a single symbol called foo, and multiple sections.
    #[test]
    fn test_multi_section() {
        let section0 = Section {
            flags: FLAG_READ | FLAG_WRITE,
            data: vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00,
                       0xFF, 0xFF, 0xFF, 0xFF],
        };
        let section1 = Section {
            flags: FLAG_ENTRYPOINT | FLAG_EXECUTE,
            data: vec![0x11, 0x00, 0x00, 0x00, 0x00, 0x11, 0x11, 0x11],
        };

        let mut symbols = SymbolTable(HashMap::with_capacity(1));
        symbols.0.insert(String::from("foo"), SymbolTableEntry {
            symbol_type: SYMBOL_TYPE_INTERNAL,
            value: Some(0x12345678),
            references: vec![Location { section_index: 0, section_offset: 0x04 },
                             Location { section_index: 1, section_offset: 0x01 },
            ],
        });

        let expected = ObjectFile {
            symbols,
            sections: vec![section0, section1],
        };

        parse_file("examples/multi-section.simobj", &expected);
    }

    /// A file with multiple symbols, and a single entrypoint section.
    #[test]
    fn test_multi_symbol() {
        let section = Section {
            flags: FLAG_ENTRYPOINT | FLAG_EXECUTE,
            data: vec![0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                       0x00, 0x00, 0x00, 0x00, 0x00, 0xFF],
        };

        let mut symbols = SymbolTable(HashMap::with_capacity(2));
        symbols.0.insert(String::from("bar"), SymbolTableEntry {
            symbol_type: SYMBOL_TYPE_INTERNAL,
            value: Some(0x12345678),
            references: vec![Location { section_index: 0, section_offset: 0x01 },
                             Location { section_index: 0, section_offset: 0x05 },
            ],
        });
        symbols.0.insert(String::from("foobaz"), SymbolTableEntry {
            symbol_type: SYMBOL_TYPE_PUBLIC,
            value: Some(0x9ABCDEF0),
            references: vec![Location { section_index: 0, section_offset: 0x09 }],
        });

        let expected = ObjectFile {
            symbols,
            sections: vec![section],
        };

        parse_file("examples/multi-symbol.simobj", &expected);
    }
}
