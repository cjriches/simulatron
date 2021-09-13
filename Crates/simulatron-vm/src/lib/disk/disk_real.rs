use log::{debug, info};
use notify::{self, Event, EventKind, event::{ModifyKind, RenameMode}, Watcher};
use std::convert::TryFrom;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::{self, Sender}};
use std::thread;

use super::disk_interface::*;

/// Commands that can be sent to the disk controller thread.
enum DiskCommand {
    Read(bool),
    Write(bool),
    JoinThread,
}

/// Data that is shared between the worker, watcher, and CPU threads.
struct SharedData {
    status: u8,
    blocks_available: u32,
    block_to_access: u32,
    buffer: Vec<u8>,
}

/// A disk controller, implemented on the host filesystem.
pub struct RealDiskController {
    dir_path: Arc<PathBuf>,
    interrupt_tx: Sender<u32>,
    interrupt_num: u32,
    worker_tx: Option<Sender<DiskCommand>>,
    worker_thread: Option<thread::JoinHandle<()>>,
    watcher: Option<notify::RecommendedWatcher>,
    shared_data: Arc<Mutex<SharedData>>,
}

impl RealDiskController {
    /// Create a new disk controller on the given directory, with the given
    /// interrupt channel and number.
    pub fn new(dir_path: impl Into<PathBuf>,
               interrupt_tx: Sender<u32>,
               interrupt_num: u32) -> Self {
        RealDiskController {
            dir_path: Arc::new(dir_path.into()),
            interrupt_tx,
            interrupt_num,
            worker_tx: None,
            worker_thread: None,
            watcher: None,
            shared_data: Arc::new(Mutex::new(SharedData {
                status: 0,
                blocks_available: 0,
                block_to_access: 0,
                buffer: vec![0; DISK_BUFFER_SIZE],
            }))
        }
    }
}

/// Set the status, send an interrupt, and return.
macro_rules! return_with_status {
    ($sd:expr, $status:expr, $tx:expr, $inum:expr) => {{
        $sd.status = $status;
        $tx.send($inum).unwrap();
        return;
    }}
}

/// Short-cut for a finished operation.
macro_rules! return_finished {
    ($sd:expr, $status:expr, $tx:expr, $inum:expr) => {{
        // Flip the F flag.
        let mut status = $status;
        if status & FLAG_FINISHED != 0 {
            status &= !FLAG_FINISHED;
        } else {
            status |= FLAG_FINISHED;
        }
        return_with_status!($sd, status, $tx, $inum)
    }}
}

/// Short-cut for a successful operation.
macro_rules! return_successful {
    ($sd:expr, $tx:expr, $inum:expr) => {{
        let mut status = $sd.status;
        // Set the S flag.
        status |= FLAG_SUCCESS;
        // Clear the B flag.
        status &= !FLAG_BAD_COMMAND;
        return_finished!($sd, status, $tx, $inum)
    }}
}

/// Short-cut for a failed operation.
macro_rules! return_failed {
    ($sd:expr, $tx:expr, $inum:expr) => {{
        let mut status = $sd.status;
        // Clear the S flag.
        status &= !FLAG_SUCCESS;
        // Clear the B flag.
        status &= !FLAG_BAD_COMMAND;
        return_finished!($sd, status, $tx, $inum)
    }}
}

/// Short-cut for a bad operation.
macro_rules! return_bad {
    ($sd:expr, $tx:expr, $inum:expr) => {{
        let mut status = $sd.status;
        // Clear the S flag.
        status &= !FLAG_SUCCESS;
        // Set the B flag.
        status |= FLAG_BAD_COMMAND;
        return_finished!($sd, status, $tx, $inum)
    }}
}

/// Short-cut for a connection.
macro_rules! return_connected {
    ($sd:expr, $tx:expr, $inum:expr) => {{
        // Set the C flag.
        let status = $sd.status | FLAG_CONNECTED;
        return_with_status!($sd, status, $tx, $inum)
    }}
}

/// Short-cut for a disconnection.
macro_rules! return_disconnected {
    ($sd:expr, $tx:expr, $inum:expr) => {{
        // Clear the C flag.
        let status = $sd.status & !FLAG_CONNECTED;
        return_with_status!($sd, status, $tx, $inum)
    }}
}

