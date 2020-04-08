use notify::{self, Watcher};
use std::convert::TryFrom;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
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
    dir_path: Arc<PathBuf>,
    interrupt_tx: mpsc::Sender<u32>,
    worker_tx: Option<mpsc::Sender<DiskCommand>>,
    worker_thread: Option<thread::JoinHandle<()>>,
    watcher: Option<notify::RecommendedWatcher>,
    shared_data: Arc<Mutex<SharedData>>,
}

impl DiskController {
    pub fn new(dir_path: impl Into<PathBuf>, interrupt_tx: mpsc::Sender<u32>) -> Self {
        DiskController {
            dir_path: Arc::new(dir_path.into()),
            interrupt_tx,
            worker_tx: None,
            worker_thread: None,
            watcher: None,
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
        let worker_dir_name = Arc::clone(&self.dir_path);
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

        // Thread 2: watcher (watches filesystem for disk inserts/ejects) (implicit).
        let watcher_interrupt_tx = self.interrupt_tx.clone();
        let watcher_dir_name = Arc::clone(&self.dir_path);
        let watcher_shared_data = Arc::clone(&self.shared_data);
        let mut watcher = notify::immediate_watcher(move |event: notify::Result<notify::Event>| {
            DiskController::watcher_iteration(
                &watcher_interrupt_tx, &watcher_dir_name,
                &watcher_shared_data, &event.unwrap());
        }).unwrap();
        watcher.configure(notify::Config::PreciseEvents(true)).unwrap();
        watcher.watch(self.dir_path.as_ref(), notify::RecursiveMode::NonRecursive).unwrap();
        self.watcher = Some(watcher);
    }

    pub fn stop(&mut self) {
        // Join the worker thread.
        let worker_thread = self.worker_thread.take()
            .expect("DiskController was already stopped.");
        let worker_tx = self.worker_tx.take().unwrap();
        worker_tx.send(DiskCommand::JoinThread).unwrap();
        worker_thread.join().unwrap();

        // Join the watcher thread.
        let watcher = self.watcher.take().unwrap();
        drop(watcher);
    }

    fn worker_iteration(interrupt_tx: &mpsc::Sender<u32>, dir_path: &Path,
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
                let result = DiskController::get_file_name(dir_path).and_then(|file_path| {
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
                let result = DiskController::get_file_name(dir_path).and_then(|file_path| {
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

    fn watcher_iteration(watcher_interrupt_tx: &mpsc::Sender<u32>, dir_path: &Path,
                         watcher_shared_data: &Arc<Mutex<SharedData>>,
                         event: &notify::Event) {
        // We only care about files being created or removed. Therefore we
        // need Create, Remove, and Modify(Name(From)) events.
        if let notify::EventKind::Create(_) | notify::EventKind::Remove(_)
                | notify::EventKind::Modify(
                    notify::event::ModifyKind::Name(
                        notify::event::RenameMode::From)) = event.kind {
            // Check the filesystem to see the new state.
            match DiskController::get_file_name(dir_path) {
                None => {
                    // Set status to disconnected.
                    let mut sd = watcher_shared_data.lock().unwrap();
                    sd.status = STATUS_DISCONNECTED;
                    sd.blocks_available = 0;
                    // Send interrupt.
                    watcher_interrupt_tx.send(INTERRUPT_DISK)
                        .expect("Failed to send disk interrupt.");
                }
                Some(file_path) => {
                    // Query the file.
                    let size = fs::metadata(file_path).ok().and_then(|metadata| {
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

    fn get_file_name(dir_path: &Path) -> Option<PathBuf> {
        let mut dir_contents = fs::read_dir(dir_path).unwrap()
            .map(|res| res.unwrap().path())
            .collect::<Vec<_>>();

        match dir_contents.len() {
            0 => None,
            1 => Some(dir_contents.remove(0)),
            _ => panic!("There were multiple files in '{:?}'.", dir_path)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand;
    use std::error::Error;
    use std::io;
    use std::time::Duration;
    use tempfile;
    use rand::Rng;

    // A test fixture with an auto-started-stopped disk controller and a temp dir.
    struct DiskControllerFixture {
        disk: DiskController,
        temp_dir: tempfile::TempDir,
        disk_dir: PathBuf,
        interrupt_rx: mpsc::Receiver<u32>,
    }

    impl DiskControllerFixture {
        fn new() -> io::Result<Self> {
            let temp_dir = tempfile::tempdir()?;
            let disk_dir = temp_dir.path().join("disk");
            fs::create_dir(&disk_dir)?;
            let (tx, rx) = mpsc::channel();
            let mut disk = DiskController::new(&disk_dir, tx);
            disk.start();
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

    #[test]
    fn test_initial_state() {
        let mut fixture = DiskControllerFixture::new().unwrap();

        // Assert disconnected state.
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_DISCONNECTED);
            assert_eq!(sd.blocks_available, 0);
        }

        // Assert that commands don't work.
        for cmd in [COMMAND_READ, COMMAND_WRITE,
                    COMMAND_SUSTAINED_READ, COMMAND_SUSTAINED_WRITE].iter() {
            fixture.disk.store_control(ADDRESS_CMD, *cmd);
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            {
                let sd = fixture.disk.shared_data.lock().unwrap();
                assert_eq!(sd.status, STATUS_DISCONNECTED);
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
            file.set_len(NUM_BLOCKS as u64 * 4096).unwrap();
        }

        // Insert disk.
        fs::rename(&outer_location, &inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.blocks_available, NUM_BLOCKS);
        }

        // Eject disk.
        fs::rename(&inner_location, &outer_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_DISCONNECTED);
            assert_eq!(sd.blocks_available, 0);
        }
    }

    #[test]
    fn test_weird_disk_files() {
        let mut fixture = DiskControllerFixture::new().unwrap();

        // Should reject a disk if not a multiple of block size.
        const BAD_SIZE_NAME: &str = "badsize.simdisk";
        let outer_location = fixture.temp_dir.path().join(BAD_SIZE_NAME);
        let inner_location = fixture.disk_dir.join(BAD_SIZE_NAME);
        {
            let file = fs::File::create(&outer_location).unwrap();
            file.set_len(4095).unwrap();
        }
        // Insert and assert not connected.
        fs::rename(&outer_location, &inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_DISCONNECTED);
            assert_eq!(sd.blocks_available, 0);
        }
        // Eject and sanity check.
        fs::remove_file(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_DISCONNECTED);
            assert_eq!(sd.blocks_available, 0);
        }

        // Should reject a disk if it's actually a directory.
        const DIR_NAME: &str = "imadir";
        let inner_location = fixture.disk_dir.join(DIR_NAME);
        // Create directly inside as there's no data to sync.
        fs::create_dir(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_DISCONNECTED);
            assert_eq!(sd.blocks_available, 0);
        }
        // Eject and sanity check.
        fs::remove_dir(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_DISCONNECTED);
            assert_eq!(sd.blocks_available, 0);
        }

        // Should accept a zero-size disk but be unable to operate on it.
        const ZERO_NAME: &str = "zero.the-extension-doesnt-matter";
        let outer_location = fixture.temp_dir.path().join(ZERO_NAME);
        let inner_location = fixture.disk_dir.join(ZERO_NAME);
        {
            let file = fs::File::create(&outer_location).unwrap();
        }
        // Insert and assert connected with size 0.
        fs::rename(&outer_location, &inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.blocks_available, 0);
        }
        // Assert that commands don't work.
        for cmd in [COMMAND_READ, COMMAND_WRITE,
            COMMAND_SUSTAINED_READ, COMMAND_SUSTAINED_WRITE].iter() {
            fixture.disk.store_control(ADDRESS_CMD, *cmd);
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            {
                let sd = fixture.disk.shared_data.lock().unwrap();
                assert_eq!(sd.status, STATUS_BAD_COMMAND);
            }
        }
        // Eject and sanity check.
        fs::remove_file(&inner_location).unwrap();
        let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(int, INTERRUPT_DISK);
        {
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_DISCONNECTED);
            assert_eq!(sd.blocks_available, 0);
        }

        // Note we do NOT test multiple files inside the directory at once, as this is an
        // explicitly forbidden situation with undefined behaviour - there's nothing to verify.
    }

    // A test fixture containing a connected disk of the given size.
    struct ConnectedDiskControllerFixture {
        disk: DiskController,
        temp_dir: tempfile::TempDir,
        interrupt_rx: mpsc::Receiver<u32>,
    }

    impl ConnectedDiskControllerFixture {
        fn new(num_blocks: u32) -> io::Result<Self> {
            // Set up temp directory.
            let temp_dir = tempfile::tempdir()?;
            let disk_dir = temp_dir.path().join("disk");
            fs::create_dir(&disk_dir)?;
            // Create disk controller.
            let (tx, rx) = mpsc::channel();
            let mut disk = DiskController::new(&disk_dir, tx);
            disk.start();
            // Create disk.
            const FILE_NAME: &str = "x.simdisk";
            let outer_location = temp_dir.path().join(FILE_NAME);
            let inner_location = disk_dir.join(FILE_NAME);
            {
                let file = fs::File::create(&outer_location).unwrap();
                file.set_len(num_blocks as u64 * 4096).unwrap();
            }
            // Insert disk.
            fs::rename(&outer_location, &inner_location).unwrap();
            let int = rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            {
                let sd = disk.shared_data.lock().unwrap();
                assert_eq!(sd.status, STATUS_SUCCESS);
                assert_eq!(sd.blocks_available, num_blocks);
            }
            Ok(Self{
                disk,
                temp_dir,
                interrupt_rx: rx,
            })
        }
    }

    impl Drop for ConnectedDiskControllerFixture {
        fn drop(&mut self) {
            self.disk.stop();
        }
    }

    fn random_block() -> [u8; 4096] {
        let mut block = [0; 4096];
        let mut rng = rand::thread_rng();
        for i in block.iter_mut() {
            *i = rng.gen();
        }
        block
    }

    #[test]
    fn test_read_write() {
        let mut fixture = ConnectedDiskControllerFixture::new(2).unwrap();

        // Ensure that we read all zeros to begin with.
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
        }
        for i in 0..4096 {
            assert_eq!(fixture.disk.load_data(i), 0);
        }

        // Ensure that write of random data succeeds.
        let data = random_block();
        *fixture.disk.shared_data.lock().unwrap().buffer.as_mut() = data;
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.block_to_access, 0);
        }

        // Ensure that we can read the written data.
        *fixture.disk.shared_data.lock().unwrap().buffer.as_mut() = [0; 4096];
        for i in 0..4096 {
            assert_eq!(fixture.disk.load_data(i), 0);
        }
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.block_to_access, 0);
        }
        for i in 0..4096 {
            assert_eq!(fixture.disk.load_data(i), data[i as usize]);
        }

        // Ensure if we read the second block it's still zeroes.
        fixture.disk.store_control(ADDRESS_DA_4, 1);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.block_to_access, 1);
        }
        for i in 0..4096 {
            assert_eq!(fixture.disk.load_data(i), 0);
        }
    }

