use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::disk::DiskController;
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
        match self {
            TypedValue::Byte(_) => 1,
            TypedValue::Half(_) => 2,
            TypedValue::Word(_) => 4,
            TypedValue::Float(_) => 4,
        }
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

// A macro for performing a privilege check.
macro_rules! privileged {
    ($self:ident) => {{
        if !$self.kernel_mode {
            $self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            Err(CPUError)
        } else {
            Ok(())
        }
    }}
}

// A macro for printing only in debug mode.
macro_rules! debug {
    ($($x:expr),*) => {{
        #[cfg(debug_assertions)]
        println!($($x),*);
    }}
}

// A macro for making flags out of an arithmetic operation.
macro_rules! make_flags {
    ($x:expr, $y:expr, $ans:expr, $left_bit:expr, $carry:expr) => {{
        let mut flags: u16 = 0;
        if $ans == 0 {
            flags |= FLAG_ZERO;
        } else if $ans & $left_bit != 0 {
            flags |= FLAG_NEGATIVE;
        }
        if $carry {
            flags |= FLAG_CARRY;
        }
        if (($x & $left_bit == 0) && ($y & $left_bit == 0) && ($ans & $left_bit != 0))
          || (($x & $left_bit != 0) && ($y & $left_bit != 0) && ($ans & $left_bit == 0)) {
            flags |= FLAG_OVERFLOW;
        }
        flags
    }}
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
        let flags: u16;
        match self.read_from_register(reg_ref)? {
            TypedValue::Byte(x) => {
                let y = Into::<Option<u8>>::into(value).unwrap();
                let ans= x.overflowing_add(y);
                self.write_to_register(reg_ref, TypedValue::Byte(ans.0))?;
                flags = make_flags!(x, y, ans.0, 0x80, ans.1);
            },
            TypedValue::Half(x) => {
                let y = Into::<Option<u16>>::into(value).unwrap();
                let ans = x.overflowing_add(y);
                self.write_to_register(reg_ref, TypedValue::Half(ans.0))?;
                flags = make_flags!(x, y, ans.0, 0x8000, ans.1);
            },
            TypedValue::Word(x) => {
                let y = Into::<Option<u32>>::into(value).unwrap();
                let ans= x.overflowing_add(y);
                self.write_to_register(reg_ref, TypedValue::Word(ans.0))?;
                flags = make_flags!(x, y, ans.0, 0x80000000, ans.1);
            },
            TypedValue::Float(x) => {
                let y = Into::<Option<f32>>::into(value).unwrap();
                let ans = x + y;
                self.write_to_register(reg_ref, TypedValue::Float(ans))?;
                flags = if ans == 0.0 {FLAG_ZERO} else if ans < 0.0 {FLAG_NEGATIVE} else {0};
            },
        }
        self.flags = flags;
        Ok(())
    }

    fn instruction_addcarry(&mut self, reg_ref: u8, value: TypedValue) -> CPUResult<()> {
        // We assume that the value has already been checked to match the register type.
        let carry: u32 = if self.flags & FLAG_CARRY != 0 {1} else {0};
        let flags: u16;
        match self.read_from_register(reg_ref)? {
            TypedValue::Byte(x) => {
                let y = Into::<Option<u8>>::into(value).unwrap();
                let (ans, c1) = x.overflowing_add(y);
                let (ans, c2) = ans.overflowing_add(carry as u8);
                self.write_to_register(reg_ref, TypedValue::Byte(ans))?;
                flags = make_flags!(x, y, ans, 0x80, c1 || c2);
            },
            TypedValue::Half(x) => {
                let y = Into::<Option<u16>>::into(value).unwrap();
                let (ans, c1) = x.overflowing_add(y);
                let (ans, c2) = ans.overflowing_add(carry as u16);
                self.write_to_register(reg_ref, TypedValue::Half(ans))?;
                flags = make_flags!(x, y, ans, 0x8000, c1 || c2);
            },
            TypedValue::Word(x) => {
                let y = Into::<Option<u32>>::into(value).unwrap();
                let (ans, c1) = x.overflowing_add(y);
                let (ans, c2) = ans.overflowing_add(carry);
                self.write_to_register(reg_ref, TypedValue::Word(ans))?;
                flags = make_flags!(x, y, ans, 0x80000000, c1 || c2);
            },
            TypedValue::Float(_) => {
                panic!("BUG: instruction_addcarry was called with a float.");
            },
        }
        self.flags = flags;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    use ntest::timeout;

    use crate::{disk::MockDiskController, display::DisplayController,
                keyboard::{KeyboardController, KeyMessage, key_str_to_u8},
                ram::RAM, rom::ROM, ui::UICommand};

    fn run(rom_data: [u8; 512], keypress: Option<KeyMessage>) -> (CPU<MockDiskController>, Vec<UICommand>) {
        // Create communication channels.
        let (interrupt_tx, interrupt_rx) = mpsc::channel();
        let interrupt_tx_keyboard = interrupt_tx.clone();
        let interrupt_tx_mmu = interrupt_tx.clone();
        let (ui_tx, ui_rx) = mpsc::channel();
        let ui_tx_display = ui_tx.clone();
        let (keyboard_tx, keyboard_rx) = mpsc::channel();
        let keyboard_tx_manual = keyboard_tx.clone();

        // Create components.
        let disk_a = MockDiskController;
        let disk_b = MockDiskController;
        let display = DisplayController::new(ui_tx_display);
        let keyboard = KeyboardController::new(
            keyboard_tx, keyboard_rx, interrupt_tx_keyboard);
        let ram = RAM::new();
        let rom = ROM::new(rom_data);
        let mmu = MMU::new(interrupt_tx_mmu, disk_a, disk_b,
                           display, keyboard, ram, rom);
        let mut cpu = CPU::new(ui_tx, mmu, interrupt_tx, interrupt_rx);

        // Run the CPU till halt.
        cpu.start();
        if let Some(message) = keypress {
            keyboard_tx_manual.send(message).unwrap();
        }
        cpu.wait_for_halt();
        // Collect any resulting UI commands.
        let ui_commands = ui_rx.try_iter().collect();
        return (cpu, ui_commands);
    }

    macro_rules! internal {
        ($cpu:ident) => { $cpu.internal.as_ref().unwrap() }
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
        assert_eq!(internal!(cpu).r[3], 0x42069696);
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

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[3], 0x13579BDF);
        assert_eq!(internal!(cpu).r[0], 0x00009BDF);
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
        assert_eq!(internal!(cpu).mmu.load_physical_32(0x00004ABC), Ok(0x12345678));
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
        assert_eq!(internal!(cpu).mmu.load_physical_32(0x00004ABC), Ok(0xABCDEF00));
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
        assert_eq!(internal!(cpu).r[7], 0xFFFFFF55);
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
        assert_eq!(internal!(cpu).r[6], 0x0000FF34);
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
        assert_eq!(internal!(cpu).r[0], 0x00000000);
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x00004000), Ok(0x66));
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
        assert_eq!(internal!(cpu).r[0], 0x00000000);
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x00005000), Ok(0x77));
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
        assert_eq!(internal!(cpu).r[1], 0x0000AAFF);
        assert_eq!(internal!(cpu).kspr, 0x00007FFF);
        assert_eq!(internal!(cpu).mmu.load_physical_32(0x00007FFC), Ok(0x00AAFFFF));
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
        assert_eq!(internal!(cpu).uspr, 0x00000063);
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x00004063), Ok(0x99));
    }

    #[test]
    #[timeout(1000)]
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
        let (cpu, ui_commands) = run(rom,
                                     Some(KeyMessage::Key(KEY, false, false).unwrap()));
        assert_eq!(ui_commands.len(), 2);

        // Assert that the key was correctly detected.
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x19B0), Ok(key_str_to_u8(KEY).unwrap()));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x19B1), Ok(0));
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

        // TODO This test has a race condition and is flaky, but it can't be fixed
        // TODO until I have instruction support for busy-wait polling.

        // Interrupt handler.
        rom[128] = 0x0A; // Copy literal
        rom[129] = 0x0D; // into r5h
        rom[130] = 0x55;
        rom[131] = 0x66; // some random number.

        rom[132] = 0x05; // IRETURN.

        const KEY: &str = "Escape";
        let (cpu, ui_commands) = run(rom,
                                     Some(KeyMessage::Key(KEY, false, false).unwrap()));
        assert_eq!(ui_commands.len(), 2);

        // Assert that the interrupt handler ran and returned.
        assert_eq!(internal!(cpu).r[5], 0x00005566);
        assert_eq!(internal!(cpu).r[6], 0x00000011);
    }

    #[test]
    #[timeout(1000)]
    fn test_bad_reg_ref() {
        let mut rom = [0; 512];
        // Try a copy with unmatched sizes. Should raise an interrupt.
        rom[0] = 0x0A; // Copy literal
        rom[1] = 0x22; // into kspr
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x50;
        rom[5] = 0x00; // address 0x00005000.

        rom[6] = 0x0A; // Copy literal
        rom[7] = 0x24; // into imr
        rom[8] = 0x00;
        rom[9] = 0x40; // illegal operation interrupt only.

        rom[10] = 0x0B; // Copy register
        rom[11] = 0x01; // into r1
        rom[12] = 0x0B; // from r3h.

        rom[13] = 0x01; // Pause. We should never hit this.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[1], 0x00000000);
    }

    #[test]
    #[timeout(1000)]
    fn test_pfsr() {
        let mut rom = [0; 512];
        // Set the page directory pointer.
        rom[0] = 0x0A; // Copy literal
        rom[1] = 0x23; // into pdpr
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x00;
        rom[5] = 0xC0; // ROM byte 0x80 (128).

        // Write the page table entry.
        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x00;  // into r0
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x50;
        rom[11] = 0x13; // Valid, Present, Execute entry at 0x00005000.

        rom[12] = 0x08; // Store into
        rom[13] = 0x00;
        rom[14] = 0x00;
        rom[15] = 0xB0;
        rom[16] = 0x00; // address 0x0000B000
        rom[17] = 0x00; // r0.

        // Write the user mode instructions to RAM.

        // (Load an execute-only address, which is not allowed).
        rom[18] = 0x0A; // Copy literal
        rom[19] = 0x08; // into r0h
        rom[20] = 0x06; // LOAD instruction
        rom[21] = 0x17; // into r7b.

        rom[22] = 0x08; // Store into
        rom[23] = 0x00;
        rom[24] = 0x00;
        rom[25] = 0x50;
        rom[26] = 0x00; // address 0x00005000
        rom[27] = 0x08; // r0h.

        rom[28] = 0x0A; // Copy literal
        rom[29] = 0x00; // into r0
        rom[30] = 0x00;
        rom[31] = 0x00;
        rom[32] = 0x00;
        rom[33] = 0x04; // virtual address 0x00000004.

        rom[34] = 0x08; // Store into
        rom[35] = 0x00;
        rom[36] = 0x00;
        rom[37] = 0x50;
        rom[38] = 0x02; // address 0x00005002
        rom[39] = 0x00; // r0.

        // (Pause, which should never be hit).
        rom[40] = 0x0A; // Copy literal
        rom[41] = 0x10; // into r0b
        rom[42] = 0x01; // PAUSE instruction.

        rom[43] = 0x08; // Store into
        rom[44] = 0x00;
        rom[45] = 0x00;
        rom[46] = 0x50;
        rom[47] = 0x06; // address 0x00005006.
        rom[48] = 0x10; // r0b.

        // Set the kernel stack pointer.
        rom[49] = 0x0A; // Copy literal
        rom[50] = 0x22; // into kspr
        rom[51] = 0x00;
        rom[52] = 0x00;
        rom[53] = 0xA0;
        rom[54] = 0x00; // address 0x0000A000.

        // Enable page fault interrupts.
        rom[55] = 0x0A; // Copy literal
        rom[56] = 0x24; // into imr
        rom[57] = 0x00;
        rom[58] = 0x10; // page fault interrupt only.

        // Set page fault interrupt handler
        rom[59] = 0x0A; // Copy literal
        rom[60] = 0x00; // into r0
        rom[61] = 0x00;
        rom[62] = 0x00;
        rom[63] = 0x01;
        rom[64] = 0x40; // ROM byte 0x100 (256).

        rom[65] = 0x08; // Store into
        rom[66] = 0x00;
        rom[67] = 0x00;
        rom[68] = 0x00;
        rom[69] = 0x10; // address 0x00000010 (page fault handler).
        rom[70] = 0x00; // r0.

        // Push the user mode address to the stack.
        rom[71] = 0x0A; // Copy literal
        rom[72] = 0x00; // into r0
        rom[73] = 0x00;
        rom[74] = 0x00;
        rom[75] = 0x00;
        rom[76] = 0x00; // virtual address 0x0.

        rom[77] = 0x0E; // Push to the stack
        rom[78] = 0x00; // r0.

        // Enter user mode!
        rom[79] = 0x04;

        // Page directory entry.
        rom[128] = 0x00;
        rom[129] = 0x00;
        rom[130] = 0xB0;
        rom[131] = 0x01; // Valid entry at 0x0000B000.

        // Page fault handler.
        rom[256] = 0x0B; // Copy between registers
        rom[257] = 0x05; // into r5
        rom[258] = 0x25; // from pfsr.

        rom[259] = 0x00; // Halt.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        // Assert the user mode process stored in its stack correctly.
        assert_eq!(internal!(cpu).r[5], internal!(cpu).mmu.page_fault_status_register());
        assert_eq!(internal!(cpu).r[5], crate::mmu::PAGE_FAULT_ILLEGAL_ACCESS);
    }

    #[test]
    #[timeout(1000)]
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
    #[timeout(1000)]
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

    #[test]
    fn test_blockcopy() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x00;  // into r0
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x40;
        rom[5] = 0x00;  // address 0x00004000.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x01;  // into r1
        rom[8] = 0x00;
        rom[9] = 0x00;
        rom[10] = 0x00;
        rom[11] = 0x80; // ROM byte 0x40 (64).

        rom[12] = 0x13; // Blockcopy literal length, ref source and dest
        rom[13] = 0x00;
        rom[14] = 0x00;
        rom[15] = 0x00;
        rom[16] = 0x40; // 64 bytes
        rom[17] = 0x00; // into address in r0
        rom[18] = 0x01; // from address in r1.

        // DATA
        rom[64] = 0x11;
        rom[65] = 0x22;
        rom[66] = 0x33;
        rom[67] = 0x44;
        rom[68] = 0x55;
        rom[69] = 0x66;
        rom[70] = 0x77;
        rom[71] = 0x88;
        rom[72] = 0x99;
        rom[73] = 0xAA;
        rom[74] = 0xBB;
        rom[75] = 0xCC;
        rom[76] = 0xDD;
        rom[77] = 0xEE;
        rom[78] = 0xFF;
        // Gap of zeroes
        rom[113] = 0x11;
        rom[114] = 0x22;
        rom[115] = 0x33;
        rom[116] = 0x44;
        rom[117] = 0x55;
        rom[118] = 0x66;
        rom[119] = 0x77;
        rom[120] = 0x88;
        rom[121] = 0x99;
        rom[122] = 0xAA;
        rom[123] = 0xBB;
        rom[124] = 0xCC;
        rom[125] = 0xDD;
        rom[126] = 0xEE;
        rom[127] = 0xFF;
        // This last byte should NOT be copied.
        rom[128] = 0xFF;

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4000), Ok(0x11));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4001), Ok(0x22));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4002), Ok(0x33));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4003), Ok(0x44));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4004), Ok(0x55));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4005), Ok(0x66));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4006), Ok(0x77));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4007), Ok(0x88));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4008), Ok(0x99));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4009), Ok(0xAA));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x400A), Ok(0xBB));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x400B), Ok(0xCC));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x400C), Ok(0xDD));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x400D), Ok(0xEE));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x400E), Ok(0xFF));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x400F), Ok(0x00));

        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4030), Ok(0x00));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4031), Ok(0x11));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4032), Ok(0x22));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4033), Ok(0x33));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4034), Ok(0x44));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4035), Ok(0x55));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4036), Ok(0x66));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4037), Ok(0x77));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4038), Ok(0x88));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4039), Ok(0x99));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x403A), Ok(0xAA));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x403B), Ok(0xBB));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x403C), Ok(0xCC));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x403D), Ok(0xDD));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x403E), Ok(0xEE));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x403F), Ok(0xFF));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x4040), Ok(0x00));
    }

    #[test]
    fn test_blockset() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x00;  // into r0
        rom[2] = 0x00;
        rom[3] = 0x00;
        rom[4] = 0x00;
        rom[5] = 0x20;  // 32 bytes.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x11;  // into r1b
        rom[8] = 0x42;  // some value.

        rom[9] = 0x1D;  // Blockset ref length, literal dest, ref value
        rom[10] = 0x00; // length in r0
        rom[11] = 0x00;
        rom[12] = 0x00;
        rom[13] = 0x80;
        rom[14] = 0x00; // destination address 0x00008000
        rom[15] = 0x11; // value in r1b.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8000), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8001), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8002), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8003), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8004), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8005), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8006), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8007), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8008), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8009), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x800A), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x800B), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x800C), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x800D), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x800E), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x800F), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8010), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8011), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8012), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8013), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8014), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8015), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8016), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8017), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8018), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8019), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x801A), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x801B), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x801C), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x801D), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x801E), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x801F), Ok(0x42));
        assert_eq!(internal!(cpu).mmu.load_physical_8(0x8020), Ok(0x00));
    }

    #[test]
    fn test_negate() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x00;  // into r0
        rom[2] = 0x12;
        rom[3] = 0x34;
        rom[4] = 0x56;
        rom[5] = 0x78;  // random number.

        rom[6] = 0x20;  // Negate
        rom[7] = 0x00;  // r0.

        rom[8] = 0x0A;  // Copy literal
        rom[9] = 0x14;  // into r4b
        rom[10] = 0x10; // random number.

        rom[11] = 0x20; // Negate
        rom[12] = 0x14; // r4b.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[0], -0x12345678_i32 as u32);
        assert_eq!(internal!(cpu).r[4], (-0x10_i32 as u8) as u32);
    }

    #[test]
    fn test_add() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0x05;  // some number.

        rom[3] = 0x0A;  // Copy literal
        rom[4] = 0x11;  // into r1b
        rom[5] = 0x06;  // some number.

        rom[6] = 0x0A;  // Copy literal
        rom[7] = 0x12;  // into r2b
        rom[8] = 0xFF;  // max number.

        rom[9] = 0x22;  // Add register
        rom[10] = 0x10; // into r0b
        rom[11] = 0x11; // r1b.

        rom[12] = 0x22; // Add register
        rom[13] = 0x11; // into r1b
        rom[14] = 0x11; // r1b.

        rom[15] = 0x21; // Add literal
        rom[16] = 0x12; // into r2b
        rom[17] = 0x01; // 1.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[0], 0x0B);
        assert_eq!(internal!(cpu).r[1], 0x0C);
        assert_eq!(internal!(cpu).r[2], 0x00);
    }

    #[test]
    fn test_flags() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0xFF;  // max number.

        rom[3] = 0x21;  // Add literal
        rom[4] = 0x10;  // into r0b
        rom[5] = 0x01;  // 1.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[0], 0x00);
        assert_eq!(internal!(cpu).flags, FLAG_ZERO | FLAG_CARRY);

        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0x7F;  // max signed number.

        rom[3] = 0x21;  // Add literal
        rom[4] = 0x10;  // into r0b
        rom[5] = 0x01;  // 1.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[0], 0x80);
        assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_OVERFLOW);

        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0xFF;  // -1.

        rom[3] = 0x22;  // Add register
        rom[4] = 0x10;  // into r0b
        rom[5] = 0x10;  // r0b.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[0], 0xFE);
        assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_CARRY);

        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0x01;  // 1.

        rom[3] = 0x22;  // Add register
        rom[4] = 0x10;  // into r0b
        rom[5] = 0x10;  // r0b.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[0], 0x02);
        assert_eq!(internal!(cpu).flags, 0);
    }

    #[test]
    fn test_addcarry() {
        let mut rom = [0; 512];
        rom[0] = 0x0A;  // Copy literal
        rom[1] = 0x10;  // into r0b
        rom[2] = 0xFF;  // max number.

        rom[3] = 0x21;  // Add literal
        rom[4] = 0x10;  // into r0b
        rom[5] = 0x01;  // 1.

        rom[6] = 0x23;  // Add literal with carry
        rom[7] = 0x10;  // into r0b
        rom[8] = 0x01;  // 1.

        let (cpu, ui_commands) = run(rom, None);
        assert_eq!(ui_commands.len(), 2);
        assert_eq!(internal!(cpu).r[0], 0x02);
    }
}
