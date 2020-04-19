use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

#[derive(PartialEq, Eq)]
enum RegisterType {
    Byte,   // u8
    Half,   // u16
    PrivilegedHalf,
    Word,   // u32
    PrivilegedWord,
    Float,  // f32
}

impl RegisterType {
    pub fn from_reg_ref(reg_ref: u8) -> Option<Self> {
        if reg_ref < 0x08 {
            Some(RegisterType::Word)
        } else if reg_ref < 0x10 {
            Some(RegisterType::Half)
        } else if reg_ref < 0x18 {
            Some(RegisterType::Byte)
        } else if reg_ref < 0x20 {
            Some(RegisterType::Float)
        } else if reg_ref == 0x20 {
            Some(RegisterType::Half)
        } else if reg_ref == 0x21 {
            Some(RegisterType::Word)
        } else if reg_ref < 0x24 {
            Some(RegisterType::PrivilegedWord)
        } else if reg_ref == 0x24 {
            Some(RegisterType::PrivilegedHalf)
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

    pub fn store_8_by_ref(&mut self, reg_ref: u8, value: u8) {
        if reg_ref < 16 || reg_ref > 23 {
            panic!("Invalid 8-bit register reference.");
        }
        let index = (reg_ref - 16) as usize;
        let masked = self.r[index] & 0xFFFFFF00;
        self.r[index] = masked | (value as u32);
    }

    pub fn store_16_by_ref(&mut self, reg_ref: u8, value: u16) {
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

    pub fn store_32_by_ref(&mut self, reg_ref: u8, value: u32) {
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

    pub fn store_float_by_ref(&mut self, reg_ref: u8, value: f32) {
        if reg_ref < 24 || reg_ref > 31 {
            panic!("Invalid float register reference.");
        }
        let index = (reg_ref - 24) as usize;
        self.f[index] = value;
    }

    pub fn load_8_by_ref(&self, reg_ref: u8) -> u8 {
        if reg_ref < 16 || reg_ref > 23 {
            panic!("Invalid 8-bit register reference.");
        }
        let index = (reg_ref - 16) as usize;
        self.r[index] as u8
    }

    pub fn load_16_by_ref(&self, reg_ref: u8) -> u16 {
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

    pub fn load_32_by_ref(&self, reg_ref: u8) -> u32 {
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

    pub fn load_float_by_ref(&self, reg_ref: u8) -> f32 {
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

enum TimerCommand {
    SetTimer(u32),
    JoinThread,
}

pub struct CPU {
    ui_tx: mpsc::Sender<UICommand>,
    timer_tx: Option<mpsc::Sender<TimerCommand>>,
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
            timer_tx: None,
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
            let (timer_tx, timer_rx) = mpsc::channel();
            let timer_interrupt_tx = self.interrupt_tx.clone();
            let timer_thread = thread::spawn(move || {
                let mut interval = 0;
                loop {
                    if interval == 0 {
                        // Wait indefinitely for a command.
                        match timer_rx.recv().unwrap() {
                            TimerCommand::SetTimer(new_interval) => interval = new_interval,
                            TimerCommand::JoinThread => return,
                        };
                    } else {
                        // Wait for a command for up to `interval`, then send a timer interrupt.
                        match timer_rx.recv_timeout(Duration::from_millis(interval as u64)) {
                            Ok(TimerCommand::SetTimer(new_interval)) => interval = new_interval,
                            Ok(TimerCommand::JoinThread) => return,
                            Err(mpsc::RecvTimeoutError::Timeout) =>
                                timer_interrupt_tx.send(INTERRUPT_TIMER).unwrap(),
                            Err(mpsc::RecvTimeoutError::Disconnected) => panic!(),
                        };
                    }
                }
            });
            self.timer_tx = Some(timer_tx);

            self.ui_tx.send(UICommand::SetEnabled(true)).unwrap();
            self.fetch_execute_cycle();
            self.ui_tx.send(UICommand::SetEnabled(false)).unwrap();

            let timer_tx = self.timer_tx.take().unwrap();
            timer_tx.send(TimerCommand::JoinThread).unwrap();
            timer_thread.join().unwrap();

            self
        })
    }

    fn fetch_execute_cycle(&mut self) {
        // Define macros for fetching opcodes and operands from memory and
        // continuing the loop on failure.
        macro_rules! fetch_float {
            () => {{
                let val = self.load_float(self.program_counter, true);
                if let None = val {continue;}
                self.program_counter += 4;
                val.unwrap()
            }}
        }

        macro_rules! fetch_32 {
            () => {{
                let val = self.load_32(self.program_counter, true);
                if let None = val {continue;}
                self.program_counter += 4;
                val.unwrap()
            }}
        }

        macro_rules! fetch_16 {
            () => {{
                let val = self.load_16(self.program_counter, true);
                if let None = val {continue;}
                self.program_counter += 2;
                val.unwrap()
            }}
        }

        macro_rules! fetch_8 {
            () => {{
                let val = self.load_8(self.program_counter, true);
                if let None = val {continue;}
                self.program_counter += 1;
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

        // A macro for printing on debug build.
        macro_rules! debug {
            ($($x:expr),*) => {{
                #[cfg(debug_assertions)]
                println!($($x),*);
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
            let opcode = fetch_8!();

            // Decode and execute instruction.
            debug!();
            match opcode {
                0x00 => {  // HALT
                    debug!("HALT");
                    privileged!();
                    break;
                }
                0x01 => {  // PAUSE
                    debug!("PAUSE");
                    privileged!();
                    pausing = true;
                }
                0x02 => {  // TIMER with literal word
                    debug!("TIMER literal word");
                    privileged!();
                    let milliseconds = fetch_32!();
                    debug!("Timer milliseconds: {:#x}", milliseconds);
                    self.timer_tx.as_ref().unwrap()
                        .send(TimerCommand::SetTimer(milliseconds)).unwrap();
                }
                0x03 => {  // TIMER with register ref word
                    debug!("TIMER register ref word");
                    privileged!();
                    let reg_ref = fetch_8!();
                    if let Some(RegisterType::Word) | Some(RegisterType::PrivilegedWord)
                            = RegisterType::from_reg_ref(reg_ref) {
                        let milliseconds = self.registers.load_32_by_ref(reg_ref);
                        debug!("Timer milliseconds: {:#x}", milliseconds);
                        self.timer_tx.as_ref().unwrap()
                            .send(TimerCommand::SetTimer(milliseconds)).unwrap();
                    } else {
                        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    }
                }
                0x04 => {  // USERMODE
                    debug!("USERMODE");
                    privileged!();
                    // Get the target address.
                    self.program_counter = pop!(pop_32);
                    // Clear flags.
                    self.registers.flags = 0;
                    // Enter user mode.
                    self.kernel_mode = false;
                }
                0x05 => {  // IRETURN
                    debug!("IRETURN");
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
                0x06 => {  // LOAD literal address into register ref
                    debug!("LOAD literal address");
                    let reg_ref_dest = fetch_8!();
                    let literal_address = fetch_32!();
                    debug!("Dest: {:#x} Address: {:#x}", reg_ref_dest, literal_address);
                    self.instruction_load(reg_ref_dest, literal_address);
                }
                0x07 => {  // LOAD register ref address into register ref
                    debug!("LOAD register ref address");
                    let reg_ref_dest = fetch_8!();
                    let reg_ref_address = fetch_8!();
                    let reg_ref_address_type = RegisterType::from_reg_ref(reg_ref_address);
                    if let Some(RegisterType::PrivilegedWord) = reg_ref_address_type {
                        privileged!();
                    }
                    if let Some(RegisterType::Word) | Some(RegisterType::PrivilegedWord)
                            = reg_ref_address_type {
                        let address = self.registers.load_32_by_ref(reg_ref_address);
                        debug!("Dest: {:#x} Address: {:#x}", reg_ref_dest, address);
                        self.instruction_load(reg_ref_dest, address);
                    } else {
                        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    }
                }
                0x08 => {  // STORE register ref into literal address
                    debug!("STORE literal address");
                    let literal_address = fetch_32!();
                    let reg_ref_source = fetch_8!();
                    debug!("Address: {:#x} Source: {:#x}", literal_address, reg_ref_source);
                    self.instruction_store(literal_address, reg_ref_source);
                }
                0x09 => {  // STORE register ref into register ref address
                    debug!("STORE register ref address");
                    let reg_ref_address = fetch_8!();
                    let reg_ref_source = fetch_8!();
                    let reg_ref_address_type = RegisterType::from_reg_ref(reg_ref_address);
                    if let Some(RegisterType::PrivilegedWord) = reg_ref_address_type {
                        privileged!();
                    }
                    if let Some(RegisterType::Word) | Some(RegisterType::PrivilegedWord)
                            = reg_ref_address_type {
                        let address = self.registers.load_32_by_ref(reg_ref_address);
                        debug!("Address: {:#x} Source: {:#x}", address, reg_ref_source);
                        self.instruction_store(address, reg_ref_source);
                    } else {
                        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    }
                }
                0x0A => {  // COPY variable literal into register ref
                    debug!("COPY variable literal");
                    let reg_ref_dest = fetch_8!();
                    debug!("into {:#x}", reg_ref_dest);
                    match RegisterType::from_reg_ref(reg_ref_dest) {
                        Some(RegisterType::Byte) => {
                            let val = fetch_8!();
                            debug!("Byte: {:#x}", val);
                            self.registers.store_8_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::Half) => {
                            let val = fetch_16!();
                            debug!("Half: {:#x}", val);
                            self.registers.store_16_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::PrivilegedHalf) => {
                            privileged!();
                            let val = fetch_16!();
                            debug!("Half: {:#x}", val);
                            self.registers.store_16_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::Word) => {
                            let val = fetch_32!();
                            debug!("Word: {:#x}", val);
                            self.registers.store_32_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::PrivilegedWord) => {
                            privileged!();
                            let val = fetch_32!();
                            debug!("Word: {:#x}", val);
                            self.registers.store_32_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::Float) => {
                            let val = fetch_float!();
                            debug!("Float: {}", val);
                            self.registers.store_float_by_ref(reg_ref_dest, val);
                        }
                        None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()
                    }
                }
                0x0B => {  // COPY register to register
                    debug!("COPY register");
                    let reg_ref_dest = fetch_8!();
                    let reg_ref_source = fetch_8!();
                    debug!("from {:#x} to {:#x}", reg_ref_source, reg_ref_dest);
                    let source_type = RegisterType::from_reg_ref(reg_ref_source);
                    let dest_type = RegisterType::from_reg_ref(reg_ref_dest);
                    if source_type != dest_type {
                        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                        continue;
                    }
                    match dest_type {
                        Some(RegisterType::Byte) => {
                            let val = self.registers.load_8_by_ref(reg_ref_source);
                            self.registers.store_8_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::Half) => {
                            let val = self.registers.load_16_by_ref(reg_ref_source);
                            self.registers.store_16_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::PrivilegedHalf) => {
                            privileged!();
                            let val = self.registers.load_16_by_ref(reg_ref_source);
                            self.registers.store_16_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::Word) => {
                            let val = self.registers.load_32_by_ref(reg_ref_source);
                            self.registers.store_32_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::PrivilegedWord) => {
                            privileged!();
                            let val = self.registers.load_32_by_ref(reg_ref_source);
                            self.registers.store_32_by_ref(reg_ref_dest, val);
                        }
                        Some(RegisterType::Float) => {
                            let val = self.registers.load_float_by_ref(reg_ref_source);
                            self.registers.store_float_by_ref(reg_ref_dest, val);
                        }
                        None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()
                    }
                }
                0x0C => {  // SWAP with literal address
                    debug!("SWAP literal address");
                    let reg_ref = fetch_8!();
                    let address = fetch_32!();
                    debug!("register {:#x} with address {:#x}", reg_ref, address);
                    self.instruction_swap(reg_ref, address);
                }
                0x0D => {  // SWAP with register ref address
                    debug!("SWAP reg ref address");
                    let reg_ref = fetch_8!();
                    let address_ref = fetch_8!();
                    let address_ref_type = RegisterType::from_reg_ref(address_ref);
                    if let Some(RegisterType::PrivilegedWord) = address_ref_type {
                        privileged!();
                    }
                    if let Some(RegisterType::Word) | Some(RegisterType::PrivilegedWord)
                            = address_ref_type {
                        let address = self.registers.load_32_by_ref(address_ref);
                        debug!("register {:#x} with address {:#x}", reg_ref, address);
                        self.instruction_swap(reg_ref, address);
                    } else {
                        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    }
                }
                0x0E => {  // PUSH
                    debug!("PUSH");
                    let reg_ref = fetch_8!();
                    debug!("Register: {:#x}", reg_ref);
                    match RegisterType::from_reg_ref(reg_ref) {
                        Some(RegisterType::Byte) => {
                            let val = self.registers.load_8_by_ref(reg_ref);
                            self.push_8(val);
                        }
                        Some(RegisterType::Half) => {
                            let val = self.registers.load_16_by_ref(reg_ref);
                            self.push_16(val);
                        }
                        Some(RegisterType::PrivilegedHalf) => {
                            privileged!();
                            let val = self.registers.load_16_by_ref(reg_ref);
                            self.push_16(val);
                        }
                        Some(RegisterType::Word) => {
                            let val = self.registers.load_32_by_ref(reg_ref);
                            self.push_32(val);
                        }
                        Some(RegisterType::PrivilegedWord) => {
                            privileged!();
                            let val = self.registers.load_32_by_ref(reg_ref);
                            self.push_32(val);
                        }
                        Some(RegisterType::Float) => {
                            let val = self.registers.load_float_by_ref(reg_ref);
                            self.push_float(val);
                        }
                        None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()
                    }
                }
                0x0F => {  // POP
                    debug!("POP");
                    let reg_ref = fetch_8!();
                    debug!("Register: {:#x}", reg_ref);
                    match RegisterType::from_reg_ref(reg_ref) {
                        Some(RegisterType::Byte) => {
                            let val = pop!(pop_8);
                            self.registers.store_8_by_ref(reg_ref, val);
                        }
                        Some(RegisterType::Half) => {
                            let val = pop!(pop_16);
                            self.registers.store_16_by_ref(reg_ref, val);
                        }
                        Some(RegisterType::PrivilegedHalf) => {
                            privileged!();
                            let val = pop!(pop_16);
                            self.registers.store_16_by_ref(reg_ref, val);
                        }
                        Some(RegisterType::Word) => {
                            let val = pop!(pop_32);
                            self.registers.store_32_by_ref(reg_ref, val);
                        }
                        Some(RegisterType::PrivilegedWord) => {
                            privileged!();
                            let val = pop!(pop_32);
                            self.registers.store_32_by_ref(reg_ref, val);
                        }
                        Some(RegisterType::Float) => {
                            let val = pop!(pop_float);
                            self.registers.store_float_by_ref(reg_ref, val);
                        }
                        None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()
                    }
                }
                _ => {  // Unrecognised
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                }
            }
        }
    }

    fn instruction_load(&mut self, destination: u8, address: u32) {
        macro_rules! privileged {
            () => {{
                if !self.kernel_mode {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return;
                }
            }}
        }

        match RegisterType::from_reg_ref(destination) {
            Some(RegisterType::Byte) => {
                let result = self.load_8(address, false);
                if let Some(val) = result {
                    self.registers.store_8_by_ref(destination, val);
                }
            }
            Some(RegisterType::Half) => {
                let result = self.load_16(address, false);
                if let Some(val) = result {
                    self.registers.store_16_by_ref(destination, val);
                }
            }
            Some(RegisterType::PrivilegedHalf) => {
                privileged!();
                let result = self.load_16(address, false);
                if let Some(val) = result {
                    self.registers.store_16_by_ref(destination, val);
                }
            }
            Some(RegisterType::Word) => {
                let result = self.load_32(address, false);
                if let Some(val) = result {
                    self.registers.store_32_by_ref(destination, val);
                }
            }
            Some(RegisterType::PrivilegedWord) => {
                privileged!();
                let result = self.load_32(address, false);
                if let Some(val) = result {
                    self.registers.store_32_by_ref(destination, val);
                }
            }
            Some(RegisterType::Float) => {
                let result = self.load_float(address, false);
                if let Some(val) = result {
                    self.registers.store_float_by_ref(destination, val);
                }
            }
            None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()
        };
    }

    fn instruction_store(&mut self, address: u32, source: u8) {
        macro_rules! privileged {
            () => {{
                if !self.kernel_mode {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return;
                }
            }}
        }

        match RegisterType::from_reg_ref(source) {
            Some(RegisterType::Byte) => {
                self.store_8(address, self.registers.load_8_by_ref(source))
            }
            Some(RegisterType::Half) => {
                self.store_16(address, self.registers.load_16_by_ref(source))
            }
            Some(RegisterType::PrivilegedHalf) => {
                privileged!();
                self.store_16(address, self.registers.load_16_by_ref(source))
            }
            Some(RegisterType::Word) => {
                self.store_32(address, self.registers.load_32_by_ref(source))
            }
            Some(RegisterType::PrivilegedWord) => {
                privileged!();
                self.store_32(address, self.registers.load_32_by_ref(source))
            }
            Some(RegisterType::Float) => {
                self.store_float(address, self.registers.load_float_by_ref(source))
            }
            None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()
        };
    }

    fn instruction_swap(&mut self, reg_ref: u8, address: u32) {
        macro_rules! privileged {
            () => {{
                if !self.kernel_mode {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return;
                }
            }}
        }

        match RegisterType::from_reg_ref(reg_ref) {
            Some(RegisterType::Byte) => {
                let mem_result = self.load_8(address, false);
                if let Some(mem_val) = mem_result {
                    let reg_val = self.registers.load_8_by_ref(reg_ref);
                    self.store_8(address, reg_val);
                    self.registers.store_8_by_ref(reg_ref, mem_val);
                }
            }
            Some(RegisterType::Half) => {
                let mem_result = self.load_16(address, false);
                if let Some(mem_val) = mem_result {
                    let reg_val = self.registers.load_16_by_ref(reg_ref);
                    self.store_16(address, reg_val);
                    self.registers.store_16_by_ref(reg_ref, mem_val);
                }
            }
            Some(RegisterType::PrivilegedHalf) => {
                privileged!();
                let mem_result = self.load_16(address, false);
                if let Some(mem_val) = mem_result {
                    let reg_val = self.registers.load_16_by_ref(reg_ref);
                    self.store_16(address, reg_val);
                    self.registers.store_16_by_ref(reg_ref, mem_val);
                }
            }
            Some(RegisterType::Word) => {
                let mem_result = self.load_32(address, false);
                if let Some(mem_val) = mem_result {
                    let reg_val = self.registers.load_32_by_ref(reg_ref);
                    self.store_32(address, reg_val);
                    self.registers.store_32_by_ref(reg_ref, mem_val);
                }
            }
            Some(RegisterType::PrivilegedWord) => {
                privileged!();
                let mem_result = self.load_32(address, false);
                if let Some(mem_val) = mem_result {
                    let reg_val = self.registers.load_32_by_ref(reg_ref);
                    self.store_32(address, reg_val);
                    self.registers.store_32_by_ref(reg_ref, mem_val);
                }
            }
            Some(RegisterType::Float) => {
                let mem_result = self.load_float(address, false);
                if let Some(mem_val) = mem_result {
                    let reg_val = self.registers.load_float_by_ref(reg_ref);
                    self.store_float(address, reg_val);
                    self.registers.store_float_by_ref(reg_ref, mem_val);
                }
            }
            None => self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap()
        };
    }

    // WARNING!
    // This is theoretically very dangerous. No conversion is performed; we
    // just reinterpret the bit pattern as the new type. This is exactly what we
    // want to let us store float values in RAM, but if misused could result in
    // undefined behaviour.
    fn store_float(&mut self, address: u32, value: f32) {
        let converted = unsafe {std::mem::transmute::<f32, u32>(value)};
        self.store_32(address, converted);
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

    fn load_float(&mut self, address: u32, is_fetch: bool) -> Option<f32> {
        self.load_32(address, is_fetch).map(|int_val| {
            unsafe {std::mem::transmute::<u32, f32>(int_val)}
        })
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

    fn push_float(&mut self, value: f32) {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        *spr -= 4;
        let spr = *spr;  // Copy value and drop mutable reference so we are allowed to call store_float.
        self.store_float(spr, value);
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

    fn pop_float(&mut self) -> Option<f32> {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        let old_spr = *spr;
        *spr += 4;
        self.load_float(old_spr, false)
    }

    fn pop_32(&mut self) -> Option<u32> {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        let old_spr = *spr;
        *spr += 4;
        self.load_32(old_spr, false)
    }

    fn pop_16(&mut self) -> Option<u16> {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        let old_spr = *spr;
        *spr += 2;
        self.load_16(old_spr, false)
    }

    fn pop_8(&mut self) -> Option<u8> {
        let spr = if self.kernel_mode {
            &mut self.registers.kspr
        } else {
            &mut self.registers.uspr
        };
        let old_spr = *spr;
        *spr += 1;
        self.load_8(old_spr, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::{disk::DiskController, display::DisplayController,
                keyboard::{KeyboardController, KeyMessage, key_str_to_u8},
                ram::RAM, rom::ROM, ui::UICommand};

    fn run(rom_data: [u8; 512], keypress: Option<KeyMessage>) -> (CPU, Vec<UICommand>) {
        // Create communication channels.
        let (interrupt_tx, interrupt_rx) = mpsc::channel();
        let interrupt_tx_keyboard = interrupt_tx.clone();
        let interrupt_tx_mmu = interrupt_tx.clone();
        let interrupt_tx_disk_a = interrupt_tx.clone();
        let interrupt_tx_disk_b = interrupt_tx.clone();
        let (ui_tx, ui_rx) = mpsc::channel();
        let ui_tx_display = ui_tx.clone();
        let (keyboard_tx, keyboard_rx) = mpsc::channel();
        let keyboard_tx_manual = keyboard_tx.clone();

        // Create components.
        let disk_a = Arc::new(Mutex::new(DiskController::new(
            "UNUSED", interrupt_tx_disk_a, INTERRUPT_DISK_A)));
        let disk_b = Arc::new(Mutex::new(DiskController::new(
            "UNUSED", interrupt_tx_disk_b, INTERRUPT_DISK_B)));
        let display = DisplayController::new(ui_tx_display);
        let keyboard = Arc::new(Mutex::new(KeyboardController::new(
            keyboard_tx, keyboard_rx, interrupt_tx_keyboard)));
        let ram = RAM::new();
        let rom = ROM::new(rom_data);
        let mmu = MMU::new(interrupt_tx_mmu, Arc::clone(&disk_a), Arc::clone(&disk_b),
                           display, Arc::clone(&keyboard), ram, rom);
        let cpu = CPU::new(ui_tx, mmu, interrupt_tx, interrupt_rx);

        // Run the CPU till halt.
        keyboard.lock().unwrap().start();
        let cpu_thread = cpu.start();
        if let Some(message) = keypress {
            keyboard_tx_manual.send(message).unwrap();
        }
        let resulting_cpu = cpu_thread.join().unwrap();
        keyboard.lock().unwrap().stop();
        // Collect any resulting UI commands.
        let ui_commands = ui_rx.try_iter().collect();
        (resulting_cpu, ui_commands)
    }

    #[test]
    fn test_halt() {
        // Simplest possible test; check the CPU halts immediately on opcode 0.
        let (_, ui_commands) = run([0; 512], None);
        assert_eq!(ui_commands.len(), 2);  // Enable and Disable messages.
    }

    #[test]
    fn test_copy_literal() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x03;  // into r3
        rom[2] = 0x42;
        rom[3] = 0x06;
        rom[4] = 0x96;
        rom[5] = 0x96;  // some random number.


        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.registers.r[3], 0x42069696);
    }

    #[test]
    fn test_copy_reg() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x03;  // into r3
        rom[2] = 0x13;
        rom[3] = 0x57;
        rom[4] = 0x9B;
        rom[5] = 0xDF;  // some random number.

        rom[6] = 0x0B;  // Copy register
        rom[7] = 0x08;  // into r0h
        rom[8] = 0x0B;  // from r3h.

        // Now try an operation with unmatched sizes. Should raise an interrupt.
        rom[9] = 0x0B;
        rom[10] = 0x01; // into r1
        rom[11] = 0x0B; // from r3h.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.registers.r[3], 0x13579BDF);
        assert_eq!(cpu.registers.r[0], 0x00009BDF);
        assert_eq!(cpu.registers.r[1], 0x00000000);
        assert!(cpu.interrupts.latched[INTERRUPT_ILLEGAL_OPERATION as usize]);
    }

    #[test]
    fn test_store_literal_address() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x00;  // into r0
        rom[2] = 0x12;
        rom[3] = 0x34;
        rom[4] = 0x56;
        rom[5] = 0x78;  // some random number.

        rom[6] = 0x08;  // Store into
        rom[7] = 0x00;
        rom[8] = 0x00;
        rom[9] = 0x4A;
        rom[10] = 0xBC; // address 0x00004ABC
        rom[11] = 0x00; // r0.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.mmu.load_physical_32(0x00004ABC), Some(0x12345678));
    }

    #[test]
    fn test_store_reg_address() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x00;  // into r0
        rom[2] = 0xAB;
        rom[3] = 0xCD;
        rom[4] = 0xEF;
        rom[5] = 0x00;  // some random number.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x01;  // into r1
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x4A;
        rom[11] = 0xBC; // address 0x00004ABC.

        rom[12] = 0x09; // Store into
        rom[13] = 0x01; // address in r1
        rom[14] = 0x00; // r0.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.mmu.load_physical_32(0x00004ABC), Some(0xABCDEF00));
    }

    #[test]
    fn test_load_literal_address() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x07;  // into r7
        rom[2] = 0xFF;
        rom[3] = 0xFF;
        rom[4] = 0xFF;
        rom[5] = 0xFF;  // some random number.

        rom[6] = 0x06;  // Load
        rom[7] = 0x17;  // into r7b
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x00;
        rom[11] = 0x80; // ROM byte 0x40 (64).

        rom[64] = 0x55;

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.registers.r[7], 0xFFFFFF55);
    }

    #[test]
    fn test_load_reg_address() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x0E;  // into r6h
        rom[2] = 0xFF;
        rom[3] = 0xFF;  // some random number.

        rom[4] = 0x0A;  // Copy literal
        rom[5] = 0x00;  // into r0
        rom[6] = 0x00;
        rom[7] = 0x00;
        rom[8] = 0x00;
        rom[9] = 0x80;  // ROM byte 0x40 (64).

        rom[10] = 0x07; // Load
        rom[11] = 0x16; // into r6b
        rom[12] = 0x00; // address in r0.

        rom[64] = 0x34;

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.registers.r[6], 0x0000FF34);
    }

    #[test]
    fn test_swap_literal() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0x66;  // some random number.

        rom[3] = 0x0C;  // Swap with literal address
        rom[4] = 0x10;  // r0b
        rom[5] = 0x00;
        rom[6] = 0x00;
        rom[7] = 0x40;
        rom[8] = 0x00;  // address 0x00004000.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.registers.r[0], 0x00000000);
        assert_eq!(cpu.mmu.load_physical_8(0x00004000), Some(0x66));
    }

    #[test]
    fn test_swap_reg() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0x77;  // some random number.

        rom[3] = 0x0A;  // Copy literal
        rom[4] = 0x01;  // into r1
        rom[5] = 0x00;
        rom[6] = 0x00;
        rom[7] = 0x50;
        rom[8] = 0x00;  // address 0x00005000.

        rom[9] = 0x0D;  // Swap with reg ref address
        rom[10] = 0x10; // r0b
        rom[11] = 0x01; // address in r1.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.registers.r[0], 0x00000000);
        assert_eq!(cpu.mmu.load_physical_8(0x00005000), Some(0x77));
    }

    #[test]
    fn test_kernel_stack() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x22;  // into kspr
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x80;
        rom[5] = 0x00;  // address 0x00008000.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x08;  // into r0h
        rom[8] = 0xFF;
        rom[9] = 0xFF;  // some random number.

        rom[10] = 0x0E; // Push to the stack
        rom[11] = 0x08; // r0h.

        rom[12] = 0x0A; // Copy literal
        rom[13] = 0x10; // into r0b
        rom[14] = 0xAA; // some random number.

        rom[15] = 0x0E; // Push to the stack
        rom[16] = 0x10; // r0b.

        rom[17] = 0x0F; // Pop from the stack
        rom[18] = 0x09; // into r1h.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(cpu.registers.r[1], 0x0000AAFF);
        assert_eq!(cpu.registers.kspr, 0x00007FFF);
        assert_eq!(cpu.mmu.load_physical_32(0x00007FFC), Some(0x00AAFFFF));
    }

    #[test]
    fn test_user_mode() {
        let mut rom = [0; 512];
        // Store a number for the user process to find.
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0x99;  // some random number.

        rom[3] = 0x08;  // Store into
        rom[4] = 0x00;
        rom[5] = 0x00;
        rom[6] = 0x40;
        rom[7] = 0x40;  // address 0x00004040
        rom[8] = 0x10;  // r0b.

        // Set the user stack pointer.
        rom[9] = 0x0A;  // Copy literal
        rom[10] = 0x21; // into uspr
        rom[11] = 0x00;
        rom[12] = 0x00;
        rom[13] = 0x00;
        rom[14] = 0x64; // virtual address 0x64.

        // Set the page directory pointer.
        rom[15] = 0x0A; // Copy literal
        rom[16] = 0x23; // into pdpr
        rom[17] = 0x00;
        rom[18] = 0x00;
        rom[19] = 0x00;
        rom[20] = 0xC0; // ROM byte 0x80 (128).

        // Write the user mode instructions to RAM.

        // (Load the number we left behind).
        rom[21] = 0x0A; // Copy literal
        rom[22] = 0x08; // into r0h
        rom[23] = 0x06; // LOAD instruction
        rom[24] = 0x17; // into r7b.

        rom[25] = 0x08; // Store into
        rom[26] = 0x00;
        rom[27] = 0x00;
        rom[28] = 0x40;
        rom[29] = 0x00; // address 0x00004000
        rom[30] = 0x08; // r0h.

        rom[31] = 0x0A; // Copy literal
        rom[32] = 0x00; // into r0
        rom[33] = 0x00;
        rom[34] = 0x00;
        rom[35] = 0x00;
        rom[36] = 0x40; // virtual address 0x40.

        rom[37] = 0x08; // Store into
        rom[38] = 0x00;
        rom[39] = 0x00;
        rom[40] = 0x40;
        rom[41] = 0x02; // address 0x00004002
        rom[42] = 0x00; // r0.

        // (Push that number to the user stack).
        rom[43] = 0x0A; // Copy literal
        rom[44] = 0x08; // into r0h
        rom[45] = 0x0E; // PUSH instruction
        rom[46] = 0x17; // register ref r7b.

        rom[47] = 0x08; // Store into
        rom[48] = 0x00;
        rom[49] = 0x00;
        rom[50] = 0x40;
        rom[51] = 0x06; // address 0x000040006
        rom[52] = 0x08; // r0h.

        // (Try and set imr. This will cause an illegal operation interrupt).
        rom[53] = 0x0A;
        rom[54] = 0x00; // into r0
        rom[55] = 0x0A; // COPY instruction
        rom[56] = 0x24; // register ref imr
        rom[57] = 0xFF;
        rom[58] = 0xFF; // some random number.

        rom[59] = 0x08; // Store into
        rom[60] = 0x00;
        rom[61] = 0x00;
        rom[62] = 0x40;
        rom[63] = 0x08; // address 0x00004008
        rom[64] = 0x00; // r0.

        // Enable the illegal operation interrupt. We won't set a handler, so it will jump to
        // memory location zero, which will contain HALT, so the machine will actually halt.
        // We do need to set kspr though.

        // Set the kernel stack pointer.
        rom[65] = 0x0A; // Copy literal
        rom[66] = 0x22; // into kspr
        rom[67] = 0x00;
        rom[68] = 0x00;
        rom[69] = 0xA0;
        rom[70] = 0x00; // address 0x0000A000.

        // Enable the interrupt.
        rom[71] = 0x0A; // Copy literal
        rom[72] = 0x24; // into imr
        rom[73] = 0x00;
        rom[74] = 0x40; // illegal operation interrupt only.

        // Push the user mode address to the stack.
        rom[75] = 0x0A; // Copy literal
        rom[76] = 0x00; // into r0
        rom[77] = 0x00;
        rom[78] = 0x00;
        rom[79] = 0x00;
        rom[80] = 0x00; // virtual address 0x0.

        rom[81] = 0x0E; // Push to the stack
        rom[82] = 0x00; // r0.

        // Almost forgot; we need to write the page table entry to main memory.
        // Sadly we can't put that in ROM, although the page directory entry will be.
        rom[83] = 0x0A; // Copy literal
        rom[84] = 0x00; // into r0
        rom[85] = 0x00;
        rom[86] = 0x00;
        rom[87] = 0x40;
        rom[88] = 0x1F; // Valid, Present, RWX entry at 0x00004000.

        rom[89] = 0x08; // Store into
        rom[90] = 0x00;
        rom[91] = 0x00;
        rom[92] = 0xB0;
        rom[93] = 0x00; // address 0x0000B000
        rom[94] = 0x00; // r0.

        // Enter user mode!
        rom[95] = 0x04;

        // Page directory entry.
        rom[128] = 0x00;
        rom[129] = 0x00;
        rom[130] = 0xB0;
        rom[131] = 0x01; // Valid entry at 0x0000B000.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        // Assert the user mode process stored in its stack correctly.
        assert_eq!(cpu.registers.uspr, 0x00000063);
        assert_eq!(cpu.mmu.load_physical_8(0x00004063), Some(0x99));
    }

    #[test]
    fn test_keyboard() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x22;  // into kspr
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x50;
        rom[5] = 0x00;  // address 0x00005000.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x00;  // into r0
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x40;
        rom[11] = 0x00; // address 0x00004000.

        rom[12] = 0x08; // Store into
        rom[13] = 0x00;
        rom[14] = 0x00;
        rom[15] = 0x00;
        rom[16] = 0x04; // keyboard interrupt handler
        rom[17] = 0x00; // r0.

        // Address 0x4000 is HALT, so we should halt on interrupt.

        rom[18] = 0x0A; // Copy literal
        rom[19] = 0x24; // into imr
        rom[20] = 0x00;
        rom[21] = 0x02; // keyboard interrupt only.

        rom[22] = 0x01; // Pause (will only be reached if this happens before interrupt sent).
        rom[23] = 0x01; // Pause (should never happen, acts as a fail condition).

        const KEY: &str = "F";
        let (cpu, ui_commands) = run(rom, Some(KeyMessage::Key(KEY, false, false).unwrap()));
        assert_eq!(ui_commands.len(), 2);

        // Assert that the key was correctly detected.
        assert_eq!(cpu.mmu.load_physical_8(0x19B0), Some(key_str_to_u8(KEY).unwrap()));
        assert_eq!(cpu.mmu.load_physical_8(0x19B1), Some(0));
    }

    #[test]
    fn test_display() {
        let mut rom = [0; 512];
        // Set character 5,32 to '#'.
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0x21;  // character '!'.

        rom[3] = 0x08;  // Store into
        rom[4] = 0x00;
        rom[5] = 0x00;
        rom[6] = 0x03;
        rom[7] = 0xF0;  // display cell (r5,c32)
        rom[8] = 0x10;  // r0b.

        // Set foreground colour 20, 50 to dark red.
        rom[9] = 0x0A;  // Copy literal
        rom[10] = 0x10; // into r0b
        rom[11] = 0x10; // RGB(85, 0, 0).

        rom[12] = 0x08;  // Store into
        rom[13] = 0x00;
        rom[14] = 0x00;
        rom[15] = 0x10;
        rom[16] = 0x82;  // foreground colour cell (r20,c50)
        rom[17] = 0x10;  // r0b.

        // Set background colour 0, 0 to yellow.
        rom[18] = 0x0A; // Copy literal
        rom[19] = 0x10; // into r0b
        rom[20] = 0x3C; // RGB(255, 255, 0).

        rom[21] = 0x08;  // Store into
        rom[22] = 0x00;
        rom[23] = 0x00;
        rom[24] = 0x11;
        rom[25] = 0xE0;  // background colour cell (r0,c0)
        rom[26] = 0x10;  // r0b.

        let (_cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 5);
        assert_eq!(ui_commands[0], UICommand::SetEnabled(true));
        assert_eq!(ui_commands[1], UICommand::SetChar(5, 32, '!'));
        assert_eq!(ui_commands[2], UICommand::SetFg(20, 50, 85, 0, 0));
        assert_eq!(ui_commands[3], UICommand::SetBg(0, 0, 255, 255, 0));
        assert_eq!(ui_commands[4], UICommand::SetEnabled(false));
    }

    #[test]
    fn test_interrupt_handle_kernel_mode() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x22;  // into kspr
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x50;
        rom[5] = 0x00;  // address 0x00005000.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x00;  // into r0
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x00;
        rom[11] = 0xC0; // ROM byte 0x80 (128).

        rom[12] = 0x08; // Store into
        rom[13] = 0x00;
        rom[14] = 0x00;
        rom[15] = 0x00;
        rom[16] = 0x04; // keyboard interrupt handler
        rom[17] = 0x00; // r0.

        rom[18] = 0x0A; // Copy literal
        rom[19] = 0x24; // into imr
        rom[20] = 0x00;
        rom[21] = 0x02; // keyboard interrupt only.

        rom[22] = 0x0A; // Copy literal
        rom[23] = 0x16; // into r6b
        rom[24] = 0x11; // some random number.

        // Interrupt handler.
        rom[128] = 0x0A; // Copy literal
        rom[129] = 0x0D; // into r5h
        rom[130] = 0x55;
        rom[131] = 0x66; // some random number.

        rom[132] = 0x05; // IRETURN.

        const KEY: &str = "Escape";
        let (cpu, ui_commands) = run(rom, Some(KeyMessage::Key(KEY, false, false).unwrap()));
        assert_eq!(ui_commands.len(), 2);

        // Assert that the interrupt handler ran and returned.
        assert_eq!(cpu.registers.r[5], 0x00005566);
        assert_eq!(cpu.registers.r[6], 0x00000011);
    }

    #[test]
    fn test_timer_literal_interval() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x22;  // into kspr
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x50;
        rom[5] = 0x00;  // address 0x00005000.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x00;  // into r0
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x40;
        rom[11] = 0x00; // address 0x00004000.

        rom[12] = 0x08; // Store into
        rom[13] = 0x00;
        rom[14] = 0x00;
        rom[15] = 0x00;
        rom[16] = 0x07; // timer interrupt handler
        rom[17] = 0x00; // r0.

        // Address 0x4000 is HALT, so we should halt on interrupt.

        rom[18] = 0x0A; // Copy literal
        rom[19] = 0x24; // into imr
        rom[20] = 0x00;
        rom[21] = 0x80; // timer interrupt only.

        rom[22] = 0x02; // Set timer to
        rom[23] = 0x00;
        rom[24] = 0x00;
        rom[25] = 0x00;
        rom[26] = 0x64; // 100 milliseconds.

        rom[27] = 0x01; // Pause.

        let (_cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        // Simply by halting we confirm that the test was successful.
    }

    #[test]
    fn test_timer_reg_interval() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x22;  // into kspr
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x50;
        rom[5] = 0x00;  // address 0x00005000.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x00;  // into r0
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x40;
        rom[11] = 0x00; // address 0x00004000.

        rom[12] = 0x08; // Store into
        rom[13] = 0x00;
        rom[14] = 0x00;
        rom[15] = 0x00;
        rom[16] = 0x07; // timer interrupt handler
        rom[17] = 0x00; // r0.

        // Address 0x4000 is HALT, so we should halt on interrupt.

        rom[18] = 0x0A; // Copy literal
        rom[19] = 0x24; // into imr
        rom[20] = 0x00;
        rom[21] = 0x80; // timer interrupt only.

        rom[22] = 0x0A; // Copy literal
        rom[23] = 0x00; // into r0
        rom[24] = 0x00;
        rom[25] = 0x00;
        rom[26] = 0x00;
        rom[27] = 0x64; // 100 milliseconds.

        rom[28] = 0x03; // Set timer to
        rom[29] = 0x00; // interval in r0.

        rom[30] = 0x01; // Pause.

        let (_cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        // Simply by halting we confirm that the test was successful.
    }
}
