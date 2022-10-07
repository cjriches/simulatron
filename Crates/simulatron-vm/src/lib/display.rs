use std::sync::mpsc::Sender;

use crate::ui::UICommand;

/// A display controller.
pub struct DisplayController {
    ui_channel: Sender<UICommand>,
}

impl DisplayController {
    /// Create a new display controller with the given UI command channel.
    pub fn new(ui_channel: Sender<UICommand>) -> Self {
        DisplayController { ui_channel }
    }

    /// Handle a memory-mapped command signal by sending a command to the UI.
    pub fn store(&self, address: u32, value: u8) {
        if address < 2000 {
            // Character value.
            let row = (address / 80) as u16;
            let col = (address % 80) as u16;
            if let Some(character) = u8_to_printable_char(value) {
                self.ui_channel
                    .send(UICommand::SetChar {
                        row,
                        col,
                        character,
                    })
                    .unwrap();
            }
        } else if address < 4000 {
            // Foreground color.
            let cell_num = address - 2000;
            let row = (cell_num / 80) as u16;
            let col = (cell_num % 80) as u16;
            let (r, g, b) = rgb(value);
            self.ui_channel
                .send(UICommand::SetFg { row, col, r, g, b })
                .unwrap();
        } else if address < 6000 {
            // Background color.
            let cell_num = address - 4000;
            let row = (cell_num / 80) as u16;
            let col = (cell_num % 80) as u16;
            let (r, g, b) = rgb(value);
            self.ui_channel
                .send(UICommand::SetBg { row, col, r, g, b })
                .unwrap();
        } else {
            unreachable!()
        }
    }
}

/// Try and convert the given byte in the Simulatron character set to a
/// printable Unicode character.
fn u8_to_printable_char(byte: u8) -> Option<char> {
    match byte {
        31 => Some('£'),
        32..=126 => Some(char::from(byte)),
        127 => Some('¬'),
        _ => None,
    }
}

/// Convert a raw color byte to its RGB components.
fn rgb(raw_byte: u8) -> (u8, u8, u8) {
    let r = (raw_byte & 0b00110000) >> 4;
    let g = (raw_byte & 0b00001100) >> 2;
    let b = raw_byte & 0b00000011;

    fn quantise(raw: u8) -> u8 {
        match raw {
            0b00 => 0,
            0b01 => 85,
            0b10 => 170,
            0b11 => 255,
            _ => unreachable!(),
        }
    }

    (quantise(r), quantise(g), quantise(b))
}
