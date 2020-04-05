use crossbeam_channel::{self, select};
use notify::{self, Watcher};
use std::convert::TryFrom;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use crate::cpu::INTERRUPT_DISK;

// Register addresses
pub const ADDRESS_STATUS: u32 = 0;
pub const ADDRESS_NBA_1: u32 = 1;
pub const ADDRESS_NBA_2: u32 = 2;
pub const ADDRESS_NBA_3: u32 = 3;
pub const ADDRESS_NBA_4: u32 = 4;
pub const ADDRESS_DA_1: u32 = 5;
pub const ADDRESS_DA_2: u32 = 6;
pub const ADDRESS_DA_3: u32 = 7;
pub const ADDRESS_DA_4: u32 = 8;
pub const ADDRESS_CMD: u32 = 9;


// Possible values for the status register.
pub const STATUS_DISCONNECTED: u8 = 0;
pub const STATUS_SUCCESS: u8 = 1;
pub const STATUS_BAD_COMMAND: u8 = 2;
pub const STATUS_ERROR: u8 = 3;

// Allowed commands.
pub const COMMAND_READ: u8 = 1;
pub const COMMAND_WRITE: u8 = 2;
pub const COMMAND_SUSTAINED_READ: u8 = 3;
pub const COMMAND_SUSTAINED_WRITE: u8 = 4;

enum DiskCommand {
    Read(bool),
    Write(bool),
    JoinThread,
}

struct SharedData {
    status: u8,
    blocks_available: u32,
    block_to_access: u32,
    buffer: Box<[u8; 4096]>,
}

pub struct DiskController {
    dir_name: Arc<String>,
    interrupt_tx: mpsc::Sender<u32>,
    worker_tx: Option<mpsc::Sender<DiskCommand>>,
    worker_thread: Option<thread::JoinHandle<()>>,
    watcher_join_tx: Option<crossbeam_channel::Sender<()>>,
    watcher_thread: Option<thread::JoinHandle<()>>,
    shared_data: Arc<Mutex<SharedData>>,
}

impl DiskController {
    pub fn new(dir_name: String, interrupt_tx: mpsc::Sender<u32>) -> Self {
        DiskController {
            dir_name: Arc::new(dir_name),
            interrupt_tx,
            worker_tx: None,
            worker_thread: None,
            watcher_join_tx: None,
            watcher_thread: None,
            shared_data: Arc::new(Mutex::new(SharedData {
                status: 0,
                blocks_available: 0,
                block_to_access: 0,
                buffer: Box::new([0; 4096]),
            }))
        }
    }

    pub fn start(&mut self) {
        if self.worker_thread.is_some() {
            panic!("DiskController was already running.");
        }

        // Thread 1: worker (handles disk commands).
        let (worker_tx, worker_rx) = mpsc::channel();
        self.worker_tx = Some(worker_tx);
        let worker_interrupt_tx = self.interrupt_tx.clone();
        let worker_dir_name = Arc::clone(&self.dir_name);
        let worker_shared_data = Arc::clone(&self.shared_data);
        let worker_thread = thread::spawn(move || loop {
            let cmd = worker_rx.recv().unwrap();
            if let DiskCommand::JoinThread = cmd {
                return;
            }
            DiskController::worker_iteration(
                &worker_interrupt_tx, &worker_dir_name, &worker_shared_data, &cmd);
        });
        self.worker_thread = Some(worker_thread);

        // Thread 2: watcher (watches filesystem for disk inserts/ejects).
        let watcher_interrupt_tx = self.interrupt_tx.clone();
        let watcher_dir_name = Arc::clone(&self.dir_name);
        let watcher_shared_data = Arc::clone(&self.shared_data);
        let (watcher_join_tx, watcher_join_rx) = crossbeam_channel::unbounded();
        self.watcher_join_tx = Some(watcher_join_tx);
        let (file_event_tx, file_event_rx_raw) = mpsc::channel();
        let mut watcher = notify::raw_watcher(file_event_tx).unwrap();
        watcher.watch(watcher_dir_name.as_ref(), notify::RecursiveMode::NonRecursive).unwrap();
        // We need to use crossbeam's channel select feature, so we must turn the mpsc receiver
        // required by notify into a fancy crossbeam one.
        let file_event_rx = crossbeam_receiver_adapter(file_event_rx_raw);
        let watcher_thread = thread::spawn(move || loop {
            // Wait for either a join message or a file event, whichever comes first.
            select! {
                recv(watcher_join_rx) -> _ => {return;}
                recv(file_event_rx) -> msg => {
                    DiskController::watcher_iteration(
                        &watcher_interrupt_tx, &watcher_dir_name,
                        &watcher_shared_data, &msg.unwrap());
                }
            }
        });
        self.watcher_thread = Some(watcher_thread);
    }

