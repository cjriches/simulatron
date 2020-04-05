use std::rc::Rc;
use std::cell::RefCell;
use std::sync::mpsc::Sender;

use crate::cpu::INTERRUPT_ILLEGAL_OPERATION;
use crate::display::DisplayController;
use crate::keyboard::KeyboardController;
use crate::ram::RAM;
use crate::rom::ROM;

pub struct MMU {
    interrupt_channel: Sender<u32>,
    display: Rc<DisplayController>,
    keyboard: Rc<RefCell<KeyboardController>>,
    ram: Rc<RAM>,
    rom: Rc<ROM>,
}

impl MMU {
    pub fn new(interrupt_channel: Sender<u32>,
               display: Rc<DisplayController>,
               keyboard: Rc<RefCell<KeyboardController>>,
               ram: Rc<RAM>,
               rom: Rc<ROM>) -> Self {
        MMU {
            interrupt_channel,
            display,
            keyboard,
            ram,
            rom,
        }
    }

    pub fn store_direct(&self, address: u32, value: u8) {
        if address < 32 {  // Interrupt handlers
            unimplemented!();
        } else if address < 577 {  // ROM and keyboard buffer
            self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION)
                .expect("Failed to send interrupt on illegal memory store.");
        } else if address < 6577 {  // Memory-mapped display
            self.display.store(address - 577, value);
        } else {
            unimplemented!();
        }
    }

    pub fn load_direct(&self, address: u32) -> u8 {
        if address < 32 {  // Interrupt handlers
            unimplemented!();
        } else if address < 576 {  // ROM
            self.rom.load(address - 64)
        } else if address == 576 {  // Keyboard buffer
            self.keyboard.borrow().load()
        } else if address < 6577 {  // Memory-mapped display
            self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION)
                .expect("Failed to send interrupt on illegal memory load.");
            return 0;
        } else {
            unimplemented!();
        }
    }
}
