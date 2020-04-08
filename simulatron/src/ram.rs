use std::convert::TryFrom;

pub struct RAM {
    bytes: Vec<u8>,
}

impl RAM {
    pub fn new() -> Self {
        RAM {
            bytes: Vec::with_capacity(1 << 26),  // 64 MiB to start with (2^26).
        }
    }

    pub fn store(&mut self, address: u32, value: u8) {
        let index = usize::try_from(address).unwrap();
        // Resize if necessary. This is not particularly efficient, as it will consume memory
        // up to the highest index that has been written to, regardless of what's in the middle.
        // It is, however, simple.
        if index >= self.bytes.len() {
            // TODO Should we over-allocate here to anticipate sequential writes?
            self.bytes.resize(index + 1, 0);
        }
        self.bytes[index] = value;
    }

    pub fn load(&self, address: u32) -> u8 {
        let index = usize::try_from(address).unwrap();
        *match self.bytes.get(index) {
            Some(value) => value,
            None => &0
        }
    }
}
