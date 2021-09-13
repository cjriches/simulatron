use std::sync::mpsc::Sender;

use crate::cpu::{CPUError::TryAgainError, CPUResult,
                 INTERRUPT_ILLEGAL_OPERATION, INTERRUPT_PAGE_FAULT};
use crate::disk::DiskController;
use crate::display::DisplayController;
use crate::keyboard::KeyboardController;
use crate::ram::RAM;

// Page fault types.
pub const PAGE_FAULT_INVALID_PAGE: u32 = 0;
pub const PAGE_FAULT_ILLEGAL_ACCESS: u32 = 1;
pub const PAGE_FAULT_NOT_PRESENT: u32 = 2;
pub const PAGE_FAULT_COW: u32 = 3;

// Memory-mapped zones.
const BEGIN_INTERRUPT_VECTOR: u32 = 0x0000;     // Read/Write
const BEGIN_RESERVED_1: u32 = 0x0020;           // No access
const BEGIN_ROM: u32 = 0x0040;                  // Read-only
const BEGIN_DISPLAY: u32 = 0x0240;              // Write-only
const BEGIN_KEYBOARD: u32 = 0x19B0;             // Read-only
const BEGIN_RESERVED_2: u32 = 0x19B2;           // No access
const BEGIN_DISK_A_STATUS: u32 = 0x1FEC;        // Read-only
const BEGIN_DISK_A_ADDRESS: u32 = 0x1FF1;       // Read/Write
const BEGIN_DISK_A_COMMAND: u32 = 0x1FF5;       // Write-only
const BEGIN_DISK_B_STATUS: u32 = 0x1FF6;        // Read-only
const BEGIN_DISK_B_ADDRESS: u32 = 0x1FFB;       // Read/WRite
const BEGIN_DISK_B_COMMAND: u32 = 0x1FFF;       // Write-only
const BEGIN_DISK_A_DATA: u32 = 0x2000;          // Read/Write
const BEGIN_DISK_B_DATA: u32 = 0x3000;          // Read/Write
const BEGIN_RAM: u32 = 0x4000;                  // Read/Write

const INTERRUPT_VECTOR_SIZE: usize = (BEGIN_RESERVED_1 - BEGIN_INTERRUPT_VECTOR) as usize;
type InterruptVector = [u8; INTERRUPT_VECTOR_SIZE];

pub const RAM_SIZE: usize = (u32::MAX - BEGIN_RAM + 1) as usize;

pub const ROM_SIZE: usize = (BEGIN_DISPLAY - BEGIN_ROM) as usize;
pub type ROM = [u8; ROM_SIZE];

/// The intent behind a memory access: important for checking virtual
/// memory permissions.
enum Intent {
    Read,
    Write,
    Execute,
}

/// A memory management unit.
pub struct MMU<D> {
    interrupt_tx: Sender<u32>,
    interrupt_vector: InterruptVector,
    disk_a: D,
    disk_b: D,
    display: DisplayController,
    keyboard: KeyboardController,
    ram: RAM,
    rom: ROM,
    pfsr: u32,  // Page Fault Status Register
}

impl<D: DiskController> MMU<D> {
    /// Construct a new MMU.
    pub fn new(interrupt_tx: Sender<u32>,
               disk_a: D,
               disk_b: D,
               display: DisplayController,
               keyboard: KeyboardController,
               rom: ROM) -> Self {
        MMU {
            interrupt_tx,
            interrupt_vector: [0; INTERRUPT_VECTOR_SIZE],
            disk_a,
            disk_b,
            display,
            keyboard,
            ram: RAM::new(),
            rom,
            pfsr: 0,
        }
    }

    /// Start all the peripherals mapped by the MMU. Panics if already running.
    pub fn start(&mut self) {
        self.disk_a.start();
        self.disk_b.start();
        self.keyboard.start();
    }

    /// Stop all the peripherals mapped by the MMU. Panics if not running.
    pub fn stop(&mut self) {
        self.disk_a.stop();
        self.disk_b.stop();
        self.keyboard.stop();
    }

    /// Read the page fault status register.
    pub fn page_fault_status_register(&self) -> u32 {
        self.pfsr
    }

