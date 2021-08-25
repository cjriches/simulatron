use super::*;

use insta::{assert_snapshot, assert_display_snapshot};
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

/// Format the given Vec<u8> nicely and then snapshot it.
macro_rules! assert_image_snapshot {
        ($img:expr) => { assert_snapshot!(pretty_print_hex_block($img)) }
    }

/// Since `move_to_start` is unsafe internally, we'd better test it.
#[test]
fn test_move_to_start() {
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

/// The simplest possible file: no symbols, one entrypoint section
/// containing a single byte.
#[test]
fn test_minimal() {
    let parsed = parse_files!("examples/minimal.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_minimal_link_rom() {
    let parsed = parse_files!("examples/minimal.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_eq!(rom, vec![0; ROM_SIZE]);
}

#[test]
fn test_minimal_link_disk() {
    let parsed = parse_files!("examples/minimal.simobj").unwrap();
    let disk = parsed.link_as_disk().unwrap();
    assert_eq!(disk, vec![0; DISK_ALIGN]);
}

/// A file with a single symbol called foo, and a single entrypoint section.
#[test]
fn test_single_symbol() {
    let parsed = parse_files!("examples/single-symbol.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_single_symbol_link() {
    let parsed = parse_files!("examples/single-symbol.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_image_snapshot!(&rom);
}

/// A file with a single symbol called foo, and multiple sections.
#[test]
fn test_multi_section() {
    let parsed = parse_files!("examples/multi-section.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_multi_section_link() {
    let parsed = parse_files!("examples/multi-section.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_image_snapshot!(&rom);
}

/// A file with multiple symbols, and a single entrypoint section.
#[test]
fn test_multi_symbol() {
    let parsed = parse_files!("examples/multi-symbol.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_multi_symbol_link() {
    let parsed = parse_files!("examples/multi-symbol.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_image_snapshot!(&rom);
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

#[test]
fn test_multiple_entrypoints() {
    let parsed = parse_files!(
            "examples/single-symbol.simobj",
            "examples/multi-section.simobj"
        ).unwrap();
    let error = parsed.link_as_rom().unwrap_err();
    assert_eq!(error.message(),
               "Multiple entrypoint sections were defined.");
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
    let error = parse_files!(
            "examples/multi-symbol.simobj",
            "examples/multi-symbol.simobj"
        ).unwrap_err();
    assert_eq!(error.message(), "Multiple definitions for symbol foobaz.");
}

/// Public symbol conflicting with an internal one.
#[test]
fn test_combine_public_internal() {
    let parsed = parse_files!(
            "examples/multi-symbol.simobj",
            "examples/internal-foobaz.simobj"
        ).unwrap();
    assert_display_snapshot!(parsed);
}

/// External symbol conflicting with an internal one.
#[test]
fn test_combine_external_internal() {
    let parsed = parse_files!(
            "examples/internal-foobaz.simobj",
            "examples/external-symbol.simobj"
        ).unwrap();
    assert_display_snapshot!(parsed);
}

/// Make sure writable sections are disallowed in ROM only.
#[test]
fn test_writable() {
    let parsed = parse_files!("examples/writable-section.simobj").unwrap();
    let error = parsed.link_as_rom().unwrap_err();
    assert_eq!(error.message(),
               "Cannot have a writable section in a read-only image.");

    let parsed = parse_files!("examples/writable-section.simobj").unwrap();
    let disk = parsed.link_as_disk().unwrap();
    assert_image_snapshot!(&disk);
}

// TODO test with truly null input: no symbols and no sections, or a single empty section.
