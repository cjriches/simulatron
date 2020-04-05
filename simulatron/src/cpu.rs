use std::sync::mpsc::Receiver;

pub const INTERRUPT_SYSCALL: u32 = 0;
pub const INTERRUPT_KEYBOARD: u32 = 1;
pub const INTERRUPT_DISK: u32 = 2;
pub const INTERRUPT_PAGE_FAULT: u32 = 3;
pub const INTERRUPT_DIV_BY_0: u32 = 4;
pub const INTERRUPT_ILLEGAL_OPERATION: u32 = 5;
pub const INTERRUPT_TIMER: u32 = 6;

pub struct CPU {
    interrupt_rx: Receiver<u32>,
}

impl CPU {
    pub fn new(interrupt_rx: Receiver<u32>) -> Self {
        CPU {
            interrupt_rx,
        }
    }

    pub fn start(&mut self) {
        unimplemented!();
    }

    pub fn stop(&mut self) {
        unimplemented!();
    }
}
