use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use colored::{Color, Colorize};
use log::{error, info, LevelFilter};
use std::fs::File;
use std::io::{self, Read, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::str::FromStr;

use simulatron_salt::{
    ast::{AstNode, ConstDecl, Program},
    codegen::CodeGenerator,
    error::SaltError,
    lexer::Lexer,
    parser::Parser,
};

const INPUT_FILES: &str = "INPUT_FILES";
const ENTRYPOINT_FILE: &str = "entrypoint";
const VERBOSITY: &str = "verbosity";

const SIMOBJ_EXTENSION: &str = "simobj";

const CONTEXT_COLOR: Color = Color::BrightCyan;
const WARNING_COLOR: Color = Color::BrightYellow;
const ERROR_COLOR: Color = Color::BrightRed;
const GENERIC_ERROR_COLOR: Color = Color::Magenta;

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
        File::create(self.output_path()).and_then(|mut file| file.write_all(buf))
    }

    /// Get the corresponding output path for the input path by changing the
    /// extension.
    fn output_path(&self) -> PathBuf {
        let mut path = PathBuf::from_str(self.path).unwrap();
        path.set_extension(SIMOBJ_EXTENSION);
        path
    }
}

fn cli() -> Command {
    // Hack to make the build dirty when the toml changes.
    include_str!("../../Cargo.toml");

    clap::command!()
        .after_help(
            "Example compilation from assembly to disk image:\n    \
                salt lib.simasm -E main-func.simasm\n    \
                silk -t DISK -o disk.img main-func.simobj lib.simobj",
        )
        .arg(
            Arg::new(INPUT_FILES)
                .help("Input assembly files.")
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new(ENTRYPOINT_FILE)
                .help(
                    "The next input file is assembled as an entrypoint \
                   (can be specified multiple times).",
                )
                .short('E')
                .long("entrypoint")
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new(VERBOSITY)
                .help("Specify up to three times to increase the verbosity of output.")
                .short('v')
                .long("verbose")
                .action(ArgAction::Count)
                .value_parser(value_parser!(u8).range(..=3)),
        )
}

fn logging_format(
    formatter: &mut env_logger::fmt::Formatter,
    record: &log::Record,
) -> io::Result<()> {
    let style = formatter.default_level_style(record.level());
    writeln!(
        formatter,
        "{:>7}  {}",
        style.value(record.level()),
        record.args()
    )
}

fn init_logging(level: LevelFilter) {
    env_logger::Builder::new()
        .filter_level(level)
        .format(logging_format)
        .target(env_logger::Target::Stdout)
        .init();
}

/// Report compiler errors to the user.
#[inline]
fn report_errors(errors: &Vec<SaltError>, reference: &str, reference_path: &str) {
    report(errors, reference, reference_path, "error", ERROR_COLOR)
}

/// Report compiler warnings to the user.
#[inline]
fn report_warnings(warnings: &Vec<SaltError>, reference: &str, reference_path: &str) {
    report(
        warnings,
        reference,
        reference_path,
        "warning",
        WARNING_COLOR,
    )
}

/// Compilation error/warning/etc. reporting function.
fn report(
    items: &Vec<SaltError>,
    reference: &str,
    reference_path: &str,
    prefix: &str,
    color: Color,
) {
    for item in items.iter() {
        let (line, highlight) = find_context(reference, &item.span);
        let message = format!("{}: {}: {}", reference_path, prefix, item.message.as_ref());

        eprintln!("{}", message.color(color));
        if highlight.is_empty() {
            eprintln!();
        } else {
            eprintln!("  {}", line);
            eprintln!("  {}\n", highlight.color(color));
        }
    }
}

/// Report a generic error that's not related to a specific source span.
macro_rules! report_generic_error {
    ($($args:expr),*) => {{
        let message = format!($($args),*);
        eprintln!("{}\n", message.color(GENERIC_ERROR_COLOR));
    }}
}

