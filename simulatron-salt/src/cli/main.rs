use clap::{App, app_from_crate, Arg, ArgMatches,
           crate_authors, crate_description,
           crate_name, crate_version};
use colored::{Color, Colorize};
use itertools::Itertools;
use log::{info, error, LevelFilter};
use std::fs::File;
use std::io::{self, Write, Read};
use std::path::PathBuf;
use std::str::FromStr;

use simulatron_salt::{
    error::SaltError,
    lexer::Lexer,
    parser::Parser,
    ast::{AstNode, ConstDecl, Program},
    codegen::CodeGenerator,
};

const INPUT_FILES: &str = "INPUT_FILES";
const ENTRYPOINT_FILE: &str = "entrypoint";
const VERBOSITY: &str = "verbosity";

const SIMOBJ_EXTENSION: &str = "simobj";

/// An input file is tagged with whether it's an entrypoint or not.
#[derive(Debug)]
struct InputFile<'a> {
    path: &'a str,
    file: File,
    entrypoint: bool,
}

impl<'a> InputFile<'a> {
    /// Write the given buffer to the output path obtained from `self.output_path`.
    fn write_output(&self, buf: &[u8]) -> io::Result<()> {
        File::create(self.output_path())
            .and_then(|mut file| {
                file.write_all(buf)
            })
    }

    /// Get the corresponding output path for the input path by changing the
    /// extension.
    fn output_path(&self) -> PathBuf {
        let mut path = PathBuf::from_str(self.path).unwrap();
        path.set_extension(SIMOBJ_EXTENSION);
        path
    }
}

fn cli() -> App<'static, 'static> {
    // Hack to make the build dirty when the toml changes.
    include_str!("../../Cargo.toml");

    app_from_crate!()
        .arg(Arg::with_name(INPUT_FILES)
            .help("Input assembly files.")
            .takes_value(true)
            .multiple(true))
        .arg(Arg::with_name(ENTRYPOINT_FILE)
            .help("The next input file is assembled as an entrypoint \
                   (can be specified multiple times).")
            .short("E")
            .long("entrypoint")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1))
        .arg(Arg::with_name(VERBOSITY)
            .help("Specify up to three times to increase the verbosity of output.")
            .short("v")
            .long("verbose")
            .multiple(true))
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

/// Report compiler errors to the user.
#[inline]
fn report_errors(errors: &Vec<SaltError>) {
    report(errors, "error", Color::BrightRed)
}

/// Report compiler warnings to the user.
#[inline]
fn report_warnings(warnings: &Vec<SaltError>) {
    report(warnings, "warning", Color::BrightYellow)
}

/// Generic error/warning/etc reporting function.
fn report(items: &Vec<SaltError>, prefix: &str, color: Color) {
    todo!()
}

/// Add the public constants from `consts` to `accumulator`.
fn add_public_consts(accumulator: &mut Vec<ConstDecl>, consts: Vec<ConstDecl>) {
    for const_ in consts.into_iter() {
        if const_.public() {
            accumulator.push(const_)
        }
    }
}

/// Main run function; returns an exit code.
fn run(args: ArgMatches) -> u8 {
    return match _run(args) {
        Some(()) => 0,
        None => 1,
    };

    fn _run(args: ArgMatches) -> Option<()> {
        // Set up logging.
        let log_level = match args.occurrences_of(VERBOSITY) {
            0 => None,
            1 => Some(LevelFilter::Info),
            2 => Some(LevelFilter::Debug),
            3 | _ => Some(LevelFilter::Trace),
        };
        if let Some(level) = log_level {
            init_logging(level);
        }

        // Collect input files.
        let normal_inputs = match args.values_of(INPUT_FILES) {
            Some(vec) => vec.collect_vec(),
            None => Vec::new(),
        };
        let entrypoint_inputs = match args.values_of(ENTRYPOINT_FILE) {
            Some(vec) => vec.collect_vec(),
            None => Vec::new(),
        };
        let mut inputs: Vec<InputFile> = Vec::with_capacity(
            normal_inputs.len() + entrypoint_inputs.len()
        );
        for path in normal_inputs.iter() {
            let file = File::open(path)
                .map_err(|e| {
                    eprintln!(
                        "Failed to open input file '{}': {}", path, e)
                }).ok()?;
            info!("Opened non-entrypoint '{}'", path);
            inputs.push(InputFile {
                path,
                file,
                entrypoint: false,
            });
        }
        for path in entrypoint_inputs.iter() {
            let file = File::open(path)
                .map_err(|e| {
                    eprintln!(
                        "Failed to open input file '{}': {}", path, e)
                }).ok()?;
            info!("Opened entrypoint '{}'", path);
            inputs.push(InputFile {
                path,
                file,
                entrypoint: true,
            });
        }

        let num_inputs = inputs.len();
        if num_inputs == 0 {
            eprintln!("{}", args.usage());
            return Some(());
        }

        // Gather all public consts as we go through.
        let mut public_consts: Vec<ConstDecl> = Vec::new();

        // Iterate through inputs and assemble each one.
        for (i, input) in inputs.iter_mut().enumerate() {
            info!("Processing file {} of {} ({}).", i+1, num_inputs, input.path);

            // Read the file's contents.
            let mut source = String::new();
            input.file.read_to_string(&mut source)
                .map_err(|e| {
                    eprintln!(
                        "Failed to read input file '{}': {}", input.path, e)
                }).ok()?;
            info!("Read file {}.", i+1);

            // Stage 1: Parsing.
            let lexer = Lexer::new(&source);
            let parser = Parser::new(lexer);
            let ast = match parser.run() {
                Ok(cst) => Program::cast(cst).unwrap(),
                Err(errors) => {
                    error!("Parsing failed for file {}.", i+1);
                    report_errors(&errors);
                    continue;
                }
            };
            info!("Parsed file {}.", i+1);

            // Stage 2: symbol table processing.
            let consts = ast.const_decls();
            let codegen = match CodeGenerator::new(ast, &public_consts) {
                Ok(gen) => gen,
                Err(failure) => {
                    error!("Symbol processing failed for file {}.", i+1);
                    report_warnings(&failure.warnings);
                    report_errors(&failure.errors);
                    continue;
                }
            };
            add_public_consts(&mut public_consts, consts);
            info!("Processed symbols for file {}.", i+1);

            // Stage 3: codegen.
            match codegen.run(input.entrypoint) {
                Ok(result) => {
                    info!("Completed codegen for file {}.", i+1);
                    report_warnings(&result.warnings);
                    // Write output.
                    input.write_output(&result.simobj)
                        .map_err(|e| {
                            eprintln!(
                                "Failed to write output file '{}': {}",
                                input.output_path().display(), e)
                        }).ok()?;
                    info!("Written result for file {}.", i+1);
                },
                Err(failure) => {
                    error!("Codegen failed for file {}.", i+1);
                    report_warnings(&failure.warnings);
                    report_errors(&failure.errors);
                }
            }
        }

        Some(())
    }
}

fn main() {
    let args = cli().get_matches();
    std::process::exit(run(args).into());
}
