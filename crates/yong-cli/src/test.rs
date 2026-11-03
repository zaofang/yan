//! `yong test` (18.4, R300, R309): run the `test_` functions of a package, and with `--gate`
//! hold the approved tests in `tests.lock` to their hashes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yong_core::ast::{self as y, ItemKind};
use yong_db::{Database, Loaded};
use yong_diag::{DEFAULT_MAX_ERRORS, Diagnostic, Severity, Span, finish};
use yong_interp::{Limits, Stop};

use crate::examples;

/// One test's outcome.
pub struct Record {
    /// `module.test_name`.
    pub id: String,
    pub hash: String,
    pub approved: bool,
    pub verdict: Verdict,
    /// What the test printed, shown when it fails.
    pub output: String,
}

#[derive(Clone, PartialEq)]
pub enum Verdict {
    Pass,
    /// The test returned a failure or panicked with this kind.
    Fail(String),
    /// The deterministic step budget ran out (R309).
    Budget,
}

impl Verdict {
    fn name(&self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail(_) => "fail",
            Verdict::Budget => "budget",
        }
    }
}

pub struct Report {
    pub diags: Vec<Diagnostic>,
    pub tests: Vec<Record>,
}

impl Report {
    /// Whether the run failed: an error, or under `--gate` a failing approved test, else any
    /// failing test.
    pub fn failed(&self, gate: bool) -> bool {
        self.diags.iter().any(|d| d.severity == Severity::Error)
            || self
                .tests
                .iter()
                .any(|t| t.verdict != Verdict::Pass && (t.approved || !gate))
    }

    pub fn json(&self) -> serde_json::Value {
        let mut out: Vec<serde_json::Value> = self
            .diags
            .iter()
            .map(|d| serde_json::to_value(d).expect("records serialize"))
            .collect();
        out.extend(self.tests.iter().map(|t| {
            serde_json::json!({
                "kind": "test",
                "id": t.id,
                "hash": t.hash,
                "approved": t.approved,
                "verdict": t.verdict.name(),
                "panic": match &t.verdict {
                    Verdict::Fail(k) => Some(k.as_str()),
                    _ => None,
                },
                "cached": false,
                "repro": repro(&t.id),
            })
        }));
        serde_json::Value::Array(out)
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.diags.iter().map(|d| d.render_line(None)).collect();
        for t in &self.tests {
            let status = if t.approved { "approved" } else { "proposed" };
            let why = match &t.verdict {
                Verdict::Pass => continue,
                Verdict::Fail(k) => format!("failed: {k}"),
                Verdict::Budget => "ran out of its step budget".to_owned(),
            };
            out.push(format!(
                "FAIL {} ({status}) {why}{} | repro: {}",
                t.id,
                if t.output.is_empty() {
                    String::new()
                } else {
                    format!(" | output: {}", t.output.trim_end().replace('\n', "\\n"))
                },
                repro(&t.id)
            ));
        }
        let passed = self.tests.iter().filter(|t| t.verdict == Verdict::Pass);
        out.push(format!(
            "{} of {} tests passed",
            passed.count(),
            self.tests.len()
        ));
        out
    }
}

fn repro(id: &str) -> String {
    format!("yong test --only {id}")
}

/// The modules of the package at `root`: every `.xin` and `.wen` file under `src/`, or under
/// `root` when it has no `src/`, keyed by module path.
fn modules(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let base = if root.join("src").is_dir() {
        root.join("src")
    } else {
        root.to_path_buf()
    };
    let mut out = BTreeMap::new();
    let mut dirs = vec![base.clone()];
    while let Some(d) = dirs.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for e in entries {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != ".yong") {
                    dirs.push(p);
                }
                continue;
            }
            let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext, "xin" | "wen") || p == root.join(format!("package.{ext}")) {
                continue;
            }
            let rel = p
                .strip_prefix(&base)
                .expect("under the base")
                .with_extension("");
            let name: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.insert(name.join("."), p);
        }
    }
    Ok(out)
}

/// A locked test: its path, hash and the line of `tests.lock` that names it.
struct Locked {
    path: String,
    hash: String,
    line: u32,
}

fn lock(root: &Path) -> Result<Vec<Locked>, String> {
    let file = root.join("tests.lock");
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(Vec::new());
    };
    let doc: toml::Table = text.parse().map_err(|e| format!("tests.lock: {e}"))?;
    if doc.get("format").and_then(toml::Value::as_integer) != Some(1) {
        return Err("tests.lock: expected `format = 1`".into());
    }
    let Some(tests) = doc.get("test") else {
        return Ok(Vec::new());
    };
    let tests = tests
        .as_array()
        .ok_or("tests.lock: `test` is not an array of tables")?;
    let mut out = Vec::new();
    for t in tests {
        let field = |k: &str| {
            t.get(k)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .ok_or(format!("tests.lock: a test without `{k}`"))
        };
        let path = field("path")?;
        let line = text
            .lines()
            .position(|l| l.contains(&format!("\"{path}\"")))
            .map_or(1, |k| k as u32 + 1);
        out.push(Locked {
            hash: field("hash")?,
            path,
            line,
        });
    }
    Ok(out)
}

