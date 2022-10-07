use clap::{Arg, ArgAction, ArgMatches, Command, value_parser, ValueEnum};
use simplelog::{ConfigBuilder, LevelFilter, LevelPadding, WriteLogger};
use std::convert::TryInto;
use std::fs::{self, File};
use std::io::{self, Write};
use time::macros::format_description;

const ROM_PATH: &str = "ROM_PATH";
const DISK_A_PATH: &str = "DISK_A_PATH";
const DISK_B_PATH: &str = "DISK_B_PATH";
const LOG_PATH: &str = "LOG_PATH";
const LOG_LEVEL: &str = "LOG_LEVEL";
const INIT: &str = "INIT";

const DISK_MSG: &str = "\
Simulatron needs a directory for each virtual disk; these must be\n\
present to launch. The folders default to ./DiskA and ./DiskB in\n\
the current working directory, but can also be specified by the\n\
--disk-a and --disk-b options.";

/// Possible log levels.
#[derive(Debug, PartialEq, Eq, Copy, Clone, ValueEnum)]
enum LogLevel {
    TRACE,
    DEBUG,
    INFO,
}

fn cli() -> Command {
    // Hack to make the build dirty when the toml changes.
    include_str!("../../Cargo.toml");

    clap::command!()
        .max_term_width(100)
        .after_help("\
This is the Simulatron Virtual Machine. To launch a Simulatron VM, simply \
ensure the disk folders are present and specify the ROM file to load. This \
will launch the Simulatron Terminal in your console, which will capture all \
keyboard input. The terminal will exit when the VM halts; this can be \
triggered manually by pressing Alt+Shift+Q.")
        .arg(Arg::new(ROM_PATH)
            .help("The path to the ROM file to use (must be exactly 512 bytes).")
            .long("rom")
            .action(ArgAction::Set)
            .default_value("./ROM"))
        .arg(Arg::new(DISK_A_PATH)
            .help("The path to the folder for Disk A.")
            .long("disk-a")
            .action(ArgAction::Set)
            .default_value("./DiskA"))
        .arg(Arg::new(DISK_B_PATH)
            .help("The path to the folder for Disk B.")
            .long("disk-b")
            .action(ArgAction::Set)
            .default_value("./DiskB"))
        .arg(Arg::new(LOG_PATH)
            .help("If set, a debug log will be written to the given path.")
            .short('l')
            .long("log")
            .action(ArgAction::Set))
        .arg(Arg::new(LOG_LEVEL)
            .help("Set the log level. Has no effect without \
                   specifying --log as well. Case insensitive.")
            .short('L')
            .long("log-level")
            .action(ArgAction::Set)
            .default_value("TRACE")
            .value_parser(value_parser!(LogLevel))
            .ignore_case(true))
        .arg(Arg::new(INIT)
            .help("Instead of running the VM, create a new skeleton directory \
                   layout suitable for running a VM. Creates the following \
                   directories: './simulatron/', './simulatron/DiskA/', \
                   './simulatron/DiskB/', and a placeholder './simulatron/ROM' \
                   file that simply halts the processor.")
            .long("init")
            .action(ArgAction::SetTrue))
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
        .set_time_format_custom(format_description!("[hour]:[minute]:[second].[subsecond digits:6]"))
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
        // Check for init option.
        if args.get_flag(INIT) {
            return match create_skeleton() {
                Ok(_) => {
                    eprintln!("Successfully created simulatron skeleton.");
                    Ok(())
                }
                Err(e) => {
                    Err(format!(
                        "Failed to create simulatron skeleton: {}", e))
                }
            };

            fn create_skeleton() -> io::Result<()> {
                fs::create_dir_all("./simulatron/DiskA")?;
                fs::create_dir_all("./simulatron/DiskB")?;
                File::create("./simulatron/ROM").and_then(|mut f|
                    f.write_all(&[0; simulatron_vm::ROM_SIZE]))
            }
        }

        // Load ROM.
        let path = args.get_one::<String>(ROM_PATH).unwrap();
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
        let disk_a_path = args.get_one::<String>(DISK_A_PATH).unwrap();
        check_disk_path(disk_a_path)?;
        let disk_b_path = args.get_one::<String>(DISK_B_PATH).unwrap();
        check_disk_path(disk_b_path)?;

        // Initialise logging if configured.
        if let Some(log_path) = args.get_one::<String>(LOG_PATH) {
            match File::create(log_path) {
                Ok(logfile) => {
                    let level = match args.get_one(LOG_LEVEL).unwrap() {
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
