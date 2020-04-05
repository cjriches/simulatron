use std::rc::Rc;
use std::cell::RefCell;
use std::sync::mpsc::Sender;

use crate::cpu::INTERRUPT_ILLEGAL_OPERATION;
use crate::disk::DiskController;
use crate::display::DisplayController;
use crate::keyboard::KeyboardController;
use crate::ram::RAM;
use crate::rom::ROM;

pub struct MMU {
    interrupt_channel: Sender<u32>,
    disk_a: Rc<RefCell<DiskController>>,
    disk_b: Rc<RefCell<DiskController>>,
    display: Rc<DisplayController>,
    keyboard: Rc<RefCell<KeyboardController>>,
    ram: Rc<RefCell<RAM>>,
    rom: Rc<ROM>,
}

impl MMU {
    pub fn new(interrupt_channel: Sender<u32>,
               disk_a: Rc<RefCell<DiskController>>,
               disk_b: Rc<RefCell<DiskController>>,
               display: Rc<DisplayController>,
               keyboard: Rc<RefCell<KeyboardController>>,
               ram: Rc<RefCell<RAM>>,
               rom: Rc<ROM>) -> Self {
        MMU {
            interrupt_channel,
            disk_a,
            disk_b,
            display,
            keyboard,
            ram,
            rom,
        }
    }

    pub fn store_direct(&mut self, address: u32, value: u8) {
        let reject = || self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION)
            .expect("Failed to send interrupt on illegal memory store.");

        if address < 32 {            // Interrupt handlers
            unimplemented!();
        } else if address < 577 {    // Reserved, ROM, and keyboard buffer
            reject();
        } else if address < 6577 {   // Memory-mapped display
            self.display.store(address - 577, value);
        } else if address < 8177 {   // Reserved, Disk A read-only
            reject();
        } else if address < 8182 {   // Disk A control
            unimplemented!();
        } else if address < 8187 {   // Disk B read-only
            reject();
        } else if address < 8192 {   // Disk B control
            unimplemented!();
        } else if address < 12288 {  // Disk A data
            unimplemented!();
        } else if address < 16384 {  // Disk B data
            unimplemented!();
        } else {                     // RAM
            self.ram.borrow_mut().store(address - 16384, value);
        }
    }

    pub fn load_direct(&self, address: u32) -> u8 {
        let reject = || self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION)
            .expect("Failed to send interrupt on illegal memory load.");

        if address < 32 {            // Interrupt handlers
            unimplemented!();
        } else if address < 64 {     // Reserved
            reject();
            0
        } else if address < 576 {    // ROM
            self.rom.load(address - 64)
        } else if address == 576 {   // Keyboard buffer
            self.keyboard.borrow().load()
        } else if address < 8172 {   // Memory-mapped display, Reserved
            reject();
            0
        } else if address < 8177 {   // Disk A read-only
            unimplemented!();
        } else if address < 8182 {   // Disk A control
            reject();
            0
        } else if address < 8187 {   // Disk B read-only
            unimplemented!();
        } else if address < 8192 {   // Disk B control
            reject();
            0
        } else if address < 12288 {  // Disk A data
            unimplemented!();
        } else if address < 16384 {  // Disk B data
            unimplemented!();
        } else {                     // RAM
            self.ram.borrow().load(address - 16384)
        }
    }
}
