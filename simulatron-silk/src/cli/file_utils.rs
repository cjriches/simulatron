use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Stdout, Write};
use std::path::PathBuf;

pub struct TransientFile {
    file: Option<File>,
    path: PathBuf,
    persist: bool,
}

/// A created file that will be deleted when the handle is dropped, unless you
/// call `self.set_persist(true)`, or `self.into_persisted`, which returns the
/// wrapped `File`.
impl TransientFile {
    pub fn create<P: Into<PathBuf>>(path: P) -> io::Result<Self> {
        let path = path.into();
        let file = File::create(&path)?;
        Ok(Self {
            file: Some(file),
            path,
            persist: false,
        })
    }

    pub fn set_persist(&mut self, persist: bool) {
        self.persist = persist;
    }

    #[allow(dead_code)]  // Currently unused.
    pub fn into_persisted(mut self) -> File {
        self.file.take().unwrap()
    }
}

impl Drop for TransientFile {
    fn drop(&mut self) {
        // If the file was not persisted, delete it.
        if !self.persist {
            // We can't report an error or panic here, so just ignore the result.
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Read for TransientFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.as_ref().unwrap().read(buf)
    }
}

impl Seek for TransientFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.as_ref().unwrap().seek(pos)
    }
}

impl Write for TransientFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.as_ref().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_ref().unwrap().flush()
    }
}

/// Possible outputs for the linker.
pub enum Output {
    File(TransientFile),
    Stdout(Stdout),
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Output::File(f) => f.write(buf),
            Output::Stdout(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Output::File(f) => f.flush(),
            Output::Stdout(s) => s.flush(),
        }
    }
}
