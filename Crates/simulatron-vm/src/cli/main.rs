use clap::{App, app_from_crate, Arg, ArgMatches,
           crate_authors, crate_description,
           crate_name, crate_version};
use std::convert::TryInto;

const ROM_PATH: &str = "ROM_PATH";
const DISK_A_PATH: &str = "DISK_A_PATH";
const DISK_B_PATH: &str = "DISK_B_PATH";

fn cli() -> App<'static, 'static> {
    // Hack to make the build dirty when the toml changes.
    include_str!("../../Cargo.toml");

    app_from_crate!()
        .arg(Arg::with_name(ROM_PATH)
            .help("The path to the ROM file to use.")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name(DISK_A_PATH)
            .help("The path to the folder for Disk A (defaults to ./DiskA).")
            .long("disk-a")
            .takes_value(true))
        .arg(Arg::with_name(DISK_B_PATH)
            .help("The path to the folder for Disk B (defaults to ./DiskB).")
            .long("disk-b")
            .takes_value(true))
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
        let rom = match std::fs::read(path) {
            Ok(rom) => rom,
            Err(e) => return Err(
                format!("Failed to open ROM file: {}", e)),
        };

        // Ensure the ROM is the correct size.
        if rom.len() != simulatron_vm::ROM_SIZE {
            return Err(format!("ROM files must be exactly {} bytes in size.",
                               simulatron_vm::ROM_SIZE));
        }

        // Run the Simulatron.
        simulatron_vm::run(rom.as_slice().try_into().unwrap());

        Ok(())
    }
}

fn main() {
    let args = cli().get_matches();
    std::process::exit(run(args).into());
}
