use std::sync::{Arc, Mutex, mpsc::Sender};

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
    interrupt_vector: [u8; 32],
    disk_a: Arc<Mutex<DiskController>>,
    disk_b: Arc<Mutex<DiskController>>,
    display: DisplayController,
    keyboard: Arc<Mutex<KeyboardController>>,
    ram: RAM,
    rom: ROM,
    pfsr: u32,  // Page Fault Status Register
}

impl MMU {
    pub fn new(interrupt_channel: Sender<u32>,
               disk_a: Arc<Mutex<DiskController>>,
               disk_b: Arc<Mutex<DiskController>>,
               display: DisplayController,
               keyboard: Arc<Mutex<KeyboardController>>,
               ram: RAM,
               rom: ROM) -> Self {
        MMU {
            interrupt_channel,
            interrupt_vector: [0; 32],
            disk_a,
            disk_b,
            display,
            keyboard,
            ram,
            rom,
            pfsr: 0,
        }
    }

    pub fn page_fault_status_register(&self) -> u32 {
        self.pfsr
    }

    pub fn store_virtual_8(&mut self, pdpr: u32, address: u32, value: u8) {
        match self.virtual_to_physical_address(address, pdpr, Intent::Write) {
            Some(physical_address) => self.store_physical_8(physical_address, value),
            None => self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap()
        };
    }

    pub fn store_virtual_16(&mut self, pdpr: u32, address: u32, value: u16) {
        match self.virtual_to_physical_address(address, pdpr, Intent::Write) {
            Some(physical_address) => self.store_physical_16(physical_address, value),
            None => self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap()
        };
    }

    pub fn store_virtual_32(&mut self, pdpr: u32, address: u32, value: u32) {
        match self.virtual_to_physical_address(address, pdpr, Intent::Write) {
            Some(physical_address) => self.store_physical_32(physical_address, value),
            None => self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap()
        };
    }

    pub fn load_virtual_8(&mut self, pdpr: u32, address: u32, is_fetch: bool) -> Option<u8> {
        let intent = if is_fetch {Intent::Execute} else {Intent::Read};
        match self.virtual_to_physical_address(address, pdpr, intent) {
            Some(physical_address) => self.load_physical_8(physical_address),
            None => {
                self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap();
                None
            }
        }
    }

    pub fn load_virtual_16(&mut self, pdpr: u32, address: u32, is_fetch: bool) -> Option<u16> {
        let intent = if is_fetch {Intent::Execute} else {Intent::Read};
        match self.virtual_to_physical_address(address, pdpr, intent) {
            Some(physical_address) => self.load_physical_16(physical_address),
            None => {
                self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap();
                None
            }
        }
    }

    pub fn load_virtual_32(&mut self, pdpr: u32, address: u32, is_fetch: bool) -> Option<u32> {
        let intent = if is_fetch {Intent::Execute} else {Intent::Read};
        match self.virtual_to_physical_address(address, pdpr, intent) {
            Some(physical_address) => self.load_physical_32(physical_address),
            None => {
                self.interrupt_channel.send(INTERRUPT_PAGE_FAULT).unwrap();
                None
            }
        }
    }

    pub fn store_physical_8(&mut self, address: u32, value: u8) {
        macro_rules! reject {
            () => {{self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()}};
        }

        if address < 32 {            // Interrupt handlers
            self.interrupt_vector[address as usize] = value;
        } else if address < 576 {    // Reserved, ROM
            reject!();
        } else if address < 6576 {   // Memory-mapped display
            self.display.store(address - 576, value);
        } else if address < 8177 {   // Keyboard, Reserved, Disk A read-only
            reject!();
        } else if address < 8182 {   // Disk A control
            self.disk_a.lock().unwrap().store_control(address - 8177, value);
        } else if address < 8187 {   // Disk B read-only
            reject!();
        } else if address < 8192 {   // Disk B control
            self.disk_b.lock().unwrap().store_control(address - 8187, value);
        } else if address < 12288 {  // Disk A data
            self.disk_a.lock().unwrap().store_data(address - 8192, value);
        } else if address < 16384 {  // Disk B data
            self.disk_b.lock().unwrap().store_data(address - 12288, value);
        } else {                     // RAM
            self.ram.store(address - 16384, value);
        }
    }

    pub fn store_physical_16(&mut self, address: u32, value: u16) {
        let [upper, lower] = u16::to_be_bytes(value);
        self.store_physical_8(address, upper);
        self.store_physical_8(address + 1, lower);
    }

    pub fn store_physical_32(&mut self, address: u32, value: u32) {
        let [upper, upper_mid, lower_mid, lower] = u32::to_be_bytes(value);
        self.store_physical_8(address, upper);
        self.store_physical_8(address + 1, upper_mid);
        self.store_physical_8(address + 2, lower_mid);
        self.store_physical_8(address + 3, lower);
    }

