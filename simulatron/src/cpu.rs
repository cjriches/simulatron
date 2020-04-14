use std::sync::mpsc;
use std::thread;

use crate::mmu;

pub const INTERRUPT_SYSCALL: u32 = 0;
pub const INTERRUPT_KEYBOARD: u32 = 1;
pub const INTERRUPT_DISK_A: u32 = 2;
pub const INTERRUPT_DISK_B: u32 = 3;
pub const INTERRUPT_PAGE_FAULT: u32 = 4;
pub const INTERRUPT_DIV_BY_0: u32 = 5;
pub const INTERRUPT_ILLEGAL_OPERATION: u32 = 6;
pub const INTERRUPT_TIMER: u32 = 7;
pub const JOIN_THREAD: u32 = 4294967295;  // Not a real interrupt, just a thread join command.

struct PublicRegisters {
    r: [u32; 8],
    f: [f32; 8],
    flags: u16,
    uspr: u32,  // User Stack Pointer Register
    kspr: u32,  // Kernel Stack Pointer Register
    // Page Directory Pointer Register is located in MMU.
    imr: u16,   // Interrupt Mask Register
}

impl PublicRegisters {
    pub fn new() -> Self {
        PublicRegisters {
            r: [0; 8],
            f: [0.0; 8],
            flags: 0,
            uspr: 0,
            kspr: 0,
            imr: 0,
        }
    }
}

pub struct CPU {
    mmu: mmu::MMU,
    interrupt_tx: mpsc::Sender<u32>,
    interrupt_rx: mpsc::Receiver<u32>,
    registers: PublicRegisters,
    program_counter: u32,
    kernel_mode: bool,
}

impl CPU {
    pub fn new(mmu: mmu::MMU, interrupt_tx: mpsc::Sender<u32>,
               interrupt_rx: mpsc::Receiver<u32>) -> Self {
        CPU {
            mmu,
            interrupt_tx,
            interrupt_rx,
            registers: PublicRegisters::new(),
            program_counter: 64,  // Start of ROM.
            kernel_mode: true,
        }
    }

    pub fn start(mut self) -> thread::JoinHandle<Self> {
        // The thread takes ownership of the CPU object, then returns it on being joined.
        thread::spawn(move || {
            self.fetch_execute_cycle();
            self
        })
    }

    fn fetch_execute_cycle(&mut self) {
        // Define a macro for fetching from memory and continuing the loop if it fails.
        macro_rules! load {
            ($f:ident, $address:expr, $fetch:expr) => {{
                let val = self.$f($address, $fetch);
                if let None = val {continue;}
                val.unwrap()
            }}
        }

        loop {
            // Check for interrupts.
            match self.interrupt_rx.try_recv() {
                Ok(interrupt) => unimplemented!(),
                Err(mpsc::TryRecvError::Disconnected) => panic!(),
                Err(mpsc::TryRecvError::Empty) => {},
            }

            // Fetch instruction.
            let opcode = load!(load_8, self.program_counter, true);
            self.program_counter += 1;
            println!("Opcode is: {:#x}", opcode);

            // Retrieve operands.
            let op1;
            let op2;
            let op3;
            if opcode < 32 {
                // No operands.
                op1 = 0;
                op2 = 0;
                op3 = 0;
            } else if opcode < 128 {
                // 1 operand.
                op1 = load!(load_32, self.program_counter, true);
                self.program_counter += 4;
                op2 = 0;
                op3 = 0;
            } else if opcode < 224 {
                // 2 operands.
                op1 = load!(load_32, self.program_counter, true);
                self.program_counter += 4;
                op2 = load!(load_32, self.program_counter, true);
                self.program_counter += 4;
                op3 = 0;
            } else {
                // 3 operands.
                op1 = load!(load_32, self.program_counter, true);
                self.program_counter += 4;
                op2 = load!(load_32, self.program_counter, true);
                self.program_counter += 4;
                op3 = load!(load_32, self.program_counter, true);
                self.program_counter += 4;
            }
            println!("Operands are: {}, {}, {}", op1, op2, op3);

            // Execute instruction.
            match opcode {
                0x00 => {  // HALT
                    if self.kernel_mode {
                        break;
                    } else {
                        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    }
                }
                0x01 => {  // PAUSE
                    unimplemented!()
                }
                0x82 => {  // STORE literal value into literal address
                    self.store_32(op2, op1);
                }
                _ => {  // Unrecognised
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                }
            }
        }
    }

    fn store_32(&mut self, address: u32, value: u32) {
        if self.kernel_mode {
            self.mmu.store_physical_32(address, value);
        } else {
            self.mmu.store_virtual_32(address, value);
        }
    }

    fn store_16(&mut self, address: u32, value: u16) {
        if self.kernel_mode {
            self.mmu.store_physical_16(address, value);
        } else {
            self.mmu.store_virtual_16(address, value);
        }
    }

    fn store_8(&mut self, address: u32, value: u8) {
        if self.kernel_mode {
            self.mmu.store_physical_8(address, value);
        } else {
            self.mmu.store_virtual_8(address, value);
        }
    }

    fn load_32(&mut self, address: u32, is_fetch: bool) -> Option<u32> {
        if self.kernel_mode {
            self.mmu.load_physical_32(address)
        } else {
            self.mmu.load_virtual_32(address, is_fetch)
        }
    }

    fn load_16(&mut self, address: u32, is_fetch: bool) -> Option<u16> {
        if self.kernel_mode {
            self.mmu.load_physical_16(address)
        } else {
            self.mmu.load_virtual_16(address, is_fetch)
        }
    }

    fn load_8(&mut self, address: u32, is_fetch: bool) -> Option<u8> {
        if self.kernel_mode {
            self.mmu.load_physical_8(address)
        } else {
            self.mmu.load_virtual_8(address, is_fetch)
        }
    }
}
