mod rotcarry;

#[macro_use]
mod macros;   // Macros moved to separate file due to length.

#[cfg(test)]  // Unit tests moved to separate file due to length.
mod tests;

use std::ops::{Add, BitAnd, BitOr, BitXor, Div, Mul, Rem, Sub};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::disk::DiskController;
use crate::mmu::MMU;
use crate::ui::UICommand;
use rotcarry::{Rcl, Rcr};

pub const INTERRUPT_SYSCALL: u32 = 0;
pub const INTERRUPT_KEYBOARD: u32 = 1;
pub const INTERRUPT_DISK_A: u32 = 2;
pub const INTERRUPT_DISK_B: u32 = 3;
pub const INTERRUPT_PAGE_FAULT: u32 = 4;
pub const INTERRUPT_DIV_BY_0: u32 = 5;
pub const INTERRUPT_ILLEGAL_OPERATION: u32 = 6;
pub const INTERRUPT_TIMER: u32 = 7;
const JOIN_THREAD: u32 = 4294967295;  // Not a real interrupt, just a thread join command.

const FLAG_ZERO: u16 = 0x01;
const FLAG_NEGATIVE: u16 = 0x02;
const FLAG_CARRY: u16 = 0x04;
const FLAG_OVERFLOW: u16 = 0x08;

#[derive(Debug, PartialEq, Eq)]
pub struct CPUError;
pub type CPUResult<T> = Result<T, CPUError>;

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

#[derive(PartialEq, Eq)]
enum ValueType {
    Byte,
    Half,
    Word,
    Float,
}

impl From<&TypedValue> for ValueType {
    fn from(tv: &TypedValue) -> Self {
        match *tv {
            TypedValue::Byte(_) => ValueType::Byte,
            TypedValue::Half(_) => ValueType::Half,
            TypedValue::Word(_) => ValueType::Word,
            TypedValue::Float(_) => ValueType::Float,
        }
    }
}

#[derive(Debug)]
enum TypedValue {
    Byte(u8),
    Half(u16),
    Word(u32),
    Float(f32),
}

impl TypedValue {
    pub fn size_in_bytes(&self) -> u32 {
        match *self {
            TypedValue::Byte(_) => 1,
            TypedValue::Half(_) => 2,
            TypedValue::Word(_) => 4,
            TypedValue::Float(_) => 4,
        }
    }

    pub fn is_integer_zero(&self) -> bool {
        match *self {
            TypedValue::Byte(x) => x == 0,
            TypedValue::Half(x) => x == 0,
            TypedValue::Word(x) => x == 0,
            TypedValue::Float(_) => false,
        }
    }

    pub fn integer_add_one(&mut self) {
        match self {
            TypedValue::Byte(x) => *x += 1,
            TypedValue::Half(x) => *x += 1,
            TypedValue::Word(x) => *x += 1,
            TypedValue::Float(_) => (),
        };
    }
}

impl From<TypedValue> for Option<u8> {
    fn from(tv: TypedValue) -> Self {
        if let TypedValue::Byte(b) = tv {
            Some(b)
        } else {
            None
        }
    }
}

impl From<TypedValue> for Option<u16> {
    fn from(tv: TypedValue) -> Self {
        if let TypedValue::Half(h) = tv {
            Some(h)
        } else {
            None
        }
    }
}

impl From<TypedValue> for Option<u32> {
    fn from(tv: TypedValue) -> Self {
        if let TypedValue::Word(w) = tv {
            Some(w)
        } else {
            None
        }
    }
}

impl From<TypedValue> for Option<f32> {
    fn from(tv: TypedValue) -> Self {
        if let TypedValue::Float(f) = tv {
            Some(f)
        } else {
            None
        }
    }
}

enum TimerCommand {
    SetTimer(u32),
    JoinThread,
}

enum PostCycleAction {
    Halt,
    Pause,
    None,
}

struct CPUInternal<D: DiskController> {
    ui_tx: mpsc::Sender<UICommand>,
    interrupt_tx: mpsc::Sender<u32>,
    timer_tx: Option<mpsc::Sender<TimerCommand>>,
    timer_thread: Option<thread::JoinHandle<()>>,
    mmu: MMU<D>,
    interrupts: InterruptLatch,
    r: [u32; 8],  // r0-r7 registers
    f: [f32; 8],  // f0-f7 registers
    flags: u16,   // Flags register
    uspr: u32,    // User Stack Pointer Register
    kspr: u32,    // Kernel Stack Pointer Register
    pdpr: u32,    // Page Directory Pointer Register
    imr: u16,     // Interrupt Mask Register
    program_counter: u32,
    kernel_mode: bool,
}

pub struct CPU<D: DiskController> {
    interrupt_tx: mpsc::Sender<u32>,
    thread_handle: Option<thread::JoinHandle<CPUInternal<D>>>,
    internal: Option<CPUInternal<D>>,
}

impl<D: DiskController + 'static> CPU<D> {
    pub fn new(ui_tx: mpsc::Sender<UICommand>, mmu: MMU<D>,
               interrupt_tx: mpsc::Sender<u32>, interrupt_rx: mpsc::Receiver<u32>) -> Self {
        CPU {
            interrupt_tx: interrupt_tx.clone(),
            thread_handle: None,
            internal: Some(CPUInternal {
                ui_tx,
                interrupt_tx,
                timer_tx: None,
                timer_thread: None,
                mmu,
                interrupts: InterruptLatch::new(interrupt_rx),
                r: [0; 8],
                f: [0.0; 8],
                flags: 0,
                uspr: 0,
                kspr: 0,
                pdpr: 0,
                imr: 0,
                program_counter: 64,  // Start of ROM.
                kernel_mode: true,
            }),
        }
    }

    pub fn start(&mut self) {
        // Spawn the worker thread and move the CPUData into it.
        let mut internal = self.internal.take()
            .expect("CPU was already running.");
        let thread_handle = thread::spawn(move || {
            // Setup.
            internal.mmu.start();
            internal.start_timer();
            internal.ui_tx.send(UICommand::SetEnabled(true)).unwrap();

            // Main loop.
            internal.cpu_loop();

            // Cleanup.
            internal.ui_tx.send(UICommand::SetEnabled(false)).unwrap();
            internal.stop_timer();
            internal.mmu.stop();

            // Move the data back out.
            internal
        });
        self.thread_handle = Some(thread_handle);
    }

    pub fn stop(&mut self) {
        self.interrupt_tx.send(JOIN_THREAD).unwrap();
        self.wait_for_halt();
    }

    fn wait_for_halt(&mut self) {
        let thread_data = self.thread_handle
            .take()
            .expect("CPU was already stopped.")
            .join()
            .expect("CPU thread terminated with error.");
        self.internal = Some(thread_data);
    }
}

impl<D: DiskController> CPUInternal<D> {
    fn start_timer(&mut self) {
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
        self.timer_thread = Some(timer_thread);
        self.timer_tx = Some(timer_tx);
    }

