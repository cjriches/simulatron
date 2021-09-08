pub mod ast;
pub mod codegen;
mod error;
mod language;
pub mod lexer;
pub mod parser;

#[cfg(test)]
mod tests;

/// Initialise logging for tests.
#[cfg(test)]
pub fn init_test_logging() {
    use std::io::Write;

    // The logger can only be initialised once, but we don't know the order of
    // tests. Therefore we use `try_init` and ignore the result.
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("trace"))
        .format(|out, record| {
            writeln!(out, "{:>7} {}", record.level(), record.args())
        })
        .is_test(true)
        .try_init();
}
