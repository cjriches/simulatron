mod rotcarry;

#[macro_use]
mod macros;   // Macros moved to separate file due to length.

#[cfg(test)]  // Unit tests moved to separate file due to length.
mod tests;

use log::{trace, debug, info};
use std::convert::{TryFrom, TryInto};
use std::ops::{Add, BitAnd, BitOr, BitXor, Div, Mul, Rem, Sub};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::disk::DiskController;
use crate::mmu::MMU;
use crate::ui::UICommand;
use rotcarry::{Rcl, Rcr};

// Interrupt values.
pub const INTERRUPT_ILLEGAL_OPERATION: u32 = 0;
pub const INTERRUPT_DIV_BY_0: u32 = 1;
pub const INTERRUPT_PAGE_FAULT: u32 = 2;
pub const INTERRUPT_KEYBOARD: u32 = 3;
pub const INTERRUPT_DISK_A: u32 = 4;
pub const INTERRUPT_DISK_B: u32 = 5;
pub const INTERRUPT_TIMER: u32 = 6;
pub const INTERRUPT_SYSCALL: u32 = 7;
const JOIN_THREAD: u32 = u32::MAX;  // Not a real interrupt, just a thread join command.

// Flag bits.
const FLAG_ZERO: u16 = 0x01;
const FLAG_NEGATIVE: u16 = 0x02;
const FLAG_CARRY: u16 = 0x04;
const FLAG_OVERFLOW: u16 = 0x08;

/// Possible errors from a CPU cycle.
#[derive(Debug, PartialEq, Eq)]
pub enum CPUError {
    TryAgainError,
    FatalError,
}
pub type CPUResult<T> = Result<T, CPUError>;

/// All possible value types that Simulatron natively supports.
#[derive(PartialEq, Eq)]
enum ValueType {
    Byte,
    Half,
    Word,
    Float,
}

/// Get the type of a TypedValue.
impl From<&TypedValue> for ValueType {
    fn from(tv: &TypedValue) -> Self {
        match tv {
            TypedValue::Byte(_) => ValueType::Byte,
            TypedValue::Half(_) => ValueType::Half,
            TypedValue::Word(_) => ValueType::Word,
            TypedValue::Float(_) => ValueType::Float,
        }
    }
}

/// A value that Simulatron can work with.
#[derive(Debug)]
enum TypedValue {
    Byte(u8),
    Half(u16),
    Word(u32),
    Float(f32),
}

impl TypedValue {
    /// Return the number of bytes in the given value.
    fn size_in_bytes(&self) -> u32 {
        match *self {
            TypedValue::Byte(_) => 1,
            TypedValue::Half(_) => 2,
            TypedValue::Word(_) => 4,
            TypedValue::Float(_) => 4,
        }
    }

    /// Is this TV an integer equal to zero?
    fn is_integer_zero(&self) -> bool {
        match *self {
            TypedValue::Byte(x) => x == 0,
            TypedValue::Half(x) => x == 0,
            TypedValue::Word(x) => x == 0,
            TypedValue::Float(_) => false,
        }
    }

    /// Mutate this TV by adding one.
    fn increment(&mut self) {
        match self {
            TypedValue::Byte(x) => *x += 1,
            TypedValue::Half(x) => *x += 1,
            TypedValue::Word(x) => *x += 1,
            TypedValue::Float(x) => *x += 1.0,
        };
    }
}

/// Error for casting a TypedValue to the wrong type.
#[derive(Debug)]
struct WrongType;

impl TryFrom<TypedValue> for u8 {
    type Error = WrongType;
    fn try_from(value: TypedValue) -> Result<Self, Self::Error> {
        if let TypedValue::Byte(b) = value {
            Ok(b)
        } else {
            Err(WrongType)
        }
    }
}

impl TryFrom<TypedValue> for u16 {
    type Error = WrongType;
    fn try_from(value: TypedValue) -> Result<Self, Self::Error> {
        if let TypedValue::Half(h) = value {
            Ok(h)
        } else {
            Err(WrongType)
        }
    }
}

impl TryFrom<TypedValue> for u32 {
    type Error = WrongType;
    fn try_from(value: TypedValue) -> Result<Self, Self::Error> {
        if let TypedValue::Word(w) = value {
            Ok(w)
        } else {
            Err(WrongType)
        }
    }
}

impl TryFrom<TypedValue> for f32 {
    type Error = WrongType;
    fn try_from(value: TypedValue) -> Result<Self, Self::Error> {
        if let TypedValue::Float(f) = value {
            Ok(f)
        } else {
            Err(WrongType)
        }
    }
}

/// An interrupt latch.
struct InterruptLatch {
    latched: [bool; 8],
    interrupt_rx: Receiver<u32>,
}

impl InterruptLatch {
    /// Create a new interrupt latch with the given interrupt channel.
    fn new(interrupt_rx: Receiver<u32>) -> Self {
        InterruptLatch {
            latched: [false; 8],
            interrupt_rx,
        }
    }

    /// Poll the next interrupt, returning immediately if none are present.
    fn try_get_next(&mut self, imr: u16) -> Option<u32> {
        // First, try and service latched interrupts, prioritising lower numbers first.
        for i in 0..8 {
            if self.latched[i] && (imr & (1 << i as u16)) > 0 {
                debug!("Returning latched interrupt {}.", i);
                self.latched[i] = false;
                return Some(i as u32);
            }
        }

        // If there aren't any enabled latched interrupts, check the channel.
        loop {
            match self.interrupt_rx.try_recv() {
                Ok(interrupt) => {
                    // If enabled, return. If disabled, latch it and check again.
                    if interrupt == JOIN_THREAD || (imr & (1 << interrupt as u16)) > 0 {
                        debug!("Returning interrupt {}.", interrupt);
                        return Some(interrupt);
                    } else {
                        debug!("Latching interrupt {}.", interrupt);
                        self.latched[interrupt as usize] = true;
                    }
                },
                Err(TryRecvError::Disconnected) => panic!(),
                Err(TryRecvError::Empty) => {
                    trace!("No enabled interrupts available.");
                    return None
                },
            }
        }
    }

