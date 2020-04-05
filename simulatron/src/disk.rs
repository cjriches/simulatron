use std::sync::mpsc::Sender;

pub struct DiskController {
    interrupt_tx: Sender<u32>,
}

impl DiskController {
    pub fn new(interrupt_tx: Sender<u32>) -> Self {
        DiskController {
            interrupt_tx,
        }
    }

    pub fn start(&mut self) {
        unimplemented!();
    }

    pub fn stop(&mut self) {
        unimplemented!();
    }

    pub fn store(&mut self, address: u32, value: u8) {
        unimplemented!();
    }

    pub fn load(&self, address: u32) -> u8 {
        unimplemented!();
    }
}
