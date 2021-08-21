// Register addresses
pub const ADDRESS_STATUS: u32 = 0;  // Status.
pub const ADDRESS_NBA_1: u32 = 1;   // Number of blocks available.
pub const ADDRESS_NBA_2: u32 = 2;
pub const ADDRESS_NBA_3: u32 = 3;
pub const ADDRESS_NBA_4: u32 = 4;
pub const ADDRESS_DA_1: u32 = 5;    // Disk address.
pub const ADDRESS_DA_2: u32 = 6;
pub const ADDRESS_DA_3: u32 = 7;
pub const ADDRESS_DA_4: u32 = 8;
pub const ADDRESS_CMD: u32 = 9;     // Command.

// Bit flags for the status register.
pub const FLAG_CONNECTED: u8 = 0x01;
pub const FLAG_FINISHED: u8 = 0x02;
pub const FLAG_SUCCESS: u8 = 0x04;
pub const FLAG_BAD_COMMAND: u8 = 0x08;

// Allowed commands.
pub const COMMAND_READ: u8 = 1;
pub const COMMAND_WRITE: u8 = 2;
pub const COMMAND_CONTIGUOUS_READ: u8 = 3;
pub const COMMAND_CONTIGUOUS_WRITE: u8 = 4;

// Size of disk buffer.
pub const DISK_BUFFER_SIZE: usize = 0x1000;  // 4096 bytes = one page.

pub trait DiskController: Send {
    fn start(&mut self);
    fn stop(&mut self);

    fn store_control(&mut self, address: u32, value: u8);
    fn load_status(&self, address: u32) -> u8;
    fn store_data(&mut self, address: u32, value: u8);
    fn load_data(&self, address: u32) -> u8;
}
