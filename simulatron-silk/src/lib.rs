#[macro_use]
mod error;
mod read_be;

#[cfg(test)]
mod tests;

use itertools::Itertools;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{Display, Formatter, Write};
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
const INVALID_FLAGS: u8 = !(FLAG_ENTRYPOINT | FLAG_READ
                           | FLAG_WRITE | FLAG_EXECUTE);

// Simulatron-specific constants.
pub const ROM_SIZE: usize = 512;
pub const DISK_ALIGN: usize = 4096;

// Is an image read-only or not?
type ImageAccess = bool;
const READ_ONLY: ImageAccess = true;
const READ_WRITE: ImageAccess = false;

/// An object code section.
#[derive(Debug, PartialEq, Eq)]
struct Section {
    flags: u8,
    start: u32,
    length: u32,
    data: Vec<u8>,
}

impl Display for Section {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Write flags.
        writeln!(f, "flags: {:08b} start: {:#010X} length: {:#010X}",
                 self.flags, self.start, self.length)?;
        // Write data.
        write!(f, "{}", pretty_print_hex_block(&self.data))
    }
}

/// A symbol table entry.
#[derive(Debug, PartialEq, Eq)]
struct SymbolTableEntry {
    symbol_type: u8,
    value: Option<u32>,
    references: Vec<u32>,
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
                                               num_symbol_table_entries)?;

        // Return the result.
        Ok(ObjectFile {
            symbols,
            sections,
        })
    }

    /// Parse the section headers and sections. Produces a vector of sections,
    /// sorted by their location in the file.
    fn parse_sections<S>(source: &mut S, base: u32,
                         num_headers: usize) -> OFResult<Vec<Section>>
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
            sections.push(Section {
                flags,
                start: section_start,
                length: section_length as u32,
                data,
            });
        }

        // Sort sections by their location within the file.
        sections.sort_unstable_by_key(|section| section.start);

        Ok(sections)
    }

    /// Parse the symbol table.
    fn parse_symbol_table<S>(source: &mut S, base: u32, num_entries: usize)
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
            let references = Self::parse_references(source, num_refs)?;
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
        }

        Ok(table)
    }

    /// Parse a list of symbol references. This validates that the reference
    /// points to a zero-filled location within a section.
    fn parse_references<S>(source: &mut S, num_refs: usize)
                           -> OFResult<Vec<u32>>
        where S: ReadBE + Seek
    {
        // Read in all the offsets.
        let mut offsets = Vec::with_capacity(num_refs);
        for _ in 0..num_refs {
            offsets.push(source.read_be_u32()?);
        }

        // Remember the current file position.
        let current_pos = source.stream_position()?;

        // Check that each referenced location is currently zero.
        for offset in offsets.iter() {
            source.seek(SeekFrom::Start(*offset as u64))?;
            let value = source.read_be_u32()?;
            assert_or_error!(value == 0, "Symbol reference was non-zero.");
        }

        // Restore the file position.
        source.seek(SeekFrom::Start(current_pos))?;

        Ok(offsets)
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
                _ => return Err(OFError::new("Invalid symbol name.")),
            }
        }

        // Strings of this format are guaranteed to be valid UTF-8.
        Ok(String::from_utf8(name).unwrap())
    }

    /// Combine the symbols and sections of two object files.
    pub fn combine(mut self, mut other: Self) -> OFResult<Self> {
        // We will need to offset all the references in `other`.
        let offset = self.length();
        // Offset the other sections.
        for section in other.sections.iter_mut() {
            section.start += offset;
        }
        // Add the other sections.
        self.sections.reserve(other.sections.len());
        self.sections.append(&mut other.sections);
        // Add the other symbols.
        self.symbols.reserve(other.symbols.len());
        for (name, mut new_entry) in other.symbols.into_iter() {
            // Relocate the references.
            for reference in new_entry.references.iter_mut() {
                *reference += offset;
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

    fn length(&self) -> u32 {
        // We depend on the invariant that sections are kept sorted.
        match self.sections.last() {
            None => 0,
            Some(last_section) => last_section.start
                                            + last_section.length,
        }
    }

    /// Process an object file into a ROM image.
    pub fn link_as_rom(self) -> OFResult<Vec<u8>> {
        // Generate image.
        let mut image = self.link_as_image(READ_ONLY)?;
        // Ensure it is the correct size.
        assert_or_error!(image.len() <= ROM_SIZE,
            format!("Binary ({} bytes) exceeds rom capacity ({} bytes).",
            image.len(), ROM_SIZE));
        image.resize(ROM_SIZE, 0);

        Ok(image)
    }

    /// Process an object file into a disk image.
    pub fn link_as_disk(self) -> OFResult<Vec<u8>> {
        // Generate image.
        let mut image = self.link_as_image(READ_WRITE)?;
        // Pad it to the next multiple of DISK_ALIGN.
        let remainder = image.len() % DISK_ALIGN;
        if remainder > 0 {
            let new_len = image.len() + DISK_ALIGN - remainder;
            image.resize(new_len, 0);
        }

        Ok(image)
    }

    /// Process an object file into a generic, unpadded image.
    fn link_as_image(mut self, read_only: ImageAccess) -> OFResult<Vec<u8>> {
        // Find the entrypoint section.
        let mut entrypoint_index = None;
        for (i, section) in self.sections.iter().enumerate() {
            assert_or_error!(section.flags & INVALID_FLAGS == 0,
                "Invalid section flags.");
            assert_or_error!(!(read_only && (section.flags & FLAG_WRITE != 0)),
                "Cannot have a writable section in a read-only image.");
            if section.flags & FLAG_ENTRYPOINT != 0 {
                assert_or_error!(section.flags & FLAG_EXECUTE != 0,
                    "Section had entrypoint but not execute set.");
                assert_or_error!(entrypoint_index.is_none(),
                    "Multiple entrypoint sections were defined.");
                entrypoint_index = Some(i);
            }
        }
        assert_or_error!(entrypoint_index.is_some(),
            "No entrypoint section was defined.");
        let entrypoint_index = entrypoint_index.unwrap();

        // Relocate the entrypoint section to the start.
        let entrypoint = &mut self.sections[entrypoint_index];
        let cutoff = entrypoint.start;
        let offset = entrypoint.length;
        entrypoint.start = 0;
        for i in 0..entrypoint_index {
            self.sections[i].start += offset;
        }
        move_to_start(&mut self.sections, entrypoint_index);

        // Resolve all symbol references.
        for (name, symbol) in self.symbols.iter() {
            assert_or_error!(symbol.value.is_some(),
                format!("Unresolved symbol: {}", name));
            let value = symbol.value.unwrap().to_be_bytes();
            for reference in symbol.references.iter() {
                // Account for relocating the entrypoint section.
                let relocated = if *reference < cutoff {
                    // This was before the entrypoint before, so add the offset.
                    *reference + offset
                } else if *reference < cutoff + offset {
                    // This was in the entrypoint before, so subtract the cutoff.
                    *reference - cutoff
                } else {
                    // This was after the entrypoint, so no change.
                    *reference
                };
                // Resolve reference.
                let mut resolved = false;
                for section in self.sections.iter_mut() {
                    if relocated < section.start + section.length {
                        // Splice into the section.
                        let section_offset: usize = (relocated - section.start)
                            .try_into().unwrap();
                        for i in 0..4 {
                            // Sanity check.
                            assert_eq!(section.data[section_offset + i], 0);
                            section.data[section_offset + i] = value[i];
                        }
                        resolved = true;
                        break;
                    }
                }
                assert!(resolved);  // Sanity check.
            }
        }

        // Concatenate sections.
        // First, calculate the true length in bytes. This has nothing to do with
        // the section `start` and `length` parameters, which are expressed in
        // terms of original file offsets.
        let mut length = 0;
        for section in self.sections.iter() {
            length += section.data.len();
        }
        // Now allocate the buffer and fill it.
        let mut image = Vec::with_capacity(length);
        for section in self.sections.iter_mut() {
            image.append(&mut section.data);
        }

        // Ensure the image is not empty.
        assert_or_error!(image.len() > 0, "Cannot produce an empty image.");

        Ok(image)
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

/// Efficiently move the given index to the start of the vector, displacing
/// prior elements and shifting them up.
/// Doing a `.remove()` followed by a `.insert()` requires two
/// linear time operations, whereas this only requires one.
fn move_to_start<T>(v: &mut Vec<T>, index: usize) {
    assert!(index < v.len(),
            "Index {} out of bounds for vector with length {}.", index, v.len());
    let ptr = v.as_mut_ptr();
    unsafe {
        // Remember the item to move.
        let item = ptr.add(index).read();
        // Shift the others up.
        for i in (0..index).rev() {
            ptr.add(i + 1).write(ptr.add(i).read());
        }
        // Put the moved item back in at the start.
        ptr.write(item);
    }
}

/// Nicely format the given Vec<u8> as a hex block.
fn pretty_print_hex_block(image: &Vec<u8>) -> String {
    // Each 16 bytes of the input produces a line consisting of:
    // - a 10-character address
    // - 32 characters of bytes
    // - 22 spaces
    // - 1 newline
    // Therefore, each 16 bytes of input produces about 65 bytes of output.
    let mut str = String::with_capacity((image.len()/16 + 1) * 65);
    for (i, byte) in image.iter().enumerate() {
        match i % 16 {
            0 => {
                // At the start of each 16 bytes, print an address header.
                write!(str, "{:#010X}    ", i).unwrap();
            }
            4 | 8 | 12 => {
                // After each 4 bytes, print a double space.
                str.push_str("  ");
            },
            _ => {
                // Single-space between bytes.
                str.push(' ');
            }
        }
        // Write each byte as two hex digits.
        write!(str, "{:02X}", byte).unwrap();
        // If this is the last byte of the not-last row, add a newline.
        if (i % 16 == 15) && (i + 1 != image.len()) {
            str.push('\n');
        }
    }

    return str;
}
