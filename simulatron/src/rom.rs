use std::convert::TryFrom;

pub struct ROM {
    bytes: Box<[u8; 512]>,  // Allocate 512 bytes on heap.
}

impl ROM {
    pub fn new() -> Self {
        ROM {
            bytes: Box::new([0; 512])
        }
    }

    pub fn load(&self, address: u32) -> u8 {
        let index = usize::try_from(address).unwrap();
        self.bytes[index]
    }
}
