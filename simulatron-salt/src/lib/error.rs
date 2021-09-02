use std::borrow::Cow;
use std::ops::Range;

/// Error representation.
#[derive(Debug)]
pub struct SaltError {
    pub(crate) span: Range<usize>,
    pub(crate) message: Cow<'static, str>,
}
