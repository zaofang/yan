use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

mod mcp;
mod patch;
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
        /// Check level (8.5); `release` enables the release-only rules.
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
    /// Apply a patch (18.3): JSON, or a plain file of whole items.
    Patch {
        #[arg(long, value_enum, default_value_t = Format::Line)]
        format: Format,
        /// Reject the patch if it introduces an error (P003).
        #[arg(long)]
        check: bool,
        /// The module a plain file patch applies to.
        #[arg(long)]
        module: Option<String>,
        patch: PathBuf,
    },
    /// Serve the agent tools over MCP on stdio (21.4), for the modules under the current directory.
    Mcp,
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
    yong_elab::install_std(yong_db::std_modules);
    match Cli::parse().command {
        Command::Check {
            format,
            max_errors,
            strict,
            level,
            files,
        } => check(
            &files,
            format,
            max_errors,
            strict,
            level.as_deref() == Some("release"),
        ),
        Command::Run {
            file,
            strict,
            level,
        } => run(&file, strict, level.as_deref() == Some("release")),
        Command::View { as_, file } => view(&as_, &file),
        Command::Patch {
            format,
            check,
            module,
            patch,
        } => apply_patch(&patch, module.as_deref(), check, format),
        Command::Mcp => {
            let diagnose = |p: &Path, src: &str| check_src(p, src, false, false);
            match mcp::serve(Path::new("."), &diagnose) {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => tool_failure(&msg),
            }
        }
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

fn load(file: &Path, src: &str, strict: bool, release: bool) -> Result<Loaded, String> {
    let name = file.display().to_string();
    match file.extension().and_then(|e| e.to_str()) {
        Some("yong") => Ok(match yong_core::parse(&name, src) {
            (Some(m), d) if d.is_empty() => Loaded::Ready(m, Vec::new()),
            (_, d) => Loaded::Stopped(d),
        }),
        Some("jian") => {
            let (m, diags) = yong_jian::parse(&name, src, strict);
            let Some(m) = m.filter(|_| !has_error(&diags)) else {
                return Ok(Loaded::Stopped(diags));
            };
            let opts = yong_elab::Options {
                strict,
                release,
                wen: false,
            };
            Ok(elaborate(&name, src, &m, diags, opts))
        }
        Some("wen") => {
            let (m, diags) = yong_wen::parse(&name, src);
            if has_error(&diags) {
                return Ok(Loaded::Stopped(diags));
            }
            match yong_wen::lower(&name, src, &m) {
                Err(yong_wen::Unsupported(what)) => Ok(Loaded::Unsupported(what, diags)),
                Ok((_, mut errs)) if has_error(&errs) => {
                    errs.extend(diags);
                    Ok(Loaded::Stopped(errs))
                }
                Ok((h, mut errs)) => {
                    errs.extend(diags);
                    // Wen has no lenient reader (14.4).
                    let opts = yong_elab::Options {
                        strict: true,
                        release,
                        wen: true,
                    };
                    Ok(elaborate(&name, src, &h, errs, opts))
                }
            }
        }
        _ => Err(front_end_missing(file)),
    }
}

fn has_error(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

/// The shared IR to Yong, for both surfaces; `diags` are the front end's warnings.
fn elaborate(
    name: &str,
    src: &str,
    m: &yong_hir::Module,
    diags: Vec<Diagnostic>,
    opts: yong_elab::Options,
) -> Loaded {
    match yong_elab::elaborate(name, src, m, opts) {
        Err(yong_elab::Unsupported(what)) => Loaded::Unsupported(what, diags),
        Ok((y, mut errs)) => {
            let stop = has_error(&errs);
            errs.extend(diags);
            if stop {
                Loaded::Stopped(errs)
            } else {
                Loaded::Ready(y, errs)
            }
        }
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
    match load(file, &src, false, false) {
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

fn run(file: &Path, strict: bool, release: bool) -> ExitCode {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return tool_failure(&format!("{}: {e}", file.display())),
    };
    let name = file.display().to_string();
    let module = match load(file, &src, strict, release) {
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

fn check(
    files: &[PathBuf],
    format: Format,
    max_errors: usize,
    strict: bool,
    release: bool,
) -> ExitCode {
    let mut diags = Vec::new();
    for file in files {
        match check_file(file, strict, release) {
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

fn check_file(file: &Path, strict: bool, release: bool) -> Result<Vec<Diagnostic>, String> {
    let src = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    check_src(file, &src, strict, release)
}

fn check_src(
    file: &Path,
    src: &str,
    strict: bool,
    release: bool,
) -> Result<Vec<Diagnostic>, String> {
    let name = file.display().to_string();
    Ok(match load(file, src, strict, release)? {
        Loaded::Ready(m, mut d) => {
            d.extend(yong_check::check(&name, src, &m));
            d
        }
        Loaded::Stopped(d) | Loaded::Unsupported(_, d) => d,
    })
}

fn apply_patch(file: &Path, module: Option<&str>, check: bool, format: Format) -> ExitCode {
    let diagnose = |p: &Path, src: &str| check_src(p, src, false, false);
    match patch::apply_file(file, module, check, &diagnose) {
        Err(msg) => tool_failure(&msg),
        Ok(Ok(_)) => ExitCode::SUCCESS,
        Ok(Err(diags)) => {
            match format {
                Format::Json => println!(
                    "{}",
                    serde_json::to_string(&diags).expect("records serialize")
                ),
                Format::Line => diags
                    .iter()
                    .for_each(|d| println!("{}", d.render_line(None))),
            }
            ExitCode::FAILURE
        }
    }
}

fn front_end_missing(file: &Path) -> String {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    format!("no front end for .{ext} files yet: {}", file.display())
}

fn tool_failure(msg: &str) -> ExitCode {
    eprintln!("yong: {msg}");
    ExitCode::from(TOOL_FAILURE)
}
