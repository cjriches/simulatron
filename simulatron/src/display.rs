use std::sync::mpsc::Sender;

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
            let row = address / 80;
            let col = address % 80;
            match u8_to_printable_char(value) {
                Some(c) => self.ui_channel.send(UICommand::SetChar(row, col, c)).unwrap(),
                None => self.ui_channel.send(UICommand::SetChar(row, col, ' ')).unwrap(),
            }
        } else if address < 4000 {
            let cell_num = address - 2000;
            let row = cell_num / 80;
            let col = cell_num % 80;
            let (r, g, b) = DisplayController::colours(value);
            self.ui_channel.send(UICommand::SetFg(row, col, r, g, b))
                .expect("Failed to send command to UI.");
        } else if address < 6000 {
            let cell_num = address - 4000;
            let row = cell_num / 80;
            let col = cell_num % 80;
            let (r, g, b) = DisplayController::colours(value);
            self.ui_channel.send(UICommand::SetBg(row, col, r, g, b))
                .expect("Failed to send command to UI.");
        } else {
            panic!("Display saw too high address of {}.", address);
        }
    }

    fn colours(raw_byte: u8) -> (u8, u8, u8) {
        let r = (raw_byte & 0b00110000) >> 4;
        let g = (raw_byte & 0b00001100) >> 2;
        let b = raw_byte & 0b00000011;

        fn quantise(raw: u8) -> u8 {
            match raw {
                0b00 => 0,
                0b01 => 85,
                0b10 => 170,
                0b11 => 255,
                _ => unreachable!()
            }
        }

        (quantise(r), quantise(g), quantise(b))
    }
}

fn u8_to_printable_char(byte: u8) -> Option<char> {
    match byte {
         31 => Some('£'),
         32 => Some(' '),
         33 => Some('!'),
         34 => Some('"'),
         35 => Some('#'),
         36 => Some('$'),
         37 => Some('%'),
         38 => Some('&'),
         39 => Some('\''),
         40 => Some('('),
         41 => Some(')'),
         42 => Some('*'),
         43 => Some('+'),
         44 => Some(','),
         45 => Some('-'),
         46 => Some('.'),
         47 => Some('/'),
         48 => Some('0'),
         49 => Some('1'),
         50 => Some('2'),
         51 => Some('3'),
         52 => Some('4'),
         53 => Some('5'),
         54 => Some('6'),
         55 => Some('7'),
         56 => Some('8'),
         57 => Some('9'),
         58 => Some(':'),
         59 => Some(';'),
         60 => Some('<'),
         61 => Some('='),
         62 => Some('>'),
         63 => Some('?'),
         64 => Some('@'),
         65 => Some('A'),
         66 => Some('B'),
         67 => Some('C'),
         68 => Some('D'),
         69 => Some('E'),
         70 => Some('F'),
         71 => Some('G'),
         72 => Some('H'),
         73 => Some('I'),
         74 => Some('J'),
         75 => Some('K'),
         76 => Some('L'),
         77 => Some('M'),
         78 => Some('N'),
         79 => Some('O'),
         80 => Some('P'),
         81 => Some('Q'),
         82 => Some('R'),
         83 => Some('S'),
         84 => Some('T'),
         85 => Some('U'),
         86 => Some('V'),
         87 => Some('W'),
         88 => Some('X'),
         89 => Some('Y'),
         90 => Some('Z'),
         91 => Some('['),
         92 => Some('\\'),
         93 => Some(']'),
         94 => Some('^'),
         95 => Some('_'),
         96 => Some('`'),
         97 => Some('a'),
         98 => Some('b'),
         99 => Some('c'),
        100 => Some('d'),
        101 => Some('e'),
        102 => Some('f'),
        103 => Some('g'),
        104 => Some('h'),
        105 => Some('i'),
        106 => Some('j'),
        107 => Some('k'),
        108 => Some('l'),
        109 => Some('m'),
        110 => Some('n'),
        111 => Some('o'),
        112 => Some('p'),
        113 => Some('q'),
        114 => Some('r'),
        115 => Some('s'),
        116 => Some('t'),
        117 => Some('u'),
        118 => Some('v'),
        119 => Some('w'),
        120 => Some('x'),
        121 => Some('y'),
        122 => Some('z'),
        123 => Some('{'),
        124 => Some('|'),
        125 => Some('}'),
        126 => Some('~'),
        127 => Some('¬'),
         _  => None,
    }
}
