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
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Run a program.
    Run { file: PathBuf },
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
            files,
        } => check(&files, format, max_errors),
        Command::Run { file } => run(&file),
    }
}

fn run(file: &Path) -> ExitCode {
    if file.extension().is_none_or(|e| e != "yong") {
        return tool_failure(&front_end_missing(file));
    }
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return tool_failure(&format!("{}: {e}", file.display())),
    };
    let (Some(module), diags) = yong_core::parse(&file.display().to_string(), &src) else {
        return tool_failure(&format!("{} does not parse", file.display()));
    };
    if let Some(d) = diags.first() {
        return tool_failure(&d.render_line(None));
    }
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

fn check(files: &[PathBuf], format: Format, max_errors: usize) -> ExitCode {
    let mut diags = Vec::new();
    for file in files {
        match check_file(file) {
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

fn check_file(file: &Path) -> Result<Vec<Diagnostic>, String> {
    if file.extension().is_none_or(|e| e != "yong") {
        return Err(front_end_missing(file));
    }
    let src = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(yong_core::parse(&file.display().to_string(), &src).1)
}

fn front_end_missing(file: &Path) -> String {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    format!("no front end for .{ext} files yet: {}", file.display())
}

fn tool_failure(msg: &str) -> ExitCode {
    eprintln!("yong: {msg}");
    ExitCode::from(TOOL_FAILURE)
}
