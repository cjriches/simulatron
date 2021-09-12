use clap::{App, app_from_crate, Arg, ArgMatches,
           crate_authors, crate_description,
           crate_name, crate_version};
use std::convert::TryInto;
use std::fs;

const ROM_PATH: &str = "ROM_PATH";
const DISK_A_PATH: &str = "DISK_A_PATH";
const DISK_B_PATH: &str = "DISK_B_PATH";

const DISK_MSG: &str = "\
Simulatron needs a directory for each virtual disk; these must be\n\
present to launch. The folders default to ./DiskA and ./DiskB in\n\
the current working directory, but can also be specified by the\n\
--disk-a and --disk-b options.";

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
triggered manually by pressing Alt+Shift+C.")
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
