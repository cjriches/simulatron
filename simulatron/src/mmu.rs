use std::rc::Rc;
use std::cell::RefCell;
use std::sync::mpsc::Sender;

use crate::cpu::{INTERRUPT_ILLEGAL_OPERATION, INTERRUPT_PAGE_FAULT};
use crate::disk::DiskController;
use crate::display::DisplayController;
use crate::keyboard::KeyboardController;
use crate::ram::RAM;
use crate::rom::ROM;

pub const PAGE_FAULT_INVALID_PAGE: u32 = 0;
pub const PAGE_FAULT_ILLEGAL_ACCESS: u32 = 1;
pub const PAGE_FAULT_NOT_PRESENT: u32 = 2;
pub const PAGE_FAULT_COW: u32 = 3;

enum Intent {
    Read,
    Write,
    Execute,
}

pub struct MMU {
    interrupt_channel: Sender<u32>,
    disk_a: Rc<RefCell<DiskController>>,
    disk_b: Rc<RefCell<DiskController>>,
    display: Rc<DisplayController>,
    keyboard: Rc<RefCell<KeyboardController>>,
    ram: Rc<RefCell<RAM>>,
    rom: Rc<ROM>,
    pdpr: u32,  // Page Directory Pointer Register
    pfsr: u32,  // Page Fault Status Register
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
            pdpr: 0,
            pfsr: 0,
        }
    }

    pub fn set_page_directory_pointer_register(&mut self, value: u32) {
        self.pdpr = value;
    }

    pub fn page_fault_status_register(&self) -> u32 {
        self.pfsr
    }

    pub fn store_virtual_8(&mut self, address: u32, value: u8) {
        match self.virtual_to_physical_address(address, Intent::Write) {
            Some(physical_address) => self.store_physical_8(physical_address, value),
            None => self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap()
        };
    }

    pub fn store_virtual_16(&mut self, address: u32, value: u16) {
        match self.virtual_to_physical_address(address, Intent::Write) {
            Some(physical_address) => self.store_physical_16(physical_address, value),
            None => self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap()
        };
    }

    pub fn store_virtual_32(&mut self, address: u32, value: u32) {
        match self.virtual_to_physical_address(address, Intent::Write) {
            Some(physical_address) => self.store_physical_32(physical_address, value),
            None => self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap()
        };
    }

    pub fn load_virtual_8(&mut self, address: u32, is_fetch: bool) -> u8 {
        let intent = if is_fetch {Intent::Execute} else {Intent::Read};
        match self.virtual_to_physical_address(address, intent) {
            Some(physical_address) => self.load_physical_8(physical_address),
            None => {
                self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap();
                0
            }
        }
    }

    pub fn load_virtual_16(&mut self, address: u32, is_fetch: bool) -> u16 {
        let intent = if is_fetch {Intent::Execute} else {Intent::Read};
        match self.virtual_to_physical_address(address, intent) {
            Some(physical_address) => self.load_physical_16(physical_address),
            None => {
                self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap();
                0
            }
        }
    }

    pub fn load_virtual_32(&mut self, address: u32, is_fetch: bool) -> u32 {
        let intent = if is_fetch {Intent::Execute} else {Intent::Read};
        match self.virtual_to_physical_address(address, intent) {
            Some(physical_address) => self.load_physical_32(physical_address),
            None => {
                self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap();
                0
            }
        }
    }

    pub fn store_physical_8(&mut self, address: u32, value: u8) {
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

    pub fn store_physical_16(&mut self, address: u32, value: u16) {
        let [upper, lower] = u16::to_le_bytes(value);
        self.store_physical_8(address, upper);
        self.store_physical_8(address + 1, lower);
    }

    pub fn store_physical_32(&mut self, address: u32, value: u32) {
        let [upper, upper_mid, lower_mid, lower] = u32::to_le_bytes(value);
        self.store_physical_8(address, upper);
        self.store_physical_8(address + 1, upper_mid);
        self.store_physical_8(address + 2, lower_mid);
        self.store_physical_8(address + 3, lower);
    }

    pub fn load_physical_8(&self, address: u32) -> u8 {
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

    pub fn load_physical_16(&self, address: u32) -> u16 {
        let upper = self.load_physical_8(address);
        let lower = self.load_physical_8(address + 1);
        u16::from_le_bytes([upper, lower])  // oui oui le baguette
    }

    pub fn load_physical_32(&self, address: u32) -> u32 {
        let upper = self.load_physical_8(address);
        let upper_mid = self.load_physical_8(address + 1);
        let lower_mid = self.load_physical_8(address + 2);
        let lower = self.load_physical_8(address + 3);
        u32::from_le_bytes([upper, upper_mid, lower_mid, lower])
    }

    fn virtual_to_physical_address(&mut self, virtual_address: u32, intent: Intent) -> Option<u32> {
        // Find the directory entry.
        let directory_entry_address = self.pdpr + 4*(virtual_address >> 22); // First 10 bits of v-addr.
        let directory_entry = self.load_physical_32(directory_entry_address);
        // Check it's valid.
        if (directory_entry & 1) == 0 {
            self.pfsr = PAGE_FAULT_INVALID_PAGE;
            return None;
        }
        // Find the page table entry.
        let page_table_base = directory_entry & 0xFFFFF000;  // First 20 bits of entry.
        let page_table_offset = 4*((virtual_address >> 12) & 0x3FF);  // Second 10 bits of v-addr.
        let page_table_entry = self.load_physical_32(page_table_base + page_table_offset);
        // Check it's valid.
        if (page_table_entry & 1) == 0 {
            self.pfsr = PAGE_FAULT_INVALID_PAGE;
            return None;
        }
        // Check it's present.
        if (page_table_entry & 2) == 0 {
            self.pfsr = PAGE_FAULT_NOT_PRESENT;
            return None;
        }
        // Check permissions.
        let legal = match intent {
            Intent::Read => page_table_entry & 4,
            Intent::Write => page_table_entry & 8,
            Intent::Execute => page_table_entry & 16,
        };
        if legal == 0 {
            self.pfsr = PAGE_FAULT_ILLEGAL_ACCESS;
            return None;
        }
        // Check COW.
        if let Intent::Write = intent {
            if (page_table_entry & 32) == 1 {
                self.pfsr = PAGE_FAULT_COW;
                return None;
            }
        }
        // It's allowed, so find the physical address.
        let frame = page_table_entry & 0xFFFFF000;
        let frame_offset = virtual_address & 0xFFF;
        Some(frame | frame_offset)
    }
}
