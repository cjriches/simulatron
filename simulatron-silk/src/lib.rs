mod data;
#[macro_use]
mod error;
mod linker;
mod parser;
mod read_be;

#[cfg(test)]
mod tests;

// Public API.
pub use data::ObjectFile;
pub use error::{OFError, OFResult};
pub use linker::Linker;
pub use parser::Parser;
