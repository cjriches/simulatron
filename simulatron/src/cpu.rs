use std::sync::mpsc::Receiver;

use crate::mmu;

pub const INTERRUPT_SYSCALL: u32 = 0;
pub const INTERRUPT_KEYBOARD: u32 = 1;
pub const INTERRUPT_DISK_A: u32 = 2;
pub const INTERRUPT_DISK_B: u32 = 3;
pub const INTERRUPT_PAGE_FAULT: u32 = 4;
pub const INTERRUPT_DIV_BY_0: u32 = 5;
pub const INTERRUPT_ILLEGAL_OPERATION: u32 = 6;
pub const INTERRUPT_TIMER: u32 = 7;

pub struct CPU {
    mmu: mmu::MMU,
    interrupt_rx: Receiver<u32>,
}

impl CPU {
    pub fn new(mmu: mmu::MMU, interrupt_rx: Receiver<u32>) -> Self {
        CPU {
            mmu,
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