/// The test hash (R300): the test function's canonical Yong text, hashed.
fn test_hash(m: &y::Module, it: &y::Item) -> String {
    let one = y::Module {
        header: Vec::new(),
        path: m.path.clone(),
        items: vec![it.clone()],
    };
    let text = yong_core::print(&one);
    format!(
        "blake3:{}",
        blake3::hash(format!("yong test hash 1\0{text}").as_bytes()).to_hex()
    )
}

/// Whether test `id` is selected by `filter`: the id itself, or a module or function prefix.
fn selected(id: &str, filter: Option<&str>) -> bool {
    filter.is_none_or(|f| {
        id == f
            || id
                .strip_prefix(f)
                .is_some_and(|rest| rest.starts_with(['.', '#']))
    })
}

/// Run every test of the package at `root` that `filter` selects: `test_` functions and `ex`
/// lines (R309).
pub fn run(root: &Path, filter: Option<&str>, gate: bool) -> Result<Report, String> {
    let locked = lock(root)?;
    let mut diags = Vec::new();
    let mut tests = Vec::new();
    let mut found = BTreeMap::new();
    for (name, file) in modules(root)? {
        let mut db = Database::new(false, false);
        let shown = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        let src = std::fs::read_to_string(&file).map_err(|e| format!("{shown}: {e}"))?;
        let (text, exs) = examples::augment(&shown, &src, false);
        let f = db.file(&file, &text);
        let m = match yong_db::load(&db, f) {
            Ok(Loaded::Ready(m, _)) => {
                let d = examples::remap(yong_ide::diagnostics(&db, f)?, &exs);
                let errors = d.iter().any(|d| d.severity == Severity::Error);
                diags.extend(d.into_iter().filter(|d| d.severity == Severity::Error));
                if errors {
                    continue;
                }
                m.clone()
            }
            Ok(Loaded::Stopped(d)) => {
                diags.extend(
                    examples::remap(d.clone(), &exs)
                        .into_iter()
                        .filter(|d| d.severity == Severity::Error),
                );
                continue;
            }
            Err(msg) => return Err(msg.clone()),
        };
        for it in &m.items {
            let ItemKind::Fn(fd) = &it.kind else {
                continue;
            };
            let fname = fd.path.join(".");
            let ex = exs.iter().find(|x| x.name == fname);
            let id = match ex {
                Some(x) => x.id(&name),
                None if fname.starts_with("test_") => format!("{name}.{fname}"),
                None => continue,
            };
            found.insert(id.clone(), (test_hash(&m, it), shown.clone(), it.span));
            if !selected(&id, filter) {
                continue;
            }
            let mut out = Vec::new();
            let verdict = match yong_interp::test(&m, &fname, Limits::default(), &mut out) {
                Ok(true) => Verdict::Pass,
                Ok(false) => Verdict::Fail("returned a failure".into()),
                Err(Stop::Panic(k)) if k == "assert" && ex.is_some() => {
                    Verdict::Fail(format!("`{}` does not hold", ex.expect("an ex line").text))
                }
                Err(Stop::Panic(k)) => Verdict::Fail(format!("panic {k}")),
                Err(Stop::Limit(_)) => Verdict::Budget,
                Err(Stop::Unsupported(what)) => {
                    return Err(format!("{id}: not supported yet: {what}"));
                }
            };
            tests.push(Record {
                approved: false,
                hash: found[&id].0.clone(),
                id,
                verdict,
                output: String::from_utf8_lossy(&out).into_owned(),
            });
        }
    }
    for l in &locked {
        match found.get(&l.path) {
            None if gate => diags.push(
                Diagnostic::error(
                    "E015",
                    "tests.lock",
                    line_span(l.line),
                    &l.path,
                    format!("the approved test `{}` is missing", l.path),
                )
                .with_item(&l.path),
            ),
            Some((hash, file, span)) if gate && *hash != l.hash => diags.push(
                Diagnostic::error(
                    "E014",
                    file,
                    Span {
                        line: span.line,
                        col: span.col,
                        end_line: span.line,
                        end_col: span.col,
                    },
                    &l.path,
                    format!(
                        "the approved test `{}` changed since tests.lock was signed; \
                         approve it again with `yong approve tests`",
                        l.path
                    ),
                )
                .with_item(&l.path)
                .with_note(format!("locked {}, now {hash}", l.hash)),
            ),
            _ => {}
        }
        if let Some(t) = tests
            .iter_mut()
            .find(|t| t.id == l.path && t.hash == l.hash)
        {
            t.approved = true;
        }
    }
    Ok(Report {
        diags: finish(diags, DEFAULT_MAX_ERRORS),
        tests,
    })
}

fn line_span(line: u32) -> Span {
    Span {
        line,
        col: 1,
        end_line: line,
        end_col: 1,
    }
}
