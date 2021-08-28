use super::*;

use insta::{assert_snapshot, assert_display_snapshot};
use log::info;
use std::fs::File;

use crate::data::{DISK_ALIGN, ROM_SIZE, pretty_print_hex_block};

/// Initialise logging.
pub fn init() {
    use std::io::Write;

    // The logger can only be initialised once, but we don't know the order of
    // tests. Therefore we use `try_init` and ignore the result.
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("debug"))
        .format(|out, record| {
            writeln!(out, "{:>7} {}", record.level(), record.args())
        })
        .is_test(true)
        .try_init();
}

/// Parse the given list of files and combine them.
macro_rules! parse_files {
    // Single file case.
    ($f:expr) => {{
        let f = File::open($f).unwrap();
        info!("Parsing '{}'", $f);
        Parser::parse(f).map(Linker::from)
    }};

    // Multiple files.
    ($f0:expr, $($fs:expr),+) => {{
        // Open and parse the first.
        let f0 = File::open($f0).unwrap();
        info!("Parsing '{}'", $f0);
        Parser::parse(f0).and_then(|parsed0| {
            // Add it to a linker.
            let mut linker = Linker::from(parsed0);
            // Add the remaining files.
            for path in [$($fs),*].iter() {
                let f = File::open(path).unwrap();
                info!("Parsing '{}'", path);
                let parsed = Parser::parse(f)?;
                linker = linker.add(parsed)?;
            }
            Ok(linker)
        })
    }};
}

/// Format the given Vec<u8> nicely and then snapshot it.
macro_rules! assert_image_snapshot {
    ($img:expr) => { assert_snapshot!(pretty_print_hex_block($img, 0)) }
}

