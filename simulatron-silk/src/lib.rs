#[macro_use]
mod error;
mod read_be;

use itertools::Itertools;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{Display, Formatter};
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

impl Display for Section {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Write flags.
        writeln!(f, "flags: {:010b}", self.flags)?;
        // Write data.
        for i in 0..self.data.len() {
            // Write each byte as two hex digits.
            write!(f, "{:02X}", self.data[i])?;
            if i + 1 == self.data.len() {
                break;  // Don't append final whitespace.
            }
            match i % 16 {
                15 => write!(f, "\n"),          // Newline after 16 bytes
                3 | 7 | 11 => write!(f, "  "),  // Double-space after 4 bytes
                _ => write!(f, " "),            // Single-space between bytes
            }?;
        }
        Ok(())
    }
}

/// A location within an object code section.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Location {
    section_index: usize,
    section_offset: usize,
}

impl Display for Location {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Section {} {:#010X}", self.section_index, self.section_offset)
    }
}

/// A symbol table entry.
#[derive(Debug, PartialEq, Eq)]
struct SymbolTableEntry {
    symbol_type: u8,
    value: Option<u32>,
    references: Vec<Location>,
}

/// A symbol table.
type SymbolTable = HashMap<String, SymbolTableEntry>;

/// A whole parsed object file. Can be combined with others, and then processed
/// into a specific target.
#[derive(Debug, PartialEq, Eq)]
pub struct ObjectFile {
    symbols: SymbolTable,
    sections: Vec<Section>,
}