    pub fn stop(&mut self) {
        // Join the worker thread.
        let worker_thread = self.worker_thread.take()
            .expect("DiskController was already stopped.");
        let worker_tx = self.worker_tx.take().unwrap();
        worker_tx.send(DiskCommand::JoinThread).unwrap();
        worker_thread.join().unwrap();

        // Join the watcher thread.
        let watcher_thread = self.watcher_thread.take().unwrap();
        let watcher_join_tx = self.watcher_join_tx.take().unwrap();
        watcher_join_tx.send(()).unwrap();
        watcher_thread.join().unwrap();
    }

    fn worker_iteration(interrupt_tx: &mpsc::Sender<u32>, dir_name: &str,
                        shared_data: &Arc<Mutex<SharedData>>, cmd: &DiskCommand) {
        // Acquire the shared data.
        let mut sd = shared_data.lock().unwrap();

        // Create a macro for simple code reuse.
        macro_rules! return_with_status {
            ($x:expr) => {
                {
                    sd.status = $x;
                    interrupt_tx.send(INTERRUPT_DISK).expect("Failed to send disk interrupt.");
                    return;
                }
            };
        }

        // If we are not connected to a disk or the address is out of
        // range, reject the command.
        if sd.status == STATUS_DISCONNECTED {
            return_with_status!(STATUS_DISCONNECTED);
        }
        if sd.block_to_access >= sd.blocks_available {
            return_with_status!(STATUS_BAD_COMMAND);
        }

        let offset = (sd.block_to_access * 4096) as u64;
        match *cmd {
            DiskCommand::Read(sustained) => {
                // Find the file.
                let result = DiskController::get_file_name(dir_name).and_then(|file_name| {
                    // Open the file.
                    fs::File::open(file_name).ok()
                }).and_then(|mut file| {
                    // Seek to the correct position. Note we are using 'and' to return the
                    // file rather than the new seek offset.
                    file.seek(SeekFrom::Start(offset)).ok().and(Some(file))
                }).and_then(|mut file| {
                    // Read into the buffer.
                    file.read_exact(&mut *sd.buffer).ok()
                });
                match result {
                    Some(_) => {
                        if sustained {  // Advance to next block automatically.
                            sd.block_to_access += 1;
                        }
                        return_with_status!(STATUS_SUCCESS)
                    }
                    None => return_with_status!(STATUS_ERROR)
                }
            }
            DiskCommand::Write(sustained) => {
                // Find the file.
                let result = DiskController::get_file_name(dir_name).and_then(|file_name| {
                    // Open the file for editing.
                    fs::OpenOptions::new().write(true).open(file_name).ok()
                }).and_then(|mut file| {
                    // Seek to the correct position. Note we are using 'and' to return the
                    // file rather than the new seek offset.
                    file.seek(SeekFrom::Start(offset)).ok().and(Some(file))
                }).and_then(|mut file| {
                    // Write from the buffer.
                    file.write_all(&*sd.buffer).ok()
                });
                match result {
                    Some(_) => {
                        if sustained {  // Advance to next block automatically.
                            sd.block_to_access += 1;
                        }
                        return_with_status!(STATUS_SUCCESS)
                    }
                    None => return_with_status!(STATUS_ERROR)
                }
            }
            DiskCommand::JoinThread => unreachable!()  // Already checked earlier.
        }
    }

    fn watcher_iteration(watcher_interrupt_tx: &mpsc::Sender<u32>, watcher_dir_name: &str,
                         watcher_shared_data: &Arc<Mutex<SharedData>>,
                         event: &notify::RawEvent) {
        // We only care about Create and Remove events.
        if let Ok(notify::op::CREATE) | Ok(notify::op::REMOVE) = event.op {
            // Check the filesystem to see the new state.
            match DiskController::get_file_name(watcher_dir_name) {
                None => {
                    // Set status to disconnected.
                    let mut sd = watcher_shared_data.lock().unwrap();
                    sd.status = STATUS_DISCONNECTED;
                    sd.blocks_available = 0;
                    // Send interrupt.
                    watcher_interrupt_tx.send(INTERRUPT_DISK)
                        .expect("Failed to send disk interrupt.");
                }
                Some(file_name) => {
                    // Query the file.
                    let size = fs::metadata(file_name).ok().and_then(|metadata| {
                        // Ensure it really is a file.
                        if metadata.is_file() {Some(metadata)} else {None}
                    }).and_then(|metadata| {
                        // Get the size in blocks.
                        let bytes = metadata.len();
                        if bytes % 4096 == 0 {
                            u32::try_from(bytes / 4096).ok()
                        } else {None}
                    });
                    let mut sd = watcher_shared_data.lock().unwrap();
                    match size {
                        Some(num_blocks) => {
                            // Set the status to connected.
                            sd.status = STATUS_SUCCESS;
                            sd.blocks_available = num_blocks;
                        }
                        None => {
                            // Set status to disconnected.
                            sd.status = STATUS_DISCONNECTED;
                            sd.blocks_available = 0;
                        }
                    }
                    // Send interrupt.
                    watcher_interrupt_tx.send(INTERRUPT_DISK)
                        .expect("Failed to send disk interrupt.");
                }
            }
        }
    }