    fn stop_timer(&mut self) {
        let timer_tx = self.timer_tx.take().unwrap();
        timer_tx.send(TimerCommand::JoinThread).unwrap();
        self.timer_thread.take().unwrap().join().expect("Timer thread terminated with error.");
    }

    fn cpu_loop(&mut self) {
        let mut pausing = false;
        loop {
            let mut rewind = 0;
            match self.interrupt_fetch_decode_execute(pausing, &mut rewind) {
                Ok(PostCycleAction::Halt) => break,
                Ok(PostCycleAction::Pause) => pausing = true,
                Ok(PostCycleAction::None) => pausing = false,
                Err(_) => {
                    self.program_counter = self.program_counter.wrapping_sub(rewind);
                    pausing = false;
                },
            }
        }
    }

    fn interrupt_fetch_decode_execute(&mut self, pausing: bool, rewind: &mut u32) -> CPUResult<PostCycleAction> {
        // Fetch the given size value and automatically increment the program counter.
        macro_rules! fetch {
            ($type:ident) => {{
                let value = self.load(self.program_counter, true, ValueType::$type)?;
                let size = value.size_in_bytes();
                self.program_counter = self.program_counter.wrapping_add(size);
                *rewind += size;
                Into::<Option<_>>::into(value).unwrap()
            }}
        }

        // A similar macro for variable size literals.
        macro_rules! fetch_variable_size {
            ($value_type:expr) => {{
                let value = self.load(self.program_counter, true, $value_type)?;
                let size = value.size_in_bytes();
                self.program_counter = self.program_counter.wrapping_add(size);
                *rewind += size;
                value
            }}
        }

        // Try and convert the TypedValue into the (inferred) plain value. If the TypedValue is
        // of the wrong type, an interrupt will be generated and this method will return early.
        // Use this when the TypedValue isn't known to be the right type.
        macro_rules! try_tv_into_v {
            ($e:expr) => {{
                if let Some(i) = $e.into() {
                    i
                } else {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError);
                }
            }}
        }

        // Like try_tv_into_v, but panics if the wrong type.
        // Use this when the TypedValue type is known.
        macro_rules! tv_into_v {
            ($e:expr) => {Into::<Option<_>>::into($e).unwrap()}
        }

