mod data;
#[macro_use]
mod error;
mod linker;
mod parser;

#[cfg(test)]
mod tests;

use simulatron_utils::read_be::ReadBE;

// Public API.
pub use data::ObjectFile;
pub use error::{OFError, OFResult};
pub use linker::Linker;
pub use parser::Parser;

/// Parse a whole list of inputs and combine them into a single linker.
pub fn parse_and_combine<I, S>(inputs: I) -> OFResult<Linker>
    where I: IntoIterator<Item=S>,
          S: ReadBE
{
    let mut linker = Linker::new();

    for input in inputs.into_iter() {
        let parsed = Parser::parse(input)?;
        linker = linker.add(parsed)?;
    }

    Ok(linker)
}
