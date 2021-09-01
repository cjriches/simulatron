use std::borrow::Cow;
use std::ops::Range;

/// Error representation.
#[derive(Debug)]
pub struct SaltError {
    span: Range<usize>,
    message: Cow<'static, str>,
}

impl SaltError {
    pub(crate) fn new(span: Range<usize>,
                      message: Cow<'static, str>) -> Self {
        Self {
            span,
            message,
        }
    }
}
