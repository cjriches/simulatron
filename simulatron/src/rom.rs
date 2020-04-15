use std::convert::TryFrom;

pub struct ROM {
    bytes: Box<[u8; 512]>,  // Allocate 512 bytes on heap.
}

impl ROM {
    pub fn new() -> Self {
        let mut test_rom = [0; 512];
        test_rom[0] = 0x86;  // Copy literal
        test_rom[4] = 0x24;  // character '$'
        test_rom[8] = 0x10;  // into r0b
        test_rom[9] = 0x82;  // Store at literal address
        test_rom[13] = 0x10; // r0b into
        test_rom[16] = 0x02; // address of first display character
        test_rom[17] = 0x40;
        test_rom[18] = 0x00; // Halt

        ROM {
            bytes: Box::new(test_rom)
        }
    }

    pub fn load(&self, address: u32) -> u8 {
        let index = usize::try_from(address).unwrap();
        self.bytes[index]
    }
}
