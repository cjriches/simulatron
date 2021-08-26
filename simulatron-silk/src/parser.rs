use log::{trace, debug, info};
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::{Seek, SeekFrom};

use crate::data::{ObjectFile, OFError, OFResult, pretty_print_hex_block,
                  Section, SymbolTable, SymbolTableEntry,
                  SYMBOL_TYPE_EXTERNAL, symbol_type_name};
use crate::read_be::ReadBE;

// File header constants.
const MAGIC_HEADER: [u8; 6] = *b"SIMOBJ";
const ABI_VERSION: u16 = 0x0001;

#[derive(Debug)]
pub struct Parser<S> {
    source: S,
}

impl<S> Parser<S>
    where S: ReadBE + Seek
{
    /// Parse the given byte stream.
    pub fn parse(source: S) -> OFResult<ObjectFile> {
        Self::new(source).run()
    }

    /// Construct a new parser from the given byte stream.
    fn new(source: S) -> Self {
        Self {
            source,
        }
    }

    /// Run the parser.
    fn run(&mut self) -> OFResult<ObjectFile> {
        // Seek to beginning.
        self.source.seek(SeekFrom::Start(0))?;

        // Verify magic and version.
        let mut magic = [0; 6];
        self.source.read_exact(&mut magic)?;
        assert_or_error!(magic == MAGIC_HEADER, "Invalid magic header.");
        let version = self.source.read_be_u16()?;
        assert_or_error!(version == ABI_VERSION, "Unsupported ABI version.");
        info!("Verified magic header and ABI version.");

        // Read the rest of the header.
        let symbol_table_start = self.source.read_be_u32()?;
        debug!("Symbol table starts at {:#010X}", symbol_table_start);
        let num_symbol_table_entries = self.source.read_be_u32()?.try_into().unwrap();
        debug!("Symbol table has {} entries.", num_symbol_table_entries);
        let section_headers_start = self.source.read_be_u32()?;
        debug!("Section headers start at {:#010X}", section_headers_start);
        let num_section_headers = self.source.read_be_u32()?.try_into().unwrap();
        debug!("There are {} section headers.", num_section_headers);

        // Parse the sections.
        info!("About to parse sections.");
        let sections = self.parse_sections(section_headers_start,
                                           num_section_headers)?;
        info!("Sections parsed successfully.");

        // Parse the symbol table.
        info!("About to parse symbol table.");
        let symbols = self.parse_symbol_table(symbol_table_start,
                                               num_symbol_table_entries)?;
        info!("Symbol table parsed successfully.");

        // Return the result.
        Ok(ObjectFile {
            symbols,
            sections,
        })
    }

    /// Parse the section headers and sections. Produces a vector of sections,
    /// sorted by their location in the file.
    fn parse_sections(&mut self, base: u32,
                      num_headers: usize) -> OFResult<Vec<Section>> {
        // Seek to the start of section headers.
        self.source.seek(SeekFrom::Start(base as u64))?;

        // Process each section.
        let mut sections = Vec::with_capacity(num_headers);
        for i in 0..num_headers {
            info!("About to parse section {}.", i);
            // Read the flags.
            let flags = self.source.read_u8()?;
            debug!("Flags: {:08b}", flags);
            // Skip the padding.
            self.source.seek(SeekFrom::Current(3))?;
            // Read the section location.
            let section_start = self.source.read_be_u32()?;
            debug!("Section starts at {:#010X}", section_start);
            let section_length = self.source.read_be_u32()?.try_into().unwrap();
            debug!("Section is {} bytes long.", section_length);
            // Remember the current position and seek to the section.
            let current_pos = self.source.stream_position()?;
            self.source.seek(SeekFrom::Start(section_start as u64))?;
            // Read the section.
            let mut data = vec![0; section_length];
            self.source.read_exact(&mut data)?;
            trace!("Section data:\n{}", pretty_print_hex_block(&data));
            // Restore the previous position.
            self.source.seek(SeekFrom::Start(current_pos))?;
            // Add the sector to the vector.
            sections.push(Section {
                flags,
                start: section_start,
                length: section_length as u32,
                data,
            });
            info!("Parsed section {} successfully.", i);
        }

        // Sort sections by their location within the file.
        sections.sort_unstable_by_key(|section| section.start);

        Ok(sections)
    }

    /// Parse the symbol table.
    fn parse_symbol_table(&mut self, base: u32,
                          num_entries: usize) -> OFResult<SymbolTable> {
        // Seek to the start of the symbol table.
        self.source.seek(SeekFrom::Start(base as u64))?;

        // Process each section.
        let mut table = HashMap::with_capacity(num_entries);
        for i in 0..num_entries {
            info!("About to parse symbol {}.", i);
            // Read the symbol type.
            let symbol_type = self.source.read_u8()?;
            trace!("Symbol type: {}", symbol_type);
            let symbol_type_str = symbol_type_name(symbol_type)?;
            debug!("Symbol type: {}", symbol_type_str);
            // Read the symbol value.
            let value = self.source.read_be_u32()?;
            debug!("Symbol value: {:#010X}", value);
            // Read the name.
            let name_len = self.source.read_u8()?.into();
            debug!("Symbol name length: {}", name_len);
            assert_or_error!(name_len > 0,
                "Symbol name cannot be the empty string.");
            let mut name_buf = vec![0; name_len];
            self.source.read_exact(&mut name_buf)?;
            let name = validate_symbol_name(name_buf)?;
            debug!("Symbol name: {}", name);
            // Check the name is unique.
            assert_or_error!(!table.contains_key(&name),
                format!("Multiple definitions for symbol {}.", name));
            // Read the number of references.
            let num_refs = self.source.read_be_u32()?.try_into().unwrap();
            debug!("Symbol has {} references.", num_refs);
            // Read the references.
            let references = self.parse_references(num_refs)?;
            debug!("All references parsed successfully.");
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
            let was_present = table.insert(name, entry);
            assert!(was_present.is_none());  // Sanity check.
            info!("Parsed symbol {} successfully.", i);
        }

        Ok(table)
    }

    /// Parse a list of symbol references. This validates that the reference
    /// points to a zero-filled location within a section.
    fn parse_references(&mut self, num_refs: usize) -> OFResult<Vec<u32>> {
        // Read in all the offsets.
        let mut offsets = Vec::with_capacity(num_refs);
        for i in 0..num_refs {
            let offset = self.source.read_be_u32()?;
            trace!("Reference {}: {:#010X}", i, offset);
            offsets.push(offset);
        }

        // Remember the current file position.
        let current_pos = self.source.stream_position()?;

        // Check that each referenced location is currently zero.
        for (i, offset) in offsets.iter().enumerate() {
            trace!("Checking reference {}.", i);
            self.source.seek(SeekFrom::Start(*offset as u64))?;
            let value = self.source.read_be_u32()?;
            assert_or_error!(value == 0, "Symbol reference was non-zero.");
        }

        // Restore the file position.
        self.source.seek(SeekFrom::Start(current_pos))?;

        Ok(offsets)
    }
}

/// Validate a symbol name, either returning it as a String, or returning
/// an error.
fn validate_symbol_name(name: Vec<u8>) -> OFResult<String> {
    // The length should be statically guaranteed by the object code format,
    // but do a sanity check anyway.
    assert!(name.len() < 256);

    // Valid bytes are in the inclusive ranges 48-57, 65-90, 95, or 97-122.
    for byte in name.iter() {
        match byte {
            48..=57 | 65..=90 | 95 | 97..=122 => {},
            _ => {
                match String::from_utf8(name) {
                    Ok(s) => {
                        debug!("Invalid name: {}", s);
                    },
                    Err(_) => {
                        debug!("Invalid name (unprintable).");
                    },
                }
                return Err(OFError::new("Invalid symbol name."));
            },
        }
    }

    // Strings of this format are guaranteed to be valid UTF-8.
    Ok(String::from_utf8(name).unwrap())
}
