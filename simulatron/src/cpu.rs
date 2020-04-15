use std::sync::mpsc;
use std::thread;

use crate::mmu::MMU;
use crate::ui::UICommand;

pub const INTERRUPT_SYSCALL: u32 = 0;
pub const INTERRUPT_KEYBOARD: u32 = 1;
pub const INTERRUPT_DISK_A: u32 = 2;
pub const INTERRUPT_DISK_B: u32 = 3;
pub const INTERRUPT_PAGE_FAULT: u32 = 4;
pub const INTERRUPT_DIV_BY_0: u32 = 5;
pub const INTERRUPT_ILLEGAL_OPERATION: u32 = 6;
pub const INTERRUPT_TIMER: u32 = 7;
pub const JOIN_THREAD: u32 = 4294967295;  // Not a real interrupt, just a thread join command.

struct Registers {
    r: [u32; 8],
    f: [f32; 8],
    flags: u16,
    uspr: u32,  // User Stack Pointer Register
    kspr: u32,  // Kernel Stack Pointer Register
    pdpr: u32,  // Page Directory Pointer Register
    imr: u16,   // Interrupt Mask Register
}

enum RegisterType {
    Byte,   // u8
    Half,   // u16
    Word,   // u32
    Float,  // f32
}

impl RegisterType {
    pub fn from_reg_ref(reg_ref: u32) -> Option<Self> {
        if reg_ref < 8 {
            Some(RegisterType::Word)
        } else if reg_ref < 16 {
            Some(RegisterType::Half)
        } else if reg_ref < 24 {
            Some(RegisterType::Byte)
        } else if reg_ref < 32 {
            Some(RegisterType::Float)
        } else if reg_ref == 32 {
            Some(RegisterType::Half)
        } else if reg_ref < 36 {
            Some(RegisterType::Word)
        } else if reg_ref == 36 {
            Some(RegisterType::Half)
        } else {
            None
        }
    }
}

impl Registers {
    pub fn new() -> Self {
        Registers {
            r: [0; 8],
            f: [0.0; 8],
            flags: 0,
            uspr: 0,
            kspr: 0,
            pdpr: 0,
            imr: 0,
        }
    }

    pub fn store_8_by_ref(&mut self, reg_ref: u32, value: u8) {
        if reg_ref < 16 || reg_ref > 23 {
            panic!("Invalid 8-bit register reference.");
        }
        let index = (reg_ref - 16) as usize;
        let masked = self.r[index] & 0xFFFFFF00;
        self.r[index] = masked | (value as u32);
    }

    pub fn store_16_by_ref(&mut self, reg_ref: u32, value: u16) {
        if (8..16).contains(&reg_ref) {
            let index = (reg_ref - 8) as usize;
            let masked = self.r[index] & 0xFFFF0000;
            self.r[index] = masked | (value as u32);
        } else if reg_ref == 32 {
            let masked_value = value & 0b0111111111111111;  // Ignore bit 15.
            self.flags = masked_value;
        } else if reg_ref == 36 {
            self.imr = value;
        } else {
            panic!("Invalid 16-bit register reference.");
        }
    }

    pub fn store_32_by_ref(&mut self, reg_ref: u32, value: u32) {
        if reg_ref < 8 {
            self.r[reg_ref as usize] = value;
        } else if reg_ref == 33 {
            self.uspr = value;
        } else if reg_ref == 34 {
            self.kspr = value;
        } else if reg_ref == 35 {
            self.pdpr = value;
        } else {
            panic!("Invalid 32-bit register reference.")
        }
    }

    pub fn store_float_by_ref(&mut self, reg_ref: u32, value: f32) {
        if reg_ref < 24 || reg_ref > 31 {
            panic!("Invalid float register reference.");
        }
        let index = (reg_ref - 24) as usize;
        self.f[index] = value;
    }