    pub fn load_physical_8(&self, address: u32) -> Option<u8> {
        macro_rules! reject {
            () => {{
                self.interrupt_channel.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                None
            }};
        }

        if address < 32 {            // Interrupt handlers
            Some(self.interrupt_vector[address as usize])
        } else if address < 64 {     // Reserved
            reject!()
        } else if address < 576 {    // ROM
            Some(self.rom.load(address - 64))
        } else if address < 6576 {   // Memory-mapped display
            reject!()
        } else if address < 6578 {   // Keyboard buffers
            Some(self.keyboard.lock().unwrap().load(address - 6576))
        } else if address < 8172 {   // Reserved
            reject!()
        } else if address < 8177 {   // Disk A read-only
            Some(self.disk_a.lock().unwrap().load_status(address - 8172))
        } else if address < 8182 {   // Disk A control
            reject!()
        } else if address < 8187 {   // Disk B read-only
            Some(self.disk_b.lock().unwrap().load_status(address - 8182))
        } else if address < 8192 {   // Disk B control
            reject!()
        } else if address < 12288 {  // Disk A data
            Some(self.disk_a.lock().unwrap().load_data(address - 8192))
        } else if address < 16384 {  // Disk B data
            Some(self.disk_b.lock().unwrap().load_data(address - 12288))
        } else {                     // RAM
            Some(self.ram.load(address - 16384))
        }
    }

    pub fn load_physical_16(&self, address: u32) -> Option<u16> {
        let upper = self.load_physical_8(address)?;
        let lower = self.load_physical_8(address + 1)?;
        Some(u16::from_be_bytes([upper, lower]))
    }

    pub fn load_physical_32(&self, address: u32) -> Option<u32> {
        let upper = self.load_physical_8(address)?;
        let upper_mid = self.load_physical_8(address + 1)?;
        let lower_mid = self.load_physical_8(address + 2)?;
        let lower = self.load_physical_8(address + 3)?;
        Some(u32::from_be_bytes([upper, upper_mid, lower_mid, lower]))
    }

    fn virtual_to_physical_address(&mut self, virtual_address: u32, pdpr: u32,
                                   intent: Intent) -> Option<u32> {
        // Find the directory entry.
        let directory_entry_address = pdpr + 4*(virtual_address >> 22); // First 10 bits of v-addr.
        let directory_entry = self.load_physical_32(directory_entry_address)?;
        // Check it's valid.
        if (directory_entry & 1) == 0 {
            self.pfsr = PAGE_FAULT_INVALID_PAGE;
            return None;
        }
        // Find the page table entry.
        let page_table_base = directory_entry & 0xFFFFF000;  // First 20 bits of entry.
        let page_table_offset = 4*((virtual_address >> 12) & 0x3FF);  // Second 10 bits of v-addr.
        let page_table_entry = self.load_physical_32(page_table_base + page_table_offset)?;
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
            if (page_table_entry & 32) != 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    const RAM_BASE: u32 = 0x4000;

    struct MMUFixture {
        mmu: MMU,
        interrupt_rx: Receiver<u32>,
    }

    impl MMUFixture {
        fn new() -> Self {
            let (interrupt_tx, interrupt_rx) = mpsc::channel();
            let disk_a = Arc::new(Mutex::new(DiskController::new(
                "UNUSED", interrupt_tx.clone(), 0)));
            let disk_b = Arc::new(Mutex::new(DiskController::new(
                "UNUSED", interrupt_tx.clone(), 0)));
            let (display_tx, _) = mpsc::channel();
            let display = DisplayController::new(display_tx);
            let (keyboard_tx, keyboard_rx) = mpsc::channel();
            let keyboard = Arc::new(Mutex::new(KeyboardController::new(
                keyboard_tx, keyboard_rx, interrupt_tx.clone())));
            let ram = RAM::new();
            let rom = ROM::new([0; 512]);

            MMUFixture {
                mmu: MMU {
                    interrupt_channel: interrupt_tx,
                    interrupt_vector: [0; 32],
                    disk_a,
                    disk_b,
                    display,
                    keyboard,
                    ram,
                    rom,
                    pfsr: 0
                },
                interrupt_rx,
            }
        }
    }

    #[test]
    fn test_physical_ram() {
        let mut fixture = MMUFixture::new();

        assert_eq!(fixture.mmu.load_physical_32(RAM_BASE), Some(0));
        fixture.mmu.store_physical_8(RAM_BASE, 0x01);
        fixture.mmu.store_physical_16(RAM_BASE + 2, 0x1234);
        assert_eq!(fixture.mmu.load_physical_32(RAM_BASE), Some(0x01001234));
    }

    #[test]
    fn test_address_translation() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = RAM_BASE;
        // Write a single page directory and page table entry.
        let directory_entry = 0x00005001;  // Frame 1 of RAM, Valid.
        fixture.mmu.store_physical_32(RAM_BASE, directory_entry);
        let page_entry = 0x00006007; // Frame 2 of RAM, Valid, Present, Readable.
        fixture.mmu.store_physical_32(RAM_BASE + 0x1000, page_entry);
        assert_eq!(fixture.mmu.virtual_to_physical_address(0, PDPR, Intent::Read),
                   Some(0x00006000));
    }

