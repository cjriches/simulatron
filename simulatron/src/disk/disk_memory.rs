use std::sync::mpsc::Sender;

use super::disk_interface::*;

/// A MemDiskController is backed by memory instead of a real file.
/// It will never report STATUS_DISCONNECTED or STATUS_ERROR.
pub struct MemDiskController {
    num_blocks: u32,
    address: u32,
    disk: Vec<u8>,
    buffer: Vec<u8>,
    status: u8,
    interrupt_tx: Sender<u32>,
    interrupt_num: u32,
}

impl MemDiskController {
    pub fn new(interrupt_tx: Sender<u32>, interrupt_num: u32, num_blocks: u32) -> Self {
        MemDiskController {
            num_blocks,
            address: 0,
            disk: vec![0; DISK_BUFFER_SIZE * num_blocks as usize],
            buffer: vec![0; DISK_BUFFER_SIZE],
            status: STATUS_SUCCESS,
            interrupt_tx,
            interrupt_num,
        }
    }
}

impl DiskController for MemDiskController {
    fn start(&mut self) {
        // no-op
    }

    fn stop(&mut self) {
        // no-op
    }

    fn store_control(&mut self, address: u32, value: u8) {
        macro_rules! return_with_status {
            ($x:expr) => {{
                self.status = $x;
                self.interrupt_tx.send(self.interrupt_num).unwrap();
                return;
            }}
        }

        match address {
            ADDRESS_DA_1 => {
                let address_masked = self.address & 0x00FFFFFF;
                let value_shifted = (value as u32) << 24;
                self.address = address_masked | value_shifted;
            }
            ADDRESS_DA_2 => {
                let address_masked = self.address & 0xFF00FFFF;
                let value_shifted = (value as u32) << 16;
                self.address = address_masked | value_shifted;
            }
            ADDRESS_DA_3 => {
                let address_masked = self.address & 0xFFFF00FF;
                let value_shifted = (value as u32) << 8;
                self.address = address_masked | value_shifted;
            }
            ADDRESS_DA_4 => {
                let address_masked = self.address & 0xFFFFFF00;
                self.address = address_masked | (value as u32);
            }
            ADDRESS_CMD => {
                match value {
                    COMMAND_READ | COMMAND_SUSTAINED_READ => {
                        if self.address >= self.num_blocks {
                            return_with_status!(STATUS_BAD_COMMAND);
                        }
                        let base: usize = self.address as usize * DISK_BUFFER_SIZE;
                        for i in 0..DISK_BUFFER_SIZE {
                            self.buffer[i] = self.disk[base + i];
                        }
                        if let COMMAND_SUSTAINED_READ = value {
                            self.address += 1;
                        }
                        return_with_status!(STATUS_SUCCESS);
                    }
                    COMMAND_WRITE | COMMAND_SUSTAINED_WRITE => {
                        if self.address >= self.num_blocks {
                            return_with_status!(STATUS_BAD_COMMAND);
                        }
                        let base: usize = self.address as usize * DISK_BUFFER_SIZE;
                        for i in 0..DISK_BUFFER_SIZE {
                            self.disk[base + i] = self.buffer[i];
                        }
                        if let COMMAND_SUSTAINED_WRITE = value {
                            self.address += 1;
                        }
                        return_with_status!(STATUS_SUCCESS);
                    }
                    _ => {
                        return_with_status!(STATUS_BAD_COMMAND);
                    }
                };
            }
            _ => unreachable!()
        }
    }

    fn load_status(&self, address: u32) -> u8 {
        match address {
            ADDRESS_STATUS => self.status,
            ADDRESS_NBA_1 => ((self.num_blocks & 0xFF000000) >> 24) as u8,
            ADDRESS_NBA_2 => ((self.num_blocks & 0x00FF0000) >> 26) as u8,
            ADDRESS_NBA_3 => ((self.num_blocks & 0x0000FF00) >> 8) as u8,
            ADDRESS_NBA_4 => (self.num_blocks & 0x000000FF) as u8,
            ADDRESS_DA_1 => ((self.address & 0xFF000000) >> 24) as u8,
            ADDRESS_DA_2 => ((self.address & 0x00FF0000) >> 26) as u8,
            ADDRESS_DA_3 => ((self.address & 0x0000FF00) >> 8) as u8,
            ADDRESS_DA_4 => (self.address & 0x000000FF) as u8,
            _ => unreachable!()
        }
    }

    fn store_data(&mut self, address: u32, value: u8) {
        self.buffer[address as usize] = value;
    }

    fn load_data(&self, address: u32) -> u8 {
        self.buffer[address as usize]
    }
}
