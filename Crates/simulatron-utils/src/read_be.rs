use std::io::{self, Read};

/// Read big-endian integers directly from a stream.
pub trait ReadBE: Read {
    fn read_u8(&mut self) -> Result<u8, io::Error>;
    fn read_be_u16(&mut self) -> Result<u16, io::Error>;
    fn read_be_u32(&mut self) -> Result<u32, io::Error>;
}

/// Everything that implements Read can also implement ReadBE.
impl<T: Read> ReadBE for T {
    fn read_u8(&mut self) -> Result<u8, io::Error> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_be_u16(&mut self) -> Result<u16, io::Error> {
        let mut buf = [0; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_be_bytes(buf))
    }

    fn read_be_u32(&mut self) -> Result<u32, io::Error> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_be_bytes(buf))
    }
}