    #[test]
    fn test_address_translation_external_interface() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00100000;
        // Write a single page directory and page table entry.
        let directory_entry = 0x00004001;  // Frame 0 of RAM, Valid.
        fixture.mmu.store_physical_32(0x00100000, directory_entry);
        let page_entry = 0x0000A00F; // Frame 10 of RAM, Valid, Present, Readable, Writable.
        fixture.mmu.store_physical_32(0x00004000, page_entry);
        // Write a pattern via virtual.
        fixture.mmu.store_virtual_8(PDPR, 0, 0x55);
        fixture.mmu.store_virtual_32(PDPR, 1, 0xDEADBEEF);
        // Assert no interrupts.
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
        // Read it back through virtual.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0, false), Some(0x55DEADBE));
        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 4, false), Some(0xEF));
        // Read it back through physical where we expect it to be.
        assert_eq!(fixture.mmu.load_physical_32(0x0000A000), Some(0x55DEADBE));
        assert_eq!(fixture.mmu.load_physical_8(0x0000A004), Some(0xEF));
    }

    #[test]
    fn test_invalid_page_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = RAM_BASE;
        // 0 is an invalid page directory entry; don't need to write anything.
        // Any translation should fail.
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0, Intent::Read), None);
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 1246, Intent::Write), None);
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 678424657, Intent::Execute), None);

        // Now write a valid page directory entry.
        fixture.mmu.store_physical_32(RAM_BASE, 0x00005001); // Frame 1 of RAM, Valid.
        // Write some invalid page table entries to make sure the correct bit is being checked.
        for i in 0..3 {
            let page_entry = rand::random::<u32>() << 1;
            fixture.mmu.store_physical_32(RAM_BASE + 0x1000 + (i*4), page_entry);
        }
        // Any translation should still fail.
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x0000, Intent::Read), None);
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x1000, Intent::Write), None);
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x2000, Intent::Execute), None);
        // Also test one where we didn't write a page entry.
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x3000, Intent::Read), None);
    }

    #[test]
    fn test_invalid_page_fault_external_interface() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00420000;
        // 0 is an invalid page directory entry; don't need to write anything.
        // Any translation should fail.
        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 0, false), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        assert_eq!(fixture.mmu.load_virtual_16(PDPR, 0x1000, true), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        fixture.mmu.store_virtual_32(PDPR, 0x10010, 420);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);

        // Now write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00420000, 0x00004001); // Frame 0 of RAM, Valid.
        // Write some invalid page table entries to make sure the correct bit is being checked.
        for i in 0..3 {
            let page_entry = rand::random::<u32>() << 1;
            fixture.mmu.store_physical_32(0x00004000 + (i*4), page_entry);
        }
        // Any translation should still fail.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x0000, false), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x1000, true), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        fixture.mmu.store_virtual_8(PDPR, 0x2000, 99);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        // Also test one where we didn't write a page entry.
        fixture.mmu.store_virtual_16(PDPR, 0x3000, 5);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
    }

    #[test]
    fn test_illegal_access_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00004000;
        // Write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00004000, 0x00005001);
        // Write a page table entry with only read.
        fixture.mmu.store_physical_32(0x00005000, 0x00006007);
        // Write a page table entry with only write.
        fixture.mmu.store_physical_32(0x00005004, 0x0000700B);
        // Write a page table entry with only execute.
        fixture.mmu.store_physical_32(0x00005008, 0x00008013);

        // First page entry should only allow read.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x0000, false), Some(0));
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
        fixture.mmu.store_virtual_32(PDPR, 0x0000, 56);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x0000, true), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);

        // Second page entry should only allow write.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x1000, false), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        fixture.mmu.store_virtual_32(PDPR, 0x1000, 56);
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x1000, true), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);

        // Third page entry should only allow execute.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x2000, false), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        fixture.mmu.store_virtual_32(PDPR, 0x2000, 56);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x2000, true), Some(0));
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
    }

    #[test]
    fn test_not_present_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00004000;
        // Write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00004000, 0x00005001);
        // Write a page table entry with all permissions but not present.
        fixture.mmu.store_physical_32(0x00005000, 0x0000601D);

        fixture.mmu.store_physical_8(0x6FFF, 12);
        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 0x0FFF, false), None);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_NOT_PRESENT);

        // Set present.
        fixture.mmu.store_physical_8(0x00005003, 0x1F);

        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 0x0FFF, true), Some(12));
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
    }

    #[test]
    fn test_cow_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00004000;
        // Write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00004000, 0x00005001);
        // Write a page table entry with all permissions but COW.
        fixture.mmu.store_physical_32(0x00005000, 0x0000603F);

        // Assert COW page fault.
        fixture.mmu.store_virtual_32(PDPR, 0x0123, 0x420);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_COW);

        // Disable write permission.
        fixture.mmu.store_physical_8(0x00005003, 0x37);

        // Assert illegal access page fault.
        fixture.mmu.store_virtual_32(PDPR, 0x0123, 0x420);
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);

        // Assert the write didn't go through either time.
        assert_eq!(fixture.mmu.load_physical_32(0x6123), Some(0));
    }
}
