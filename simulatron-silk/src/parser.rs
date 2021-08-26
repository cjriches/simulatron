use log::{trace, debug, info};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};

use crate::data::{ObjectFile, pretty_print_hex_block,
                  Section, SymbolTable, SymbolTableEntry,
                  SYMBOL_TYPE_EXTERNAL, symbol_type_name};
use crate::error::{OFError, OFResult};
use crate::read_be::ReadBE;

// File header constants.
const MAGIC_HEADER: [u8; 6] = *b"SIMOBJ";
const ABI_VERSION: u16 = 0x0001;

/// All the data that can be known about a section from just reading its header.
struct SectionHeader {
    flags: u8,
    length: u32,
}

#[derive(Debug)]
pub struct Parser<S> {
    source: S,
    bytes_read: u32,
}

impl<S: ReadBE> Parser<S> {
    /// Parse the given byte stream.
    pub fn parse(source: S) -> OFResult<ObjectFile> {
        Self::new(source).run()
    }

    /// Construct a new parser from the given byte stream.
    fn new(source: S) -> Self {
        Self {
            source,
            bytes_read: 0,
        }
    }

    /// Run the parser.
    fn run(&mut self) -> OFResult<ObjectFile> {
        // Verify magic and version.
        let mut magic = [0; 6];
        self.read_buffer(&mut magic)?;
        assert_or_error!(magic == MAGIC_HEADER, "Invalid magic header.");
        let version = self.read_u16()?;
        assert_or_error!(version == ABI_VERSION, "Unsupported ABI version.");
        info!("Verified magic header and ABI version.");

        // Read the rest of the header.
        let num_symbol_table_entries = self.read_u32()?.try_into().unwrap();
        debug!("Symbol table has {} entries.", num_symbol_table_entries);
        let num_section_headers = self.read_u32()?.try_into().unwrap();
        debug!("There are {} section headers.", num_section_headers);

        // Parse the symbol table.
        info!("About to parse symbol table.");
        let mut symbols = self.parse_symbol_table(num_symbol_table_entries)?;
        info!("Symbol table parsed successfully.");

        // Parse the section headers.
        info!("About to parse section headers.");
        let section_headers = self.parse_section_headers(num_section_headers)?;
        info!("Section headers parsed successfully.");

        // Parse the sections.
        info!("About to parse sections.");
        let sections_start = self.bytes_read;
        let sections = self.parse_sections(&section_headers, sections_start)?;
        info!("Sections parsed successfully.");

        // Relocate and verify all the symbols.
        info!("About to relocate and verify symbols.");
        for symbol in symbols.iter_mut() {
            relocate_and_verify_symbol(symbol, sections_start, &sections)?;
        }
        info!("Symbols relocated and verified successfully.");

        // Return the result.
        Ok(ObjectFile {
            symbols,
            sections,
        })
    }

