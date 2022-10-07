use std::io::{self, Write};

/// Write big-endian integers directly to a stream.
pub trait WriteBE: Write {
    fn write_u8(&mut self, val: u8) -> io::Result<()>;
    fn write_be_u16(&mut self, val: u16) -> io::Result<()>;
    fn write_be_u32(&mut self, val: u32) -> io::Result<()>;
}

/// Everything that implements Write can also implement WriteBE.
impl<T: Write> WriteBE for T {
    fn write_u8(&mut self, val: u8) -> io::Result<()> {
        let buf = [val];
        self.write_all(&buf)
    }

    fn write_be_u16(&mut self, val: u16) -> io::Result<()> {
        let buf = val.to_be_bytes();
        self.write_all(&buf)
    }

    fn write_be_u32(&mut self, val: u32) -> io::Result<()> {
        let buf = val.to_be_bytes();
        self.write_all(&buf)
    }
}