        // Ensure that two register references are of the same type.
        macro_rules! check_same_type {
            ($r1:expr, $r2:expr) => {{
                if self.reg_ref_type($r1)? != self.reg_ref_type($r2)? {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError);
                }
            }}
        }

        // Disallow float registers.
        macro_rules! reject_float {
            ($r:expr) => {{
                if self.reg_ref_type($r)? == ValueType::Float {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError);
                }
            }}
        }

        // Check for interrupts.
        let possible_interrupt = if pausing {
            Some(self.interrupts.wait_for_next(self.imr))
        } else {
            self.interrupts.try_get_next(self.imr)
        };
        if let Some(interrupt) = possible_interrupt {
            // If it's the join thread command, exit.
            if interrupt == JOIN_THREAD {
                return Ok(PostCycleAction::Halt);
            }
            debug!("Interrupt: {:#x}", interrupt);
            // Remember mode and switch to kernel mode.
            let old_mode = if self.kernel_mode {
                0b1000000000000000
            } else {
                0
            };
            self.kernel_mode = true;
            // Push flags to stack, with bit 15 set to the old mode.
            let flags = self.flags | old_mode;
            self.push(TypedValue::Half(flags))?;
            // Push the program counter to stack.
            self.push(TypedValue::Word(self.program_counter))?;
            // Push the IMR to stack.
            self.push(TypedValue::Half(self.imr))?;
            // Disable all interrupts.
            self.imr = 0;
            // Jump to the interrupt handler.
            self.program_counter = tv_into_v!(self.load(interrupt * 4, false, ValueType::Word)?);
        }

        // Fetch next instruction.
        let opcode: u8 = fetch!(Byte);
        // Decode and execute instruction.
        debug!();
        match opcode {
            0x00 => {  // HALT
                debug!("HALT");
                privileged!(self)?;
                return Ok(PostCycleAction::Halt);
            }
            0x01 => {  // PAUSE
                debug!("PAUSE");
                privileged!(self)?;
                return Ok(PostCycleAction::Pause);
            }
            0x02 => {  // TIMER with literal word
                debug!("TIMER literal word");
                privileged!(self)?;
                let milliseconds = fetch!(Word);
                debug!("Timer milliseconds: {:#x}", milliseconds);
                self.timer_tx.as_ref().unwrap()
                    .send(TimerCommand::SetTimer(milliseconds)).unwrap();
            }
            0x03 => {  // TIMER with register ref word
                debug!("TIMER register ref word");
                privileged!(self)?;
                let reg_ref = fetch!(Byte);
                let milliseconds = try_tv_into_v!(self.read_from_register(reg_ref)?);
                debug!("Timer milliseconds: {:#x}", milliseconds);
                self.timer_tx.as_ref().unwrap()
                    .send(TimerCommand::SetTimer(milliseconds)).unwrap();
            }
            0x04 => {  // USERMODE
                debug!("USERMODE");
                privileged!(self)?;
                // Get the target address.
                self.program_counter = tv_into_v!(self.pop(ValueType::Word)?);
                // Clear flags.
                self.flags = 0;
                // Enter user mode.
                self.kernel_mode = false;
            }
            0x05 => {  // IRETURN
                debug!("IRETURN");
                privileged!(self)?;
                // Restore the IMR from the stack.
                self.imr = tv_into_v!(self.pop(ValueType::Half)?);
                // Pop the program counter off the stack.
                self.program_counter = tv_into_v!(self.pop(ValueType::Word)?);
                // Pop the flags off the stack.
                let flags: u16 = tv_into_v!(self.pop(ValueType::Half)?);
                // If bit 15 is 0, enter user mode.
                if (flags & 0b1000000000000000) == 0 {
                    self.kernel_mode = false;
                }
                // Set the flags.
                self.flags = flags & 0b0111111111111111;
            }
            0x06 => {  // LOAD literal address into register ref
                debug!("LOAD literal address");
                let reg_ref_dest = fetch!(Byte);
                let literal_address = fetch!(Word);
                debug!("Dest: {:#x} Address: {:#x}", reg_ref_dest, literal_address);
                self.instruction_load(reg_ref_dest, literal_address)?;
            }
            0x07 => {  // LOAD register ref address into register ref
                debug!("LOAD register ref address");
                let reg_ref_dest = fetch!(Byte);
                let reg_ref_address = fetch!(Byte);
                let address = try_tv_into_v!(self.read_from_register(reg_ref_address)?);
                debug!("Dest: {:#x} Address: {:#x}", reg_ref_dest, address);
                self.instruction_load(reg_ref_dest, address)?;
            }
            0x08 => {  // STORE register ref into literal address
                debug!("STORE literal address");
                let literal_address = fetch!(Word);
                let reg_ref_source = fetch!(Byte);
                debug!("Address: {:#x} Source: {:#x}", literal_address, reg_ref_source);
                self.instruction_store(literal_address, reg_ref_source)?;
            }
            0x09 => {  // STORE register ref into register ref address
                debug!("STORE register ref address");
                let reg_ref_address = fetch!(Byte);
                let reg_ref_source = fetch!(Byte);
                let address = try_tv_into_v!(self.read_from_register(reg_ref_address)?);
                debug!("Address: {:#x} Source: {:#x}", address, reg_ref_source);
                self.instruction_store(address, reg_ref_source)?;
            }
            0x0A => {  // COPY variable literal into register ref
                debug!("COPY variable literal");
                let reg_ref_dest = fetch!(Byte);
                debug!("into {:#x}", reg_ref_dest);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref_dest)?);
                debug!("{:?}", value);
                self.write_to_register(reg_ref_dest, value)?;
            }
            0x0B => {  // COPY register to register
                debug!("COPY register");
                let reg_ref_dest = fetch!(Byte);
                let reg_ref_source = fetch!(Byte);
                check_same_type!(reg_ref_dest, reg_ref_source);
                debug!("from {:#x} to {:#x}", reg_ref_source, reg_ref_dest);
                let value = self.read_from_register(reg_ref_source)?;
                self.write_to_register(reg_ref_dest, value)?;
            }
            0x0C => {  // SWAP with literal address
                debug!("SWAP literal address");
                let reg_ref = fetch!(Byte);
                let address = fetch!(Word);
                debug!("register {:#x} with address {:#x}", reg_ref, address);
                self.instruction_swap(reg_ref, address)?;
            }
            0x0D => {  // SWAP with register ref address
                debug!("SWAP reg ref address");
                let reg_ref = fetch!(Byte);
                let address_ref = fetch!(Byte);
                let address = try_tv_into_v!(self.read_from_register(address_ref)?);
                debug!("register {:#x} with address {:#x}", reg_ref, address);
                self.instruction_swap(reg_ref, address)?;
            }
            0x0E => {  // PUSH
                debug!("PUSH");
                let reg_ref = fetch!(Byte);
                debug!("Register: {:#x}", reg_ref);
                let value = self.read_from_register(reg_ref)?;
                self.push(value)?;
            }
            0x0F => {  // POP
                debug!("POP");
                let reg_ref = fetch!(Byte);
                debug!("Register: {:#x}", reg_ref);
                let value = self.pop(self.reg_ref_type(reg_ref)?)?;
                self.write_to_register(reg_ref, value)?;
            }
            0x10 => {  // BLOCKCOPY literal literal literal
                debug!("BLOCKCOPY literal literal literal");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let source_address = fetch!(Word);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x11 => {  // BLOCKCOPY literal literal ref
                debug!("BLOCKCOPY literal literal ref");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let source_address_ref = fetch!(Byte);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x12 => {  // BLOCKCOPY literal ref literal
                debug!("BLOCKCOPY literal ref literal");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let source_address = fetch!(Word);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x13 => {  // BLOCKCOPY literal ref ref
                debug!("BLOCKCOPY literal ref ref");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let source_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x14 => {  // BLOCKCOPY ref literal literal
                debug!("BLOCKCOPY ref literal literal");
                let length_ref = fetch!(Byte);
                let dest_address = fetch!(Word);
                let source_address = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x15 => {  // BLOCKCOPY ref literal ref
                debug!("BLOCKCOPY ref literal ref");
                let length_ref = fetch!(Byte);
                let dest_address = fetch!(Word);
                let source_address_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x16 => {  // BLOCKCOPY ref ref literal
                debug!("BLOCKCOPY ref ref literal");
                let length_ref = fetch!(Byte);
                let dest_address_ref = fetch!(Byte);
                let source_address = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x17 => {  // BLOCKCOPY ref ref ref
                debug!("BLOCKCOPY ref ref ref");
                let length_ref = fetch!(Byte);
                let dest_address_ref = fetch!(Byte);
                let source_address_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                debug!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x18 => {  // BLOCKSET literal literal literal
                debug!("BLOCKSET literal literal literal");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let value = fetch!(Byte);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x19 => {  // BLOCKSET literal literal ref
                debug!("BLOCKSET literal literal ref");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1A => {  // BLOCKSET literal ref literal
                debug!("BLOCKSET literal ref literal");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value = fetch!(Byte);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1B => {  // BLOCKSET literal ref ref
                debug!("BLOCKSET literal ref ref");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1C => {  // BLOCKSET ref literal literal
                debug!("BLOCKSET ref literal literal");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = fetch!(Word);
                let value = fetch!(Byte);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1D => {  // BLOCKSET ref literal ref
                debug!("BLOCKSET ref literal ref");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = fetch!(Word);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1E => {  // BLOCKSET ref ref literal
                debug!("BLOCKSET ref ref literal");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value = fetch!(Byte);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1F => {  // BLOCKSET ref ref ref
                debug!("BLOCKSET ref ref ref");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x20 => {  // NEGATE
                debug!("NEGATE");
                let reg_ref = fetch!(Byte);
                debug!("Negating register {:#x}", reg_ref);
                let value = self.read_from_register(reg_ref)?;
                let negated = match value {
                    TypedValue::Byte(b) => TypedValue::Byte(-(b as i8) as u8),
                    TypedValue::Half(h) => TypedValue::Half(-(h as i16) as u16),
                    TypedValue::Word(w) => TypedValue::Word(-(w as i32) as u32),
                    TypedValue::Float(f) => TypedValue::Float(-f),
                };
                self.write_to_register(reg_ref, negated)?;
            }
            0x21 => {  // ADD literal
                debug!("ADD literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Adding {:?} to register {:#x}", value, reg_ref);
                self.instruction_add(reg_ref, value)?;
            }
            0x22 => {  // ADD ref
                debug!("ADD ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Adding {:?} to register {:#x}", value, dest);
                self.instruction_add(dest, value)?;
            }
            0x23 => {  // ADDCARRY literal
                debug!("ADDCARRY literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                let carry = self.flags & FLAG_CARRY > 0;
                debug!("Adding {:?} to register {:#x} with carry={}", value, reg_ref, carry);
                self.instruction_addcarry(reg_ref, value)?;
            }
            0x24 => {  // ADDCARRY ref
                debug!("ADDCARRY ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                let carry = self.flags & FLAG_CARRY > 0;
                debug!("Adding {:?} to register {:#x} with carry={}", value, dest, carry);
                self.instruction_addcarry(dest, value)?;
            }
            0x25 => {  // SUB literal
                debug!("SUB literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Subtracting {:?} from register {:#x}", value, reg_ref);
                self.instruction_sub(reg_ref, value)?;
            }
            0x26 => {  // SUB ref
                debug!("SUB ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Subtracting {:?} from register {:#x}", value, dest);
                self.instruction_sub(dest, value)?;
            }
            0x27 => {  // SUBBORROW literal
                debug!("SUBBORROW literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                let carry = self.flags & FLAG_CARRY > 0;
                debug!("Subtracting {:?} from register {:#x} with carry={}", value, reg_ref, carry);
                self.instruction_subborrow(reg_ref, value)?;
            }
            0x28 => {  // SUBBORROW ref
                debug!("SUBBORROW ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                let carry = self.flags & FLAG_CARRY > 0;
                debug!("Subtracting {:?} from register {:#x} with carry={}", value, dest, carry);
                self.instruction_subborrow(dest, value)?;
            }
            0x29 => {  // MULT literal
                debug!("MULT literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Multiplying register {:#x} by {:?}", reg_ref, value);
                self.instruction_mult(reg_ref, value)?;
            }
            0x2A => {  // MULT ref
                debug!("MULT ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Multiplying register {:#x} by {:?}", dest, value);
                self.instruction_mult(dest, value)?;
            }
            0x2B => {  // SDIV literal
                debug!("SDIV literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Signed dividing register {:#x} by {:?}", reg_ref, value);
                self.instruction_sdiv(reg_ref, value)?;
            }
            0x2C => {  // SDIV ref
                debug!("SDIV ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Signed dividing register {:#x} by {:?}", dest, value);
                self.instruction_sdiv(dest, value)?;
            }
            0x2D => {  // UDIV literal
                debug!("UDIV literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Unsigned dividing register {:#x} by {:?}", reg_ref, value);
                self.instruction_udiv(reg_ref, value)?;
            }
            0x2E => {  // UDIV ref
                debug!("UDIV ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Unsigned dividing register {:#x} by {:?}", dest, value);
                self.instruction_udiv(dest, value)?;
            }
            0x2F => {  // SREM literal
                debug!("SREM literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Signed remainder register {:#x} by {:?}", reg_ref, value);
                self.instruction_srem(reg_ref, value)?;
            }
            0x30 => {  // SREM ref
                debug!("SREM ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Signed remainder register {:#x} by {:?}", dest, value);
                self.instruction_srem(dest, value)?;
            }
            0x31 => {  // UREM literal
                debug!("UREM literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Unsigned remainder register {:#x} by {:?}", reg_ref, value);
                self.instruction_urem(reg_ref, value)?;
            }
            0x32 => {  // UREM ref
                debug!("UREM ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Unsigned remainder register {:#x} by {:?}", dest, value);
                self.instruction_urem(dest, value)?;
            }
            0x33 => {  // NOT
                debug!("NOT");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                debug!("Negating register {:#x}", reg_ref);
                let value = self.read_from_register(reg_ref)?;
                let flags: u16;
                let negated = match value {
                    TypedValue::Byte(x) => {
                        let not_x = !x;
                        flags = make_flags_int!(not_x as i8, false, false);
                        TypedValue::Byte(not_x)
                    },
                    TypedValue::Half(x) => {
                        let not_x = !x;
                        flags = make_flags_int!(not_x as i16, false, false);
                        TypedValue::Half(!x)
                    },
                    TypedValue::Word(x) => {
                        let not_x = !x;
                        flags = make_flags_int!(not_x as i32, false, false);
                        TypedValue::Word(!x)
                    },
                    TypedValue::Float(_) => unreachable!(),
                };
                self.write_to_register(reg_ref, negated)?;
                self.flags = flags;
            }
            0x34 => {  // AND literal
                debug!("AND literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Bitwise AND register {:#x} by {:?}", reg_ref, value);
                self.instruction_and(reg_ref, value)?;
            }
            0x35 => {  // AND ref
                debug!("AND ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Bitwise AND register {:#x} by {:?}", dest, value);
                self.instruction_and(dest, value)?;
            }
            0x36 => {  // OR literal
                debug!("OR literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Bitwise OR register {:#x} by {:?}", reg_ref, value);
                self.instruction_or(reg_ref, value)?;
            }
            0x37 => {  // OR ref
                debug!("OR ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Bitwise OR register {:#x} by {:?}", dest, value);
                self.instruction_or(dest, value)?;
            }
            0x38 => {  // XOR literal
                debug!("XOR literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Bitwise XOR register {:#x} by {:?}", reg_ref, value);
                self.instruction_xor(reg_ref, value)?;
            }
            0x39 => {  // XOR ref
                debug!("XOR ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Bitwise XOR register {:#x} by {:?}", dest, value);
                self.instruction_xor(dest, value)?;
            }
            0x3A => {  // LSHIFT literal
                debug!("LSHIFT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                debug!("Left shift register {:#x} by {}", reg_ref, value);
                self.instruction_lshift(reg_ref, value)?;
            }
            0x3B => {  // LSHIFT ref
                debug!("LSHIFT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("Left shift register {:#x} by {}", reg_ref, value);
                self.instruction_lshift(reg_ref, value)?;
            }
            0x3C => {  // SRSHIFT literal
                debug!("SRSHIFT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                debug!("Signed right shift register {:#x} by {}", reg_ref, value);
                self.instruction_srshift(reg_ref, value)?;
            }
            0x3D => {  // SRSHIFT ref
                debug!("SRSHIFT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("Signed right shift register {:#x} by {}", reg_ref, value);
                self.instruction_srshift(reg_ref, value)?;
            }
            0x3E => {  // URSHIFT literal
                debug!("URSHIFT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                debug!("Unsigned right shift register {:#x} by {}", reg_ref, value);
                self.instruction_urshift(reg_ref, value)?;
            }
            0x3F => {  // URSHIFT ref
                debug!("URSHIFT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("Unsigned right shift register {:#x} by {}", reg_ref, value);
                self.instruction_urshift(reg_ref, value)?;
            }
            0x40 => {  // LROT literal
                debug!("LROT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                debug!("Left rotate register {:#x} by {}", reg_ref, value);
                self.instruction_lrot(reg_ref, value)?;
            }
            0x41 => {  // LROT ref
                debug!("LROT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("Left rotate register {:#x} by {}", reg_ref, value);
                self.instruction_lrot(reg_ref, value)?;
            }
            0x42 => {  // RROT literal
                debug!("RROT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                debug!("Right rotate register {:#x} by {}", reg_ref, value);
                self.instruction_rrot(reg_ref, value)?;
            }
            0x43 => {  // RROT ref
                debug!("RROT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("Right rotate register {:#x} by {}", reg_ref, value);
                self.instruction_rrot(reg_ref, value)?;
            }
            0x44 => {  // LROTCARRY literal
                debug!("LROTCARRY literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                debug!("Left rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_lrotcarry(reg_ref, value)?;
            }
            0x45 => {  // LROTCARRY ref
                debug!("LROTCARRY ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("Left rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_lrotcarry(reg_ref, value)?;
            }
            0x46 => {  // RROTCARRY literal
                debug!("RROTCARRY literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                debug!("Right rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_rrotcarry(reg_ref, value)?;
            }
            0x47 => {  // RROTCARRY ref
                debug!("RROTCARRY ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                debug!("Right rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_rrotcarry(reg_ref, value)?;
            }
            0x48 => {  // JUMP literal
                debug!("JUMP literal");
                let address = fetch!(Word);
                debug!("Jumping to {:#x}", address);
                self.program_counter = address;
            }
            0x49 => {  // JUMP ref
                debug!("JUMP ref");
                let reg_ref = fetch!(Byte);
                debug!("Jumping to address in {:#x}", reg_ref);
                let address = try_tv_into_v!(self.read_from_register(reg_ref)?);
                debug!("Jumping to {:#x}", address);
                self.program_counter = address;
            }
            0x4A => {  // COMPARE literal
                debug!("COMPARE literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                debug!("Comparing register {:#x} with {:?}", reg_ref, value);
                self.instruction_compare(reg_ref, value)?;
            }
            0x4B => {  // COMPARE ref
                debug!("COMPARE ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                debug!("Comparing register {:#x} with {:?}", dest, value);
                self.instruction_compare(dest, value)?;
            }
            0x4C => {  // BLOCKCMP literal literal literal
                debug!("BLOCKCMP literal literal literal");
                let length = fetch!(Word);
                let source1 = fetch!(Word);
                let source2 = fetch!(Word);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x4D => {  // BLOCKCMP literal literal ref
                debug!("BLOCKCMP literal literal ref");
                let length = fetch!(Word);
                let source1 = fetch!(Word);
                let source2_ref = fetch!(Byte);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x4E => {  // BLOCKCMP literal ref literal
                debug!("BLOCKCMP literal ref literal");
                let length = fetch!(Word);
                let source1_ref = fetch!(Byte);
                let source2 = fetch!(Word);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x4F => {  // BLOCKCMP literal ref ref
                debug!("BLOCKCMP literal ref ref");
                let length = fetch!(Word);
                let source1_ref = fetch!(Byte);
                let source2_ref = fetch!(Byte);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x50 => {  // BLOCKCMP ref literal literal
                debug!("BLOCKCMP ref literal literal");
                let length_ref = fetch!(Byte);
                let source1 = fetch!(Word);
                let source2 = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x51 => {  // BLOCKCMP ref literal ref
                debug!("BLOCKCMP ref literal ref");
                let length_ref = fetch!(Byte);
                let source1 = fetch!(Word);
                let source2_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x52 => {  // BLOCKCMP ref ref literal
                debug!("BLOCKCMP ref ref literal");
                let length_ref = fetch!(Byte);
                let source1_ref = fetch!(Byte);
                let source2 = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x53 => {  // BLOCKCMP ref ref ref
                debug!("BLOCKCMP ref ref ref");
                let length_ref = fetch!(Byte);
                let source1_ref = fetch!(Byte);
                let source2_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                debug!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x54 =>  {  // JEQUAL literal
                cond_jump_literal!(self, jequal!(self))
            }
            0x55 => {  // JEQUAL ref
                cond_jump_reference!(self, jequal!(self))
            }
            0x56 => {  // JNOTEQUAL literal
                cond_jump_literal!(self, jnotequal!(self))
            }
            0x57 => {  // JNOTEQUAL ref
                cond_jump_reference!(self, jnotequal!(self))
            }
            0x58 => {  // SJGREATER literal
                cond_jump_literal!(self, sjgreater!(self))
            }
            0x59 => {  // SJGREATER ref
                cond_jump_reference!(self, sjgreater!(self))
            }
            0x5A => {  // SJGREATEREQ literal
                cond_jump_literal!(self, sjgreatereq!(self))
            }
            0x5B => {  // SJGREATEREQ ref
                cond_jump_reference!(self, sjgreatereq!(self))
            }
            0x5C => {  // UJGREATER literal
                cond_jump_literal!(self, ujgreater!(self))
            }
            0x5D => {  // UJGREATER ref
                cond_jump_reference!(self, ujgreater!(self))
            }
            0x5E => {  // UJGREATEREQ literal
                cond_jump_literal!(self, ujgreatereq!(self))
            }
            0x5F => {  // UJGREATEREQ ref
                cond_jump_reference!(self, ujgreatereq!(self))
            }
            0x60 => {  // SJLESSER literal
                cond_jump_literal!(self, sjlesser!(self))
            }
            0x61 => {  // SJLESSER ref
                cond_jump_reference!(self, sjlesser!(self))
            }
            0x62 => {  // SJLESSEREQ literal
                cond_jump_literal!(self, sjlessereq!(self))
            }
            0x63 => {  // SJLESSEREQ ref
                cond_jump_reference!(self, sjlessereq!(self))
            }
            0x64 => {  // UJLESSER literal
                cond_jump_literal!(self, ujlesser!(self))
            }
            0x65 => {  // UJLESSER ref
                cond_jump_reference!(self, ujlesser!(self))
            }
            0x66 => {  // UJLESSEREQ literal
                cond_jump_literal!(self, ujlessereq!(self))
            }
            0x67 => {  // UJLESSEREQ ref
                cond_jump_reference!(self, ujlessereq!(self))
            }
            0x68 => {  // CALL literal
                debug!("CALL literal");
                let address = fetch!(Word);
                debug!("Calling {:#x}", address);
                self.instruction_call(address)?;
            }
            0x69 => {  // CALL ref
                debug!("CALL ref");
                let reg_ref = fetch!(Byte);
                debug!("Calling address in {:#x}", reg_ref);
                let address = try_tv_into_v!(self.read_from_register(reg_ref)?);
                debug!("Calling {:#x}", address);
                self.instruction_call(address)?;
            }
            0x6A => {  // RETURN
                debug!("RETURN");
                let flags: u16 = tv_into_v!(self.pop(ValueType::Half)?);
                let address: u32 = tv_into_v!(self.pop(ValueType::Word)?);
                self.flags = flags & 0b0111111111111111;  // Ignore bit 15.
                self.program_counter = address;
            }
            0x6F => {  // SYSCALL
                debug!("SYSCALL");
                self.interrupt_tx.send(INTERRUPT_SYSCALL).unwrap();
            }
            0x70 => {  // SCONVERT
                debug!("SCONVERT");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                let dest_type = self.reg_ref_type(dest)?;
                let src_type = self.reg_ref_type(src)?;
                debug!("Signed conversion from {:#x} to {:#x}", src, dest);
                if src_type == ValueType::Word && dest_type == ValueType::Float {
                    // Signed integer to float.
                    let u: u32 = tv_into_v!(self.read_from_register(src)?);
                    let i: i32 = u as i32;
                    let f: f32 = i as f32;
                    self.write_to_register(dest, TypedValue::Float(f))?;
                } else if src_type == ValueType::Float && dest_type == ValueType::Word {
                    // Float to signed integer.
                    let f: f32 = tv_into_v!(self.read_from_register(src)?);
                    let i: i32 = f as i32;
                    let u: u32 = i as u32;
                    self.write_to_register(dest, TypedValue::Word(u))?;
                } else {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError);
                }
            }
            0x71 => {  // UCONVERT
            debug!("UCONVERT");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                let dest_type = self.reg_ref_type(dest)?;
                let src_type = self.reg_ref_type(src)?;
                debug!("Unsigned conversion from {:#x} to {:#x}", src, dest);
                if src_type == ValueType::Word && dest_type == ValueType::Float {
                    // Unsigned integer to float.
                    let u: u32 = tv_into_v!(self.read_from_register(src)?);
                    let f: f32 = u as f32;
                    self.write_to_register(dest, TypedValue::Float(f))?;
                } else if src_type == ValueType::Float && dest_type == ValueType::Word {
                    // Float to unsigned integer.
                    let f: f32 = tv_into_v!(self.read_from_register(src)?);
                    let u: u32 = f as u32;
                    self.write_to_register(dest, TypedValue::Word(u))?;
                } else {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError);
                }
            }
            _ => {  // Unrecognised
                self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            }
        }
        Ok(PostCycleAction::None)
    }

    fn instruction_load(&mut self, destination: u8, address: u32) -> CPUResult<()> {
        let dest_type = self.reg_ref_type(destination)?;
        let value = self.load(address, false, dest_type)?;
        self.write_to_register(destination, value)
    }

    fn instruction_store(&mut self, address: u32, source: u8) -> CPUResult<()> {
        let value = self.read_from_register(source)?;
        self.store(address, value)
    }

    fn instruction_swap(&mut self, reg_ref: u8, address: u32) -> CPUResult<()> {
        let reg_value = self.read_from_register(reg_ref)?;
        let mem_value = self.load(address, false, (&reg_value).into())?;
        self.write_to_register(reg_ref, mem_value)?;
        self.store(address, reg_value)
    }

    fn instruction_blockcopy(&mut self, length: u32, dest_address: u32,
                             source_address: u32) -> CPUResult<()> {
        if self.kernel_mode {
            for i in 0..length {
                let val = self.mmu.load_physical_8(source_address + i)?;
                self.mmu.store_physical_8(dest_address + i, val)?;
            }
        } else {
            for i in 0..length {
                let val = self.mmu.load_virtual_8(self.pdpr, source_address + i, false)?;
                self.mmu.store_virtual_8(self.pdpr, dest_address + i, val)?;
            }
        }
        Ok(())
    }

    fn instruction_blockset(&mut self, length: u32, dest_address: u32,
                            value: u8) -> CPUResult<()> {
        if self.kernel_mode {
            for i in 0..length {
                self.mmu.store_physical_8(dest_address + i, value)?;
            }
        } else {
            for i in 0..length {
                self.mmu.store_virtual_8(self.pdpr, dest_address + i, value)?;
            }
        }
        Ok(())
    }

    fn instruction_add(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        bin_op_multisigned!(self, reg_ref, value, overflowing_add, add)
    }

    fn instruction_addcarry(&mut self, reg_ref: u8, mut value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if self.flags & FLAG_CARRY != 0 {
            value.integer_add_one();
        }
        bin_op_multisigned!(self, reg_ref, value, overflowing_add, add)  // The float op is unused in this case.
    }

    fn instruction_sub(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        bin_op_multisigned!(self, reg_ref, value, overflowing_sub, sub)
    }

    fn instruction_subborrow(&mut self, reg_ref: u8, mut value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if self.flags & FLAG_CARRY != 0 {
            value.integer_add_one();
        }
        bin_op_multisigned!(self, reg_ref, value, overflowing_sub, sub)  // The float op is unused in this case.
    }

    fn instruction_mult(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        bin_op_multisigned!(self, reg_ref, value, overflowing_mul, mul)
    }

    fn instruction_sdiv(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if value.is_integer_zero() {
            self.interrupt_tx.send(INTERRUPT_DIV_BY_0).unwrap();
            return Err(CPUError);
        }
        bin_op_signed!(self, reg_ref, value, overflowing_div, div)
    }

    fn instruction_udiv(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if value.is_integer_zero() {
            self.interrupt_tx.send(INTERRUPT_DIV_BY_0).unwrap();
            return Err(CPUError);
        }
        bin_op_unsigned!(self, reg_ref, value, overflowing_div)
    }

    fn instruction_srem(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if value.is_integer_zero() {
            self.interrupt_tx.send(INTERRUPT_DIV_BY_0).unwrap();
            return Err(CPUError);
        }
        bin_op_signed!(self, reg_ref, value, overflowing_rem, rem)
    }

    fn instruction_urem(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if value.is_integer_zero() {
            self.interrupt_tx.send(INTERRUPT_DIV_BY_0).unwrap();
            return Err(CPUError);
        }
        bin_op_unsigned!(self, reg_ref, value, overflowing_rem)
    }

    fn instruction_and(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        bin_op_bitwise!(self, reg_ref, value, bitand)
    }

    fn instruction_or(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        bin_op_bitwise!(self, reg_ref, value, bitor)
    }

    fn instruction_xor(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        bin_op_bitwise!(self, reg_ref, value, bitxor)
    }

    fn instruction_lshift(&mut self, reg_ref: u8, value: u8) -> CPUResult<()> {
        let value: u32 = value as u32;
        let flags: u16;
        match self.read_from_register(reg_ref)? {
            TypedValue::Byte(x) => {
                let (ans, carry) = if let Some(z) = x.checked_shl(value) {
                    let c = x.leading_zeros() < value;
                    (z, c)
                } else {
                    (0, true)
                };
                self.write_to_register(reg_ref, TypedValue::Byte(ans))?;
                flags = make_flags_int!(ans as i8, carry, false);
            },
            TypedValue::Half(x) => {
                let (ans, carry) = if let Some(z) = x.checked_shl(value) {
                    let c = x.leading_zeros() < value;
                    (z, c)
                } else {
                    (0, true)
                };
                self.write_to_register(reg_ref, TypedValue::Half(ans))?;
                flags = make_flags_int!(ans as i16, carry, false);
            },
            TypedValue::Word(x) => {
                let (ans, carry) = if let Some(z) = x.checked_shl(value) {
                    let c = x.leading_zeros() < value;
                    (z, c)
                } else {
                    (0, true)
                };
                self.write_to_register(reg_ref, TypedValue::Word(ans))?;
                flags = make_flags_int!(ans as i32, carry, false);
            },
            TypedValue::Float(_) => {
                unreachable!()
            },
        }
        self.flags = flags;
        Ok(())
    }

    fn instruction_srshift(&mut self, reg_ref: u8, value: u8) -> CPUResult<()> {
        let value: u32 = value as u32;
        let flags: u16;
        match self.read_from_register(reg_ref)? {
            TypedValue::Byte(x) => {
                let (ans, carry) = if let Some(z) = (x as i8).checked_shr(value) {
                    let c = x.trailing_zeros() < value;
                    (z as u8, c)
                } else {
                    let z = if (x as i8) < 0 {u8::MAX} else {0};
                    (z, true)
                };
                self.write_to_register(reg_ref, TypedValue::Byte(ans))?;
                flags = make_flags_int!(ans as i8, carry, false);
            },
            TypedValue::Half(x) => {
                let (ans, carry) = if let Some(z) = (x as i16).checked_shr(value) {
                    let c = x.trailing_zeros() < value;
                    (z as u16, c)
                } else {
                    let z = if (x as i16) < 0 {u16::MAX} else {0};
                    (z, true)
                };
                self.write_to_register(reg_ref, TypedValue::Half(ans))?;
                flags = make_flags_int!(ans as i16, carry, false);
            },
            TypedValue::Word(x) => {
                let (ans, carry) = if let Some(z) = (x as i32).checked_shr(value) {
                    let c = x.trailing_zeros() < value;
                    (z as u32, c)
                } else {
                    let z = if (x as i32) < 0 {u32::MAX} else {0};
                    (z, true)
                };
                self.write_to_register(reg_ref, TypedValue::Word(ans))?;
                flags = make_flags_int!(ans as i32, carry, false);
            },
            TypedValue::Float(_) => {
                unreachable!()
            },
        }
        self.flags = flags;
        Ok(())
    }

    fn instruction_urshift(&mut self, reg_ref: u8, value: u8) -> CPUResult<()> {
        let value: u32 = value as u32;
        let flags: u16;
        match self.read_from_register(reg_ref)? {
            TypedValue::Byte(x) => {
                let (ans, carry) = if let Some(z) = x.checked_shr(value) {
                    let c = x.trailing_zeros() < value;
                    (z, c)
                } else {
                    (0, true)
                };
                self.write_to_register(reg_ref, TypedValue::Byte(ans))?;
                flags = make_flags_int!(ans as i8, carry, false);
            },
            TypedValue::Half(x) => {
                let (ans, carry) = if let Some(z) = x.checked_shr(value) {
                    let c = x.trailing_zeros() < value;
                    (z, c)
                } else {
                    (0, true)
                };
                self.write_to_register(reg_ref, TypedValue::Half(ans))?;
                flags = make_flags_int!(ans as i16, carry, false);
            },
            TypedValue::Word(x) => {
                let (ans, carry) = if let Some(z) = x.checked_shr(value) {
                    let c = x.trailing_zeros() < value;
                    (z, c)
                } else {
                    (0, true)
                };
                self.write_to_register(reg_ref, TypedValue::Word(ans))?;
                flags = make_flags_int!(ans as i32, carry, false);
            },
            TypedValue::Float(_) => {
                unreachable!()
            },
        }
        self.flags = flags;
        Ok(())
    }

    fn instruction_lrot(&mut self, reg_ref: u8, value: u8) -> CPUResult<()> {
        let value: u32 = value as u32;
        bin_op_rotate!(self, reg_ref, value, rotate_left)
    }

    fn instruction_rrot(&mut self, reg_ref: u8, value: u8) -> CPUResult<()> {
        let value: u32 = value as u32;
        bin_op_rotate!(self, reg_ref, value, rotate_right)
    }

    fn instruction_lrotcarry(&mut self, reg_ref: u8, value: u8) -> CPUResult<()> {
        bin_op_rotate_carry!(self, reg_ref, value, rcl)
    }

    fn instruction_rrotcarry(&mut self, reg_ref: u8, value: u8) -> CPUResult<()> {
        bin_op_rotate_carry!(self, reg_ref, value, rcr)
    }

    fn instruction_compare(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        self.flags = match self.read_from_register(reg_ref)? {
            TypedValue::Byte(x) => {
                let y = Into::<Option<u8>>::into(value).unwrap();
                let u_ans = x.overflowing_sub(y);
                let s_ans = (x as i8).overflowing_sub(y as i8);
                debug_assert_eq!(u_ans.0, s_ans.0 as u8);
                make_flags_int!(s_ans.0, u_ans.1, s_ans.1)
            },
            TypedValue::Half(x) => {
                let y = Into::<Option<u16>>::into(value).unwrap();
                let u_ans = x.overflowing_sub(y);
                let s_ans = (x as i16).overflowing_sub(y as i16);
                debug_assert_eq!(u_ans.0, s_ans.0 as u16);
                make_flags_int!(s_ans.0, u_ans.1, s_ans.1)
            },
            TypedValue::Word(x) => {
                let y = Into::<Option<u32>>::into(value).unwrap();
                let u_ans = x.overflowing_sub(y);
                let s_ans = (x as i32).overflowing_sub(y as i32);
                debug_assert_eq!(u_ans.0, s_ans.0 as u32);
                make_flags_int!(s_ans.0, u_ans.1, s_ans.1)
            },
            TypedValue::Float(x) => {
                let y = Into::<Option<f32>>::into(value).unwrap();
                let ans = x - y;
                make_flags_float!(ans)
            },
        };
        Ok(())
    }

    fn instruction_blockcmp(&mut self, length: u32, source1: u32, source2: u32) -> CPUResult<()> {
        if self.kernel_mode {
            for i in 0..length {
                let val1 = self.mmu.load_physical_8(source1 + i)?;
                let val2 = self.mmu.load_physical_8(source2 + i)?;
                if val1 > val2 {
                    self.flags = 0;
                    return Ok(());
                } else if val2 > val1 {
                    self.flags = FLAG_NEGATIVE;
                    return Ok(());
                }
            }
        } else {
            for i in 0..length {
                let val1 = self.mmu.load_virtual_8(self.pdpr, source1 + i, false)?;
                let val2 = self.mmu.load_virtual_8(self.pdpr, source2 + i, false)?;
                if val1 > val2 {
                    self.flags = 0;
                    return Ok(());
                } else if val2 > val1 {
                    self.flags = FLAG_NEGATIVE;
                    return Ok(());
                }
            }
        }
        self.flags = FLAG_ZERO;
        Ok(())
    }

    fn instruction_call(&mut self, address: u32) -> CPUResult<()> {
        self.push(TypedValue::Word(self.program_counter))?;
        self.push(TypedValue::Half(self.flags)).map(|success| {
            self.program_counter = address;
            success
        }).map_err(|error| {
            // Ensure this is atomic: don't leave the PC on the stack.
            // If the push succeeded, the pop should always too.
            self.pop(ValueType::Word).expect("Failed to clean up CALL that died halfway.");
            error
        })
    }

    fn reg_ref_type(&self, reg_ref: u8) -> CPUResult<ValueType> {
        if reg_ref < 0x08 {          // r0-7
            Ok(ValueType::Word)
        } else if reg_ref < 0x10 {   // r0h-r7h
            Ok(ValueType::Half)
        } else if reg_ref < 0x18 {   // r0b-r7b
            Ok(ValueType::Byte)
        } else if reg_ref < 0x20 {   // f0-f7
            Ok(ValueType::Float)
        } else if reg_ref == 0x20 {  // FLAGS
            Ok(ValueType::Half)
        } else if reg_ref < 0x24 {   // USPR, KSPR, PDPR
            Ok(ValueType::Word)
        } else if reg_ref == 0x24 {  // IMR
            Ok(ValueType::Half)
        } else if reg_ref == 0x25 {  // PFSR
            Ok(ValueType::Word)
        } else {
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            Err(CPUError)
        }
    }

    fn write_to_register(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        if reg_ref < 0x08 {
            if let TypedValue::Word(w) = value {
                self.r[reg_ref as usize] = w;
                return Ok(());
            }
        } else if reg_ref < 0x10 {
            if let TypedValue::Half(h) = value {
                let index = (reg_ref - 0x08) as usize;
                let masked = self.r[index] & 0xFFFF0000;
                self.r[index] = masked | (h as u32);
                return Ok(());
            }
        } else if reg_ref < 0x18 {
            if let TypedValue::Byte(b) = value {
                let index = (reg_ref - 0x10) as usize;
                let masked = self.r[index] & 0xFFFFFF00;
                self.r[index] = masked | (b as u32);
                return Ok(());
            }
        } else if reg_ref < 0x20 {
            if let TypedValue::Float(f) = value {
                let index = (reg_ref - 0x18) as usize;
                self.f[index] = f;
                return Ok(());
            }
        } else if reg_ref == 0x20 {
            if let TypedValue::Half(h) = value {
                let masked = h & 0b0111111111111111;  // Ignore bit 15.
                self.flags = masked;
                return Ok(());
            }
        } else if reg_ref == 0x21 {
            if let TypedValue::Word(w) = value {
                self.uspr = w;
                return Ok(());
            }
        } else if reg_ref == 0x22 {
            if let TypedValue::Word(w) = value {
                privileged!(self)?;
                self.kspr = w;
                return Ok(());
            }
        } else if reg_ref == 0x23 {
            if let TypedValue::Word(w) = value {
                privileged!(self)?;
                self.pdpr = w;
                return Ok(());
            }
        } else if reg_ref == 0x24 {
            if let TypedValue::Half(h) = value {
                privileged!(self)?;
                self.imr = h;
                return Ok(());
            }
        } else if reg_ref == 0x25 {
            debug!("Illegal write to PFSR.");
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            return Err(CPUError);
        } else {
            debug!("Invalid register reference: {:#x}", reg_ref);
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            return Err(CPUError);
        };
        debug!("Register size mismatch: register {:#x} with size {}", reg_ref, value.size_in_bytes());
        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
        return Err(CPUError);
    }

    fn read_from_register(&mut self, reg_ref: u8) -> CPUResult<TypedValue> {
        if reg_ref < 0x08 {
            Ok(TypedValue::Word(self.r[reg_ref as usize]))
        } else if reg_ref < 0x10 {
            Ok(TypedValue::Half(self.r[(reg_ref - 0x08) as usize] as u16))
        } else if reg_ref < 0x18 {
            Ok(TypedValue::Byte(self.r[(reg_ref - 0x10) as usize] as u8))
        } else if reg_ref < 0x20 {
            Ok(TypedValue::Float(self.f[(reg_ref - 0x18) as usize]))
        } else if reg_ref == 0x20 {
            Ok(TypedValue::Half(self.flags))
        } else if reg_ref == 0x21 {
            Ok(TypedValue::Word(self.uspr))
        } else if reg_ref == 0x22 {
            privileged!(self)?;
            Ok(TypedValue::Word(self.kspr))
        } else if reg_ref == 0x23 {
            privileged!(self)?;
            Ok(TypedValue::Word(self.pdpr))
        } else if reg_ref == 0x24 {
            privileged!(self)?;
            Ok(TypedValue::Half(self.imr))
        } else if reg_ref == 0x25 {
            privileged!(self)?;
            Ok(TypedValue::Word(self.mmu.page_fault_status_register()))
        } else {
            debug!("Invalid register reference: {:#x}", reg_ref);
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            Err(CPUError)
        }
    }

    fn store(&mut self, address: u32, value: TypedValue) -> CPUResult<()> {
        match value {
            TypedValue::Byte(b) => {
                if self.kernel_mode {
                    self.mmu.store_physical_8(address, b)
                } else {
                    self.mmu.store_virtual_8(self.pdpr, address, b)
                }
            }
            TypedValue::Half(h) => {
                if self.kernel_mode {
                    self.mmu.store_physical_16(address, h)
                } else {
                    self.mmu.store_virtual_16(self.pdpr, address, h)
                }
            }
            TypedValue::Word(w) => {
                if self.kernel_mode {
                    self.mmu.store_physical_32(address, w)
                } else {
                    self.mmu.store_virtual_32(self.pdpr, address, w)
                }
            }
            TypedValue::Float(f) => {
                // No conversion is performed; we just reinterpret the bits as an integer.
                // This is exactly what we want to let us store float values in RAM.
                let converted = unsafe {std::mem::transmute::<f32, u32>(f)};
                if self.kernel_mode {
                    self.mmu.store_physical_32(address, converted)
                } else {
                    self.mmu.store_virtual_32(self.pdpr, address, converted)
                }
            }
        }
    }

    fn load(&mut self, address: u32, is_fetch: bool, value_type: ValueType) -> CPUResult<TypedValue> {
        match value_type {
            ValueType::Byte => {
                if self.kernel_mode {
                    self.mmu.load_physical_8(address)
                        .map(|b| TypedValue::Byte(b))
                } else {
                    self.mmu.load_virtual_8(self.pdpr, address, is_fetch)
                        .map(|b| TypedValue::Byte(b))
                }
            }
            ValueType::Half => {
                if self.kernel_mode {
                    self.mmu.load_physical_16(address)
                        .map(|h| TypedValue::Half(h))
                } else {
                    self.mmu.load_virtual_16(self.pdpr, address, is_fetch)
                        .map(|h| TypedValue::Half(h))
                }
            }
            ValueType::Word => {
                if self.kernel_mode {
                    self.mmu.load_physical_32(address)
                        .map(|w| TypedValue::Word(w))
                } else {
                    self.mmu.load_virtual_32(self.pdpr, address, is_fetch)
                        .map(|w| TypedValue::Word(w))
                }
            }
            ValueType::Float => {
                if self.kernel_mode {
                    self.mmu.load_physical_32(address)
                        .map(|f| TypedValue::Float(unsafe {std::mem::transmute::<u32, f32>(f)}))
                } else {
                    self.mmu.load_virtual_32(self.pdpr, address, is_fetch)
                        .map(|f| TypedValue::Float(unsafe {std::mem::transmute::<u32, f32>(f)}))
                }
            }
        }
    }

    fn push(&mut self, value: TypedValue) -> CPUResult<()> {
        if self.kernel_mode {
            self.kspr = self.kspr.wrapping_sub(value.size_in_bytes());
            self.store(self.kspr, value)
        } else {
            self.uspr = self.uspr.wrapping_sub(value.size_in_bytes());
            self.store(self.uspr, value)
        }
    }

    fn pop(&mut self, value_type: ValueType) -> CPUResult<TypedValue> {
        if self.kernel_mode {
            let value = self.load(self.kspr, false, value_type)?;
            self.kspr = self.kspr.wrapping_add(value.size_in_bytes());
            Ok(value)
        } else {
            let value = self.load(self.uspr, false, value_type)?;
            self.uspr = self.uspr.wrapping_add(value.size_in_bytes());
            Ok(value)
        }
    }
}