    /// Parse the symbol table.
    fn parse_symbol_table(&mut self, num_entries: usize) -> OFResult<SymbolTable> {
        let mut table = HashMap::with_capacity(num_entries);
        for i in 0..num_entries {
            debug!("About to parse symbol {}.", i);
            // Read the symbol type.
            let symbol_type = self.read_u8()?;
            trace!("Symbol type: {}", symbol_type);
            let symbol_type_str = symbol_type_name(symbol_type)?;
            debug!("Symbol type: {}", symbol_type_str);
            // Read the symbol value.
            let value = self.read_u32()?;
            debug!("Symbol value: {:#010X}", value);
            // Read the name.
            let name_len = self.read_u8()?.into();
            debug!("Symbol name length: {}", name_len);
            assert_or_error!(name_len > 0,
                "Symbol name cannot be the empty string.");
            let mut name_buf = vec![0; name_len];
            self.read_buffer(&mut name_buf)?;
            let name = validate_symbol_name(name_buf)?;
            debug!("Symbol name: {}", name);
            // Check the name is unique.
            assert_or_error!(!table.contains_key(&name),
                format!("Multiple definitions for symbol {}.", name));
            // Read the number of references.
            let num_refs = self.read_u32()?.try_into().unwrap();
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

    /// Parse a list of symbol references.
    fn parse_references(&mut self, num_refs: usize) -> OFResult<Vec<u32>> {
        // Read in all the offsets.
        let mut offsets = Vec::with_capacity(num_refs);
        for i in 0..num_refs {
            let offset = self.read_u32()?;
            trace!("Reference {}: {:#010X}", i, offset);
            offsets.push(offset);
        }
        Ok(offsets)
    }

    /// Parse the section headers.
    fn parse_section_headers(&mut self, num_headers: usize) -> OFResult<Vec<SectionHeader>> {
        let mut headers = Vec::with_capacity(num_headers);
        for i in 0..num_headers {
            debug!("About to parse section header {}.", i);
            // Read the flags.
            let flags = self.read_u8()?;
            debug!("Flags: {:08b}", flags);
            // Read the section length.
            let length = self.read_u32()?;
            debug!("Section is {} bytes long.", length);
            // Add the section header to the vector.
            headers.push(SectionHeader {
                flags,
                length,
            });
            info!("Parsed section header {} successfully.", i);
        }
        Ok(headers)
    }

    /// Parse the sections themselves.
    fn parse_sections(&mut self, headers: &Vec<SectionHeader>,
                      sections_start: u32) -> OFResult<Vec<Section>> {
        let mut sections = Vec::with_capacity(headers.len());
        for (i, header) in headers.iter().enumerate() {
            debug!("About to parse section {}.", i);
            // Read the data.
            let section_start = self.bytes_read;
            let mut data = vec![0; header.length.try_into().unwrap()];
            self.read_buffer(&mut data)?;
            trace!("Section data:\n{}", pretty_print_hex_block(&data));
            // Add the section to the vector.
            sections.push(Section {
                flags: header.flags,
                start: section_start - sections_start,
                length: header.length,
                data,
            });
            info!("Parsed section {} successfully.", i);
        }
        Ok(sections)
    }

    fn read_u8(&mut self) -> OFResult<u8> {
        self.source.read_u8().and_then(|val| {
            self.bytes_read += 1;
            Ok(val)
        }).map_err(Into::into)
    }

    fn read_u16(&mut self) -> OFResult<u16> {
        self.source.read_be_u16().and_then(|val| {
            self.bytes_read += 2;
            Ok(val)
        }).map_err(Into::into)
    }

    fn read_u32(&mut self) -> OFResult<u32> {
        self.source.read_be_u32().and_then(|val| {
            self.bytes_read += 4;
            Ok(val)
        }).map_err(Into::into)
    }

    fn read_buffer(&mut self, buf: &mut [u8]) -> OFResult<()> {
        self.source.read_exact(buf).and_then(|val| {
            self.bytes_read += u32::try_from(buf.len()).unwrap();
            Ok(val)
        }).map_err(Into::into)
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
                return match String::from_utf8(name) {
                    Ok(s) => {
                        Err(OFError::new(format!("Invalid symbol name: {}", s)))
                    },
                    Err(_) => {
                        Err(OFError::new("Invalid symbol name (unprintable)."))
                    },
                }
            },
        }
    }

    // Strings of this format are guaranteed to be valid UTF-8.
    Ok(String::from_utf8(name).unwrap())
}

/// Relocate the given symbol's value and references according to the given
/// `section_start`, verify that the value and all references point within
/// a section, and additionally verify that references point to a
/// zero-filled location.
fn relocate_and_verify_symbol(symbol: (&String, &mut SymbolTableEntry),
                              sections_start: u32,
                              sections: &Vec<Section>) -> OFResult<()> {
    debug!("About to relocate and verify symbol {}", symbol.0);
    // Relocate and verify the value.
    symbol.1.value = match symbol.1.value {
        None => None,
        Some(val) => {
            let relocated = try_sub(val, sections_start)?;
            let file_length = match sections.last() {
                None => 0,
                Some(last_section) => last_section.start + last_section.length,
            };
            assert_or_error!(relocated < file_length,
                format!("Address too large: {:#010X}", val));
            Some(relocated)
        },
    };
    debug!("Relocated and verified value.");

    // Relocate and verify the references.
    for (i, reference) in symbol.1.references.iter_mut().enumerate() {
        trace!("Relocating and verifying reference {}.", i);
        // Relocate the reference.
        *reference = try_sub(*reference, sections_start)?;
        // Verify it points to zero.
        let section = Section::find(sections, *reference)
            .ok_or(OFError::new(
                format!("Address too large: {:#010X}", *reference + sections_start)))?;
        let section_offset = *reference - section.start;
        for i in 0..4 {
            assert_or_error!(section.data[usize::try_from(section_offset).unwrap() + i] == 0,
                "Symbol reference was non-zero.");
        }
    }
    info!("Relocated and verified symbol {}", symbol.0);

    Ok(())
}

/// Try and relocate x by y, failing gracefully if the result is negative.
fn try_sub(x: u32, y: u32) -> OFResult<u32> {
    x.checked_sub(y).ok_or(OFError::new(
        format!("Address too small: {:#010X}", x)
    ))
}
