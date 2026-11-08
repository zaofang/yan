use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

mod agent;
mod build;
mod examples;
mod export;
mod fix;
mod lock;
mod mcp;
mod patch;
mod publish;
mod pyimport;
mod review;
mod schema;
mod test;
mod trust;
use yan_db::{Database, Loaded};
use yan_diag::{DEFAULT_MAX_ERRORS, Diagnostic, Severity, finish};
use yan_ide::ViewError;
use yan_interp::{EXIT_PANIC, Stop};

#[global_allocator]
static HEAP: yan_rt::heap::Counting = yan_rt::heap::Counting;

/// Exit status for a failure of the tool itself, as opposed to errors in the program.
const TOOL_FAILURE: u8 = 2;

#[derive(Parser)]
#[command(name = "yan", version)]
struct Cli {
    /// What `run` executes on: the interpreter, or native code from Cranelift (M1b).
    #[arg(long, global = true, value_enum, default_value_t = Backend::Interp)]
    backend: Backend,
    /// With the Cranelift backend: compile each function at its first call (debug), or all of
    /// them before `main` starts.
    #[arg(long, global = true, value_enum, default_value_t = Jit::Lazy)]
    jit: Jit,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum GrammarFormat {
    Gbnf,
    Xgrammar,
    Llguidance,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Backend {
    Interp,
    Cranelift,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Jit {
    Lazy,
    Eager,
}

#[derive(Subcommand)]
enum OpCommand {
    /// List the logged operations, oldest first.
    Log,
}

#[derive(Subcommand)]
enum McpCommand {
    /// Print the JSON Schema of every tool's input.
    Schema,
}

/// The agent skill file; committed as golden text.
const SKILL: &str = include_str!("../skill.md");

#[derive(Subcommand)]
enum SchemaCommand {
    /// Print the row record of a Parquet file and the hash of its schema.
    Gen { file: PathBuf },
}

#[derive(Subcommand)]
enum Command {
    /// Print the Yan Agent grammar for constrained decoding (21.3).
    Grammar {
        #[arg(long, value_enum)]
        format: GrammarFormat,
    },
    /// Write the row record type of a data file as Yan Agent source (11.9).
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    /// Refuse a manifest that cannot be published (19.4, E016).
    Publish {
        /// Only check the manifest; publishing itself waits for the registry.
        #[arg(long, required = true)]
        check: bool,
    },
    /// Check files and report diagnostics.
    Check {
        /// `prose` when output is a terminal, `line` otherwise (18.2).
        #[arg(long, value_enum)]
        format: Option<Format>,
        #[arg(long, default_value_t = DEFAULT_MAX_ERRORS)]
        max_errors: usize,
        /// Disable the lenient reader (14.4).
        #[arg(long)]
        strict: bool,
        /// Check level (8.5); `release` enables the release-only rules.
        #[arg(long)]
        level: Option<String>,
        /// Fail with E020 unless `yan.lock` pins every requirement of the manifest (R896).
        #[arg(long)]
        locked: bool,
        /// With none, the package in the current directory: `package.yag` and its modules.
        files: Vec<PathBuf>,
    },
    /// Print a module in another surface: Yan Agent or Yan as Yan Core, Yan as Yan Agent.
    View {
        #[arg(long = "as")]
        as_: String,
        /// Fail on spelling marks that raise did not use (RFC 0034).
        #[arg(long)]
        strict: bool,
        /// Raise without spelling marks (the bare check of RFC 0034 D7).
        #[arg(long)]
        bare: bool,
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
    /// Export a module as typed Python 3.12 (17.8): the program text on stdout.
    Export {
        #[arg(long, required = true)]
        python: bool,
        file: PathBuf,
    },
    /// Import typed Python 3.12 as Yan Agent (17.8): the program text on stdout.
    Import {
        #[arg(long, required = true)]
        python: bool,
        file: PathBuf,
    },
    /// Apply the `auto` fixes of the diagnostics to files (18.2, R292).
    Fix {
        /// Change nothing; report what would change.
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
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
    /// Revert the last operations in `.yan/oplog.jsonl` (18.3 rule 7).
    Undo {
        /// How many operations to revert.
        #[arg(default_value_t = 1)]
        count: usize,
    },
    /// Operations (18.1).
    Op {
        #[command(subcommand)]
        command: OpCommand,
    },
    /// Print the agent skill file: the Yan Agent primer and its canonical examples (21.3).
    Skill,
    /// Serve the agent tools over MCP on stdio (21.4), for the modules under the current directory.
    Mcp {
        #[command(subcommand)]
        command: Option<McpCommand>,
    },
    /// Trust a directory, so `yan mcp` runs its tests and stage 0 there (21.4, R676).
    Trust {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Run the package's tests (18.4): its `test_` functions, under the current directory.
    Test {
        #[arg(long, value_enum, default_value_t = Format::Line)]
        format: Format,
        /// Hold approved tests to tests.lock (R300): a changed, missing or failing approved
        /// test fails the run, and proposed tests only report.
        #[arg(long)]
        gate: bool,
        /// Run only this test (`module.test_name`, `module.f#ex1`), or the tests under a module
        /// or function prefix.
        #[arg(long)]
        only: Option<String>,
        /// Run every test instead of reusing cached verdicts (18.4).
        #[arg(long)]
        no_cache: bool,
        /// Fail with E020 unless `yan.lock` pins every requirement of the manifest (R896).
        #[arg(long)]
        locked: bool,
    },
    /// Run one task through the spec gate as a client of `yan mcp` (the reference agent
    /// loop), and print its run record.
    Agent {
        /// The task: a JSON file with `id` and `prompt`.
        task: PathBuf,
        /// `stub:FILE` (scripted replies) or `cmd:PROGRAM` (one JSON line per turn).
        #[arg(long)]
        model: String,
        /// The system prompt; the Yan Agent primer.
        #[arg(long)]
        primer: Option<PathBuf>,
        /// A directory of `.yag` examples appended to the system prompt.
        #[arg(long)]
        shots: Option<PathBuf>,
        /// The work directory; a new temporary one by default.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// The module the task is written in.
        #[arg(long, default_value = "task")]
        module: String,
        #[arg(long, default_value_t = 2)]
        spec_repairs: usize,
        #[arg(long, default_value_t = 3)]
        body_repairs: usize,
        /// The output budget of each model turn.
        #[arg(long, default_value_t = 16000)]
        max_tokens: u64,
        /// Write the run record here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Explain a diagnostic code, with its conformance examples and their fixes (R299).
    Explain { code: String },
    /// Compare two revisions of the package item by item (RFC 0023); exit 1 when a change
    /// needs a human.
    Review {
        /// `BASE` or `BASE..HEAD`, git revisions; HEAD left out is the working tree.
        #[arg(default_value = "HEAD")]
        range: String,
        #[arg(long, value_enum, default_value_t = ReviewFormat::Line)]
        format: ReviewFormat,
    },
    /// Compile a program to an executable with the system linker (`cc`).
    Build {
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        level: Option<String>,
        /// The executable to write.
        #[arg(short, long)]
        output: PathBuf,
        file: PathBuf,
    },
    /// Run a program.
    Run {
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        level: Option<String>,
        /// Print `live: N` to stderr at exit: runtime blocks (Str, List, closure captures) of native code not freed.
        #[arg(long)]
        report_live: bool,
        /// Interpret optimized Yan Core (R771).
        #[arg(long)]
        opt: bool,
        file: PathBuf,
        /// The program's arguments, after `--`.
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Line,
    Prose,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum ReviewFormat {
    Line,
    Json,
    Md,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Check {
            format,
            max_errors,
            strict,
            level,
            locked,
            files,
        } => check(
            &files,
            locked,
            format,
            max_errors,
            Database::new(strict, level.as_deref() == Some("release")),
        ),
        Command::Publish { .. } => match publish::check(Path::new(".")) {
            Ok(d) if d.is_empty() => ExitCode::SUCCESS,
            Ok(d) => {
                print_diags(&d);
                ExitCode::FAILURE
            }
            Err(msg) => tool_failure(&msg),
        },
        Command::Explain { code } => explain(&code),
        Command::Run {
            file,
            strict,
            level,
            report_live,
            opt,
            args,
        } => run(
            &file,
            Database::new(strict, level.as_deref() == Some("release")),
            cli.backend,
            cli.jit,
            args,
            report_live,
            opt,
        ),
        Command::Build {
            file,
            strict,
            level,
            output,
        } => build::run(
            &file,
            &output,
            Database::new(strict, level.as_deref() == Some("release")),
        ),
        Command::Test {
            format,
            gate,
            only,
            no_cache,
            locked,
        } => run_tests(format, gate, only.as_deref(), no_cache, locked),
        Command::Review { range, format } => review(&range, format),
        Command::View {
            as_,
            file,
            strict,
            bare,
        } => view(&as_, &file, yan_ide::Marks { strict, bare }),
        Command::Export { file, .. } => export::run(&file),
        Command::Import { file, .. } => pyimport::run(&file),
        Command::Grammar { format } => {
            let g = yan_yag::Grammar::yag();
            match format {
                GrammarFormat::Gbnf => print!("{}", g.gbnf()),
                GrammarFormat::Xgrammar => print!("{}", g.xgrammar()),
                GrammarFormat::Llguidance => print!("{}", g.llguidance()),
            }
            ExitCode::SUCCESS
        }
        Command::Schema {
            command: SchemaCommand::Gen { file },
        } => match schema::generate(&file) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
        Command::Fmt { check, files } => fmt(&files, check),
        Command::Fix {
            dry_run,
            json,
            files,
        } => fix(&files, dry_run, json),
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
        Command::Agent {
            task,
            model,
            primer,
            shots,
            dir,
            module,
            spec_repairs,
            body_repairs,
            max_tokens,
            out,
        } => {
            let o = agent::Options {
                task,
                model,
                primer,
                shots,
                dir,
                module,
                spec_repairs,
                body_repairs,
                max_tokens,
            };
            match agent::run(&o) {
                Ok((rec, green)) => {
                    let text = format!("{rec:#}\n");
                    match out {
                        Some(f) => {
                            if let Err(e) = std::fs::write(&f, text) {
                                return tool_failure(&format!("{}: {e}", f.display()));
                            }
                        }
                        None => print!("{text}"),
                    }
                    ExitCode::from(u8::from(!green))
                }
                Err(msg) => tool_failure(&msg),
            }
        }
        Command::Undo { count } => match patch::undo(Path::new("."), count) {
            Ok(ops) => {
                ops.iter()
                    .rev()
                    .for_each(|o| println!("reverted {}", o.file));
                ExitCode::SUCCESS
            }
            Err(msg) => tool_failure(&msg),
        },
        Command::Op {
            command: OpCommand::Log,
        } => match patch::log(Path::new(".")) {
            Ok(ops) => {
                for (i, o) in ops.iter().enumerate() {
                    let (before, after) = o.hashes();
                    println!("{} {} {before} -> {after}", i + 1, o.file);
                }
                ExitCode::SUCCESS
            }
            Err(msg) => tool_failure(&msg),
        },
        Command::Skill => {
            print!("{SKILL}");
            ExitCode::SUCCESS
        }
        Command::Mcp {
            command: Some(McpCommand::Schema),
        } => {
            println!("{}", mcp::input_schemas());
            ExitCode::SUCCESS
        }
        Command::Mcp { command: None } => match mcp::serve(Path::new(".")) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => tool_failure(&msg),
        },
        Command::Trust { dir } => match trust::add(&dir) {
            Ok(d) => {
                println!("trusted {}", d.display());
                ExitCode::SUCCESS
            }
            Err(msg) => tool_failure(&msg),
        },
    }
}

/// Read `yan.lock` of the package in the current directory (E019, and E020 under `--locked`).
fn lock_gate(locked: bool) -> Result<(), ExitCode> {
    let reqs = std::fs::read_to_string("package.yag")
        .ok()
        .and_then(|src| {
            let (m, _) = yan_yag::parse("package.yag", &src, false);
            yan_elab::manifest(&m?)
        })
        .map(|m| m.requirements)
        .unwrap_or_default();
    lock::check(Path::new("."), &reqs, locked).map_err(|d| {
        print_diags(&[*d]);
        ExitCode::FAILURE
    })
}

fn run_tests(
    format: Format,
    gate: bool,
    only: Option<&str>,
    no_cache: bool,
    locked: bool,
) -> ExitCode {
    if let Err(code) = lock_gate(locked) {
        return code;
    }
    let root = Path::new(".");
    let cache = (!no_cache).then(|| test::cache_file(root));
    let report = match test::run(root, only, gate, cache.as_deref(), None) {
        Ok(r) => r,
        Err(msg) => return tool_failure(&msg),
    };
    match format {
        Format::Json => println!("{}", report.json()),
        Format::Line | Format::Prose => report.lines().iter().for_each(|l| println!("{l}")),
    }
    if report.failed(gate) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn review(range: &str, format: ReviewFormat) -> ExitCode {
    let (base, head) = match range.split_once("..") {
        Some((b, h)) => (b, Some(h)),
        None => (range, None),
    };
    let r = match review::review(Path::new("."), base, head) {
        Ok(r) => r,
        Err(msg) => return tool_failure(&msg),
    };
    match format {
        ReviewFormat::Line => r.lines().iter().for_each(|l| println!("{l}")),
        ReviewFormat::Json => println!("{}", r.json()),
        ReviewFormat::Md => print!("{}", r.markdown()),
    }
    if r.needs_human() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

mod cases {
    include!(concat!(env!("OUT_DIR"), "/explain_cases.rs"));
}

/// `yan explain CODE`: the catalogue entry, then each surface's first `fail/` conformance
/// case for the code, checked and rendered as prose, so its fixes show as diffs.
fn explain(code: &str) -> ExitCode {
    let Some((meaning, sections)) = yan_diag::catalogue(code) else {
        return tool_failure(&format!("`{code}` is not in the diagnostics catalogue"));
    };
    println!("{code}: {meaning}");
    if !sections.is_empty() {
        println!("spec: {sections}");
    }
    let mut any = false;
    for (_, path, flags, text) in cases::CASES.iter().filter(|c| c.0 == code) {
        let mut db = Database::new(
            flags.contains("--strict"),
            flags.contains("--level release"),
        );
        let f = db.file(Path::new(path), text);
        let Ok(diags) = yan_ide::diagnostics(&db, f) else {
            continue;
        };
        for d in diags.iter().filter(|d| d.code == code) {
            println!("\nexample: {path}\n{}", d.render_prose(text));
            any = true;
        }
    }
    if !any {
        println!("\nNo conformance example for {code} yet.");
    }
    ExitCode::SUCCESS
}

fn print_diags(diags: &[Diagnostic]) {
    diags
        .iter()
        .for_each(|d| eprintln!("{}", d.render_line(None)));
}

fn view(as_: &str, file: &Path, marks: yan_ide::Marks) -> ExitCode {
    let as_ = match as_ {
        "yir" => yan_names::Surface::Yir,
        "yag" => yan_names::Surface::Yag,
        "yn" => yan_names::Surface::Yn,
        _ => return tool_failure(&format!("no printer for `{as_}` yet")),
    };
    let mut db = Database::new(false, false);
    let f = match db.open(file) {
        Ok(f) => f,
        Err(msg) => return tool_failure(&msg),
    };
    match yan_ide::view(&db, f, as_, marks) {
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
                "yan: outside the {} subset: {what}",
                yan_ide::surface_title(as_)
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
        let text = match yan_ide::canonical(file, &src) {
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
    let hashes = match yan_db::load(&db, f) {
        Ok(Loaded::Ready(m, _, _)) if !reverse => yan_core::hash::hash_module(m),
        Ok(Loaded::Ready(..)) => match yan_db::load_reversed(&db, f) {
            Ok(Loaded::Ready(m, _, _)) => yan_core::hash::hash_module(&m),
            _ => return tool_failure("the reversed module does not elaborate"),
        },
        Ok(Loaded::Stopped(d)) => {
            print_diags(d);
            return ExitCode::FAILURE;
        }
        Err(msg) => return tool_failure(msg),
    };
    match format {
        Format::Line | Format::Prose => {
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

fn run(
    file: &Path,
    db: Database,
    backend: Backend,
    jit: Jit,
    args: Vec<String>,
    report_live: bool,
    opt: bool,
) -> ExitCode {
    let code = run_program(file, db, backend, jit, args, opt);
    if report_live {
        eprintln!("live: {}", yan_rt::heap::live_blocks());
    }
    code
}

/// The module of `file` once it loads and checks clean; otherwise the failure is reported and
/// its exit code returned.
fn checked<'a>(db: &'a mut Database, file: &Path) -> Result<&'a yan_core::ast::Module, ExitCode> {
    let f = match db.open(file) {
        Ok(f) => f,
        Err(msg) => return Err(tool_failure(&msg)),
    };
    let module = match yan_db::load(db, f) {
        Ok(Loaded::Ready(m, _, _)) => {
            let diags = yan_ide::diagnostics(db, f).expect("a ready module checks");
            let diags = finish(diags, DEFAULT_MAX_ERRORS);
            if diags.iter().any(|d| d.severity == Severity::Error) {
                print_diags(&diags);
                return Err(ExitCode::FAILURE);
            }
            m
        }
        Ok(Loaded::Stopped(d)) if yan_db::has_error(d) => {
            print_diags(&finish(d.clone(), DEFAULT_MAX_ERRORS));
            return Err(ExitCode::FAILURE);
        }
        Ok(Loaded::Stopped(_)) => {
            return Err(tool_failure(&format!("{} does not parse", file.display())));
        }
        Err(msg) => return Err(tool_failure(msg)),
    };
    Ok(module)
}

fn run_program(
    file: &Path,
    mut db: Database,
    backend: Backend,
    jit: Jit,
    args: Vec<String>,
    opt: bool,
) -> ExitCode {
    let module = match checked(&mut db, file) {
        Ok(m) => m,
        Err(code) => return code,
    };
    if backend == Backend::Cranelift {
        return native(&file.display().to_string(), module, jit);
    }
    let optimized;
    let module = if opt {
        match yan_opt::optimize(&file.display().to_string(), module) {
            Ok(m) => {
                optimized = m;
                &optimized
            }
            Err(yan_opt::Error::Diag(d)) => {
                print_diags(&[*d]);
                return ExitCode::FAILURE;
            }
            Err(yan_opt::Error::Unsupported(what)) => {
                return tool_failure(&format!("not supported yet: {what} in optimized Yan Core"));
            }
        }
    } else {
        module
    };
    let mut out = std::io::stdout().lock();
    match yan_interp::run_reporting(module, &mut out, args) {
        Ok((status, failure)) => {
            if let Some(f) = failure {
                eprintln!("{f}");
            }
            ExitCode::from(status as u8)
        }
        Err(Stop::Panic(p)) => {
            eprintln!("{}", p.render(&file.display().to_string()));
            ExitCode::from(EXIT_PANIC as u8)
        }
        Err(Stop::Unsupported(what)) => tool_failure(&format!("not supported yet: {what}")),
        Err(Stop::Limit(what)) => tool_failure(&format!("stage-0 {what} limit at run time")),
    }
}

/// The modules of the package rooted at `root`, its `package.yag` first, as paths below it.
fn package_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.join("package.yag").is_file() {
        return Err("no files given and no package.yag in the current directory".into());
    }
    fn walk(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            let (path, rel) = (e.path(), rel.join(&name));
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                walk(&path, &rel, out)?;
            } else if path.extension().is_some_and(|x| x == "yag" || x == "yn") {
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out)?;
    out.sort_by_key(|p| p != Path::new("package.yag"));
    Ok(out)
}

fn check(
    files: &[PathBuf],
    locked: bool,
    format: Option<Format>,
    max_errors: usize,
    mut db: Database,
) -> ExitCode {
    use std::io::IsTerminal;
    if let Err(code) = lock_gate(locked) {
        return code;
    }
    let mut diags = Vec::new();
    let mut texts = std::collections::HashMap::new();
    let package;
    let files = if files.is_empty() {
        package = match package_files(Path::new(".")) {
            Ok(fs) => fs,
            Err(msg) => return tool_failure(&msg),
        };
        &package[..]
    } else {
        files
    };
    for file in files {
        let d = db.open(file).and_then(|f| {
            texts.insert(file.display().to_string(), f.text(&db).clone());
            yan_ide::diagnostics(&db, f)
        });
        match d {
            Ok(mut d) => diags.append(&mut d),
            Err(msg) => return tool_failure(&msg),
        }
    }
    let diags = finish(diags, max_errors);
    let format = format.unwrap_or(if std::io::stdout().is_terminal() {
        Format::Prose
    } else {
        Format::Line
    });
    match format {
        Format::Prose => diags.iter().for_each(|d| {
            let src = texts.get(&d.file).map_or("", String::as_str);
            println!("{}", d.render_prose(src));
        }),
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

fn fix(files: &[PathBuf], dry_run: bool, json: bool) -> ExitCode {
    let db = RefCell::new(Database::new(false, false));
    let diagnose = |p: &Path, src: &str| {
        let mut db = db.borrow_mut();
        let f = db.file(p, src);
        yan_ide::diagnostics(&*db, f)
    };
    let mut report = Vec::new();
    for file in files {
        match fix::run(file, dry_run, &diagnose) {
            Ok(r) => report.push((file, r)),
            Err(msg) => return tool_failure(&msg),
        }
    }
    if json {
        let rows: Vec<_> = report
            .iter()
            .map(|(f, r)| serde_json::json!({"file": f.display().to_string(), "fixes": r.applied}))
            .collect();
        println!("{}", serde_json::Value::Array(rows));
    } else {
        for (f, r) in report.iter().filter(|(_, r)| r.applied > 0) {
            println!("{}: {} fixes", f.display(), r.applied);
        }
    }
    ExitCode::SUCCESS
}

fn apply_patch(file: &Path, module: Option<&str>, check: bool, format: Format) -> ExitCode {
    let db = RefCell::new(Database::new(false, false));
    let diagnose = |p: &Path, src: &str| {
        let mut db = db.borrow_mut();
        let f = db.file(p, src);
        yan_ide::diagnostics(&*db, f)
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
                Format::Line | Format::Prose => diags
                    .iter()
                    .for_each(|d| println!("{}", d.render_line(None))),
            }
            ExitCode::FAILURE
        }
    }
}

/// Run `m` as native code. A panic exits from inside the program.
/// Optimized Yan Core of `m` lowered for the native backends; failures are reported.
fn lower_native(file: &str, m: &yan_core::ast::Module) -> Result<yan_mir::Program, ExitCode> {
    let m = match yan_opt::optimize(file, m) {
        Ok(m) => m,
        Err(yan_opt::Error::Diag(d)) => {
            print_diags(&[*d]);
            return Err(ExitCode::FAILURE);
        }
        Err(yan_opt::Error::Unsupported(what)) => {
            return Err(tool_failure(&format!(
                "not supported yet: {what} in native code"
            )));
        }
    };
    yan_mir::lower(&m).map_err(|what| tool_failure(&format!("not supported yet: {what}")))
}

fn native(file: &str, m: &yan_core::ast::Module, jit: Jit) -> ExitCode {
    let jit = match jit {
        Jit::Lazy => yan_cg_cranelift::Jit::Lazy,
        Jit::Eager => yan_cg_cranelift::Jit::Eager,
    };
    let program = match lower_native(file, m) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let compiled = yan_cg_cranelift::compile_with(&program, jit);
    match compiled {
        Ok(c) => {
            c.run();
            ExitCode::SUCCESS
        }
        Err(what) => tool_failure(&format!("not supported yet: {what}")),
    }
}

fn tool_failure(msg: &str) -> ExitCode {
    eprintln!("yan: {msg}");
    ExitCode::from(TOOL_FAILURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_is_the_primer_and_its_examples() {
        assert!(SKILL.starts_with("You write Yan Agent"));
        assert_eq!(SKILL.matches("```yag").count(), 5);
    }
}
