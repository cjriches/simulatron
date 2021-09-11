use std::borrow::Cow;
use std::ops::Range;

/// Error representation.
#[derive(Debug)]
pub struct SaltError {
    pub span: Range<usize>,
    pub message: Cow<'static, str>,
}

pub type SaltResult<T> = Result<T, SaltError>;
