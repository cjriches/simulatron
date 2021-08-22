use std::io;

/// Error type for this module.
#[derive(Debug)]
pub struct OFError {
    desc: String,
}

impl OFError {
    /// Convenient creation.
    pub(super) fn new<S>(desc: S) -> Self
        where S: Into<String>
    {
        OFError {
            desc: desc.into(),
        }
    }
}

/// Result type alias.
pub type OFResult<T> = Result<T, OFError>;

/// Convert IO errors to OF errors.
impl From<io::Error> for OFError {
    fn from(e: io::Error) -> Self {
        let msg = match e.kind() {
            io::ErrorKind::PermissionDenied => "Permission denied",
            io::ErrorKind::UnexpectedEof => "Unexpected EOF",
            io::ErrorKind::OutOfMemory => "Out of memory",
            _ => "Unexpected IO error",
        };
        OFError {
            desc: format!("IO error: {}.", msg)
        }
    }
}

/// Return an error with the given message if the provided condition is false.
macro_rules! assert_or_error {
    ($condition:expr, $message:expr) => {{
        if !$condition {
            return Err(OFError::new($message));
        }
    }}
}
