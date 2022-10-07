mod error;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser, ValueEnum};
use log::{info, error, LevelFilter};
use simulatron_utils::file::{Output, TransientFile};
use std::fs::File;
use std::io::{self, BufReader, Write};

use crate::error::LinkError;

const LINK_TARGET: &str = "link-target";
const OUTPUT_PATH: &str = "output-path";
const OBJECT_FILES: &str = "OBJECT_FILES";
const VERBOSITY: &str = "verbosity";

/// All supported link targets.
#[derive(Debug, PartialEq, Eq, Copy, Clone, ValueEnum)]
enum LinkTarget {
    ROM,
    DISK,
}

fn cli() -> Command {
    // Hack to make the build dirty when the toml changes.
    include_str!("../../Cargo.toml");

    clap::command!()
        .arg(Arg::new(LINK_TARGET)
            .help("The type of result that should be produced.")
            .short('t')
            .long("target")
            .action(ArgAction::Set)
            .required(true)
            .value_parser(value_parser!(LinkTarget))
            .ignore_case(true))
        .arg(Arg::new(OUTPUT_PATH)
            .help("Where to place the output. \
                   If omitted, the result will be sent to stdout.")
            .short('o')
            .long("output")
            .action(ArgAction::Set))
        .arg(Arg::new(OBJECT_FILES)
            .help("One or more object files to link.")
            .action(ArgAction::Append)
            .required(true))
        .arg(Arg::new(VERBOSITY)
            .help("Specify up to three times to increase the verbosity of output.")
            .short('v')
            .long("verbose")
            .action(ArgAction::Count)
            .value_parser(value_parser!(u8).range(..=3)))
}

fn logging_format(formatter: &mut env_logger::fmt::Formatter,
                  record: &log::Record) -> io::Result<()> {
    let style = formatter.default_level_style(record.level());
    writeln!(formatter, "{:>7}  {}", style.value(record.level()), record.args())
}

/// Logging setup for normal build (not testing).
#[cfg(not(test))]
fn init_logging(level: LevelFilter) {
    env_logger::Builder::new()
        .filter_level(level)
        .format(logging_format)
        .init();
}

/// Logging setup for testing build (properly captures stdout and ignores
/// multiple invocations).
#[cfg(test)]
fn init_logging(level: LevelFilter) {
    let _ = env_logger::Builder::new()
        .filter_level(level)
        .format(logging_format)
        .is_test(true)
        .try_init();
}

/// Main run function; returns an exit code.
fn run(args: ArgMatches) -> u8 {
    return match _run(args) {
        Ok(()) => 0,
        Err(e) => {
            error!("{}", e.0);
            1
        }
    };

    fn _run(args: ArgMatches) -> Result<(), LinkError> {
        // Set up logging.
        let log_level = match args.get_count(VERBOSITY) {
            0 => LevelFilter::Warn,
            1 => LevelFilter::Info,
            2 => LevelFilter::Debug,
            3 => LevelFilter::Trace,
            _ => unreachable!(),
        };
        init_logging(log_level);

        // Open output path.
        let mut output = match args.get_one::<String>(OUTPUT_PATH) {
            None => {
                info!("Silk will write the linked result to stdout.");
                Output::Stdout(io::stdout())
            },
            Some(path) => {
                info!("Silk will write the linked result to '{}'.", path);
                let f = TransientFile::create(path)
                    .map_err(|e| {
                        LinkError(format!(
                            "Failed to create output file '{}': {}", path, e))
                    })?;
                Output::File(f)
            }
        };

        // Open input files.
        let inputs = args.get_many::<String>(OBJECT_FILES).unwrap()
            .map(|path| File::open(path)
                .map(BufReader::new)
                .map_err(|e| {
                    LinkError(format!(
                        "Couldn't open input file '{}': {}", path, e))
                }))
            .collect::<Result<Vec<_>, _>>()?;
        info!("Opened all input files successfully.");

        // Run the linker.
        let linker = simulatron_silk::parse_and_combine(inputs)?;
        info!("Parsed all inputs.");
        let link_target = args.get_one(LINK_TARGET).unwrap();
        let result = match link_target {
            LinkTarget::ROM => linker.link_as_rom(),
            LinkTarget::DISK => linker.link_as_disk(),
        }?;
        info!("Linking complete.");

        // Write the result.
        output.write_all(&result)
            .map_err(|e| {
                LinkError(format!("Failed to write output: {}", e))
            })?;
        if let Output::File(f) = &mut output {
            f.set_persist(true);
        }
        info!("Result written.");

        Ok(())
    }
}

fn main() {
    let args = cli().get_matches();
    std::process::exit(run(args).into());
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use tempfile;

    macro_rules! invoke {
        ($($args:expr),+) => {{
            let args = cli().try_get_matches_from(
                    vec!["silk".to_string(), $($args.to_string()),*])
                .unwrap();
            run(args)
        }}
    }

    /// Ensure a successful invocation persists the file.
    #[test]
    fn test_success_output_persist() {
        let tempdir = tempfile::tempdir().unwrap();
        let out = tempdir.path().join("out");
        let ret = invoke!("-t", "ROM", "-o", out.to_str().unwrap(),
            "examples/single-symbol.simobj");
        assert_eq!(ret, 0);
        assert!(fs::metadata(out).is_ok());
    }

    /// Ensure an unsuccessful invocation does not persist the file.
    #[test]
    fn test_fail_output_delete() {
        let tempdir = tempfile::tempdir().unwrap();
        let out = tempdir.path().join("out");
        let ret = invoke!("-t", "ROM", "-o", out.to_str().unwrap(),
            "examples/bad-reference.simobj");
        assert_eq!(ret, 1);
        assert!(fs::metadata(out).is_err());
    }

    /// Ensure a bad command line does not persist the file (technically the
    /// file should never be created).
    #[test]
    fn test_output_transience() {
        let tempdir = tempfile::tempdir().unwrap();
        let out = tempdir.path().join("out");
        let ret = std::panic::catch_unwind(|| {
            invoke!("-o", out.to_str().unwrap(),
                "examples/bad-reference.simobj")
        });
        assert!(ret.is_err());
        assert!(fs::metadata(out).is_err())
    }
}