impl DiskController for RealDiskController {
    /// Start the disk controller thread. Panics if already running.
    fn start(&mut self) {
        if self.worker_thread.is_some() {
            panic!("DiskController was already running.");
        }
        info!("Disk Controller '{}' starting.", self.dir_path.display());

        // Copied to both threads.
        let interrupt_num = self.interrupt_num;

        // Thread 1: worker (handles disk commands).
        let (worker_tx, worker_rx) = mpsc::channel();
        self.worker_tx = Some(worker_tx);
        let worker_interrupt_tx = self.interrupt_tx.clone();
        let worker_dir_name = Arc::clone(&self.dir_path);
        let worker_shared_data = Arc::clone(&self.shared_data);

        let worker_thread = thread::spawn(move || loop {
            // Get the next command.
            let cmd = worker_rx.recv().unwrap();
            // Check for thread join.
            if let DiskCommand::JoinThread = cmd {
                return;
            }
            // Handle the command.
            worker_iteration(&worker_interrupt_tx, interrupt_num,
                             &worker_dir_name, &worker_shared_data, &cmd);
        });
        self.worker_thread = Some(worker_thread);

        // Thread 2: watcher (watches filesystem for disk inserts/ejects).
        let watcher_interrupt_tx = self.interrupt_tx.clone();
        let watcher_dir_name = Arc::clone(&self.dir_path);
        let watcher_shared_data = Arc::clone(&self.shared_data);

        let mut watcher = notify::recommended_watcher(
                move |event: notify::Result<Event>| {
            // We only care about files being created or removed. Therefore we
            // need Create, Remove, and Modify(Name(From)) events.
            if let EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(
                    ModifyKind::Name(RenameMode::From)) = event.unwrap().kind {
                watcher_iteration(&watcher_interrupt_tx, interrupt_num,
                                  &watcher_dir_name, &watcher_shared_data);
            }
        }).unwrap();

        watcher.configure(notify::Config::PreciseEvents(true)).unwrap();
        watcher.watch(self.dir_path.as_path(), notify::RecursiveMode::NonRecursive).unwrap();
        // Trigger an update in case there was already a disk present before we started.
        watcher_iteration(&self.interrupt_tx, interrupt_num, &self.dir_path, &self.shared_data);
        self.watcher = Some(watcher);
    }

    /// Stop the disk controller thread. Panics if not running.
    fn stop(&mut self) {
        // Join the worker thread.
        let worker_thread = self.worker_thread.take()
            .expect("DiskController was already stopped.");
        let worker_tx = self.worker_tx.take().unwrap();
        worker_tx.send(DiskCommand::JoinThread).unwrap();
        worker_thread.join().unwrap();

        // Join the watcher thread.
        let mut watcher = self.watcher.take().unwrap();
        watcher.unwatch(self.dir_path.as_ref()).unwrap();
        info!("Disk Controller '{}' stopping.", self.dir_path.display());
    }

    /// Handle a memory-mapped control signal.
    fn store_control(&mut self, address: u32, value: u8) {
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
                    COMMAND_CONTIGUOUS_READ => {
                        self.worker_tx.as_ref().unwrap().send(DiskCommand::Read(true))
                            .expect("Failed to send command to disk worker.");
                    }
                    COMMAND_CONTIGUOUS_WRITE => {
                        self.worker_tx.as_ref().unwrap().send(DiskCommand::Write(true))
                            .expect("Failed to send command to disk worker.");
                    }
                    _ => {
                        let mut sd = self.shared_data.lock().unwrap();
                        return_bad!(sd, self.interrupt_tx, self.interrupt_num);
                    }
                };
            }
            _ => unreachable!()
        }
    }

    /// Handle a memory-mapped status request.
    fn load_status(&self, address: u32) -> u8 {
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
            ADDRESS_DA_1 =>
                ((self.shared_data.lock().unwrap().block_to_access & 0xFF000000) >> 24) as u8,
            ADDRESS_DA_2 =>
                ((self.shared_data.lock().unwrap().block_to_access & 0x00FF0000) >> 26) as u8,
            ADDRESS_DA_3 =>
                ((self.shared_data.lock().unwrap().block_to_access & 0x0000FF00) >> 8) as u8,
            ADDRESS_DA_4 =>
                (self.shared_data.lock().unwrap().block_to_access & 0x000000FF) as u8,
            _ => unreachable!()
        }
    }

    /// Write to the memory-mapped data buffer.
    fn store_data(&mut self, address: u32, value: u8) {
        let buffer = &mut self.shared_data.lock().unwrap().buffer;
        buffer[address as usize] = value;
    }

    /// Read from the memory-mapped data buffer.
    fn load_data(&self, address: u32) -> u8 {
        let buffer = &self.shared_data.lock().unwrap().buffer;
        buffer[address as usize]
    }
}

