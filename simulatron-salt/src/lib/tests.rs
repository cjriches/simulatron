use insta::{assert_snapshot, assert_debug_snapshot};

use crate::{
    ast::{AstNode, Program},
    codegen::CodeGenerator,
    init_test_logging,
    lexer::Lexer,
    parser::Parser,
};

/// Nicely format the given Vec<u8> as a hex block.
fn pretty_print_hex_block(buf: &Vec<u8>) -> String {
    use std::fmt::Write;

    // Each 16 bytes of the input produces a line consisting of:
    // - a 10-character address
    // - 32 characters of bytes
    // - 16 characters of ASCII
    // - 24 spaces
    // - 1 newline
    // - 2 vertical bars
    // Therefore, each 16 bytes of input produces about 85 bytes of output.
    let mut str = String::with_capacity((buf.len()/16 + 1) * 85);
    for (i, byte) in buf.iter().enumerate() {
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
        // If this is the last byte of a line, add the ASCII representation.
        if i % 16 == 15 {
            str.push_str("  |");
            for j in (i-15)..=i {
                str.push(printable(buf[j]));
            }
            str.push('|');
            // If this isn't the last row, add a newline.
            if i + 1 != buf.len() {
                str.push('\n');
            }
        }
    }
    // If this wasn't a multiple of 16 bytes, we need to add the last
    // line's ASCII representation.
    let remainder = buf.len() % 16;
    if remainder != 0 {
        // Pad the missing bytes.
        let spaces =
              (16 - remainder) * 2  // Two spaces per missing byte,
            + 18                    // Plus 18 spaces normally,
            - (remainder - 1)       // Reduce by existing between-byte spaces,
            - (remainder - 1) / 4;  // Account for double spaces.
        for _ in 0..spaces {
            str.push(' ');
        }
        // Print the ASCII.
        str.push_str("  |");
        for i in (buf.len() - remainder)..buf.len() {
            str.push(printable(buf[i]));
        }
        str.push('|');
    }

    return str;
}

fn printable(chr: u8) -> char {
    match chr {
        32..=126 => chr.into(),
        _ => '.',
    }
}

fn test_success(path: &str, entrypoint: bool) {
    init_test_logging();
    let input = std::fs::read_to_string(path).unwrap();
    let parser = Parser::new(Lexer::new(&input));
    let cst = parser.run().unwrap();
    let ast = Program::cast(cst).unwrap();
    let codegen = CodeGenerator::new(ast, &Vec::new()).unwrap();
    let success = codegen.run(entrypoint).unwrap();
    assert_eq!(success.warnings.len(), 0);
    assert_snapshot!(pretty_print_hex_block(&success.simobj));
}

fn test_success_with_warnings(path: &str, entrypoint: bool) {
    init_test_logging();
    let input = std::fs::read_to_string(path).unwrap();
    let parser = Parser::new(Lexer::new(&input));
    let cst = parser.run().unwrap();
    let ast = Program::cast(cst).unwrap();
    let codegen = CodeGenerator::new(ast, &Vec::new()).unwrap();
    let success = codegen.run(entrypoint).unwrap();
    assert!(success.warnings.len() > 0);
    assert_snapshot!(pretty_print_hex_block(&success.simobj));
    assert_debug_snapshot!(success.warnings);
}

fn test_failure(path: &str) {
    init_test_logging();
    let input = std::fs::read_to_string(path).unwrap();
    let parser = Parser::new(Lexer::new(&input));
    let cst = parser.run().unwrap();
    let ast = Program::cast(cst).unwrap();
    let failure = CodeGenerator::new(ast, &Vec::new()).and_then(|cg| {
        cg.run(true)
    }).unwrap_err();
    assert_debug_snapshot!(failure);
}

#[test]
fn test_addressing_modes() {
    test_success("examples/addressing-modes.simasm", true);
    test_success("examples/addressing-modes-2.simasm", true);
}

#[test]
fn test_blockcopy() {
    test_success("examples/blockcopy.simasm", false);
    test_failure("examples/blockcopy-bad.simasm");
}

#[test]
fn test_blockset() {
    test_success("examples/blockset.simasm", true);
    test_failure("examples/blockset-bad.simasm");
}

#[test]
fn test_comments() {
    test_success("examples/comments.simasm", false);
}

#[test]
fn test_convert() {
    test_success("examples/convert.simasm", true);
    test_failure("examples/convert-bad.simasm");
}

#[test]
fn test_copy() {
    test_success("examples/copy.simasm", true);
    test_failure("examples/copy-bad.simasm");
}

#[test]
fn test_empty() {
    test_failure("examples/empty-file.simasm");
}

#[test]
fn test_external_refs() {
    test_success_with_warnings("examples/external-refs.simasm", false);
}

#[test]
fn test_minimal() {
    test_success("examples/minimal.simasm", true);
}

#[test]
fn test_push_pop() {
    test_success("examples/push-pop.simasm", true);
    test_failure("examples/push-pop-bad.simasm");
}

#[test]
fn test_store() {
    test_success("examples/store.simasm", true);
}

#[test]
fn test_swap() {
    test_success("examples/swap.simasm", true);
}