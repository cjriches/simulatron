use log::{trace, debug, info};
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{Display, Formatter};

use crate::data::{DISK_ALIGN, DISK_BASE, FLAG_ENTRYPOINT,
                  FLAG_EXECUTE, FLAG_WRITE, INVALID_FLAGS,
                  ObjectFile, ROM_BASE, ROM_SIZE, Section,
                  SYMBOL_TYPE_EXTERNAL,
                  SYMBOL_TYPE_INTERNAL, SYMBOL_TYPE_PUBLIC};
use crate::error::{OFError, OFResult};

// Is an image read-only or not?
type ImageAccess = bool;
const READ_ONLY: ImageAccess = true;
const READ_WRITE: ImageAccess = false;

#[derive(Debug)]
pub struct Linker {
    data: ObjectFile,
}

impl Display for Linker {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.data)
    }
}

impl Linker {
    /// Construct a new empty linker.
    pub fn new() -> Self {
        // Start with a completely empty object file.
        Self {
            data: ObjectFile {
                symbols: HashMap::new(),
                sections: Vec::new(),
            },
        }
    }

    /// Construct a new linker starting with the given object file.
    pub fn from(of: ObjectFile) -> Self {
        // Start with a completely empty object file.
        Self {
            data: of,
        }
    }

    /// Add the symbols and sections of an object file.
    pub fn add(mut self, mut of: ObjectFile) -> OFResult<Self> {
        let data = &mut self.data;

        // We will need to offset all the sections, symbol values, and
        // symbol references in `other`.
        let offset = match data.sections.last() {
            None => 0,
            Some(last_section) => last_section.start + last_section.length,
        };
        debug!("Offsetting other by {:#010X}", offset);
        // Offset the other sections.
        for section in of.sections.iter_mut() {
            section.start += offset;
        }
        // Add the other sections.
        data.sections.reserve(of.sections.len());
        data.sections.append(&mut of.sections);
        debug!("Gobbled all sections.");
        // Add the other symbols.
        data.symbols.reserve(of.symbols.len());
        for (name, mut new_entry) in of.symbols.into_iter() {
            // Relocate the value.
            new_entry.value = new_entry.value.map(|v| v + offset);
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
            match data.symbols.get_mut(&name) {
                None => {
                    debug!("Adding new symbol {}", name);
                    data.symbols.insert(name, new_entry);
                },
                Some(existing_entry) => {
                    trace!("New symbol conflicts.");
                    // Case a) rename an internal symbol.
                    if new_entry.symbol_type == SYMBOL_TYPE_INTERNAL {
                        // Rename the new entry before inserting.
                        let new_name = gen_non_conflicting_name(&data.symbols, &name)?;
                        debug!("Renaming new symbol {} to {} and adding.", name, new_name);
                        let was_present = data.symbols.insert(new_name, new_entry);
                        assert!(was_present.is_none());
                    } else if existing_entry.symbol_type == SYMBOL_TYPE_INTERNAL {
                        // Rename the existing entry then insert.
                        let new_name = gen_non_conflicting_name(&data.symbols, &name)?;
                        debug!("Renaming existing symbol {} to {} and \
                                adding a new one with the old name.", name, new_name);
                        let old = data.symbols.remove(&name).unwrap();
                        let was_present = data.symbols.insert(new_name, old)
                            .or(data.symbols.insert(name, new_entry));
                        assert!(was_present.is_none());
                        // Case b) resolve external and public.
                    } else if new_entry.symbol_type == SYMBOL_TYPE_EXTERNAL
                        && existing_entry.symbol_type == SYMBOL_TYPE_PUBLIC {
                        // Eat the new entry's references.
                        debug!("Adding external reference to {}.", name);
                        existing_entry.references.append(&mut new_entry.references);
                    } else if new_entry.symbol_type == SYMBOL_TYPE_PUBLIC
                        && existing_entry.symbol_type == SYMBOL_TYPE_EXTERNAL {
                        // Eat the new entry's references, take its value, and
                        // change type to public.
                        debug!("Resolving external reference {}.", name);
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

    /// Process into a ROM image.
    pub fn link_as_rom(self) -> OFResult<Vec<u8>> {
        // Generate image.
        info!("Generating ROM image.");
        let mut image = self.link_as_image(READ_ONLY, ROM_BASE)?;
        info!("Image generated.");
        debug!("Raw size: {} bytes.", image.len());
        // Ensure it is the correct size.
        assert_or_error!(image.len() <= ROM_SIZE,
            format!("Binary ({} bytes) exceeds rom capacity ({} bytes).",
            image.len(), ROM_SIZE));
        image.resize(ROM_SIZE, 0);

        Ok(image)
    }

    /// Process into a disk image.
    pub fn link_as_disk(self) -> OFResult<Vec<u8>> {
        // Generate image.
        info!("Generating disk image.");
        let mut image = self.link_as_image(READ_WRITE, DISK_BASE)?;
        info!("Image generated.");
        debug!("Raw size: {} bytes.", image.len());
        // Pad it to the next multiple of DISK_ALIGN.
        let remainder = image.len() % DISK_ALIGN;
        if remainder > 0 {
            let new_len = image.len() + DISK_ALIGN - remainder;
            debug!("Padding to {} bytes.", new_len);
            image.resize(new_len, 0);
        }

        Ok(image)
    }

    /// Process into a generic, unpadded image.
    fn link_as_image(mut self, read_only: ImageAccess,
                     base_address: u32) -> OFResult<Vec<u8>> {
        let data = &mut self.data;

        // Find the entrypoint section.
        info!("Looking for entrypoint section.");
        let mut entrypoint_index = None;
        for (i, section) in data.sections.iter().enumerate() {
            debug!("Checking section {}.", i);
            assert_or_error!(section.flags & INVALID_FLAGS == 0,
                "Invalid section flags.");
            assert_or_error!(!(read_only && (section.flags & FLAG_WRITE != 0)),
                "Cannot have a writable section in a read-only image.");
            if section.flags & FLAG_ENTRYPOINT != 0 {
                assert_or_error!(section.flags & FLAG_EXECUTE != 0,
                    "Section had entrypoint but not execute set.");
                assert_or_error!(entrypoint_index.is_none(),
                    "Multiple entrypoint sections were defined.");
                debug!("Section {} is entrypoint.", i);
                entrypoint_index = Some(i);
            }
        }
        assert_or_error!(entrypoint_index.is_some(),
            "No entrypoint section was defined.");
        let entrypoint_index = entrypoint_index.unwrap();

        // Relocate the entrypoint section to the start.
        let entrypoint = &mut data.sections[entrypoint_index];
        let cutoff = entrypoint.start;
        let offset = entrypoint.length;
        entrypoint.start = 0;
        debug!("Offsetting pre-entrypoint sections by {:#010X}", offset);
        for i in 0..entrypoint_index {
            data.sections[i].start += offset;
        }
        move_to_start(&mut data.sections, entrypoint_index);

        // Define a closure for relocating a value relative to how the
        // entrypoint section moved.
        let relocate = |v: u32| {
            if v < cutoff {
                // This was before the entrypoint before, so add the offset.
                v + offset
            } else if v < cutoff + offset {
                // This was in the entrypoint before, so subtract the cutoff.
                v - cutoff
            } else {
                // This was after the entrypoint, so no change.
                v
            }
        };

        // Resolve all symbol references.
        for (name, symbol) in data.symbols.iter() {
            debug!("Linking symbol {}", name);
            assert_or_error!(symbol.value.is_some(),
                format!("Unresolved symbol: {}", name));
            // Relocate the value.
            let value = {
                let value = symbol.value.unwrap();
                let value = base_address + relocate(value);
                value.to_be_bytes()
            };
            // Resolve the references.
            for reference in symbol.references.iter() {
                // Relocate the reference.
                let relocated = relocate(*reference);
                trace!("Relocating reference from {:#010X} to {:#010X}",
                    reference, relocated);
                // Resolve reference.
                let section = Section::find_mut(&mut data.sections, relocated)
                    .expect("BUG: an invalid reference escaped the parsing stage.");
                let section_offset: usize = (relocated - section.start).try_into().unwrap();
                for i in 0..4 {
                    // Sanity check.
                    assert_eq!(section.data[section_offset + i], 0);
                    section.data[section_offset + i] = value[i];
                }
            }
        }

        // Concatenate sections.
        debug!("Linking complete; concatenating sections.");
        // First, calculate the true length in bytes.
        let mut length = 0;
        for section in data.sections.iter() {
            length += section.data.len();
        }
        // Now allocate the buffer and fill it.
        let mut image = Vec::with_capacity(length);
        for section in data.sections.iter_mut() {
            image.append(&mut section.data);
        }

        // Ensure the image is not empty.
        assert_or_error!(image.len() > 0, "Cannot produce an empty image.");

        Ok(image)
    }
}

/// Generate a variation on the given name that is not already used as a key
/// in the given hashmap. Achieved by repeatedly incrementing an appended
/// number until an unused key is found.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::init;

    /// Since `move_to_start` is unsafe internally, we'd better test it.
    #[test]
    fn test_move_to_start() {
        init();
        let mut v = vec![0, 1, 2, 3, 4];

        // Test identity.
        move_to_start(&mut v, 0);
        assert_eq!(v, vec![0, 1, 2, 3, 4]);

        // Test a small move.
        move_to_start(&mut v, 1);
        assert_eq!(v, vec![1, 0, 2, 3, 4]);

        // Test full length move.
        move_to_start(&mut v, 4);
        assert_eq!(v, vec![4, 1, 0, 2, 3]);

        // Test move from the middle.
        move_to_start(&mut v, 2);
        assert_eq!(v, vec![0, 4, 1, 2, 3]);

        // Test out-of-bounds.
        let result = std::panic::catch_unwind(move || {
            move_to_start(&mut v, 5);
        });
        assert!(result.is_err());
    }
}