    /// Block until an interrupt is available.
    fn wait_for_next(&mut self, imr: u16) -> u32 {
        debug!("Waiting on interrupt.");
        // First, try and service latched interrupts, prioritising lower numbers first.
        for i in 0..8 {
            if self.latched[i] && (imr & (1 << i as u16)) > 0 {
                debug!("Returning latched interrupt {}.", i);
                self.latched[i] = false;
                return i as u32;
            }
        }

        // If there aren't any enabled latched interrupts, wait on the channel.
        loop {
            let interrupt = self.interrupt_rx.recv().unwrap();
            // If enabled, directly return. If disabled, latch it and check again.
            // Also directly return JOIN_THREAD.
            if interrupt == JOIN_THREAD || (imr & (1 << interrupt as u16)) > 0 {
                debug!("Returning interrupt {}.", interrupt);
                return interrupt;
            } else {
                debug!("Latching interrupt {}.", interrupt);
                self.latched[interrupt as usize] = true;
            }
        }
    }
}

/// A CPU timer.
struct Timer {
    interrupt_tx: Option<Sender<u32>>,
    command_tx: Option<Sender<TimerCommand>>,
    thread_handle: Option<thread::JoinHandle<Sender<u32>>>,
}

impl Timer {
    /// Create a new Timer with the given interrupt channel.
    fn new(interrupt_tx: Sender<u32>) -> Self {
        Self {
            interrupt_tx: Some(interrupt_tx),
            command_tx: None,
            thread_handle: None,
        }
    }

    /// Start the timer thread. Panics if already started.
    fn start(&mut self) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let interrupt_tx = self.interrupt_tx.take().unwrap();
        let timer_thread = thread::spawn(move || {
            let mut interval = 0;
            loop {
                if interval == 0 {
                    // Wait indefinitely for a command.
                    match cmd_rx.recv().unwrap() {
                        TimerCommand::SetTimer(new_interval) => interval = new_interval,
                        TimerCommand::JoinThread => return interrupt_tx,
                    };
                } else {
                    // Wait for a command for up to `interval`, then send a timer interrupt.
                    match cmd_rx.recv_timeout(Duration::from_millis(interval as u64)) {
                        Ok(TimerCommand::SetTimer(new_interval)) => interval = new_interval,
                        Ok(TimerCommand::JoinThread) => return interrupt_tx,
                        Err(RecvTimeoutError::Timeout) =>
                            interrupt_tx.send(INTERRUPT_TIMER).unwrap(),
                        Err(RecvTimeoutError::Disconnected) => panic!(),
                    };
                }
            }
        });
        self.thread_handle = Some(timer_thread);
        self.command_tx = Some(cmd_tx);
    }

    /// Stop the timer thread. Panics if not running.
    fn stop(&mut self) {
        let cmd_tx = self.command_tx.take().unwrap();
        cmd_tx.send(TimerCommand::JoinThread).unwrap();
        let interrupt_tx = self.thread_handle.take().unwrap()
            .join().expect("Timer thread terminated with error.");
        self.interrupt_tx = Some(interrupt_tx);
    }
}

/// Commands that can be sent to the timer thread.
enum TimerCommand {
    SetTimer(u32),
    JoinThread,
}

/// Actions that the CPU might take after a successful cycle.
enum PostCycleAction {
    Halt,
    Pause,
    None,
}

/// The internals of a CPU, which get moved to a separate thread while running.
struct CPUInternal<D> {
    timer: Timer,
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
    ui_tx: Sender<UICommand>,
    interrupt_tx: Sender<u32>,
}

/// The public-facing CPU interface.
pub struct CPU<D> {
    interrupt_tx: Sender<u32>,
    thread_handle: Option<thread::JoinHandle<CPUInternal<D>>>,
    internal: Option<CPUInternal<D>>,
}

