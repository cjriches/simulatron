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
        macro_rules! reject {
            () => {{self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()}};
        }

        if address < 32 {            // Interrupt handlers
            unimplemented!();
        } else if address < 576 {    // Reserved, ROM
            reject!();
        } else if address < 6576 {   // Memory-mapped display
            self.display.store(address - 576, value);
        } else if address < 8177 {   // Keyboard, Reserved, Disk A read-only
            reject!();
        } else if address < 8182 {   // Disk A control
            self.disk_a.borrow_mut().store_control(address - 8177, value);
        } else if address < 8187 {   // Disk B read-only
            reject!();
        } else if address < 8192 {   // Disk B control
            self.disk_b.borrow_mut().store_control(address - 8187, value);
        } else if address < 12288 {  // Disk A data
            self.disk_a.borrow_mut().store_data(address - 8192, value);
        } else if address < 16384 {  // Disk B data
            self.disk_b.borrow_mut().store_data(address - 12288, value);
        } else {                     // RAM
            self.ram.borrow_mut().store(address - 16384, value);
        }
    }

    pub fn load_direct(&self, address: u32) -> u8 {
        macro_rules! reject {
            () => {{self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()}};
        }

        if address < 32 {            // Interrupt handlers
            unimplemented!();
        } else if address < 64 {     // Reserved
            reject!();
            0
        } else if address < 576 {    // ROM
            self.rom.load(address - 64)
        } else if address < 6576 {   // Memory-mapped display
            reject!();
            0
        } else if address < 6578 {   // Keyboard buffers
            self.keyboard.borrow().load(address - 6576)
        } else if address < 8172 {   // Reserved
            reject!();
            0
        } else if address < 8177 {   // Disk A read-only
            self.disk_a.borrow().load_status(address - 8172)
        } else if address < 8182 {   // Disk A control
            reject!();
            0
        } else if address < 8187 {   // Disk B read-only
            self.disk_b.borrow().load_status(address - 8182)
        } else if address < 8192 {   // Disk B control
            reject!();
            0
        } else if address < 12288 {  // Disk A data
            self.disk_a.borrow().load_data(address - 8192)
        } else if address < 16384 {  // Disk B data
            self.disk_b.borrow().load_data(address - 12288)
        } else {                     // RAM
            self.ram.borrow().load(address - 16384)
        }
    }
}