    pub fn get_8_by_ref(&self, reg_ref: u32) -> u8 {
        if reg_ref < 16 || reg_ref > 23 {
            panic!("Invalid 8-bit register reference.");
        }
        let index = (reg_ref - 16) as usize;
        self.r[index] as u8
    }

    pub fn get_16_by_ref(&self, reg_ref: u32) -> u16 {
        match reg_ref {
            8 => self.r[0] as u16,
            9 => self.r[1] as u16,
            10 => self.r[2] as u16,
            11 => self.r[3] as u16,
            12 => self.r[4] as u16,
            13 => self.r[5] as u16,
            14 => self.r[6] as u16,
            15 => self.r[7] as u16,
            32 => self.flags,
            36 => self.imr,
            _ => panic!("Invalid 16-bit register reference."),
        }
    }

    pub fn get_32_by_ref(&self, reg_ref: u32) -> u32 {
        if reg_ref < 8 {
            self.r[reg_ref as usize]
        } else if reg_ref == 33 {
            self.uspr
        } else if reg_ref == 34 {
            self.kspr
        } else if reg_ref == 35 {
            self.pdpr
        } else {
            panic!("Invalid 32-bit register reference.")
        }
    }

    pub fn get_float_by_ref(&self, reg_ref: u32) -> f32 {
        if reg_ref < 24 || reg_ref > 31 {
            panic!("Invalid float register reference.");
        }
        let index = (reg_ref - 24) as usize;
        self.f[index]
    }
}

struct InterruptLatch {
    latched: [bool; 8],
    interrupt_rx: mpsc::Receiver<u32>,
}

impl InterruptLatch {
    pub fn new(interrupt_rx: mpsc::Receiver<u32>) -> Self {
        InterruptLatch {
            latched: [false; 8],
            interrupt_rx,
        }
    }

    pub fn try_get_next(&mut self, imr: u16) -> Option<u32> {
        // First, try and service latched interrupts, prioritising higher numbers first.
        for i in (0..8).rev() {
            if self.latched[i] && (imr & (1 << i as u16)) > 0 {
                self.latched[i] = false;
                return Some(i as u32);
            }
        }

        // If there aren't any enabled latched interrupts, check the channel.
        loop {
            match self.interrupt_rx.try_recv() {
                Ok(interrupt) => {
                    // If enabled, directly return. If disabled, latch it and check again.
                    // Also directly return JOIN_THREAD.
                    if interrupt == JOIN_THREAD || (imr & (1 << interrupt as u16)) > 0 {
                        return Some(interrupt);
                    } else {
                        self.latched[interrupt as usize] = true;
                    }
                },
                Err(mpsc::TryRecvError::Disconnected) => panic!(),
                Err(mpsc::TryRecvError::Empty) => return None,  // If the channel's empty, return.
            }
        }
    }

    pub fn wait_for_next(&mut self, imr: u16) -> u32 {
        // First, try and service latched interrupts, prioritising higher numbers first.
        for i in (0..8).rev() {
            if self.latched[i] && (imr & (1 << i as u16)) > 0 {
                self.latched[i] = false;
                return i as u32;
            }
        }

        // If there aren't any enabled latched interrupts, block on a channel receive.
        loop {
            let interrupt = self.interrupt_rx.recv().unwrap();
            // If enabled, directly return. If disabled, latch it and check again.
            // Also directly return JOIN_THREAD.
            if interrupt == JOIN_THREAD || (imr & (1 << interrupt as u16)) > 0 {
                return interrupt;
            } else {
                self.latched[interrupt as usize] = true;
            }
        }
    }
}

pub struct CPU {
    ui_tx: mpsc::Sender<UICommand>,
    mmu: MMU,
    interrupt_tx: mpsc::Sender<u32>,
    interrupts: InterruptLatch,
    registers: Registers,
    program_counter: u32,
    kernel_mode: bool,
}

