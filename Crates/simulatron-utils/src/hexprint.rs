#![allow(clippy::needless_range_loop)]

use std::fmt::Write;

/// Nicely format the given `Vec<u8>` as a hex block. The listed addresses will
/// start from `start`.
pub fn pretty_print_hex_block(buf: &Vec<u8>, start: usize) -> String {
    // Each 16 bytes of the input produces a line consisting of:
    // - a 10-character address
    // - 32 characters of bytes
    // - 16 characters of ASCII
    // - 24 spaces
    // - 1 newline
    // - 2 vertical bars
    // Therefore, each 16 bytes of input produces 85 bytes of output.
    let mut str = String::with_capacity((buf.len() / 16 + 1) * 85);
    for (i, byte) in buf.iter().enumerate() {
        match i % 16 {
            0 => {
                // At the start of each 16 bytes, print an address header.
                write!(str, "{:#010X}    ", start + i).unwrap();
            }
            4 | 8 | 12 => {
                // After each 4 bytes, print a double space.
                str.push_str("  ");
            }
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
            for j in (i - 15)..=i {
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
        let spaces = (16 - remainder) * 2        // Two spaces per missing byte,
                + 18                    // Plus 18 spaces normally,
                - (remainder - 1)       // Reduce by existing between-byte spaces,
                - (remainder - 1) / 4; // Account for double spaces.
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

    str
}

/// Shortcut for starting the addresses at zero.
#[inline]
pub fn pretty_print_hex_block_zero(buf: &Vec<u8>) -> String {
    pretty_print_hex_block(buf, 0)
}

fn printable(chr: u8) -> char {
    match chr {
        32..=126 => chr.into(),
        _ => '.',
    }
}