    pub fn store_virtual_8(&mut self, pdpr: u32, address: u32, value: u8) -> CPUResult<()> {
        let physical_address = self.virtual_to_physical_address(address, pdpr, Intent::Write)?;
        self.store_physical_8(physical_address, value)
    }

    pub fn store_virtual_16(&mut self, pdpr: u32, address: u32, value: u16) -> CPUResult<()> {
        let [upper, lower] = u16::to_be_bytes(value);
        self.store_virtual_8(pdpr, address, upper)?;
        self.store_virtual_8(pdpr, address + 1, lower)
    }

    pub fn store_virtual_32(&mut self, pdpr: u32, address: u32, value: u32) -> CPUResult<()> {
        let [upper, upper_mid, lower_mid, lower] = u32::to_be_bytes(value);
        self.store_virtual_8(pdpr, address, upper)?;
        self.store_virtual_8(pdpr, address + 1, upper_mid)?;
        self.store_virtual_8(pdpr, address + 2, lower_mid)?;
        self.store_virtual_8(pdpr, address + 3, lower)
    }

    pub fn load_virtual_8(&mut self, pdpr: u32, address: u32, is_fetch: bool) -> CPUResult<u8> {
        let intent = if is_fetch {Intent::Execute} else {Intent::Read};
        let physical_address = self.virtual_to_physical_address(address, pdpr, intent)?;
        self.load_physical_8(physical_address)
    }

    pub fn load_virtual_16(&mut self, pdpr: u32, address: u32, is_fetch: bool) -> CPUResult<u16> {
        let upper = self.load_virtual_8(pdpr, address, is_fetch)?;
        let lower = self.load_virtual_8(pdpr, address + 1, is_fetch)?;
        Ok(u16::from_be_bytes([upper, lower]))
    }

    pub fn load_virtual_32(&mut self, pdpr: u32, address: u32, is_fetch: bool) -> CPUResult<u32> {
        let upper = self.load_virtual_8(pdpr, address, is_fetch)?;
        let upper_mid = self.load_virtual_8(pdpr, address + 1, is_fetch)?;
        let lower_mid = self.load_virtual_8(pdpr, address + 2, is_fetch)?;
        let lower = self.load_virtual_8(pdpr, address + 3, is_fetch)?;
        Ok(u32::from_be_bytes([upper, upper_mid, lower_mid, lower]))
    }

    pub fn store_physical_8(&mut self, address: u32, value: u8) -> CPUResult<()> {
        macro_rules! reject {
            () => {{
                self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                Err(TryAgainError)
            }};
        }

        if address < BEGIN_RESERVED_1 {  // Interrupt handlers
            self.interrupt_vector[address as usize] = value;
            Ok(())
        } else if address < BEGIN_DISPLAY {  // Reserved, ROM
            reject!()
        } else if address < BEGIN_KEYBOARD {  // Memory-mapped display
            self.display.store(address - BEGIN_DISPLAY, value);
            Ok(())
        } else if address < BEGIN_DISK_A_ADDRESS {  // Keyboard, Reserved, Disk A read-only
            reject!()
        } else if address < BEGIN_DISK_B_STATUS {  // Disk A control
            self.disk_a.store_control(address - BEGIN_DISK_A_STATUS, value);
            Ok(())
        } else if address < BEGIN_DISK_B_ADDRESS {  // Disk B read-only
            reject!()
        } else if address < BEGIN_DISK_A_DATA {  // Disk B control
            self.disk_b.store_control(address - BEGIN_DISK_B_STATUS, value);
            Ok(())
        } else if address < BEGIN_DISK_B_DATA {  // Disk A data
            self.disk_a.store_data(address - BEGIN_DISK_A_DATA, value);
            Ok(())
        } else if address < BEGIN_RAM {  // Disk B data
            self.disk_b.store_data(address - BEGIN_DISK_B_DATA, value);
            Ok(())
        } else {  // RAM
            self.ram[(address - BEGIN_RAM) as usize] = value;
            Ok(())
        }
    }

    pub fn store_physical_16(&mut self, address: u32, value: u16) -> CPUResult<()> {
        let [upper, lower] = u16::to_be_bytes(value);
        self.store_physical_8(address, upper)?;
        self.store_physical_8(address + 1, lower)
    }

