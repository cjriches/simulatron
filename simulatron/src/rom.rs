use std::convert::TryFrom;

pub struct ROM {
    bytes: Box<[u8; 512]>,  // Allocate 512 bytes on heap.
}

impl ROM {
    pub fn new() -> Self {
        let mut test_rom = [0; 512];
        test_rom[0] = 0x86;  // Copy literal
        test_rom[4] = 0x24;  // character '$'
        test_rom[8] = 0x10;  // into r0b.

        test_rom[9] = 0x82;  // Store
        test_rom[13] = 0x10; // r0b into
        test_rom[16] = 0x02; // address of first display character.
        test_rom[17] = 0x40;

        test_rom[18] = 0x86; // Copy literal
        test_rom[21] = 0x50; // address 0x00005000
        test_rom[26] = 0x22; // into kspr.

        test_rom[27] = 0x86; // Copy literal
        test_rom[30] = 0x40; // address 0x00004000
                             // into r0.

        test_rom[36] = 0x82; // Store
                             // r0 into
        test_rom[44] = 0x04; // keyboard interrupt handler.

        test_rom[45] = 0x86; // Copy literal
        test_rom[49] = 0x05; // instruction IRETURN
        test_rom[53] = 0x10; // into r0b.

        test_rom[54] = 0x82; // Store
        test_rom[58] = 0x10; // r0b into
        test_rom[61] = 0x40; // literal address 0x00004000.

        test_rom[63] = 0x86; // Copy literal
        test_rom[67] = 0x02; // keyboard interrupt only
        test_rom[71] = 0x24; // into imr.

        test_rom[72] = 0x01; // Pause.

        test_rom[73] = 0x80; // Load from literal
        test_rom[76] = 0x19; // address of key buffer
        test_rom[77] = 0xB0;
        test_rom[81] = 0x10; // into r0b.

        test_rom[82] = 0x82; // Store
        test_rom[86] = 0x10; // r0b into
        test_rom[89] = 0x02; // address of second display character.
        test_rom[90] = 0x41;

        test_rom[91] = 0x01; // Pause.

        test_rom[92] = 0x00; // Halt.

        ROM {
            bytes: Box::new(test_rom)
        }
    }

    pub fn load(&self, address: u32) -> u8 {
        let index = usize::try_from(address).unwrap();
        self.bytes[index]
    }
}
