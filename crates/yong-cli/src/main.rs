use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use yong_diag::{DEFAULT_MAX_ERRORS, Diagnostic, Severity, finish};

/// Exit status for a failure of the tool itself, as opposed to errors in the program.
use yong_interp::{EXIT_PANIC, Stop};

const TOOL_FAILURE: u8 = 2;

#[derive(Parser)]
#[command(name = "yong", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check files and report diagnostics.
    Check {
        #[arg(long, value_enum, default_value_t = Format::Line)]
        format: Format,
        #[arg(long, default_value_t = DEFAULT_MAX_ERRORS)]
        max_errors: usize,
        /// Disable the lenient reader (14.4).
        #[arg(long)]
        strict: bool,
        /// Check level (8.5); accepted, not yet acted on.
        #[arg(long)]
        level: Option<String>,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Print a module in another surface; only Jian to Yong for now.
    View {
        #[arg(long = "as")]
        as_: String,
        file: PathBuf,
    },
    /// Run a program.
    Run {
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        level: Option<String>,
        file: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Line,
    Json,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Check {
            format,
            max_errors,
            strict,
            files,
            ..
        } => check(&files, format, max_errors, strict),
        Command::Run { file, strict, .. } => run(&file, strict),
        Command::View { as_, file } => view(&as_, &file),
    }
}

/// A module ready to check or run: parsed and, for Jian, elaborated to Yong.
enum Loaded {
    Ready(yong_core::ast::Module, Vec<Diagnostic>),
    /// Front-end diagnostics that stop before the Yong checker.
    Stopped(Vec<Diagnostic>),
    /// Jian the elaborator does not lower yet; its front-end diagnostics.
    Unsupported(String, Vec<Diagnostic>),
}

fn load(file: &Path, src: &str, strict: bool) -> Result<Loaded, String> {
    let name = file.display().to_string();
    match file.extension().and_then(|e| e.to_str()) {
        Some("yong") => Ok(match yong_core::parse(&name, src) {
            (Some(m), d) if d.is_empty() => Loaded::Ready(m, Vec::new()),
            (_, d) => Loaded::Stopped(d),
        }),
        Some("jian") => {
            let (m, diags) = yong_jian::parse(&name, src, strict);
            let Some(m) = m.filter(|_| !diags.iter().any(|d| d.severity == Severity::Error)) else {
                return Ok(Loaded::Stopped(diags));
            };
            Ok(match yong_elab::elaborate(&name, src, &m, strict) {
                Err(yong_elab::Unsupported(what)) => Loaded::Unsupported(what, diags),
                Ok((y, mut errs)) if !errs.iter().any(|d| d.severity == Severity::Error) => {
                    errs.extend(diags);
                    Loaded::Ready(y, errs)
                }
                Ok((_, mut errs)) => {
                    errs.extend(diags);
                    Loaded::Stopped(errs)
                }
            })
        }
        _ => Err(front_end_missing(file)),
    }
}

fn view(as_: &str, file: &Path) -> ExitCode {
    if as_ != "yong" {
        return tool_failure(&format!("no printer for `{as_}` yet"));
    }
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return tool_failure(&format!("{}: {e}", file.display())),
    };
    match load(file, &src, false) {
        Ok(Loaded::Ready(m, _)) => {
            print!("{}", yong_core::print(&m));
            ExitCode::SUCCESS
        }
        Ok(Loaded::Stopped(d)) => {
            d.iter().for_each(|d| eprintln!("{}", d.render_line(None)));
            ExitCode::FAILURE
        }
        Ok(Loaded::Unsupported(what, _)) => tool_failure(&format!("not supported yet: {what}")),
        Err(msg) => tool_failure(&msg),
    }
}

fn run(file: &Path, strict: bool) -> ExitCode {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return tool_failure(&format!("{}: {e}", file.display())),
    };
    let name = file.display().to_string();
    let module = match load(file, &src, strict) {
        Ok(Loaded::Ready(m, mut diags)) => {
            diags.extend(yong_check::check(&name, &src, &m));
            let diags = finish(diags, DEFAULT_MAX_ERRORS);
            if diags.iter().any(|d| d.severity == Severity::Error) {
                diags
                    .iter()
                    .for_each(|d| eprintln!("{}", d.render_line(None)));
                return ExitCode::FAILURE;
            }
            m
        }
        Ok(Loaded::Stopped(d)) if d.iter().any(|d| d.severity == Severity::Error) => {
            finish(d, DEFAULT_MAX_ERRORS)
                .iter()
                .for_each(|d| eprintln!("{}", d.render_line(None)));
            return ExitCode::FAILURE;
        }
        Ok(Loaded::Stopped(_)) => return tool_failure(&format!("{name} does not parse")),
        Ok(Loaded::Unsupported(what, _)) => {
            return tool_failure(&format!("not supported yet: {what}"));
        }
        Err(msg) => return tool_failure(&msg),
    };
    let mut out = std::io::stdout().lock();
    match yong_interp::run(&module, &mut out) {
        Ok(status) => ExitCode::from(status as u8),
        Err(Stop::Panic(kind)) => {
            eprintln!("panic: {kind}");
            ExitCode::from(EXIT_PANIC as u8)
        }
        Err(Stop::Unsupported(what)) => tool_failure(&format!("not supported yet: {what}")),
    }
}

fn check(files: &[PathBuf], format: Format, max_errors: usize, strict: bool) -> ExitCode {
    let mut diags = Vec::new();
    for file in files {
        match check_file(file, strict) {
            Ok(mut d) => diags.append(&mut d),
            Err(msg) => return tool_failure(&msg),
        }
    }
    let diags = finish(diags, max_errors);
    match format {
        Format::Json => println!(
            "{}",
            serde_json::to_string(&diags).expect("records serialize")
        ),
        Format::Line => diags
            .iter()
            .for_each(|d| println!("{}", d.render_line(None))),
    }
    if diags.iter().any(|d| d.severity == Severity::Error) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check_file(file: &Path, strict: bool) -> Result<Vec<Diagnostic>, String> {
    let src = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = file.display().to_string();
    Ok(match load(file, &src, strict)? {
        Loaded::Ready(m, mut d) => {
            d.extend(yong_check::check(&name, &src, &m));
            d
        }
        Loaded::Stopped(d) | Loaded::Unsupported(_, d) => d,
    })
}

fn front_end_missing(file: &Path) -> String {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    format!("no front end for .{ext} files yet: {}", file.display())
}

fn tool_failure(msg: &str) -> ExitCode {
    eprintln!("yong: {msg}");
    ExitCode::from(TOOL_FAILURE)
}