/// Handle a single disk command.
fn worker_iteration(interrupt_tx: &Sender<u32>, interrupt_num: u32, dir_path: &Path,
                    shared_data: &Arc<Mutex<SharedData>>, cmd: &DiskCommand) {
    // Acquire the shared data.
    let mut sd = shared_data.lock().unwrap();

    // If we are not connected to a disk or the address is out of
    // range, reject the command.
    if sd.status & FLAG_CONNECTED == 0 {
        return_disconnected!(sd, interrupt_tx, interrupt_num);
    }
    if sd.block_to_access >= sd.blocks_available {
        return_bad!(sd, interrupt_tx, interrupt_num);
    }

    // Command is good, service it.
    let offset = sd.block_to_access as u64 * DISK_BUFFER_SIZE as u64;
    let (result, sustained) = match *cmd {
        DiskCommand::Read(sustained) => {
            // Find the file.
            let result = get_file_name(dir_path).and_then(|file_path| {
                // Open the file.
                fs::File::open(file_path).ok()
            }).and_then(|mut file| {
                // Seek to the correct position. Note we are using 'and' to return the
                // file rather than the new seek offset.
                file.seek(SeekFrom::Start(offset)).ok().and(Some(file))
            }).and_then(|mut file| {
                // Read into the buffer.
                file.read_exact(&mut *sd.buffer).ok()
            });
            (result, sustained)
        }
        DiskCommand::Write(sustained) => {
            // Find the file.
            let result = get_file_name(dir_path).and_then(|file_path| {
                // Open the file for editing.
                fs::OpenOptions::new().write(true).open(file_path).ok()
            }).and_then(|mut file| {
                // Seek to the correct position. Note we are using 'and' to return the
                // file rather than the new seek offset.
                file.seek(SeekFrom::Start(offset)).ok().and(Some(file))
            }).and_then(|mut file| {
                // Write from the buffer.
                file.write_all(&*sd.buffer).ok()
            });
            (result, sustained)
        }
        DiskCommand::JoinThread => unreachable!()  // Already checked earlier.
    };

    match result {
        Some(_) => {
            if sustained {  // Advance to next block automatically.
                sd.block_to_access += 1;
            }
            return_successful!(sd, interrupt_tx, interrupt_num);
        }
        None => {
            debug!("IO error on disk {}", dir_path.display());
            return_failed!(sd, interrupt_tx, interrupt_num)
        }
    }
}

/// React to a filesystem event: perhaps the disk changed?
fn watcher_iteration(watcher_interrupt_tx: &Sender<u32>, interrupt_num: u32,
                     dir_path: &Path, watcher_shared_data: &Arc<Mutex<SharedData>>) {
    // Check the filesystem to see the new state.
    match get_file_name(dir_path) {
        None => {
            // Set status to disconnected.
            let mut sd = watcher_shared_data.lock().unwrap();
            sd.blocks_available = 0;
            debug!("Disk '{}' became disconnected.", dir_path.display());
            return_disconnected!(sd, watcher_interrupt_tx, interrupt_num);
        }
        Some(file_path) => {
            // Query the file.
            let size = fs::metadata(file_path).ok().and_then(|metadata| {
                // Ensure it really is a file.
                if metadata.is_file() {Some(metadata)} else {None}
            }).and_then(|metadata| {
                // Get the size in blocks.
                let bytes = metadata.len();
                if bytes > 0 && bytes % DISK_BUFFER_SIZE as u64 == 0 {
                    u32::try_from(bytes / DISK_BUFFER_SIZE as u64).ok()
                } else {None}
            });
            let mut sd = watcher_shared_data.lock().unwrap();
            match size {
                Some(num_blocks) => {
                    // Set the status to connected.
                    sd.blocks_available = num_blocks;
                    debug!("Disk '{}' became connected with {} blocks.",
                        dir_path.display(), num_blocks);
                    return_connected!(sd, watcher_interrupt_tx, interrupt_num);
                }
                None => {
                    // Set status to disconnected.
                    sd.blocks_available = 0;
                    debug!("Disk '{}' became disconnected.", dir_path.display());
                    return_disconnected!(sd, watcher_interrupt_tx, interrupt_num);
                }
            }
        }
    }
}

