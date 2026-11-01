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
    /// Print a module in another surface: Jian or Wen as Yong, Wen as Jian.
    View {
        #[arg(long = "as")]
        as_: String,
        file: PathBuf,
    },
    /// Print the item hashes (3.6): key, salted hash and identity hash, one item per line.
    Hash {
        #[arg(long, value_enum, default_value_t = Format::Line)]
        format: Format,
        /// Reverse the item order before elaborating, for checking law 4 (16.5).
        #[arg(long, hide = true)]
        reverse_items: bool,
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
        Command::Hash {
            format,
            reverse_items,
            file,
        } => hash(&file, reverse_items, format),
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
    load_ordered(file, src, strict, release, false)
}

/// [`load`], with the module's items reversed first when `reverse`.
fn load_ordered(
    file: &Path,
    src: &str,
    strict: bool,
    release: bool,
    reverse: bool,
) -> Result<Loaded, String> {
    let name = file.display().to_string();
    match file.extension().and_then(|e| e.to_str()) {
        Some("yong") => Ok(match yong_core::parse(&name, src) {
            (Some(mut m), d) if d.is_empty() => {
                if reverse {
                    m.items.reverse();
                }
                Loaded::Ready(m, Vec::new())
            }
            (_, d) => Loaded::Stopped(d),
        }),
        Some("jian") => {
            let (m, diags) = yong_jian::parse(&name, src, strict);
            let Some(mut m) = m.filter(|_| !has_error(&diags)) else {
                return Ok(Loaded::Stopped(diags));
            };
            if reverse {
                m.items.reverse();
            }
            let opts = yong_elab::Options {
                strict,
                release,
                wen: false,
                ffi: is_ffi(file),
            };
            Ok(elaborate(&name, src, &m, diags, opts))
        }
        Some("wen") => {
            let (mut m, diags) = yong_wen::parse(&name, src);
            if reverse {
                m.items.reverse();
            }
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
                        ffi: is_ffi(file),
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
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return tool_failure(&format!("{}: {e}", file.display())),
    };
    let ext = file.extension().and_then(|e| e.to_str());
    match (as_, ext) {
        ("yong", _) => {}
        ("jian", Some("wen")) => return view_wen_as_jian(file, &src),
        _ => return tool_failure(&format!("no printer for `{as_}` yet")),
    }
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

/// Wen's HIR printed by the Jian printer: the Jian projection of law 3 (16.5).
fn view_wen_as_jian(file: &Path, src: &str) -> ExitCode {
    let name = file.display().to_string();
    let (m, diags) = yong_wen::parse(&name, src);
    if has_error(&diags) {
        diags
            .iter()
            .for_each(|d| eprintln!("{}", d.render_line(None)));
        return ExitCode::FAILURE;
    }
    match yong_wen::lower(&name, src, &m) {
        Err(yong_wen::Unsupported(what)) => tool_failure(&format!("not supported yet: {what}")),
        Ok((_, errs)) if has_error(&errs) => {
            errs.iter()
                .for_each(|d| eprintln!("{}", d.render_line(None)));
            ExitCode::FAILURE
        }
        Ok((h, _)) => match yong_jian::resugar(h) {
            Ok(j) => {
                print!("{}", yong_jian::print(&j));
                ExitCode::SUCCESS
            }
            Err(what) => {
                eprintln!("yong: outside the Jian subset: {what}");
                ExitCode::FAILURE
            }
        },
    }
}

fn hash(file: &Path, reverse: bool, format: Format) -> ExitCode {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return tool_failure(&format!("{}: {e}", file.display())),
    };
    let m = match load_ordered(file, &src, false, false, reverse) {
        Ok(Loaded::Ready(m, _)) => m,
        Ok(Loaded::Stopped(d)) => {
            d.iter().for_each(|d| eprintln!("{}", d.render_line(None)));
            return ExitCode::FAILURE;
        }
        Ok(Loaded::Unsupported(what, _)) => {
            return tool_failure(&format!("not supported yet: {what}"));
        }
        Err(msg) => return tool_failure(&msg),
    };
    let hashes = yong_core::hash::hash_module(&m);
    match format {
        Format::Line => {
            for h in hashes {
                println!("{}\t{}\t{}", h.key, h.salted, h.identity);
            }
        }
        Format::Json => {
            let items: Vec<_> = hashes
                .iter()
                .map(|h| {
                    serde_json::json!({
                        "key": h.key,
                        "hash": h.salted.to_string(),
                        "identity": h.identity.to_string(),
                    })
                })
                .collect();
            println!("{}", serde_json::Value::Array(items));
        }
    }
    ExitCode::SUCCESS
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
        Loaded::Stopped(d) => d,
        // Front-end errors found before the unsupported construct are still real.
        Loaded::Unsupported(_, d) if has_error(&d) => d,
        Loaded::Unsupported(what, _) => return Err(format!("not supported yet: {what}")),
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

/// 12: a module under a directory named `ffi` (conformance assumption A9).
fn is_ffi(file: &Path) -> bool {
    file.parent()
        .is_some_and(|d| d.components().any(|c| c.as_os_str() == "ffi"))
}

fn front_end_missing(file: &Path) -> String {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    format!("no front end for .{ext} files yet: {}", file.display())
}

fn tool_failure(msg: &str) -> ExitCode {
    eprintln!("yong: {msg}");
    ExitCode::from(TOOL_FAILURE)
}