/// The simplest possible file: no symbols, one entrypoint section
/// containing a single byte.
#[test]
fn test_minimal() {
    init();
    let parsed = parse_files!("examples/minimal.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_minimal_link_rom() {
    init();
    let parsed = parse_files!("examples/minimal.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_eq!(rom, vec![0; ROM_SIZE]);
}

#[test]
fn test_minimal_link_disk() {
    init();
    let parsed = parse_files!("examples/minimal.simobj").unwrap();
    let disk = parsed.link_as_disk().unwrap();
    assert_eq!(disk, vec![0; DISK_ALIGN]);
}

/// A file with a single symbol called foo, and a single entrypoint section.
#[test]
fn test_single_symbol() {
    init();
    let parsed = parse_files!("examples/single-symbol.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_single_symbol_link() {
    init();
    let parsed = parse_files!("examples/single-symbol.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_image_snapshot!(&rom);
}

/// A file with a single symbol called foo, and multiple sections.
#[test]
fn test_multi_section() {
    init();
    let parsed = parse_files!("examples/multi-section.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_multi_section_link() {
    init();
    let parsed = parse_files!("examples/multi-section.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_image_snapshot!(&rom);
}

/// A file with multiple symbols, and a single entrypoint section.
#[test]
fn test_multi_symbol() {
    init();
    let parsed = parse_files!("examples/multi-symbol.simobj").unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_multi_symbol_link() {
    init();
    let parsed = parse_files!("examples/multi-symbol.simobj").unwrap();
    let rom = parsed.link_as_rom().unwrap();
    assert_image_snapshot!(&rom);
}

/// Combine the single-symbol and multi-section files.
#[test]
fn test_combine_internal() {
    init();
    let parsed = parse_files!(
            "examples/single-symbol.simobj",
            "examples/multi-section.simobj"
        ).unwrap();
    assert_display_snapshot!(parsed);
}

#[test]
fn test_multiple_entrypoints() {
    init();
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
    init();
    let parsed = parse_files!(
            "examples/multi-symbol.simobj",
            "examples/external-symbol.simobj"
        ).unwrap();
    assert_display_snapshot!(parsed);
}

/// Same as combine_external but with the files the other way around.
#[test]
fn test_combine_external_reversed() {
    init();
    let parsed = parse_files!(
            "examples/external-symbol.simobj",
            "examples/multi-symbol.simobj"
        ).unwrap();
    assert_display_snapshot!(parsed);
}

/// Try (and fail) to combine two files with the same public symbol.
#[test]
fn test_combine_public() {
    init();
    let error = parse_files!(
            "examples/multi-symbol.simobj",
            "examples/multi-symbol.simobj"
        ).unwrap_err();
    assert_eq!(error.message(), "Multiple definitions for symbol foobaz.");
}

/// Public symbol conflicting with an internal one.
#[test]
fn test_combine_public_internal() {
    init();
    let parsed = parse_files!(
            "examples/multi-symbol.simobj",
            "examples/internal-foobaz.simobj"
        ).unwrap();
    assert_display_snapshot!(parsed);
}

/// External symbol conflicting with an internal one.
#[test]
fn test_combine_external_internal() {
    init();
    let parsed = parse_files!(
            "examples/internal-foobaz.simobj",
            "examples/external-symbol.simobj"
        ).unwrap();
    assert_display_snapshot!(parsed);
}

/// Make sure writable sections are disallowed in ROM only.
#[test]
fn test_writable() {
    init();
    let parsed = parse_files!("examples/writable-section.simobj").unwrap();
    let error = parsed.link_as_rom().unwrap_err();
    assert_eq!(error.message(),
               "Cannot have a writable section in a read-only image.");

    let parsed = parse_files!("examples/writable-section.simobj").unwrap();
    let disk = parsed.link_as_disk().unwrap();
    assert_image_snapshot!(&disk);
}

/// Test with a single empty section.
#[test]
fn test_empty_section() {
    init();
    let parsed = parse_files!("examples/null-section.simobj").unwrap();
    let error = parsed.link_as_rom().unwrap_err();
    assert_eq!(error.message(), "Cannot produce an empty image.");
}

/// Test with no sections at all.
#[test]
fn test_no_sections() {
    init();
    let parsed = parse_files!("examples/no-sections.simobj").unwrap();
    let error = parsed.link_as_rom().unwrap_err();
    assert_eq!(error.message(), "No entrypoint section was defined.");
}

/// Test with no entrypoint section.
#[test]
fn test_no_entrypoint() {
    init();
    let parsed = parse_files!("examples/no-entrypoint.simobj").unwrap();
    let error = parsed.link_as_disk().unwrap_err();
    assert_eq!(error.message(), "No entrypoint section was defined.");
}

/// Test with a non-executable entrypoint.
#[test]
fn test_non_exec_entrypoint() {
    init();
    let parsed = parse_files!("examples/non-exec-entrypoint.simobj").unwrap();
    let error = parsed.link_as_rom().unwrap_err();
    assert_eq!(error.message(), "Section had entrypoint but not execute set.");
}

/// Ensure things too big for ROM are rejected.
#[test]
fn test_too_big_for_rom() {
    init();
    let parsed = parse_files!("examples/big.simobj").unwrap();
    let error = parsed.link_as_rom().unwrap_err();
    assert_eq!(error.message(),
               format!("Binary (5000 bytes) exceeds rom capacity ({} bytes).", ROM_SIZE));
}

/// Ensure that disk images get padded appropriately.
#[test]
fn test_disk_padding() {
    init();
    let parsed = parse_files!("examples/big.simobj").unwrap();
    let disk = parsed.link_as_disk().unwrap();
    let mut expected = vec![0x42; 5000];
    expected.resize(DISK_ALIGN * 2, 0);
    assert_eq!(disk, expected);
}

/// Try a malformed (truncated) file.
#[test]
fn test_too_small() {
    init();
    let error = parse_files!("examples/too-small.simobj").unwrap_err();
    assert_eq!(error.message(), "IO error: Unexpected EOF.");
}

/// Try a file where section references don't point to zeros.
#[test]
fn test_bad_reference() {
    init();
    let error = parse_files!("examples/bad-reference.simobj").unwrap_err();
    assert_eq!(error.message(), "Symbol reference target was non-zero.");
}

/// Ensure invalid symbol types are rejected.
#[test]
fn test_invalid_symbol_type() {
    init();
    let error = parse_files!("examples/invalid-symbol-type.simobj").unwrap_err();
    assert_eq!(error.message(), "Invalid symbol type.");
}

/// Ensure zero-length symbol names are rejected.
#[test]
fn test_zero_length_name() {
    init();
    let error = parse_files!("examples/zero-length-name.simobj").unwrap_err();
    assert_eq!(error.message(), "Symbol name cannot be the empty string.");
}

/// Ensure names with illegal characters are rejected.
#[test]
fn test_invalid_name() {
    init();
    let error = parse_files!("examples/invalid-name.simobj").unwrap_err();
    assert_eq!(error.message(), "Invalid symbol name: !yeet$");
}

/// Ensure unprintable names with illegal characters are rejected.
#[test]
fn test_really_invalid_name() {
    init();
    let error = parse_files!("examples/really-invalid-name.simobj").unwrap_err();
    assert_eq!(error.message(), "Invalid symbol name (unprintable).");
}

/// Ensure symbol values outside of a section are rejected.
#[test]
fn test_symbol_value_range() {
    init();
    let error = parse_files!("examples/sym-val-too-small.simobj").unwrap_err();
    assert_eq!(error.message(), "Address too small: 0x00000000");
    let error = parse_files!("examples/sym-val-too-big.simobj").unwrap_err();
    assert_eq!(error.message(), "Address too large: 0x00000080");
}

/// Ensure symbol references outside of a section are rejected.
#[test]
fn test_symbol_reference_range() {
    init();
    let error = parse_files!("examples/sym-ref-too-small.simobj").unwrap_err();
    assert_eq!(error.message(), "Address too small: 0x00000008");
    let error = parse_files!("examples/sym-ref-too-big.simobj").unwrap_err();
    assert_eq!(error.message(), "Address too large: 0x0000003A");
}
