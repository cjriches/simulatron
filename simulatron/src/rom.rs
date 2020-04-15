pub struct ROM {
    bytes: [u8; 512],
}

impl ROM {
    pub fn new(data: [u8; 512]) -> Self {
        ROM {
            bytes: data,
        }
    }

    pub fn load(&self, address: u32) -> u8 {
        self.bytes[address as usize]
    }
}