    pub fn store_physical_32(&mut self, address: u32, value: u32) -> CPUResult<()> {
        let [upper, upper_mid, lower_mid, lower] = u32::to_be_bytes(value);
        self.store_physical_8(address, upper)?;
        self.store_physical_8(address + 1, upper_mid)?;
        self.store_physical_8(address + 2, lower_mid)?;
        self.store_physical_8(address + 3, lower)
    }

    pub fn load_physical_8(&self, address: u32) -> CPUResult<u8> {
        macro_rules! reject {
            () => {{
                self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                Err(TryAgainError)
            }};
        }

        if address < BEGIN_RESERVED_1 {  // Interrupt handlers
            Ok(self.interrupt_vector[address as usize])
        } else if address < BEGIN_ROM {  // Reserved
            reject!()
        } else if address < BEGIN_DISPLAY {  // ROM
            Ok(self.rom[(address - BEGIN_ROM) as usize])
        } else if address < BEGIN_KEYBOARD {  // Memory-mapped display
            reject!()
        } else if address < BEGIN_RESERVED_2 {  // Keyboard buffers
            Ok(self.keyboard.load(address - BEGIN_KEYBOARD))
        } else if address < BEGIN_DISK_A_STATUS {  // Reserved
            reject!()
        } else if address < BEGIN_DISK_A_COMMAND {  // Disk A readable
            Ok(self.disk_a.load_status(address - BEGIN_DISK_A_STATUS))
        } else if address < BEGIN_DISK_B_STATUS {  // Disk A control
            reject!()
        } else if address < BEGIN_DISK_B_COMMAND {  // Disk B readable
            Ok(self.disk_b.load_status(address - BEGIN_DISK_B_STATUS))
        } else if address < BEGIN_DISK_A_DATA {  // Disk B control
            reject!()
        } else if address < BEGIN_DISK_B_DATA {  // Disk A data
            Ok(self.disk_a.load_data(address - BEGIN_DISK_A_DATA))
        } else if address < BEGIN_RAM {  // Disk B data
            Ok(self.disk_b.load_data(address - BEGIN_DISK_B_DATA))
        } else {  // RAM
            Ok(self.ram[(address - BEGIN_RAM) as usize])
        }
    }

    pub fn load_physical_16(&self, address: u32) -> CPUResult<u16> {
        let upper = self.load_physical_8(address)?;
        let lower = self.load_physical_8(address + 1)?;
        Ok(u16::from_be_bytes([upper, lower]))
    }

    pub fn load_physical_32(&self, address: u32) -> CPUResult<u32> {
        let upper = self.load_physical_8(address)?;
        let upper_mid = self.load_physical_8(address + 1)?;
        let lower_mid = self.load_physical_8(address + 2)?;
        let lower = self.load_physical_8(address + 3)?;
        Ok(u32::from_be_bytes([upper, upper_mid, lower_mid, lower]))
    }