    fn get_file_name(dir_name: &str) -> Option<path::PathBuf>  {
        let mut dir_contents = fs::read_dir(dir_name).unwrap()
            .map(|res| res.unwrap().path())
            .collect::<Vec<_>>();

        match dir_contents.len() {
            0 => None,
            1 => Some(dir_contents.remove(0)),
            _ => panic!("There were multiple files in '{}'.", dir_name)
        }
    }

    pub fn store_control(&mut self, address: u32, value: u8) {
        match address {
            ADDRESS_DA_1 => {
                let block_to_access = &mut self.shared_data.lock().unwrap().block_to_access;
                let address_masked = *block_to_access & 0x00FFFFFF;
                let value_shifted = (value as u32) << 24;
                *block_to_access = address_masked | value_shifted;
            }
            ADDRESS_DA_2 => {
                let block_to_access = &mut self.shared_data.lock().unwrap().block_to_access;
                let address_masked = *block_to_access & 0xFF00FFFF;
                let value_shifted = (value as u32) << 16;
                *block_to_access = address_masked | value_shifted;
            }
            ADDRESS_DA_3 => {
                let block_to_access = &mut self.shared_data.lock().unwrap().block_to_access;
                let address_masked = *block_to_access & 0xFFFF00FF;
                let value_shifted = (value as u32) << 8;
                *block_to_access = address_masked | value_shifted;
            }
            ADDRESS_DA_4 => {
                let block_to_access = &mut self.shared_data.lock().unwrap().block_to_access;
                let address_masked = *block_to_access & 0xFFFFFF00;
                *block_to_access = address_masked | (value as u32);
            }
            ADDRESS_CMD => {
                match value {
                    COMMAND_READ => {
                        self.worker_tx.as_ref().unwrap().send(DiskCommand::Read(false))
                            .expect("Failed to send command to disk worker.");
                    }
                    COMMAND_WRITE => {
                        self.worker_tx.as_ref().unwrap().send(DiskCommand::Write(false))
                            .expect("Failed to send command to disk worker.");
                    }
                    COMMAND_SUSTAINED_READ => {
                        self.worker_tx.as_ref().unwrap().send(DiskCommand::Read(true))
                            .expect("Failed to send command to disk worker.");
                    }
                    COMMAND_SUSTAINED_WRITE => {
                        self.worker_tx.as_ref().unwrap().send(DiskCommand::Write(true))
                            .expect("Failed to send command to disk worker.");
                    }
                    _ => {
                        let status = &mut self.shared_data.lock().unwrap().status;
                        *status = STATUS_BAD_COMMAND;
                        self.interrupt_tx.send(INTERRUPT_DISK)
                            .expect("Failed to send disk interrupt.");
                    }
                };
            }
            _ => panic!("Invalid address in disk::store_control.")
        }
    }

    pub fn load_status(&self, address: u32) -> u8 {
        match address {
            ADDRESS_STATUS => self.shared_data.lock().unwrap().status,
            ADDRESS_NBA_1 =>
                ((self.shared_data.lock().unwrap().blocks_available & 0xFF000000) >> 24) as u8,
            ADDRESS_NBA_2 =>
                ((self.shared_data.lock().unwrap().blocks_available & 0x00FF0000) >> 26) as u8,
            ADDRESS_NBA_3 =>
                ((self.shared_data.lock().unwrap().blocks_available & 0x0000FF00) >> 8) as u8,
            ADDRESS_NBA_4 =>
                (self.shared_data.lock().unwrap().blocks_available & 0x000000FF) as u8,
            _ => panic!("Invalid address in disk::load_status.")
        }
    }

    pub fn store_data(&mut self, address: u32, value: u8) {
        let index = usize::try_from(address).unwrap();
        let buffer = &mut self.shared_data.lock().unwrap().buffer;
        if index >= buffer.len() {
            panic!("Invalid address in disk::store_data.");
        }
        buffer[index] = value;
    }

    pub fn load_data(&self, address: u32) -> u8 {
        let index = usize::try_from(address).unwrap();
        let buffer = &self.shared_data.lock().unwrap().buffer;
        if index >= buffer.len() {
            panic!("Invalid address in disk::load_data.");
        }
        buffer[index]
    }
}

fn crossbeam_receiver_adapter<T: Send + 'static>(
    mpsc_receiver: mpsc::Receiver<T>) -> crossbeam_channel::Receiver<T> {
    // Create a new crossbeam channel.
    let (cb_tx, cb_rx) = crossbeam_channel::unbounded();
    // Launch a thread to forward messages from mpsc_receiver to cb_tx.
    // If either end is disconnected, the thread will exit neatly.
    thread::spawn(move || {
        while let Ok(msg) = mpsc_receiver.recv() {
            if let Err(_) = cb_tx.send(msg) {
                return;
            }
        }
    });
    // Return our linked crossbeam receiver.
    cb_rx
}
