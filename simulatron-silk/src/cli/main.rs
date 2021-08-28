mod file_utils;

use clap::{App, app_from_crate, Arg, arg_enum, ArgMatches,
           crate_authors, crate_description,
           crate_name, crate_version, value_t_or_exit};
use log::{info, error, LevelFilter};
use std::fs::{self, File};
use std::io::{self, BufReader, Write};

use crate::file_utils::{Output, TransientFile};

const LINK_TARGET: &str = "link-target";
const OUTPUT_PATH: &str = "output-path";
const OBJECT_FILES: &str = "OBJECT_FILES";
const VERBOSITY: &str = "verbosity";

arg_enum! {
    /// All supported link targets.
    #[derive(Debug, PartialEq, Eq)]
    enum LinkTarget {
        ROM,
        DISK,
    }
}

fn cli() -> App<'static, 'static> {
    // Hack to make the build dirty when the toml changes.
    include_str!("../../Cargo.toml");

    app_from_crate!()
        .arg(Arg::with_name(LINK_TARGET)
            .help("The type of result that should be produced.")
            .short("t")
            .long("target")
            .takes_value(true)
            .required(true)
            .possible_values(&LinkTarget::variants())
            .case_insensitive(true))
        .arg(Arg::with_name(OUTPUT_PATH)
            .help("Where to place the output. \
                   If omitted, the result will be sent to stdout.")
            .short("o")
            .long("output")
            .takes_value(true))
        .arg(Arg::with_name(OBJECT_FILES)
            .help("One or more object files to link.")
            .takes_value(true)
            .required(true)
            .multiple(true)
            .min_values(1))
        .arg(Arg::with_name(VERBOSITY)
            .help("Specify up to three times to increase the verbosity of output.")
            .short("v")
            .long("verbose")
            .multiple(true))
}

fn init_logging(level: LevelFilter) {
    // May be called multiple times in tests, so ignore the error.
    let _ = env_logger::Builder::new()
        .filter_level(level)
        .format(|formatter, record| {
            let style = formatter.default_level_style(record.level());
            writeln!(formatter, "{:>7} {}", style.value(record.level()), record.args())
        })
        .try_init();
}

/// Main run function; returns an exit code.
fn run(args: ArgMatches) -> u8 {
    /// Unwrap the given result or print the error and exit the process.
    macro_rules! unwrap_or_error {
        ($result:expr) => {{
            match $result {
                Ok(x) => x,
                Err(e) => {
                    error!("{:?}", e);
                    return 1;
                }
            }
        }}
    }

    // Set up logging.
    let log_level = match args.occurrences_of(VERBOSITY) {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        3 | _ => LevelFilter::Trace,
    };
    init_logging(log_level);

    // Open output path.
    let mut output = match args.value_of(OUTPUT_PATH) {
        None => {
            info!("Silk will write the linked result to stdout.");
            Output::Stdout(io::stdout())
        },
        Some(path) => {
            info!("Silk will write the linked result to '{}'.", path);
            let f = unwrap_or_error!(TransientFile::create(path));
            Output::File(f)
        }
    };

    // Open input files.
    let inputs = unwrap_or_error!(args.values_of(OBJECT_FILES).unwrap()
        .map(|path| File::open(path)
                               .map(BufReader::new))
        .collect::<Result<Vec<_>, _>>());
    info!("Opened all input files successfully.");

    // Run the linker.
    let linker = unwrap_or_error!(simulatron_silk::parse_and_combine(inputs));
    info!("Parsed all inputs.");
    let link_target = value_t_or_exit!(args, LINK_TARGET, LinkTarget);
    let result = unwrap_or_error!(match link_target {
        LinkTarget::ROM => linker.link_as_rom(),
        LinkTarget::DISK => linker.link_as_disk(),
    });
    info!("Linking complete.");

    // Write the result.
    unwrap_or_error!(output.write_all(&result));
    if let Output::File(f) = &mut output {
        f.set_persist(true);
    }
    info!("Result written.");

    return 0;
}

fn main() {
    let args = cli().get_matches();
    std::process::exit(run(args).into());
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile;

    macro_rules! invoke {
        ($($args:expr),+) => {{
            let args = cli().get_matches_from_safe(
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
