use clap::{app_from_crate, Arg, arg_enum, ArgMatches,
           crate_authors, crate_description,
           crate_name, crate_version, value_t_or_exit};
use log::{info, error, LevelFilter};
use std::fs::File;
use std::io::{BufReader, stdout, Write};

use simulatron_silk::parse_and_combine;

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

fn parse_args() -> ArgMatches<'static> {
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
        .get_matches()
}

fn init_logging(level: LevelFilter) {
    env_logger::Builder::new()
        .filter_level(level)
        .format(|formatter, record| {
            let style = formatter.default_level_style(record.level());
            writeln!(formatter, "{:>7} {}", style.value(record.level()), record.args())
        })
        .init()
}

/// Main run function; returns an exit code.
fn run() -> u8 {
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

    // Parse arguments.
    let args = parse_args();

    // Set up logging.
    let log_level = match args.occurrences_of(VERBOSITY) {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        3 | _ => LevelFilter::Trace,
    };
    init_logging(log_level);

    // Open output path.
    let mut output: Box<dyn Write> = match args.value_of(OUTPUT_PATH) {
        None => {
            info!("Silk will write the linked result to stdout.");
            Box::new(stdout())
        },
        Some(path) => {
            info!("Silk will write the linked result to '{}'.", path);
            let f = unwrap_or_error!(File::create(path));
            Box::new(f)
        }
    };

    // Open input files.
    let inputs = unwrap_or_error!(args.values_of(OBJECT_FILES).unwrap()
        .map(|path| File::open(path)
                               .map(BufReader::new))
        .collect::<Result<Vec<_>, _>>());
    info!("Opened all input files successfully.");

    // Run the linker.
    let linker = unwrap_or_error!(parse_and_combine(inputs));
    info!("Parsed all inputs.");
    let link_target = value_t_or_exit!(args, LINK_TARGET, LinkTarget);
    let result = unwrap_or_error!(match link_target {
        LinkTarget::ROM => linker.link_as_rom(),
        LinkTarget::DISK => linker.link_as_disk(),
    });
    info!("Linking complete.");

    // Write the result.
    unwrap_or_error!(output.write_all(&result));
    info!("Result written.");

    return 0;
}

fn main() {
    std::process::exit(run().into());
}