/// Find the context of `range` within `reference`. Returns two formatted
/// strings; the first has the whole line of `range`, and the second contains
/// a series of '^' characters highlighting `range` within it.
fn find_context(reference: &str, range: &Range<usize>) -> (String, String) {
    // Find the line that encloses `range`.
    let mut line: usize = 1;
    let mut line_start: usize = 0;
    for (i, c) in reference.bytes().enumerate() {
        if i == range.start {
            break;
        } else if c == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let mut line_end: usize = reference.len();
    for (i, c) in reference.bytes().enumerate().skip(range.end) {
        if c == b'\n' {
            line_end = i;
            break;
        }
    }

    // Construct the context line.
    let line_num = format!("{} | ", line).color(CONTEXT_COLOR);
    let line_context = format!("{}{}", line_num, &reference[line_start..line_end]);

    // If there's no highlight, return early.
    if range.start == range.end {
        return (line_context, String::new());
    }

    // Find the highlight range, eliminating any whitespace that snuck into
    // the token.
    let mut highlight_start = range.start;
    let mut highlight_end = range.end;
    while reference.bytes().nth(highlight_start).unwrap() == b' ' {
        highlight_start += 1;
    }
    while reference.bytes().nth(highlight_end - 1).unwrap() == b' ' {
        highlight_end -= 1;
    }
    // Explicitly specify order of operations to avoid wraparound.
    highlight_start = (highlight_start + line_num.len()) - line_start;
    highlight_end = (highlight_end + line_num.len()) - line_start;
    // Build the string.
    let mut highlight = String::with_capacity(highlight_end);
    for _ in 0..highlight_start {
        highlight.push(' ');
    }
    for _ in highlight_start..highlight_end {
        highlight.push('^');
    }

    (line_context, highlight)
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
        let log_level = match args.get_count(VERBOSITY) {
            0 => None,
            1 => Some(LevelFilter::Info),
            2 => Some(LevelFilter::Debug),
            3 => Some(LevelFilter::Trace),
            _ => unreachable!(),
        };
        if let Some(level) = log_level {
            init_logging(level);
        }

        // Collect input files.
        let normal_inputs = args.get_many::<String>(INPUT_FILES).unwrap_or_default();
        let entrypoint_inputs = args.get_many::<String>(ENTRYPOINT_FILE).unwrap_or_default();
        let mut inputs: Vec<InputFile> =
            Vec::with_capacity(normal_inputs.len() + entrypoint_inputs.len());
        for path in normal_inputs {
            let file = File::open(path)
                .map_err(|e| {
                    report_generic_error!("IO Error: Failed to open input file '{}': {}", path, e)
                })
                .ok()?;
            info!("Opened non-entrypoint '{}'", path);
            inputs.push(InputFile {
                path,
                file,
                entrypoint: false,
            });
        }
        for path in entrypoint_inputs {
            let file = File::open(path)
                .map_err(|e| {
                    report_generic_error!("IO Error: Failed to open input file '{}': {}", path, e)
                })
                .ok()?;
            info!("Opened entrypoint '{}'", path);
            inputs.push(InputFile {
                path,
                file,
                entrypoint: true,
            });
        }

        let num_inputs = inputs.len();
        if num_inputs == 0 {
            eprintln!("{}", cli().render_usage());
            return Some(());
        }

        // Gather all public consts as we go through.
        let mut public_consts: Vec<ConstDecl> = Vec::new();

        // Iterate through inputs and assemble each one.
        for (i, input) in inputs.iter_mut().enumerate() {
            info!(
                "Processing file {} of {} ({}).",
                i + 1,
                num_inputs,
                input.path
            );

            // Read the file's contents.
            let mut source = String::new();
            input
                .file
                .read_to_string(&mut source)
                .map_err(|e| {
                    report_generic_error!(
                        "IO Error: Failed to read input file '{}': {}",
                        input.path,
                        e
                    )
                })
                .ok()?;
            info!("Read file {}.", i + 1);

            // Stage 1: Parsing.
            let lexer = Lexer::new(&source);
            let parser = Parser::new(lexer);
            let ast = match parser.run() {
                Ok(cst) => Program::cast(cst).unwrap(),
                Err(errors) => {
                    error!("Parsing failed for file {}.", i + 1);
                    report_errors(&errors, &source, input.path);
                    report_generic_error!("File '{}' failed to assemble.", input.path);
                    continue;
                }
            };
            info!("Parsed file {}.", i + 1);

            // Stage 2: symbol table processing.
            let consts = ast.const_decls();
            let codegen = match CodeGenerator::new(ast, &public_consts) {
                Ok(gen) => gen,
                Err(failure) => {
                    error!("Symbol processing failed for file {}.", i + 1);
                    report_warnings(&failure.warnings, &source, input.path);
                    report_errors(&failure.errors, &source, input.path);
                    report_generic_error!("File '{}' failed to assemble.", input.path);
                    continue;
                }
            };
            add_public_consts(&mut public_consts, consts);
            info!("Processed symbols for file {}.", i + 1);

            // Stage 3: codegen.
            match codegen.run(input.entrypoint) {
                Ok(result) => {
                    info!("Completed codegen for file {}.", i + 1);
                    report_warnings(&result.warnings, &source, input.path);
                    // Write output.
                    input
                        .write_output(&result.simobj)
                        .map_err(|e| {
                            report_generic_error!(
                                "IO Error: Failed to write output file '{}': {}",
                                input.output_path().display(),
                                e
                            )
                        })
                        .ok()?;
                    info!("Written result for file {}.", i + 1);
                }
                Err(failure) => {
                    error!("Codegen failed for file {}.", i + 1);
                    report_warnings(&failure.warnings, &source, input.path);
                    report_errors(&failure.errors, &source, input.path);
                    report_generic_error!("File '{}' failed to assemble.", input.path);
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