    #[test]
    fn test_sustained_read_write() {
        let mut fixture = ConnectedDiskControllerFixture::new(2).unwrap();

        // Sustained write two blocks of random data.
        let data1 = random_block();
        let data2 = random_block();
        *fixture.disk.shared_data.lock().unwrap().buffer.as_mut() = data1;
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_SUSTAINED_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.block_to_access, 1);
        }
        *fixture.disk.shared_data.lock().unwrap().buffer.as_mut() = data2;
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_SUSTAINED_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.block_to_access, 2);
        }

        // Sustained read it back.
        fixture.disk.store_control(ADDRESS_DA_4, 0);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_SUSTAINED_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.block_to_access, 1);
        }
        for i in 0..4096 {
            assert_eq!(fixture.disk.load_data(i), data1[i as usize]);
        }
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_SUSTAINED_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            let sd = fixture.disk.shared_data.lock().unwrap();
            assert_eq!(sd.status, STATUS_SUCCESS);
            assert_eq!(sd.block_to_access, 2);
        }
        for i in 0..4096 {
            assert_eq!(fixture.disk.load_data(i), data2[i as usize]);
        }
    }

    #[test]
    fn test_public_interface() {
        let mut fixture = ConnectedDiskControllerFixture::new(300).unwrap();

        // Check the status.
        assert_eq!(fixture.disk.load_status(ADDRESS_STATUS), STATUS_SUCCESS);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_1), 0);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_2), 0);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_3), 0b1);
        assert_eq!(fixture.disk.load_status(ADDRESS_NBA_4), 0b00101100);

        // Write a pattern to bytes 20-30 in block 15.
        for i in 20..30 {
            fixture.disk.store_data(i as u32, i - 19);
        }
        fixture.disk.store_control(ADDRESS_DA_4, 15);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_WRITE);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS), STATUS_SUCCESS);
        }

        // Read block 275.
        fixture.disk.store_control(ADDRESS_DA_3, 0b1);
        fixture.disk.store_control(ADDRESS_DA_4, 0b00010011);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS), STATUS_SUCCESS);
            for i in 0..4096 {
                assert_eq!(fixture.disk.load_data(i), 0);
            }
        }

        // Read the first pattern back.
        fixture.disk.store_control(ADDRESS_DA_3, 0);
        fixture.disk.store_control(ADDRESS_DA_4, 15);
        fixture.disk.store_control(ADDRESS_CMD, COMMAND_READ);
        {
            let int = fixture.interrupt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(int, INTERRUPT_DISK);
            assert_eq!(fixture.disk.load_status(ADDRESS_STATUS), STATUS_SUCCESS);
            for i in 0..20 {
                assert_eq!(fixture.disk.load_data(i), 0);
            }
            for i in 20..30 {
                assert_eq!(fixture.disk.load_data(i as u32), i - 19);
            }
            for i in 30..4096 {
                assert_eq!(fixture.disk.load_data(i), 0);
            }
        }
    }
}