impl Display for ObjectFile {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "---Symbols---")?;
        for (name, symbol) in self.symbols.iter()
                                                           .sorted_by_key(|(k, _)| *k) {
            let value_str = match symbol.value {
                None => String::new(),
                Some(val) => format!(" {:#010X} ", val),
            };
            writeln!(f, "{} {}{}", name,char::from(symbol.symbol_type), value_str)?;
            for reference in &symbol.references {
                writeln!(f, "  {}", reference)?;
            }
        }
        writeln!(f, "---Sections---")?;
        for (i, section) in self.sections.iter().enumerate() {
            writeln!(f, "Section {} {}", i, section)?;
        }

        Ok(())
    }
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

        Ok(table)
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
    pub fn combine(mut self, mut other: Self) -> OFResult<Self> {
        // Remember the number of sections in self.
        let self_sections = self.sections.len();
        // Add the other sections.
        self.sections.reserve(other.sections.len());
        self.sections.append(&mut other.sections);
        // Add the other symbols.
        self.symbols.reserve(other.symbols.len());
        for (name, mut new_entry) in other.symbols {
            // Relocate the references.
            for reference in &mut new_entry.references {
                reference.section_index += self_sections;
            }
            // If this symbol is new, we can add it straight away. Otherwise,
            // we must either:
            // a) Rename to avoid collision between an internal symbol and
            //    another (of any type).
            // b) Resolve between an external and a public symbol.
            // c) Reject two public symbols.
            match self.symbols.get_mut(&name) {
                None => {
                    self.symbols.insert(name, new_entry);
                },
                Some(existing_entry) => {
                    // Case a) rename an internal symbol.
                    if new_entry.symbol_type == SYMBOL_TYPE_INTERNAL {
                        // Rename the new entry before inserting.
                        let new_name = gen_non_conflicting_name(&self.symbols, &name)?;
                        let was_present = self.symbols.insert(new_name, new_entry);
                        assert!(was_present.is_none());
                    } else if existing_entry.symbol_type == SYMBOL_TYPE_INTERNAL {
                        // Rename the existing entry then insert.
                        let new_name = gen_non_conflicting_name(&self.symbols, &name)?;
                        let old = self.symbols.remove(&name).unwrap();
                        let was_present = self.symbols.insert(new_name, old)
                            .or(self.symbols.insert(name, new_entry));
                        assert!(was_present.is_none());
                    // Case b) resolve external and public.
                    } else if new_entry.symbol_type == SYMBOL_TYPE_EXTERNAL
                            && existing_entry.symbol_type == SYMBOL_TYPE_PUBLIC {
                        // Eat the new entry's references.
                        existing_entry.references.append(&mut new_entry.references);
                    } else if new_entry.symbol_type == SYMBOL_TYPE_PUBLIC
                            && existing_entry.symbol_type == SYMBOL_TYPE_EXTERNAL {
                        // Eat the new entry's references, take its value, and
                        // change type to public.
                        existing_entry.references.append(&mut new_entry.references);
                        assert!(existing_entry.value.is_none());
                        assert!(new_entry.value.is_some());
                        existing_entry.value = new_entry.value;
                        existing_entry.symbol_type = SYMBOL_TYPE_PUBLIC;
                    // Case c) reject two public symbols.
                    } else if new_entry.symbol_type == SYMBOL_TYPE_PUBLIC
                            && existing_entry.symbol_type == SYMBOL_TYPE_PUBLIC {
                        return Err(OFError::new(
                            format!("Multiple definitions for symbol {}.", name)));
                    } else {
                        // Sanity check.
                        unreachable!();
                    }
                }
            }
        }

        Ok(self)
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

fn gen_non_conflicting_name<V>(map: &HashMap<String, V>,
                               base: &String) -> OFResult<String> {
    for suffix in 0..=u32::MAX {
        let candidate = format!("{}{}", base, suffix);
        if !map.contains_key(&candidate) {
            return Ok(candidate);
        }
    }
    Err(OFError::new(
        format!("Failed to rename symbol {} to a unique value.", base)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use insta::assert_display_snapshot;
    use std::fs::File;

    /// Parse the given list of files and combine them.
    macro_rules! parse_files {
        // Single file case.
        ($f:expr) => {{
            let mut f = File::open($f).unwrap();
            ObjectFile::new(&mut f)
        }};

        // Multiple files.
        ($f0:expr, $($fs:expr),+) => {{
            // Open and parse the first.
            let mut f0 = File::open($f0).unwrap();
            let parsed0 = ObjectFile::new(&mut f0);
            // Fold with the remaining files.
            [$($fs),*].iter().fold(parsed0, |parsed, path| {
                // If the previous parse succeeded, parse the next one.
                parsed.and_then(|of1| {
                    let mut f = File::open(path).unwrap();
                    ObjectFile::new(&mut f).and_then(|of2| {
                        // If that succeeded too, combine them.
                        of1.combine(of2)
                    })
                })
            })
        }};
    }

    /// The simplest possible file: no symbols, one entrypoint section
    /// containing a single byte.
    #[test]
    fn test_minimal() {
        let parsed = parse_files!("examples/minimal.simobj").unwrap();
        assert_display_snapshot!(parsed);
    }

    /// A file with a single symbol called foo, and a single entrypoint section.
    #[test]
    fn test_single_symbol() {
        let parsed = parse_files!("examples/single-symbol.simobj").unwrap();
        assert_display_snapshot!(parsed);
    }

    /// A file with a single symbol called foo, and multiple sections.
    #[test]
    fn test_multi_section() {
        let parsed = parse_files!("examples/multi-section.simobj").unwrap();
        assert_display_snapshot!(parsed);
    }

    /// A file with multiple symbols, and a single entrypoint section.
    #[test]
    fn test_multi_symbol() {
        let parsed = parse_files!("examples/multi-symbol.simobj").unwrap();
        assert_display_snapshot!(parsed);
    }

    /// Combine the single-symbol and multi-section files.
    #[test]
    fn test_combine_internal() {
        let parsed = parse_files!(
            "examples/single-symbol.simobj",
            "examples/multi-section.simobj"
        ).unwrap();
        assert_display_snapshot!(parsed);
    }

    /// Combine the multi-symbol file with one that has an external reference
    /// to the public symbol.
    #[test]
    fn test_combine_external() {
        let parsed = parse_files!(
            "examples/multi-symbol.simobj",
            "examples/external-symbol.simobj"
        ).unwrap();
        assert_display_snapshot!(parsed);
    }

    /// Same as combine_external but with the files the other way around.
    #[test]
    fn test_combine_external_reversed() {
        let parsed = parse_files!(
            "examples/external-symbol.simobj",
            "examples/multi-symbol.simobj"
        ).unwrap();
        assert_display_snapshot!(parsed);
    }

    /// Try (and fail) to combine two files with the same public symbol.
    #[test]
    fn test_combine_public() {
        let parsed = parse_files!(
            "examples/multi-symbol.simobj",
            "examples/multi-symbol.simobj"
        );
        match parsed {
            Ok(_) => panic!(),
            Err(e) =>
                assert_eq!(e.message(), "Multiple definitions for symbol foobaz."),
        }
    }

    // TODO test public and external conflicting with internal.
}
