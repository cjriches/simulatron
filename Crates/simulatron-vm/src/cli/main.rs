use clap::{App, app_from_crate, Arg, arg_enum, ArgMatches,
           crate_authors, crate_description,
           crate_name, crate_version, value_t_or_exit};
use simplelog::{ConfigBuilder, LevelFilter, LevelPadding, WriteLogger};
use std::convert::TryInto;
use std::fs::{self, File};

const ROM_PATH: &str = "ROM_PATH";
const DISK_A_PATH: &str = "DISK_A_PATH";
const DISK_B_PATH: &str = "DISK_B_PATH";
const LOG_PATH: &str = "LOG_PATH";
const LOG_LEVEL: &str = "LOG_LEVEL";

const DISK_MSG: &str = "\
Simulatron needs a directory for each virtual disk; these must be\n\
present to launch. The folders default to ./DiskA and ./DiskB in\n\
the current working directory, but can also be specified by the\n\
--disk-a and --disk-b options.";

arg_enum! {
    /// Possible log levels.
    #[derive(Debug, PartialEq, Eq)]
    enum LogLevel {
        TRACE,
        DEBUG,
        INFO,
    }
}

fn cli() -> App<'static, 'static> {
    // Hack to make the build dirty when the toml changes.
    include_str!("../../Cargo.toml");

    app_from_crate!()
        .max_term_width(100)
        .after_help("\
This is the Simulatron Virtual Machine. To launch a Simulatron VM, simply \
ensure the disk folders are present and specify the ROM file to load. This \
will launch the Simulatron Terminal in your console, which will capture all \
keyboard input. The terminal will exit when the VM halts; this can be \
triggered manually by pressing Alt+Shift+Q.")
        .arg(Arg::with_name(ROM_PATH)
            .help("The path to the ROM file to use (must be exactly 512 bytes).")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name(DISK_A_PATH)
            .help("The path to the folder for Disk A (defaults to ./DiskA).")
            .long("disk-a")
            .takes_value(true)
            .default_value("DiskA"))
        .arg(Arg::with_name(DISK_B_PATH)
            .help("The path to the folder for Disk B (defaults to ./DiskB).")
            .long("disk-b")
            .takes_value(true)
            .default_value("DiskB"))
        .arg(Arg::with_name(LOG_PATH)
            .help("If set, a debug log will be written to the given path.")
            .short("l")
            .long("log")
            .takes_value(true))
        .arg(Arg::with_name(LOG_LEVEL)
            .help("Set the log level. Has no effect without \
                   specifying --log as well. Case insensitive.")
            .short("L")
            .long("log-level")
            .takes_value(true)
            .default_value("TRACE")
            .possible_values(&LogLevel::variants())
            .case_insensitive(true))
}

/// Ensure that the given path exists and is a directory.
fn check_disk_path(path: &str) -> Result<(), String> {
    match fs::metadata(path) {
        Ok(meta) => {
            if !meta.is_dir() {
                Err(format!("'{}' is not a directory.\n\n{}", path, DISK_MSG))
            } else {
                Ok(())
            }
        }
        Err(e) => {
            Err(format!("Could not access '{}': {}\n\n{}", path, e, DISK_MSG))
        }
    }
}

/// Initialise logging to the given file.
fn init_logging(logfile: File, level: LevelFilter) {
    // I would have used env_logger like in the other crates, but at time of
    // writing, env_logger writing to a file is completely broken.
    // Turns out SimpleLog is pretty nice too.
    let config = ConfigBuilder::new()
        .set_level_padding(LevelPadding::Right)
        .set_location_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .set_thread_level(LevelFilter::Off)
        .set_time_format_str("%H:%M:%S%.6f")
        .add_filter_ignore_str("mio")
        .build();

    WriteLogger::init(level, config, logfile).unwrap();
}

/// Main run function; returns an exit code.
fn run(args: ArgMatches) -> u8 {
    return match _run(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    };

    fn _run(args: ArgMatches) -> Result<(), String> {
        // Load ROM.
        let path = args.value_of(ROM_PATH).unwrap();
        let rom = match fs::read(path) {
            Ok(rom) => rom,
            Err(e) => return Err(
                format!("Failed to open ROM file: {}", e)),
        };

        // Ensure the ROM is the correct size.
        if rom.len() != simulatron_vm::ROM_SIZE {
            return Err(format!("ROM files must be exactly {} bytes in size.",
                               simulatron_vm::ROM_SIZE));
        }

        // Ensure that the disk paths exist.
        let disk_a_path = args.value_of(DISK_A_PATH).unwrap();
        check_disk_path(disk_a_path)?;
        let disk_b_path = args.value_of(DISK_B_PATH).unwrap();
        check_disk_path(disk_b_path)?;

        // Initialise logging if configured.
        if let Some(log_path) = args.value_of(LOG_PATH) {
            match File::create(log_path) {
                Ok(logfile) => {
                    let level = match value_t_or_exit!(args, LOG_LEVEL, LogLevel) {
                        LogLevel::TRACE => LevelFilter::Trace,
                        LogLevel::DEBUG => LevelFilter::Debug,
                        LogLevel::INFO => LevelFilter::Info,
                    };
                    init_logging(logfile, level);
                },
                Err(e) => return Err(
                    format!("Failed to create log file: {}", e)),
            }
        }

        // Run the Simulatron.
        simulatron_vm::run(rom.as_slice().try_into().unwrap(),
                           disk_a_path, disk_b_path);

        Ok(())
    }
}

fn main() {
    let args = cli().get_matches();
    std::process::exit(run(args).into());
}