impl CPU {
    pub fn new(ui_tx: mpsc::Sender<UICommand>, mmu: MMU,
               interrupt_tx: mpsc::Sender<u32>, interrupt_rx: mpsc::Receiver<u32>) -> Self {
        CPU {
            ui_tx,
            mmu,
            interrupt_tx,
            interrupts: InterruptLatch::new(interrupt_rx),
            registers: Registers::new(),
            program_counter: 64,  // Start of ROM.
            kernel_mode: true,
        }
    }

    pub fn start(mut self) -> thread::JoinHandle<Self> {
        // The thread takes ownership of the CPU object, then returns it on being joined.
        thread::spawn(move || {
            self.ui_tx.send(UICommand::SetEnabled(true)).unwrap();
            self.fetch_execute_cycle();
            self.ui_tx.send(UICommand::SetEnabled(false)).unwrap();
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

        // Similar macro for stack.
        macro_rules! pop {
            ($f:ident) => {{
                let val = self.$f();
                if let None = val {continue;}
                val.unwrap()
            }}
        }

        // Define a guard macro for privileged operations.
        macro_rules! privileged {
            () => {{
                if !self.kernel_mode {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    continue;
                }
            }}
        }

        let mut pausing = false;
        loop {
            // Check for interrupts.
            let possible_interrupt = if pausing {
                pausing = false;
                Some(self.interrupts.wait_for_next(self.registers.imr))
            } else {
                self.interrupts.try_get_next(self.registers.imr)
            };
            if let Some(interrupt) = possible_interrupt {
                // If it's the join thread command, exit.
                if interrupt == JOIN_THREAD {
                    break;
                }
                // Remember mode and switch to kernel mode.
                let old_mode = if self.kernel_mode {
                    0b1000000000000000
                } else {
                    0
                };
                self.kernel_mode = true;
                // Push flags to stack, with bit 15 set to the old mode.
                let flags = self.registers.flags | old_mode;
                self.push_16(flags);
                // Push the program counter to stack.
                self.push_32(self.program_counter);
                // Push the IMR to stack.
                self.push_16(self.registers.imr);
                // Disable all interrupts.
                self.registers.imr = 0;
                // Jump to the interrupt handler.
                self.program_counter = self.load_32(interrupt * 4, false).unwrap();
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
                    privileged!();
                    break;
                }
                0x01 => {  // PAUSE
                    privileged!();
                    pausing = true;
                }
                0x05 => {  // IRETURN
                    privileged!();
                    // Restore the IMR from the stack.
                    self.registers.imr = pop!(pop_16);
                    // Pop the program counter off the stack.
                    self.program_counter = pop!(pop_32);
                    // Pop the flags off the stack.
                    let flags = pop!(pop_16);
                    // If bit 15 is 0, enter user mode.
                    if (flags & 0b1000000000000000) == 0 {
                        self.kernel_mode = false;
                    }
                    // Set the flags.
                    self.registers.flags = flags & 0b0111111111111111;
                }
                0x80 => {  // LOAD literal address into register ref
                    match RegisterType::from_reg_ref(op2) {
                        Some(RegisterType::Byte) => {
                            let val = load!(load_8, op1, false);
                            self.registers.store_8_by_ref(op2, val);
                        }
                        Some(RegisterType::Half) => {
                            let val = load!(load_16, op1, false);
                            self.registers.store_16_by_ref(op2, val);
                        }
                        Some(RegisterType::Word) => {
                            let val = load!(load_32, op1, false);
                            self.registers.store_32_by_ref(op2, val);
                        }
                        Some(RegisterType::Float) => {
                            let val = u32_to_f32(load!(load_32, op1, false));
                            self.registers.store_float_by_ref(op2, val);
                        }
                        None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap(),
                    };
                }
                0x82 => {  // STORE register ref into literal address
                    match RegisterType::from_reg_ref(op1) {
                        Some(RegisterType::Byte) =>
                            self.store_8(op2, self.registers.get_8_by_ref(op1)),
                        Some(RegisterType::Half) =>
                            self.store_16(op2, self.registers.get_16_by_ref(op1)),
                        Some(RegisterType::Word) =>
                            self.store_32(op2, self.registers.get_32_by_ref(op1)),
                        Some(RegisterType::Float) =>
                            self.store_32(op2, f32_to_u32(self.registers.get_float_by_ref(op1))),
                        None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap(),
                    };
                }
                0x86 => {  // COPY literal into register ref
                    match RegisterType::from_reg_ref(op2) {
                        Some(RegisterType::Byte) =>
                            self.registers.store_8_by_ref(op2, op1 as u8),
                        Some(RegisterType::Half) =>
                            self.registers.store_16_by_ref(op2, op1 as u16),
                        Some(RegisterType::Word) =>
                            self.registers.store_32_by_ref(op2, op1),
                        Some(RegisterType::Float) =>
                            self.registers.store_float_by_ref(op2, u32_to_f32(op1)),
                        None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap(),
                    };
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
            self.mmu.store_virtual_32(self.registers.pdpr, address, value);
        }
    }

    fn store_16(&mut self, address: u32, value: u16) {
        if self.kernel_mode {
            self.mmu.store_physical_16(address, value);
        } else {
            self.mmu.store_virtual_16(self.registers.pdpr, address, value);
        }
    }

    fn store_8(&mut self, address: u32, value: u8) {
        if self.kernel_mode {
            self.mmu.store_physical_8(address, value);
        } else {
            self.mmu.store_virtual_8(self.registers.pdpr, address, value);
        }
    }

    fn load_32(&mut self, address: u32, is_fetch: bool) -> Option<u32> {
        if self.kernel_mode {
            self.mmu.load_physical_32(address)
        } else {
            self.mmu.load_virtual_32(self.registers.pdpr, address, is_fetch)
        }
    }

    fn load_16(&mut self, address: u32, is_fetch: bool) -> Option<u16> {
        if self.kernel_mode {
            self.mmu.load_physical_16(address)
        } else {
            self.mmu.load_virtual_16(self.registers.pdpr, address, is_fetch)
        }
    }

    fn load_8(&mut self, address: u32, is_fetch: bool) -> Option<u8> {
        if self.kernel_mode {
            self.mmu.load_physical_8(address)
        } else {
            self.mmu.load_virtual_8(self.registers.pdpr, address, is_fetch)
        }
    }

    fn push_32(&mut self, value: u32) {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        *spr -= 4;
        let spr = *spr;  // Copy value and drop mutable reference so we are allowed to call store_32.
        self.store_32(spr, value);
    }

    fn push_16(&mut self, value: u16) {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        *spr -= 2;
        let spr = *spr;
        self.store_16(spr, value);
    }

    fn push_8(&mut self, value: u8) {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        *spr -= 1;
        let spr = *spr;
        self.store_8(spr, value);
    }

    fn pop_32(&mut self) -> Option<u32> {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        // Perform the dance of the borrow checker.
        let old_spr = *spr;
        *spr += 4;
        let result = self.load_32(old_spr, false);
        result
    }

    fn pop_16(&mut self) -> Option<u16> {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        let old_spr = *spr;
        *spr += 2;
        let result = self.load_16(old_spr, false);
        result
    }

    fn pop_8(&mut self) -> Option<u8> {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        let old_spr = *spr;
        *spr += 1;
        let result = self.load_8(old_spr, false);
        result
    }
}

// WARNING!
// These functions are theoretically very dangerous. They do not perform any conversion, they
// just reinterpret the bit pattern as the new type. This is exactly what we
// want to let us store float values in RAM, but if misused could result in
// undefined behaviour.

fn u32_to_f32(u: u32) -> f32 {
    unsafe {
        std::mem::transmute::<u32, f32>(u)
    }
}

fn f32_to_u32(f: f32) -> u32 {
    unsafe {
        std::mem::transmute::<f32, u32>(f)
    }
}