impl<D: DiskController + 'static> CPU<D> {
    /// Create a new CPU with the given MMU, interrupt channel, and UI command
    /// channel.
    pub fn new(ui_tx: Sender<UICommand>, mmu: MMU<D>,
               interrupt_tx: Sender<u32>, interrupt_rx: Receiver<u32>) -> Self {
        CPU {
            interrupt_tx: interrupt_tx.clone(),
            thread_handle: None,
            internal: Some(CPUInternal {
                timer: Timer::new(interrupt_tx.clone()),
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
                ui_tx,
                interrupt_tx,
            }),
        }
    }

    /// Start the CPU on its own thread. Panics if already running.
    pub fn start(&mut self) {
        // Spawn the worker thread and move the CPUInternal into it.
        let mut internal = self.internal.take()
            .expect("CPU was already running.");
        let thread_handle = thread::spawn(move || {
            // Setup.
            internal.mmu.start();
            internal.timer.start();

            // Main loop.
            internal.cpu_loop();

            // Cleanup.
            internal.ui_tx.send(UICommand::CPUHalted).unwrap();
            internal.timer.stop();
            internal.mmu.stop();

            // Move the data back out.
            internal
        });
        self.thread_handle = Some(thread_handle);
    }

    /// Stop the CPU thread. Panics if not running.
    pub fn stop(&mut self) {
        self.interrupt_tx.send(JOIN_THREAD).unwrap();
        self.wait_for_halt();
    }

    /// Block until the CPU thread terminates. Panics if not running.
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
    /// Top-level CPU loop.
    fn cpu_loop(&mut self) {
        let mut pausing = false;
        info!("CPU starting.");
        loop {
            // Yes, `rewind` could be integrated into CPUError::TryAgainError.
            // However, this would be a pain, as TryAgainError is also generated
            // by the MMU, which cannot provide the `rewind` value. Having a
            // &mut u32 parameter is less ugly than giving the MMU a separate
            // error type and wrapping every MMU access in a conversion macro.
            let mut rewind = 0;
            // Perform one cycle.
            match self.interrupt_fetch_decode_execute(pausing, &mut rewind) {
                Ok(PostCycleAction::Halt) => {
                    info!("CPU halting.");
                    break
                },
                Ok(PostCycleAction::Pause) => {
                    info!("CPU pausing.");
                    pausing = true;
                },
                Ok(PostCycleAction::None) => {
                    pausing = false;
                },
                Err(CPUError::TryAgainError) => {
                    trace!("CPU cycle resulted in an error.");
                    self.program_counter = self.program_counter.wrapping_sub(rewind);
                    pausing = false;
                },
                Err(CPUError::FatalError) => {
                    info!("Fatal CPU error, halting.");
                    break;
                },
            }
        }
    }

    /// Perform a single cycle. If `pausing` is true, the CPU will pause before
    /// doing anything and wait for an interrupt. The `rewind` parameter is
    /// incremented every time the program counter is incremented, so
    /// subtracting `rewind` from the program counter will always restore it
    /// to the value it had before this cycle.
    fn interrupt_fetch_decode_execute(&mut self, pausing: bool,
                                      rewind: &mut u32) -> CPUResult<PostCycleAction> {
        /// Fetch the given statically-sized value and increment the program counter.
        macro_rules! fetch {
            ($type:ident) => {{
                let value = self.load(self.program_counter, true, ValueType::$type)?;
                let size = value.size_in_bytes();
                self.program_counter = self.program_counter.wrapping_add(size);
                *rewind += size;
                value.try_into().unwrap()
            }}
        }

        /// Fetch the given dynamically-sized value and increment the program counter.
        macro_rules! fetch_variable_size {
            ($value_type:expr) => {{
                let value = self.load(self.program_counter, true, $value_type)?;
                let size = value.size_in_bytes();
                self.program_counter = self.program_counter.wrapping_add(size);
                *rewind += size;
                value
            }}
        }

        /// Try and convert the TypedValue into the (inferred) raw value. If the
        /// TypedValue is of the wrong type, the cycle will return with an
        /// illegal operation interrupt.
        /// Use this when the TypedValue isn't known to be the right type.
        macro_rules! try_tv_into_v {
            ($e:expr) => {{
                if let Ok(i) = $e.try_into() {
                    i
                } else {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError::TryAgainError);
                }
            }}
        }

        /// Like try_tv_into_v, but panics if the type is wrong.
        /// Use this when the ValueType is statically known to be correct.
        macro_rules! tv_into_v {
            ($e:expr) => {$e.try_into().unwrap()}
        }

        /// Ensure that two register references are of the same type, returning
        /// with an illegal operation interrupt otherwise.
        macro_rules! check_same_type {
            ($r1:expr, $r2:expr) => {{
                if self.reg_ref_type($r1)? != self.reg_ref_type($r2)? {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError::TryAgainError);
                }
            }}
        }

        /// Ensure the given value is not a float, returning with an illegal
        /// operation interrupt otherwise.
        macro_rules! reject_float {
            ($r:expr) => {{
                if self.reg_ref_type($r)? == ValueType::Float {
                    self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
                    return Err(CPUError::TryAgainError);
                }
            }}
        }

        /// Safely unwrap an operation for which an error is fatal.
        macro_rules! critical {
            ($op:expr) => { $op.or(Err(CPUError::FatalError))? }
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
            trace!("Interrupt: {:#x}", interrupt);
            // Remember mode and switch to kernel mode.
            let old_mode = if self.kernel_mode {
                0b1000000000000000
            } else {
                0
            };
            self.kernel_mode = true;
            // Push flags to stack, with bit 15 set to the old mode.
            // These operations must not fail; we have no way to recover.
            let flags = self.flags | old_mode;
            critical!(self.push(TypedValue::Half(flags)));
            // Push the program counter to stack.
            critical!(self.push(TypedValue::Word(self.program_counter)));
            // Push the IMR to stack.
            critical!(self.push(TypedValue::Half(self.imr)));
            // Disable all interrupts.
            self.imr = 0;
            // Jump to the interrupt handler.
            self.program_counter = tv_into_v!(critical!(
                self.load(interrupt * 4, false, ValueType::Word)));
        }

        // Fetch next instruction.
        let opcode: u8 = fetch!(Byte);
        // Decode and execute instruction.
        match opcode {
            0x00 => {  // HALT
                trace!("HALT");
                privileged!(self)?;
                return Ok(PostCycleAction::Halt);
            }
            0x01 => {  // PAUSE
                trace!("PAUSE");
                privileged!(self)?;
                return Ok(PostCycleAction::Pause);
            }
            0x02 => {  // TIMER literal
                trace!("TIMER literal");
                privileged!(self)?;
                let milliseconds = fetch!(Word);
                trace!("Timer milliseconds: {:#x}", milliseconds);
                self.timer.command_tx.as_ref().unwrap()
                    .send(TimerCommand::SetTimer(milliseconds)).unwrap();
            }
            0x03 => {  // TIMER ref
                trace!("TIMER ref");
                privileged!(self)?;
                let reg_ref = fetch!(Byte);
                let milliseconds = try_tv_into_v!(self.read_from_register(reg_ref)?);
                trace!("Timer milliseconds: {:#x}", milliseconds);
                self.timer.command_tx.as_ref().unwrap()
                    .send(TimerCommand::SetTimer(milliseconds)).unwrap();
            }
            0x04 => {  // USERMODE
                trace!("USERMODE");
                privileged!(self)?;
                // Get the target address.
                self.program_counter = tv_into_v!(self.pop(ValueType::Word)?);
                // Clear flags.
                self.flags = 0;
                // Enter user mode.
                self.kernel_mode = false;
            }
            0x05 => {  // IRETURN
                trace!("IRETURN");
                privileged!(self)?;
                // Pop the IMR off the stack.
                let imr: u16 = tv_into_v!(self.pop(ValueType::Half)?);
                // Pop the program counter off the stack.
                let pc: u32 = tv_into_v!(self.pop(ValueType::Word).map_err(|e| {
                    // Ensure this operation is atomic by undoing any changes.
                    // If the pop worked, push should too.
                    self.push(TypedValue::Half(imr))
                        .expect("Failed to clean up partially-failed IRETURN.");
                    e
                })?);
                // Pop the flags off the stack.
                let flags: u16 = tv_into_v!(self.pop(ValueType::Half).map_err(|e| {
                    // Ensure this operation is atomic by undoing any changes.
                    // If the pops worked, pushes should too.
                    self.push(TypedValue::Word(pc))
                        .expect("Failed to clean up partially-failed IRETURN.");
                    self.push(TypedValue::Half(imr))
                        .expect("Failed to clean up partially-failed IRETURN.");
                    e
                })?);
                // If bit 15 is 0, enter user mode.
                if (flags & 0b1000000000000000) == 0 {
                    self.kernel_mode = false;
                }
                // Set the registers.
                self.imr = imr;
                self.program_counter = pc;
                self.flags = flags & 0b0111111111111111;
            }
            0x06 => {  // LOAD ref literal
                trace!("LOAD ref literal");
                let reg_ref_dest = fetch!(Byte);
                let literal_address = fetch!(Word);
                trace!("Dest: {:#x} Address: {:#x}", reg_ref_dest, literal_address);
                self.instruction_load(reg_ref_dest, literal_address)?;
            }
            0x07 => {  // LOAD ref ref
                trace!("LOAD ref");
                let reg_ref_dest = fetch!(Byte);
                let reg_ref_address = fetch!(Byte);
                let address = try_tv_into_v!(self.read_from_register(reg_ref_address)?);
                trace!("Dest: {:#x} Address: {:#x}", reg_ref_dest, address);
                self.instruction_load(reg_ref_dest, address)?;
            }
            0x08 => {  // STORE literal ref
                trace!("STORE literal ref");
                let literal_address = fetch!(Word);
                let reg_ref_source = fetch!(Byte);
                trace!("Address: {:#x} Source: {:#x}", literal_address, reg_ref_source);
                self.instruction_store(literal_address, reg_ref_source)?;
            }
            0x09 => {  // STORE ref ref
                trace!("STORE ref ref");
                let reg_ref_address = fetch!(Byte);
                let reg_ref_source = fetch!(Byte);
                let address = try_tv_into_v!(self.read_from_register(reg_ref_address)?);
                trace!("Address: {:#x} Source: {:#x}", address, reg_ref_source);
                self.instruction_store(address, reg_ref_source)?;
            }
            0x0A => {  // COPY ref literal
                trace!("COPY ref literal");
                let reg_ref_dest = fetch!(Byte);
                trace!("into {:#x}", reg_ref_dest);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref_dest)?);
                trace!("{:?}", value);
                self.write_to_register(reg_ref_dest, value)?;
            }
            0x0B => {  // COPY ref ref
                trace!("COPY ref ref");
                let reg_ref_dest = fetch!(Byte);
                let reg_ref_source = fetch!(Byte);
                check_same_type!(reg_ref_dest, reg_ref_source);
                trace!("from {:#x} to {:#x}", reg_ref_source, reg_ref_dest);
                let value = self.read_from_register(reg_ref_source)?;
                self.write_to_register(reg_ref_dest, value)?;
            }
            0x0C => {  // SWAP ref literal
                trace!("SWAP ref literal");
                let reg_ref = fetch!(Byte);
                let address = fetch!(Word);
                trace!("register {:#x} with address {:#x}", reg_ref, address);
                self.instruction_swap(reg_ref, address)?;
            }
            0x0D => {  // SWAP ref ref
                trace!("SWAP ref ref");
                let reg_ref = fetch!(Byte);
                let address_ref = fetch!(Byte);
                let address = try_tv_into_v!(self.read_from_register(address_ref)?);
                trace!("register {:#x} with address {:#x}", reg_ref, address);
                self.instruction_swap(reg_ref, address)?;
            }
            0x0E => {  // PUSH
                trace!("PUSH");
                let reg_ref = fetch!(Byte);
                trace!("Register: {:#x}", reg_ref);
                let value = self.read_from_register(reg_ref)?;
                self.push(value)?;
            }
            0x0F => {  // POP
                trace!("POP");
                let reg_ref = fetch!(Byte);
                trace!("Register: {:#x}", reg_ref);
                let value = self.pop(self.reg_ref_type(reg_ref)?)?;
                self.write_to_register(reg_ref, value)?;
            }
            0x10 => {  // BLOCKCOPY literal literal literal
                trace!("BLOCKCOPY literal literal literal");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let source_address = fetch!(Word);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x11 => {  // BLOCKCOPY literal literal ref
                trace!("BLOCKCOPY literal literal ref");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let source_address_ref = fetch!(Byte);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x12 => {  // BLOCKCOPY literal ref literal
                trace!("BLOCKCOPY literal ref literal");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let source_address = fetch!(Word);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x13 => {  // BLOCKCOPY literal ref ref
                trace!("BLOCKCOPY literal ref ref");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let source_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x14 => {  // BLOCKCOPY ref literal literal
                trace!("BLOCKCOPY ref literal literal");
                let length_ref = fetch!(Byte);
                let dest_address = fetch!(Word);
                let source_address = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x15 => {  // BLOCKCOPY ref literal ref
                trace!("BLOCKCOPY ref literal ref");
                let length_ref = fetch!(Byte);
                let dest_address = fetch!(Word);
                let source_address_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x16 => {  // BLOCKCOPY ref ref literal
                trace!("BLOCKCOPY ref ref literal");
                let length_ref = fetch!(Byte);
                let dest_address_ref = fetch!(Byte);
                let source_address = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x17 => {  // BLOCKCOPY ref ref ref
                trace!("BLOCKCOPY ref ref ref");
                let length_ref = fetch!(Byte);
                let dest_address_ref = fetch!(Byte);
                let source_address_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let source_address = try_tv_into_v!(self.read_from_register(source_address_ref)?);
                trace!("{} bytes from {:#x} to {:#x}", length, source_address, dest_address);
                self.instruction_blockcopy(length, dest_address, source_address)?;
            }
            0x18 => {  // BLOCKSET literal literal literal
                trace!("BLOCKSET literal literal literal");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let value = fetch!(Byte);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x19 => {  // BLOCKSET literal literal ref
                trace!("BLOCKSET literal literal ref");
                let length = fetch!(Word);
                let dest_address = fetch!(Word);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1A => {  // BLOCKSET literal ref literal
                trace!("BLOCKSET literal ref literal");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value = fetch!(Byte);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1B => {  // BLOCKSET literal ref ref
                trace!("BLOCKSET literal ref ref");
                let length = fetch!(Word);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1C => {  // BLOCKSET ref literal literal
                trace!("BLOCKSET ref literal literal");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = fetch!(Word);
                let value = fetch!(Byte);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1D => {  // BLOCKSET ref literal ref
                trace!("BLOCKSET ref literal ref");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address = fetch!(Word);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1E => {  // BLOCKSET ref ref literal
                trace!("BLOCKSET ref ref literal");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value = fetch!(Byte);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x1F => {  // BLOCKSET ref ref ref
                trace!("BLOCKSET ref ref ref");
                let length_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let dest_address_ref = fetch!(Byte);
                let dest_address = try_tv_into_v!(self.read_from_register(dest_address_ref)?);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("{} bytes of {:#x} into {:#x}", length, value, dest_address);
                self.instruction_blockset(length, dest_address, value)?;
            }
            0x20 => {  // NEGATE
                trace!("NEGATE");
                let reg_ref = fetch!(Byte);
                trace!("Negating register {:#x}", reg_ref);
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
                trace!("ADD literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Adding {:?} to register {:#x}", value, reg_ref);
                self.instruction_add(reg_ref, value)?;
            }
            0x22 => {  // ADD ref
                trace!("ADD ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Adding {:?} to register {:#x}", value, dest);
                self.instruction_add(dest, value)?;
            }
            0x23 => {  // ADDCARRY literal
                trace!("ADDCARRY literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Adding {:?} to register {:#x} with carry={}", value, reg_ref,
                    self.flags & FLAG_CARRY > 0);
                self.instruction_addcarry(reg_ref, value)?;
            }
            0x24 => {  // ADDCARRY ref
                trace!("ADDCARRY ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Adding {:?} to register {:#x} with carry={}", value, dest,
                    self.flags & FLAG_CARRY > 0);
                self.instruction_addcarry(dest, value)?;
            }
            0x25 => {  // SUB literal
                trace!("SUB literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Subtracting {:?} from register {:#x}", value, reg_ref);
                self.instruction_sub(reg_ref, value)?;
            }
            0x26 => {  // SUB ref
                trace!("SUB ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Subtracting {:?} from register {:#x}", value, dest);
                self.instruction_sub(dest, value)?;
            }
            0x27 => {  // SUBBORROW literal
                trace!("SUBBORROW literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Subtracting {:?} from register {:#x} with carry={}", value, reg_ref,
                    self.flags & FLAG_CARRY > 0);
                self.instruction_subborrow(reg_ref, value)?;
            }
            0x28 => {  // SUBBORROW ref
                trace!("SUBBORROW ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Subtracting {:?} from register {:#x} with carry={}", value, dest,
                    self.flags & FLAG_CARRY > 0);
                self.instruction_subborrow(dest, value)?;
            }
            0x29 => {  // MULT literal
                trace!("MULT literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Multiplying register {:#x} by {:?}", reg_ref, value);
                self.instruction_mult(reg_ref, value)?;
            }
            0x2A => {  // MULT ref
                trace!("MULT ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Multiplying register {:#x} by {:?}", dest, value);
                self.instruction_mult(dest, value)?;
            }
            0x2B => {  // SDIV literal
                trace!("SDIV literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Signed dividing register {:#x} by {:?}", reg_ref, value);
                self.instruction_sdiv(reg_ref, value)?;
            }
            0x2C => {  // SDIV ref
                trace!("SDIV ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Signed dividing register {:#x} by {:?}", dest, value);
                self.instruction_sdiv(dest, value)?;
            }
            0x2D => {  // UDIV literal
                trace!("UDIV literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Unsigned dividing register {:#x} by {:?}", reg_ref, value);
                self.instruction_udiv(reg_ref, value)?;
            }
            0x2E => {  // UDIV ref
                trace!("UDIV ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Unsigned dividing register {:#x} by {:?}", dest, value);
                self.instruction_udiv(dest, value)?;
            }
            0x2F => {  // SREM literal
                trace!("SREM literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Signed remainder register {:#x} by {:?}", reg_ref, value);
                self.instruction_srem(reg_ref, value)?;
            }
            0x30 => {  // SREM ref
                trace!("SREM ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Signed remainder register {:#x} by {:?}", dest, value);
                self.instruction_srem(dest, value)?;
            }
            0x31 => {  // UREM literal
                trace!("UREM literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Unsigned remainder register {:#x} by {:?}", reg_ref, value);
                self.instruction_urem(reg_ref, value)?;
            }
            0x32 => {  // UREM ref
                trace!("UREM ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Unsigned remainder register {:#x} by {:?}", dest, value);
                self.instruction_urem(dest, value)?;
            }
            0x33 => {  // NOT
                trace!("NOT");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                trace!("Negating register {:#x}", reg_ref);
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
                trace!("AND literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Bitwise AND register {:#x} by {:?}", reg_ref, value);
                self.instruction_and(reg_ref, value)?;
            }
            0x35 => {  // AND ref
                trace!("AND ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Bitwise AND register {:#x} by {:?}", dest, value);
                self.instruction_and(dest, value)?;
            }
            0x36 => {  // OR literal
                trace!("OR literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Bitwise OR register {:#x} by {:?}", reg_ref, value);
                self.instruction_or(reg_ref, value)?;
            }
            0x37 => {  // OR ref
                trace!("OR ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Bitwise OR register {:#x} by {:?}", dest, value);
                self.instruction_or(dest, value)?;
            }
            0x38 => {  // XOR literal
                trace!("XOR literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Bitwise XOR register {:#x} by {:?}", reg_ref, value);
                self.instruction_xor(reg_ref, value)?;
            }
            0x39 => {  // XOR ref
                trace!("XOR ref");
                let dest = fetch!(Byte);
                reject_float!(dest);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Bitwise XOR register {:#x} by {:?}", dest, value);
                self.instruction_xor(dest, value)?;
            }
            0x3A => {  // LSHIFT literal
                trace!("LSHIFT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                trace!("Left shift register {:#x} by {}", reg_ref, value);
                self.instruction_lshift(reg_ref, value)?;
            }
            0x3B => {  // LSHIFT ref
                trace!("LSHIFT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("Left shift register {:#x} by {}", reg_ref, value);
                self.instruction_lshift(reg_ref, value)?;
            }
            0x3C => {  // SRSHIFT literal
                trace!("SRSHIFT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                trace!("Signed right shift register {:#x} by {}", reg_ref, value);
                self.instruction_srshift(reg_ref, value)?;
            }
            0x3D => {  // SRSHIFT ref
                trace!("SRSHIFT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("Signed right shift register {:#x} by {}", reg_ref, value);
                self.instruction_srshift(reg_ref, value)?;
            }
            0x3E => {  // URSHIFT literal
                trace!("URSHIFT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                trace!("Unsigned right shift register {:#x} by {}", reg_ref, value);
                self.instruction_urshift(reg_ref, value)?;
            }
            0x3F => {  // URSHIFT ref
                trace!("URSHIFT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("Unsigned right shift register {:#x} by {}", reg_ref, value);
                self.instruction_urshift(reg_ref, value)?;
            }
            0x40 => {  // LROT literal
                trace!("LROT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                trace!("Left rotate register {:#x} by {}", reg_ref, value);
                self.instruction_lrot(reg_ref, value)?;
            }
            0x41 => {  // LROT ref
                trace!("LROT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("Left rotate register {:#x} by {}", reg_ref, value);
                self.instruction_lrot(reg_ref, value)?;
            }
            0x42 => {  // RROT literal
                trace!("RROT literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                trace!("Right rotate register {:#x} by {}", reg_ref, value);
                self.instruction_rrot(reg_ref, value)?;
            }
            0x43 => {  // RROT ref
                trace!("RROT ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("Right rotate register {:#x} by {}", reg_ref, value);
                self.instruction_rrot(reg_ref, value)?;
            }
            0x44 => {  // LROTCARRY literal
                trace!("LROTCARRY literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                trace!("Left rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_lrotcarry(reg_ref, value)?;
            }
            0x45 => {  // LROTCARRY ref
                trace!("LROTCARRY ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("Left rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_lrotcarry(reg_ref, value)?;
            }
            0x46 => {  // RROTCARRY literal
                trace!("RROTCARRY literal");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value = fetch!(Byte);
                trace!("Right rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_rrotcarry(reg_ref, value)?;
            }
            0x47 => {  // RROTCARRY ref
                trace!("RROTCARRY ref");
                let reg_ref = fetch!(Byte);
                reject_float!(reg_ref);
                let value_ref = fetch!(Byte);
                let value = try_tv_into_v!(self.read_from_register(value_ref)?);
                trace!("Right rotate register with carry {:#x} by {}", reg_ref, value);
                self.instruction_rrotcarry(reg_ref, value)?;
            }
            0x48 => {  // JUMP literal
                trace!("JUMP literal");
                let address = fetch!(Word);
                trace!("Jumping to {:#x}", address);
                self.program_counter = address;
            }
            0x49 => {  // JUMP ref
                trace!("JUMP ref");
                let reg_ref = fetch!(Byte);
                trace!("Jumping to address in {:#x}", reg_ref);
                let address = try_tv_into_v!(self.read_from_register(reg_ref)?);
                trace!("Jumping to {:#x}", address);
                self.program_counter = address;
            }
            0x4A => {  // COMPARE literal
                trace!("COMPARE literal");
                let reg_ref = fetch!(Byte);
                let value = fetch_variable_size!(self.reg_ref_type(reg_ref)?);
                trace!("Comparing register {:#x} with {:?}", reg_ref, value);
                self.instruction_compare(reg_ref, value)?;
            }
            0x4B => {  // COMPARE ref
                trace!("COMPARE ref");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                check_same_type!(dest, src);
                let value = self.read_from_register(src)?;
                trace!("Comparing register {:#x} with {:?}", dest, value);
                self.instruction_compare(dest, value)?;
            }
            0x4C => {  // BLOCKCMP literal literal literal
                trace!("BLOCKCMP literal literal literal");
                let length = fetch!(Word);
                let source1 = fetch!(Word);
                let source2 = fetch!(Word);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x4D => {  // BLOCKCMP literal literal ref
                trace!("BLOCKCMP literal literal ref");
                let length = fetch!(Word);
                let source1 = fetch!(Word);
                let source2_ref = fetch!(Byte);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x4E => {  // BLOCKCMP literal ref literal
                trace!("BLOCKCMP literal ref literal");
                let length = fetch!(Word);
                let source1_ref = fetch!(Byte);
                let source2 = fetch!(Word);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x4F => {  // BLOCKCMP literal ref ref
                trace!("BLOCKCMP literal ref ref");
                let length = fetch!(Word);
                let source1_ref = fetch!(Byte);
                let source2_ref = fetch!(Byte);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x50 => {  // BLOCKCMP ref literal literal
                trace!("BLOCKCMP ref literal literal");
                let length_ref = fetch!(Byte);
                let source1 = fetch!(Word);
                let source2 = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x51 => {  // BLOCKCMP ref literal ref
                trace!("BLOCKCMP ref literal ref");
                let length_ref = fetch!(Byte);
                let source1 = fetch!(Word);
                let source2_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x52 => {  // BLOCKCMP ref ref literal
                trace!("BLOCKCMP ref ref literal");
                let length_ref = fetch!(Byte);
                let source1_ref = fetch!(Byte);
                let source2 = fetch!(Word);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
                self.instruction_blockcmp(length, source1, source2)?;
            }
            0x53 => {  // BLOCKCMP ref ref ref
                trace!("BLOCKCMP ref ref ref");
                let length_ref = fetch!(Byte);
                let source1_ref = fetch!(Byte);
                let source2_ref = fetch!(Byte);
                let length = try_tv_into_v!(self.read_from_register(length_ref)?);
                let source1 = try_tv_into_v!(self.read_from_register(source1_ref)?);
                let source2 = try_tv_into_v!(self.read_from_register(source2_ref)?);
                trace!("Comparing {} bytes at {:#x} and {:#x}", length, source1, source2);
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
                trace!("CALL literal");
                let address = fetch!(Word);
                trace!("Calling {:#x}", address);
                self.instruction_call(address)?;
            }
            0x69 => {  // CALL ref
                trace!("CALL ref");
                let reg_ref = fetch!(Byte);
                trace!("Calling address in {:#x}", reg_ref);
                let address = try_tv_into_v!(self.read_from_register(reg_ref)?);
                trace!("Calling {:#x}", address);
                self.instruction_call(address)?;
            }
            0x6A => {  // RETURN
                trace!("RETURN");
                let address: u32 = tv_into_v!(self.pop(ValueType::Word)?);
                self.program_counter = address;
            }
            0x6B => {  // SYSCALL
                trace!("SYSCALL");
                self.interrupt_tx.send(INTERRUPT_SYSCALL).unwrap();
            }
            0x6C => {  // SCONVERT
                trace!("SCONVERT");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                let dest_type = self.reg_ref_type(dest)?;
                let src_type = self.reg_ref_type(src)?;
                trace!("Signed conversion from {:#x} to {:#x}", src, dest);
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
                    return Err(CPUError::TryAgainError);
                }
            }
            0x6D => {  // UCONVERT
                trace!("UCONVERT");
                let dest = fetch!(Byte);
                let src = fetch!(Byte);
                let dest_type = self.reg_ref_type(dest)?;
                let src_type = self.reg_ref_type(src)?;
                trace!("Unsigned conversion from {:#x} to {:#x}", src, dest);
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
                    return Err(CPUError::TryAgainError);
                }
            }
            _ => {  // Unrecognised
                trace!("Unrecognised opcode: {:#x}", opcode);
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
            value.increment();
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
            value.increment();
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
            return Err(CPUError::TryAgainError);
        }
        bin_op_signed!(self, reg_ref, value, overflowing_div, div)
    }

    fn instruction_udiv(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if value.is_integer_zero() {
            self.interrupt_tx.send(INTERRUPT_DIV_BY_0).unwrap();
            return Err(CPUError::TryAgainError);
        }
        bin_op_unsigned!(self, reg_ref, value, overflowing_div)
    }

    fn instruction_srem(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if value.is_integer_zero() {
            self.interrupt_tx.send(INTERRUPT_DIV_BY_0).unwrap();
            return Err(CPUError::TryAgainError);
        }
        bin_op_signed!(self, reg_ref, value, overflowing_rem, rem)
    }

    fn instruction_urem(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        if value.is_integer_zero() {
            self.interrupt_tx.send(INTERRUPT_DIV_BY_0).unwrap();
            return Err(CPUError::TryAgainError);
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
                let y = u8::try_from(value).unwrap();
                let u_ans = x.overflowing_sub(y);
                let s_ans = (x as i8).overflowing_sub(y as i8);
                debug_assert_eq!(u_ans.0, s_ans.0 as u8);
                make_flags_int!(s_ans.0, u_ans.1, s_ans.1)
            },
            TypedValue::Half(x) => {
                let y = u16::try_from(value).unwrap();
                let u_ans = x.overflowing_sub(y);
                let s_ans = (x as i16).overflowing_sub(y as i16);
                debug_assert_eq!(u_ans.0, s_ans.0 as u16);
                make_flags_int!(s_ans.0, u_ans.1, s_ans.1)
            },
            TypedValue::Word(x) => {
                let y = u32::try_from(value).unwrap();
                let u_ans = x.overflowing_sub(y);
                let s_ans = (x as i32).overflowing_sub(y as i32);
                debug_assert_eq!(u_ans.0, s_ans.0 as u32);
                make_flags_int!(s_ans.0, u_ans.1, s_ans.1)
            },
            TypedValue::Float(x) => {
                let y = f32::try_from(value).unwrap();
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
        self.program_counter = address;
        Ok(())
    }

    /// Get the type of a register from its reference.
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
            trace!("Invalid register reference: {:#x}.", reg_ref);
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            Err(CPUError::TryAgainError)
        }
    }

    fn write_to_register(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        if reg_ref < 0x08 {  // r0-7
            if let TypedValue::Word(w) = value {
                self.r[reg_ref as usize] = w;
                return Ok(());
            }
        } else if reg_ref < 0x10 {  // r0h-r7h
            if let TypedValue::Half(h) = value {
                let index = (reg_ref - 0x08) as usize;
                let masked = self.r[index] & 0xFFFF0000;
                self.r[index] = masked | (h as u32);
                return Ok(());
            }
        } else if reg_ref < 0x18 {  // r0b-r7b
            if let TypedValue::Byte(b) = value {
                let index = (reg_ref - 0x10) as usize;
                let masked = self.r[index] & 0xFFFFFF00;
                self.r[index] = masked | (b as u32);
                return Ok(());
            }
        } else if reg_ref < 0x20 {  // f0-f7
            if let TypedValue::Float(f) = value {
                let index = (reg_ref - 0x18) as usize;
                self.f[index] = f;
                return Ok(());
            }
        } else if reg_ref == 0x20 {  // FLAGS
            if let TypedValue::Half(h) = value {
                let masked = h & 0b0111111111111111;  // Ignore bit 15.
                self.flags = masked;
                return Ok(());
            }
        } else if reg_ref == 0x21 {  // USPR
            if let TypedValue::Word(w) = value {
                self.uspr = w;
                return Ok(());
            }
        } else if reg_ref == 0x22 {  // KSPR
            if let TypedValue::Word(w) = value {
                privileged!(self)?;
                self.kspr = w;
                return Ok(());
            }
        } else if reg_ref == 0x23 {  // PDPR
            if let TypedValue::Word(w) = value {
                privileged!(self)?;
                self.pdpr = w;
                return Ok(());
            }
        } else if reg_ref == 0x24 {  // IMR
            if let TypedValue::Half(h) = value {
                privileged!(self)?;
                self.imr = h;
                return Ok(());
            }
        } else if reg_ref == 0x25 {  // PFSR
            trace!("Illegal write to PFSR.");
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            return Err(CPUError::TryAgainError);
        } else {
            trace!("Invalid register reference: {:#x}", reg_ref);
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            return Err(CPUError::TryAgainError);
        };
        trace!("Register size mismatch: register {:#x} with size {}",
            reg_ref, value.size_in_bytes());
        self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
        return Err(CPUError::TryAgainError);
    }

    /// Read the value of the given register reference.
    fn read_from_register(&mut self, reg_ref: u8) -> CPUResult<TypedValue> {
        if reg_ref < 0x08 {  // r0-r7
            Ok(TypedValue::Word(self.r[reg_ref as usize]))
        } else if reg_ref < 0x10 {  // r0h-r7h
            Ok(TypedValue::Half(self.r[(reg_ref - 0x08) as usize] as u16))
        } else if reg_ref < 0x18 {  // r0b-r7b
            Ok(TypedValue::Byte(self.r[(reg_ref - 0x10) as usize] as u8))
        } else if reg_ref < 0x20 {  // f0-f7
            Ok(TypedValue::Float(self.f[(reg_ref - 0x18) as usize]))
        } else if reg_ref == 0x20 {  // FLAGS
            Ok(TypedValue::Half(self.flags))
        } else if reg_ref == 0x21 {  // USPR
            Ok(TypedValue::Word(self.uspr))
        } else if reg_ref == 0x22 {  // KSPR
            privileged!(self)?;
            Ok(TypedValue::Word(self.kspr))
        } else if reg_ref == 0x23 {  // PDPR
            privileged!(self)?;
            Ok(TypedValue::Word(self.pdpr))
        } else if reg_ref == 0x24 {  // IMR
            privileged!(self)?;
            Ok(TypedValue::Half(self.imr))
        } else if reg_ref == 0x25 {  // PFSR
            privileged!(self)?;
            Ok(TypedValue::Word(self.mmu.page_fault_status_register()))
        } else {
            trace!("Invalid register reference: {:#x}", reg_ref);
            self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            Err(CPUError::TryAgainError)
        }
    }

    /// Store the given value to the given memory address.
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

    /// Load a value of the given size from the given memory address.
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

    /// Push the given value to the stack.
    fn push(&mut self, value: TypedValue) -> CPUResult<()> {
        if self.kernel_mode {
            self.kspr = self.kspr.wrapping_sub(value.size_in_bytes());
            self.store(self.kspr, value)
        } else {
            self.uspr = self.uspr.wrapping_sub(value.size_in_bytes());
            self.store(self.uspr, value)
        }
    }

    /// Pop the given value from the stack.
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