    fn virtual_to_physical_address(&mut self, virtual_address: u32, pdpr: u32,
                                   intent: Intent) -> CPUResult<u32> {
        // Find the directory entry.
        let directory_entry_address = pdpr + 4*(virtual_address >> 22); // First 10 bits of v-addr.
        let directory_entry = self.load_physical_32(directory_entry_address)?;
        // Check it's valid.
        if (directory_entry & 1) == 0 {
            self.pfsr = PAGE_FAULT_INVALID_PAGE;
            self.interrupt_tx.send(INTERRUPT_PAGE_FAULT).unwrap();
            return Err(TryAgainError);
        }
        // Find the page table entry.
        let page_table_base = directory_entry & 0xFFFFF000;  // First 20 bits of entry.
        let page_table_offset = 4*((virtual_address >> 12) & 0x3FF);  // Second 10 bits of v-addr.
        let page_table_entry = self.load_physical_32(page_table_base + page_table_offset)?;
        // Check it's valid.
        if (page_table_entry & 1) == 0 {
            self.pfsr = PAGE_FAULT_INVALID_PAGE;
            self.interrupt_tx.send(INTERRUPT_PAGE_FAULT).unwrap();
            return Err(TryAgainError);
        }
        // Check it's present.
        if (page_table_entry & 2) == 0 {
            self.pfsr = PAGE_FAULT_NOT_PRESENT;
            self.interrupt_tx.send(INTERRUPT_PAGE_FAULT).unwrap();
            return Err(TryAgainError);
        }
        // Check permissions.
        let legal = match intent {
            Intent::Read => page_table_entry & 4,
            Intent::Write => page_table_entry & 8,
            Intent::Execute => page_table_entry & 16,
        };
        if legal == 0 {
            self.pfsr = PAGE_FAULT_ILLEGAL_ACCESS;
            self.interrupt_tx.send(INTERRUPT_PAGE_FAULT).unwrap();
            return Err(TryAgainError);
        }
        // Check COW.
        if let Intent::Write = intent {
            if (page_table_entry & 32) != 0 {
                self.pfsr = PAGE_FAULT_COW;
                self.interrupt_tx.send(INTERRUPT_PAGE_FAULT).unwrap();
                return Err(TryAgainError);
            }
        }
        // It's allowed, so find the physical address.
        let frame = page_table_entry & 0xFFFFF000;
        let frame_offset = virtual_address & 0xFFF;
        Ok(frame | frame_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ntest::timeout;
    use rand::{self, distributions::Distribution, SeedableRng};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    use crate::init_test_logging;
    use crate::disk::MockDiskController;

    struct MMUFixture {
        mmu: MMU<MockDiskController>,
        interrupt_rx: Receiver<u32>,
    }

    impl MMUFixture {
        fn new() -> Self {
            init_test_logging();

            let (interrupt_tx, interrupt_rx) = mpsc::channel();
            let disk_a = MockDiskController;
            let disk_b = MockDiskController;
            let (display_tx, _) = mpsc::channel();
            let display = DisplayController::new(display_tx);
            let (keyboard_tx, keyboard_rx) = mpsc::channel();
            let keyboard = KeyboardController::new(
                keyboard_tx, keyboard_rx, interrupt_tx.clone());
            let rom = [0; ROM_SIZE];

            MMUFixture {
                mmu: MMU::new(
                    interrupt_tx,
                    disk_a,
                    disk_b,
                    display,
                    keyboard,
                    rom,
                ),
                interrupt_rx,
            }
        }
    }

    #[test]
    fn test_physical_ram() {
        let mut fixture = MMUFixture::new();

        assert_eq!(fixture.mmu.load_physical_32(BEGIN_RAM), Ok(0));
        fixture.mmu.store_physical_8(BEGIN_RAM, 0x01).unwrap();
        fixture.mmu.store_physical_16(BEGIN_RAM + 2, 0x1234).unwrap();
        assert_eq!(fixture.mmu.load_physical_32(BEGIN_RAM), Ok(0x01001234));
    }

    #[test]
    fn test_address_translation() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = BEGIN_RAM;
        // Write a single page directory and page table entry.
        let directory_entry = 0x00005001;  // Frame 1 of RAM, Valid.
        fixture.mmu.store_physical_32(BEGIN_RAM, directory_entry).unwrap();
        let page_entry = 0x00006007; // Frame 2 of RAM, Valid, Present, Readable.
        fixture.mmu.store_physical_32(BEGIN_RAM + 0x1000, page_entry).unwrap();
        assert_eq!(fixture.mmu.virtual_to_physical_address(0, PDPR, Intent::Read),
                   Ok(0x00006000));
    }

    #[test]
    fn test_address_translation_external_interface() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00100000;
        // Write a single page directory and page table entry.
        let directory_entry = 0x00004001;  // Frame 0 of RAM, Valid.
        fixture.mmu.store_physical_32(0x00100000, directory_entry).unwrap();
        let page_entry = 0x0000A00F; // Frame 10 of RAM, Valid, Present, Readable, Writable.
        fixture.mmu.store_physical_32(0x00004000, page_entry).unwrap();
        // Write a pattern via virtual.
        fixture.mmu.store_virtual_8(PDPR, 0, 0x55).unwrap();
        fixture.mmu.store_virtual_32(PDPR, 1, 0xDEADBEEF).unwrap();
        // Assert no interrupts.
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
        // Read it back through virtual.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0, false), Ok(0x55DEADBE));
        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 4, false), Ok(0xEF));
        // Read it back through physical where we expect it to be.
        assert_eq!(fixture.mmu.load_physical_32(0x0000A000), Ok(0x55DEADBE));
        assert_eq!(fixture.mmu.load_physical_8(0x0000A004), Ok(0xEF));
    }

    #[test]
    fn test_invalid_page_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = BEGIN_RAM;
        // 0 is an invalid page directory entry; don't need to write anything.
        // Any translation should fail.
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0, Intent::Read), Err(TryAgainError));
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 1246, Intent::Write), Err(TryAgainError));
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 678424657, Intent::Execute), Err(TryAgainError));

        // Now write a valid page directory entry.
        fixture.mmu.store_physical_32(BEGIN_RAM, 0x00005001).unwrap(); // Frame 1 of RAM, Valid.
        // Write some invalid page table entries to make sure the correct bit is being checked.
        for i in 0..3 {
            let page_entry = rand::random::<u32>() << 1;
            fixture.mmu.store_physical_32(BEGIN_RAM + 0x1000 + (i*4), page_entry).unwrap();
        }
        // Any translation should still fail.
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x0000, Intent::Read), Err(TryAgainError));
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x1000, Intent::Write), Err(TryAgainError));
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x2000, Intent::Execute), Err(TryAgainError));
        // Also test one where we didn't write a page entry.
        assert_eq!(fixture.mmu.virtual_to_physical_address(PDPR, 0x3000, Intent::Read), Err(TryAgainError));
    }

    #[test]
    fn test_invalid_page_fault_external_interface() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00420000;
        // 0 is an invalid page directory entry; don't need to write anything.
        // Any translation should fail.
        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 0, false), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        assert_eq!(fixture.mmu.load_virtual_16(PDPR, 0x1000, true), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        fixture.mmu.store_virtual_32(PDPR, 0x10010, 420).unwrap_err();
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);

        // Now write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00420000, 0x00004001).unwrap(); // Frame 0 of RAM, Valid.
        // Write some invalid page table entries to make sure the correct bit is being checked.
        for i in 0..3 {
            let page_entry = rand::random::<u32>() << 1;
            fixture.mmu.store_physical_32(0x00004000 + (i*4), page_entry).unwrap();
        }
        // Any translation should still fail.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x0000, false), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x1000, true), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        fixture.mmu.store_virtual_8(PDPR, 0x2000, 99).unwrap_err();
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
        // Also test one where we didn't write a page entry.
        fixture.mmu.store_virtual_16(PDPR, 0x3000, 5).unwrap_err();
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_INVALID_PAGE);
    }

    #[test]
    fn test_illegal_access_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00004000;
        // Write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00004000, 0x00005001).unwrap();
        // Write a page table entry with only read.
        fixture.mmu.store_physical_32(0x00005000, 0x00006007).unwrap();
        // Write a page table entry with only write.
        fixture.mmu.store_physical_32(0x00005004, 0x0000700B).unwrap();
        // Write a page table entry with only execute.
        fixture.mmu.store_physical_32(0x00005008, 0x00008013).unwrap();

        // First page entry should only allow read.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x0000, false), Ok(0));
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
        fixture.mmu.store_virtual_32(PDPR, 0x0000, 56).unwrap_err();
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x0000, true), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);

        // Second page entry should only allow write.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x1000, false), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        fixture.mmu.store_virtual_32(PDPR, 0x1000, 56).unwrap();
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x1000, true), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);

        // Third page entry should only allow execute.
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x2000, false), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        fixture.mmu.store_virtual_32(PDPR, 0x2000, 56).unwrap_err();
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);
        assert_eq!(fixture.mmu.load_virtual_32(PDPR, 0x2000, true), Ok(0));
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
    }

    #[test]
    fn test_not_present_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00004000;
        // Write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00004000, 0x00005001).unwrap();
        // Write a page table entry with all permissions but not present.
        fixture.mmu.store_physical_32(0x00005000, 0x0000601D).unwrap();

        fixture.mmu.store_physical_8(0x6FFF, 12).unwrap();
        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 0x0FFF, false), Err(TryAgainError));
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_NOT_PRESENT);

        // Set present.
        fixture.mmu.store_physical_8(0x00005003, 0x1F).unwrap();

        assert_eq!(fixture.mmu.load_virtual_8(PDPR, 0x0FFF, true), Ok(12));
        fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)).unwrap_err();
    }

    #[test]
    fn test_cow_fault() {
        let mut fixture = MMUFixture::new();

        const PDPR: u32 = 0x00004000;
        // Write a valid page directory entry.
        fixture.mmu.store_physical_32(0x00004000, 0x00005001).unwrap();
        // Write a page table entry with all permissions but COW.
        fixture.mmu.store_physical_32(0x00005000, 0x0000603F).unwrap();

        // Assert COW page fault.
        fixture.mmu.store_virtual_32(PDPR, 0x0123, 0x420).unwrap_err();
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_COW);

        // Disable write permission.
        fixture.mmu.store_physical_8(0x00005003, 0x37).unwrap();

        // Assert illegal access page fault.
        fixture.mmu.store_virtual_32(PDPR, 0x0123, 0x420).unwrap_err();
        assert_eq!(fixture.interrupt_rx.recv_timeout(Duration::from_millis(10)),
                   Ok(INTERRUPT_PAGE_FAULT));
        assert_eq!(fixture.mmu.page_fault_status_register(), PAGE_FAULT_ILLEGAL_ACCESS);

        // Assert the write didn't go through either time.
        assert_eq!(fixture.mmu.load_physical_32(0x6123), Ok(0));
    }

    #[test]
    #[timeout(100)]
    fn test_ram_performance() {
        let mut fixture = MMUFixture::new();
        const RAM_SIZE: u32 = super::RAM_SIZE as u32;  // Easier than casting every time.
        const U32_SIZE: u32 = std::mem::size_of::<u32>() as u32;

        // Write to very start, halfway through, near the end, and very end of RAM.
        fixture.mmu.store_physical_8(BEGIN_RAM, 1).unwrap();
        fixture.mmu.store_physical_8(BEGIN_RAM + RAM_SIZE / 2, 2).unwrap();
        fixture.mmu.store_physical_8(BEGIN_RAM + (RAM_SIZE - 10), 3).unwrap();
        fixture.mmu.store_physical_8(BEGIN_RAM + (RAM_SIZE - 1), 4).unwrap();

        // Read back the same locations.
        assert_eq!(fixture.mmu.load_physical_8(BEGIN_RAM), Ok(1));
        assert_eq!(fixture.mmu.load_physical_8(BEGIN_RAM + RAM_SIZE / 2), Ok(2));
        assert_eq!(fixture.mmu.load_physical_8(BEGIN_RAM + (RAM_SIZE - 10)), Ok(3));
        assert_eq!(fixture.mmu.load_physical_8(BEGIN_RAM + (RAM_SIZE - 1)), Ok(4));

        // Perform some random access.
        const NUM_RANDOMS: usize = 1000;

        // Generate some random numbers to store.
        let mut random_data = Vec::with_capacity(NUM_RANDOMS);
        random_data.resize_with(NUM_RANDOMS, rand::random::<u32>);

        // Generate some (deterministic) random addresses, aligned by u32 and non-repeating.
        let uniform_gen = rand::distributions::Uniform::new(
            BEGIN_RAM / U32_SIZE,
            u32::MAX / U32_SIZE);
        let mut rng = rand::rngs::StdRng::seed_from_u64(0x9636734947793487);
        let mut random_addresses = Vec::with_capacity(NUM_RANDOMS);
        let mut i: usize = 0;
        while i < NUM_RANDOMS {
            let address_candidate = uniform_gen.sample(&mut rng) * U32_SIZE;
            if !random_addresses.contains(&address_candidate) {
                random_addresses.push(address_candidate);
                i += 1;
            }
        }

        // Random write.
        for i in 0..NUM_RANDOMS {
            fixture.mmu.store_physical_32(random_addresses[i], random_data[i]).unwrap();
        }

        // Random read.
        for i in 0..NUM_RANDOMS {
            assert_eq!(fixture.mmu.load_physical_32(random_addresses[i]), Ok(random_data[i]));
        }
    }
}