/// Inspect the given directory, looking for a single file which has its path
/// returned. If there is anything other than a single file, None is returned.
/// The directory MUST exist.
fn get_file_name(dir_path: &Path) -> Option<PathBuf> {
    let mut dir_contents = fs::read_dir(dir_path)
        .expect(&format!("Fatal: Failed to read directory '{}', does it exist?", dir_path.display()))
        .map(|res| res.unwrap().path())
        .collect::<Vec<_>>();

    if dir_contents.len() == 1 {
        Some(dir_contents.remove(0))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand;
    use std::io;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;
    use tempfile;

    use crate::init_test_logging;

    const INTERRUPT_NUM: u32 = 42;

    /// A test fixture with an auto-started-stopped disk controller and a temp dir.
    struct DiskControllerFixture {
        disk: RealDiskController,
        temp_dir: tempfile::TempDir,
        disk_dir: PathBuf,
        interrupt_rx: Receiver<u32>,
    }

    impl DiskControllerFixture {
        fn new() -> io::Result<Self> {
            init_test_logging();

            let temp_dir = tempfile::tempdir()?;
            let disk_dir = temp_dir.path().join("disk");
            fs::create_dir(&disk_dir)?;
            let (tx, rx) = mpsc::channel();
            let mut disk = RealDiskController::new(&disk_dir, tx, INTERRUPT_NUM);
            disk.start();
            // Consume the first interrupt from the initial check.
            let int_num = rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int_num, INTERRUPT_NUM);
            Ok(DiskControllerFixture {
                disk,
                temp_dir,
                disk_dir,
                interrupt_rx: rx,
            })
        }
    }

    impl Drop for DiskControllerFixture {
        fn drop(&mut self) {
            self.disk.stop();
        }
    }

    /// A test fixture containing a connected disk of the given size.
    struct ConnectedDiskControllerFixture {
        disk: RealDiskController,
        _temp_dir: tempfile::TempDir,
        interrupt_rx: Receiver<u32>,
    }

    impl ConnectedDiskControllerFixture {
        fn new(num_blocks: u32) -> io::Result<Self> {
            init_test_logging();

            // Set up temp directory.
            let temp_dir = tempfile::tempdir()?;
            let disk_dir = temp_dir.path().join("disk");
            fs::create_dir(&disk_dir)?;
            // Create disk controller.
            let (tx, rx) = mpsc::channel();
            let mut disk = RealDiskController::new(&disk_dir, tx, INTERRUPT_NUM);
            disk.start();
            // Consume the first interrupt from the initial check.
            let mut int_num = rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int_num, INTERRUPT_NUM);
            // Create disk.
            const FILE_NAME: &str = "x.simdisk";
            let outer_location = temp_dir.path().join(FILE_NAME);
            let inner_location = disk_dir.join(FILE_NAME);
            {
                let file = fs::File::create(&outer_location).unwrap();
                file.set_len(num_blocks as u64 * DISK_BUFFER_SIZE as u64).unwrap();
            }
            // Insert disk.
            fs::rename(&outer_location, &inner_location).unwrap();
            int_num = rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int_num, INTERRUPT_NUM);
            {
                let sd = disk.shared_data.lock().unwrap();
                assert_eq!(sd.status, FLAG_CONNECTED);
                assert_eq!(sd.blocks_available, num_blocks);
            }
            Ok(Self{
                disk,
                _temp_dir: temp_dir,
                interrupt_rx: rx,
            })
        }
    }

    impl Drop for ConnectedDiskControllerFixture {
        fn drop(&mut self) {
            self.disk.stop();
        }
    }

    /// Generate a disk block sized vector of random numbers.
    fn random_block() -> Vec<u8> {
        let mut block = Vec::with_capacity(DISK_BUFFER_SIZE);
        block.resize_with(DISK_BUFFER_SIZE, rand::random);
        block
    }

    #[test]
    fn test_initial_state() {
        let mut fixture = DiskControllerFixture::new().unwrap();

        // Assert disconnected state.
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, 0);
            assert_eq!(sd.blocks_available, 0);
        }

        // Assert that commands don't work.
        for cmd in [COMMAND_READ, COMMAND_WRITE,
            COMMAND_CONTIGUOUS_READ, COMMAND_CONTIGUOUS_WRITE].iter() {
            fixture.disk.store_control(ADDRESS_CMD, *cmd);
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            {
                let sd = fixture.disk.shared_data.lock().unwrap();
                assert_eq!(sd.status & FLAG_CONNECTED, 0);
                assert_eq!(sd.blocks_available, 0);
            }
        }
    }

    #[test]
    fn test_detects_file() {
        let fixture = DiskControllerFixture::new().unwrap();

        // Create disk.
        const NUM_BLOCKS: u32 = 1;
        const FILE_NAME: &str = "x.simdisk";
        let outer_location = fixture.temp_dir.path().join(FILE_NAME);
        let inner_location = fixture.disk_dir.join(FILE_NAME);
        {
            let file = fs::File::create(&outer_location).unwrap();
            file.set_len(NUM_BLOCKS as u64 * DISK_BUFFER_SIZE as u64).unwrap();
        }

        // Insert disk.
        fs::rename(&outer_location, &inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, FLAG_CONNECTED);
            assert_eq!(sd.blocks_available, NUM_BLOCKS);
        }

        // Eject disk.
        fs::rename(&inner_location, &outer_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }
    }

    #[test]
    fn test_weird_disk_files() {
        let fixture = DiskControllerFixture::new().unwrap();

        // Should reject a disk if not a multiple of block size.
        const BAD_SIZE_NAME: &str = "badsize.simdisk";
        let outer_location = fixture.temp_dir.path().join(BAD_SIZE_NAME);
        let inner_location = fixture.disk_dir.join(BAD_SIZE_NAME);
        {
            let file = fs::File::create(&outer_location).unwrap();
            file.set_len(DISK_BUFFER_SIZE as u64 - 1).unwrap();
        }
        // Insert and assert not connected.
        fs::rename(&outer_location, &inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }
        // Eject and sanity check.
        fs::remove_file(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }

        // Should reject a disk if it's actually a directory.
        const DIR_NAME: &str = "imadir";
        let inner_location = fixture.disk_dir.join(DIR_NAME);
        // Create directly inside as there's no data to sync.
        fs::create_dir(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }
        // Eject and sanity check.
        fs::remove_dir(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }

        // Should reject a zero-size disk.
        const ZERO_NAME: &str = "zero";
        let outer_location = fixture.temp_dir.path().join(ZERO_NAME);
        let inner_location = fixture.disk_dir.join(ZERO_NAME);
        fs::File::create(&outer_location).unwrap();
        fs::rename(&outer_location, &inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }
        // Eject and sanity check.
        fs::remove_file(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }

        // Should reject multiple files in the directory.
        const F1_NAME: &str = "foo.the-name-doesnt-matter";
        let outer_location_1 = fixture.temp_dir.path().join(F1_NAME);
        let inner_location_1 = fixture.disk_dir.join(F1_NAME);
        {
            let file = fs::File::create(&outer_location_1).unwrap();
            file.set_len(DISK_BUFFER_SIZE as u64).unwrap();
        }
        const F2_NAME: &str = "bar";
        let outer_location_2 = fixture.temp_dir.path().join(F2_NAME);
        let inner_location_2 = fixture.disk_dir.join(F2_NAME);
        {
            let file = fs::File::create(&outer_location_2).unwrap();
            file.set_len(DISK_BUFFER_SIZE as u64 * 2).unwrap();
        }
        // Insert first file: should work.
        fs::rename(&outer_location_1, &inner_location_1).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, FLAG_CONNECTED);
            assert_eq!(sd.blocks_available, 1);
        }
        // Insert second file: should disconnect.
        fs::rename(&outer_location_2, &inner_location_2).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }
        // Remove first file: should reconnect.
        fs::remove_file(&inner_location_1).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, FLAG_CONNECTED);
            assert_eq!(sd.blocks_available, 2);
        }
        // Eject and sanity check.
        fs::remove_file(&inner_location_2).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_NUM);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_CONNECTED, 0);
            assert_eq!(sd.blocks_available, 0);
        }
    }

    #[test]
    fn test_read_write() {
        let mut fixture = ConnectedDiskControllerFixture::new(2).unwrap();

        // Ensure that we read all zeros to begin with.
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, FLAG_FINISHED);
        }
        for i in 0..DISK_BUFFER_SIZE {
            assert_eq!(fixture.disk.load_data(i as u32), 0);
        }

        // Ensure that write of random data succeeds.
        let data = random_block();
        fixture.disk.shared_data.lock().unwrap().buffer = data.clone();
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, 0);
            assert_eq!(sd.block_to_access, 0);
        }

        // Ensure that we can read the written data.
        fixture.disk.shared_data.lock().unwrap().buffer = vec![0; DISK_BUFFER_SIZE];
        for i in 0..DISK_BUFFER_SIZE {
            assert_eq!(fixture.disk.load_data(i as u32), 0);
        }
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, FLAG_FINISHED);
            assert_eq!(sd.block_to_access, 0);
        }
        for i in 0..DISK_BUFFER_SIZE {
            assert_eq!(fixture.disk.load_data(i as u32), data[i]);
        }

        // Ensure if we read the second block it's still zeroes.
        fixture.disk.store_control(ADDRESS_DA_4, 1);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, 0);
            assert_eq!(sd.block_to_access, 1);
        }
        for i in 0..DISK_BUFFER_SIZE {
            assert_eq!(fixture.disk.load_data(i as u32), 0);
        }
    }

    #[test]
    fn test_sustained_read_write() {
        let mut fixture = ConnectedDiskControllerFixture::new(2).unwrap();

        // Sustained write two blocks of random data.
        let data1 = random_block();
        let data2 = random_block();
        fixture.disk.shared_data.lock().unwrap().buffer = data1.clone();
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_CONTIGUOUS_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, FLAG_FINISHED);
            assert_eq!(sd.block_to_access, 1);
        }
        fixture.disk.shared_data.lock().unwrap().buffer = data2.clone();
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_CONTIGUOUS_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, 0);
            assert_eq!(sd.block_to_access, 2);
        }

        // Sustained read it back.
        fixture.disk.store_control(ADDRESS_DA_4, 0);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_CONTIGUOUS_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, FLAG_FINISHED);
            assert_eq!(sd.block_to_access, 1);
        }
        for i in 0..DISK_BUFFER_SIZE {
            assert_eq!(fixture.disk.load_data(i as u32), data1[i]);
        }
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_CONTIGUOUS_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(sd.status & FLAG_FINISHED, 0);
            assert_eq!(sd.block_to_access, 2);
        }
        for i in 0..DISK_BUFFER_SIZE {
            assert_eq!(fixture.disk.load_data(i as u32), data2[i]);
        }
    }

    #[test]
    fn test_public_interface() {
        let mut fixture = ConnectedDiskControllerFixture::new(300).unwrap();

        // Check the status.
        assert_eq!(fixture.disk.load_status(ADDRESS_STATUS), FLAG_CONNECTED);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_1), 0);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_2), 0);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_3), 0b1);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_4), 0b00101100);

        // Write a pattern to bytes 20-29 in block 15.
        for i in 20..30 {
            fixture.disk.store_data(i as u32, i - 19);
        }
        fixture.disk.store_control(ADDRESS_DA_4, 15);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS) & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS) & FLAG_FINISHED, FLAG_FINISHED);
        }

        // Read block 275.
        fixture.disk.store_control(ADDRESS_DA_3, 0b1);
        fixture.disk.store_control(ADDRESS_DA_4, 0b00010011);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS) & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS) & FLAG_FINISHED, 0);
            for i in 0..DISK_BUFFER_SIZE {
                assert_eq!(fixture.disk.load_data(i as u32), 0);
            }
        }

        // Read the first pattern back.
        fixture.disk.store_control(ADDRESS_DA_3, 0);
        fixture.disk.store_control(ADDRESS_DA_4, 15);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_NUM);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS) & FLAG_SUCCESS, FLAG_SUCCESS);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS) & FLAG_FINISHED, FLAG_FINISHED);
            for i in 0..20 {
                assert_eq!(fixture.disk.load_data(i), 0);
            }
            for i in 20..30 {
                assert_eq!(fixture.disk.load_data(i), i as u8 - 19);
            }
            for i in 30..DISK_BUFFER_SIZE {
                assert_eq!(fixture.disk.load_data(i as u32), 0);
            }
        }
    }
}
