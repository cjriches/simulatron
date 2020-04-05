use std::sync::mpsc::Sender;

use crate::char_mapping;
use crate::ui::UICommand;

pub struct DisplayController {
    ui_channel: Sender<UICommand>,
}

impl DisplayController {
    pub fn new(ui_channel: Sender<UICommand>) -> Self {
        DisplayController {
            ui_channel
        }
    }

    pub fn store(&self, address: u32, value: u8) {
        if address < 2000 {
            let row = address / 25;
            let col = address % 80;
            let character = char_mapping::u8_to_char(value);
            self.ui_channel.send(UICommand::SetChar(row, col, character))
                .expect("Failed to send command to UI.");
        } else if address < 4000 {
            let cell_num = address - 2000;
            let row = cell_num / 25;
            let col = cell_num % 80;
            let r = value & 0b00110000;
            let g = value & 0b00001100;
            let b = value & 0b00000011;
            self.ui_channel.send(UICommand::SetFg(row, col, r, g, b))
                .expect("Failed to send command to UI.");
        } else if address < 6000 {
            let cell_num = address - 4000;
            let row = cell_num / 25;
            let col = cell_num % 80;
            let r = value & 0b00110000;
            let g = value & 0b00001100;
            let b = value & 0b00000011;
            self.ui_channel.send(UICommand::SetBg(row, col, r, g, b))
                .expect("Failed to send command to UI.");
        } else {
            panic!("Display saw too high address of {}.", address);
        }
    }
}
