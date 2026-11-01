use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

mod mcp;
mod patch;
mod test;
use yong_db::{Database, Loaded};
use yong_diag::{DEFAULT_MAX_ERRORS, Diagnostic, Severity, finish};
use yong_ide::ViewError;
use yong_interp::{EXIT_PANIC, Stop};

/// Exit status for a failure of the tool itself, as opposed to errors in the program.
const TOOL_FAILURE: u8 = 2;

#[derive(Parser)]
#[command(name = "yong", version)]
struct Cli {
    /// What `run` executes on: the interpreter, or native code from Cranelift (M1b).
    #[arg(long, global = true, value_enum, default_value_t = Backend::Interp)]
    backend: Backend,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Backend {
    Interp,
    Cranelift,
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
    /// Print a module in another surface: Xin or Wen as Yong, Wen as Xin.
    View {
        #[arg(long = "as")]
        as_: String,
        file: PathBuf,
    },
    /// Rewrite files in canonical form (18.4): print(parse(F)) (R559).
    Fmt {
        /// Change nothing; print the files that are not canonical and exit 1.
        #[arg(long)]
        check: bool,
        #[arg(required = true)]
        files: Vec<PathBuf>,
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
    /// Run the package's tests (18.4): its `test_` functions, under the current directory.
    Test {
        #[arg(long, value_enum, default_value_t = Format::Line)]
        format: Format,
        /// Hold approved tests to tests.lock (R300): a changed, missing or failing approved
        /// test fails the run, and proposed tests only report.
        #[arg(long)]
        gate: bool,
        /// Run only this test, `module.test_name`.
        #[arg(long)]
        only: Option<String>,
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
    let cli = Cli::parse();
    match cli.command {
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
            Database::new(strict, level.as_deref() == Some("release")),
        ),
        Command::Run {
            file,
            strict,
            level,
        } => run(
            &file,
            Database::new(strict, level.as_deref() == Some("release")),
            cli.backend,
        ),
        Command::Test { format, gate, only } => run_tests(format, gate, only.as_deref()),
        Command::View { as_, file } => view(&as_, &file),
        Command::Fmt { check, files } => fmt(&files, check),
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
        Command::Mcp => match mcp::serve(Path::new(".")) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => tool_failure(&msg),
        },
    }
}

fn run_tests(format: Format, gate: bool, only: Option<&str>) -> ExitCode {
    let report = match test::run(Path::new("."), only, gate) {
        Ok(r) => r,
        Err(msg) => return tool_failure(&msg),
    };
    match format {
        Format::Json => println!("{}", report.json()),
        Format::Line => report.lines().iter().for_each(|l| println!("{l}")),
    }
    if report.failed(gate) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_diags(diags: &[Diagnostic]) {
    diags
        .iter()
        .for_each(|d| eprintln!("{}", d.render_line(None)));
}

fn view(as_: &str, file: &Path) -> ExitCode {
    let as_ = match as_ {
        "yong" => yong_names::Surface::Yong,
        "xin" => yong_names::Surface::Xin,
        "wen" => yong_names::Surface::Wen,
        _ => return tool_failure(&format!("no printer for `{as_}` yet")),
    };
    let mut db = Database::new(false, false);
    let f = match db.open(file) {
        Ok(f) => f,
        Err(msg) => return tool_failure(&msg),
    };
    match yong_ide::view(&db, f, as_) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(ViewError::Diagnostics(d)) => {
            print_diags(&d);
            ExitCode::FAILURE
        }
        Err(ViewError::Outside(what)) => {
            eprintln!(
                "yong: outside the {} subset: {what}",
                yong_ide::surface_title(as_)
            );
            ExitCode::FAILURE
        }
        Err(ViewError::Tool(msg)) => tool_failure(&msg),
    }
}

fn fmt(files: &[PathBuf], check: bool) -> ExitCode {
    let mut changed = false;
    for file in files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => return tool_failure(&format!("{}: {e}", file.display())),
        };
        let text = match yong_ide::canonical(file, &src) {
            Ok(t) => t,
            Err(ViewError::Diagnostics(d)) => {
                print_diags(&d);
                return ExitCode::FAILURE;
            }
            Err(ViewError::Outside(msg) | ViewError::Tool(msg)) => return tool_failure(&msg),
        };
        if text == src {
            continue;
        }
        changed = true;
        if check {
            println!("{}", file.display());
        } else if let Err(e) = std::fs::write(file, text) {
            return tool_failure(&format!("{}: {e}", file.display()));
        }
    }
    if check && changed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn hash(file: &Path, reverse: bool, format: Format) -> ExitCode {
    let mut db = Database::new(false, false);
    let f = match db.open(file) {
        Ok(f) => f,
        Err(msg) => return tool_failure(&msg),
    };
    let hashes = match yong_db::load(&db, f) {
        Ok(Loaded::Ready(m, _)) if !reverse => yong_core::hash::hash_module(m),
        Ok(Loaded::Ready(..)) => match yong_db::load_reversed(&db, f) {
            Ok(Loaded::Ready(m, _)) => yong_core::hash::hash_module(&m),
            _ => return tool_failure("the reversed module does not elaborate"),
        },
        Ok(Loaded::Stopped(d)) => {
            print_diags(d);
            return ExitCode::FAILURE;
        }
        Ok(Loaded::Unsupported(what, _)) => {
            return tool_failure(&format!("not supported yet: {what}"));
        }
        Err(msg) => return tool_failure(msg),
    };
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

fn run(file: &Path, mut db: Database, backend: Backend) -> ExitCode {
    let f = match db.open(file) {
        Ok(f) => f,
        Err(msg) => return tool_failure(&msg),
    };
    let module = match yong_db::load(&db, f) {
        Ok(Loaded::Ready(m, _)) => {
            let diags = yong_ide::diagnostics(&db, f).expect("a ready module checks");
            let diags = finish(diags, DEFAULT_MAX_ERRORS);
            if diags.iter().any(|d| d.severity == Severity::Error) {
                print_diags(&diags);
                return ExitCode::FAILURE;
            }
            m
        }
        Ok(Loaded::Stopped(d)) if yong_db::has_error(d) => {
            print_diags(&finish(d.clone(), DEFAULT_MAX_ERRORS));
            return ExitCode::FAILURE;
        }
        Ok(Loaded::Stopped(_)) => {
            return tool_failure(&format!("{} does not parse", file.display()));
        }
        Ok(Loaded::Unsupported(what, _)) => {
            return tool_failure(&format!("not supported yet: {what}"));
        }
        Err(msg) => return tool_failure(msg),
    };
    if backend == Backend::Cranelift {
        return native(module);
    }
    let mut out = std::io::stdout().lock();
    match yong_interp::run(module, &mut out) {
        Ok(status) => ExitCode::from(status as u8),
        Err(Stop::Panic(kind)) => {
            eprintln!("panic: {kind}");
            ExitCode::from(EXIT_PANIC as u8)
        }
        Err(Stop::Unsupported(what)) => tool_failure(&format!("not supported yet: {what}")),
        Err(Stop::Limit(what)) => tool_failure(&format!("stage-0 {what} limit at run time")),
    }
}

fn check(files: &[PathBuf], format: Format, max_errors: usize, mut db: Database) -> ExitCode {
    let mut diags = Vec::new();
    for file in files {
        let d = db.open(file).and_then(|f| yong_ide::diagnostics(&db, f));
        match d {
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

fn apply_patch(file: &Path, module: Option<&str>, check: bool, format: Format) -> ExitCode {
    let db = RefCell::new(Database::new(false, false));
    let diagnose = |p: &Path, src: &str| {
        let mut db = db.borrow_mut();
        let f = db.file(p, src);
        yong_ide::diagnostics(&*db, f)
    };
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

/// Run `m` as native code. A panic exits from inside the program.
fn native(m: &yong_core::ast::Module) -> ExitCode {
    let compiled = yong_mir::lower(m).and_then(|p| yong_cg_cranelift::compile(&p));
    match compiled {
        Ok(c) => {
            c.run();
            ExitCode::SUCCESS
        }
        Err(what) => tool_failure(&format!("not supported yet: {what}")),
    }
}

fn tool_failure(msg: &str) -> ExitCode {
    eprintln!("yong: {msg}");
    ExitCode::from(TOOL_FAILURE)
}
