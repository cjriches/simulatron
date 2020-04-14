use std::convert::TryFrom;

pub struct ROM {
    bytes: Box<[u8; 512]>,  // Allocate 512 bytes on heap.
}

impl ROM {
    pub fn new() -> Self {
        let mut test_rom = [0; 512];
        test_rom[0] = 0x82;  // Store literal into literal address.
        test_rom[4] = 0x24;  // Character 36: $.
        test_rom[7] = 0x02;  // Address 576: first display character.
        test_rom[8] = 0x40;
        test_rom[9] = 0x01;  // Pause.

        ROM {
            bytes: Box::new(test_rom)
        }
    }

    pub fn load(&self, address: u32) -> u8 {
        let index = usize::try_from(address).unwrap();
        self.bytes[index]
    }
}
