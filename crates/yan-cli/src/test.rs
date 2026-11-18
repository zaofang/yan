//! `yan test` (18.4, R300, R309): run the `test_` functions of a package, and with `--gate`
//! hold the approved tests in `tests.lock` to their hashes. Verdicts are cached as 18.4 allows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use yan_core::ast::{self as y, ItemKind};
use yan_core::hash::Hashed;
use yan_db::{Database, Loaded};
use yan_diag::{DEFAULT_MAX_ERRORS, Diagnostic, Severity, Span, finish};
use yan_interp::{Limits, Stop};

use crate::mcp::Budget;

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
    pub cached: bool,
    /// Keys of the module's items the test reaches, itself included.
    pub reach: Vec<String>,
    /// The seed and fault plan hash of a failing simulated run (RFC 0070 D3).
    pub sim: Option<(i128, String)>,
}

/// `yan test --sim[=N]` (RFC 0070): `sweep` seeds per simulated test, or exactly `seed`.
#[derive(Clone, Copy)]
pub struct Sim {
    pub sweep: u32,
    pub seed: Option<i128>,
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
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail(_) => "fail",
            Verdict::Budget => "budget",
        }
    }

    fn json(&self) -> serde_json::Value {
        match self {
            Verdict::Fail(k) => serde_json::json!({"verdict": "fail", "panic": k}),
            v => serde_json::json!({"verdict": v.name()}),
        }
    }

    fn from_json(v: &serde_json::Value) -> Option<Verdict> {
        match v.get("verdict")?.as_str()? {
            "pass" => Some(Verdict::Pass),
            "budget" => Some(Verdict::Budget),
            "fail" => Some(Verdict::Fail(v.get("panic")?.as_str()?.to_owned())),
            _ => None,
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
                "cached": t.cached,
                "repro": repro(&t.id, t.sim.as_ref()),
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
                Verdict::Fail(k) => match &t.sim {
                    Some((seed, plan)) => format!("failed: {k} | seed: {seed} | faults: {plan}"),
                    None => format!("failed: {k}"),
                },
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
                repro(&t.id, t.sim.as_ref())
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

fn repro(id: &str, sim: Option<&(i128, String)>) -> String {
    match sim {
        Some((seed, _)) => format!("yan test --sim --seed {seed} --only {id}"),
        None => format!("yan test --only {id}"),
    }
}

/// The modules of the package at `root`: every `.yag` and `.yn` file under `src/`, or under
/// `root` when it has no `src/`, keyed by module path, then the black-box roots `tests/` and
/// `examples/` keyed with the root's name leading (RFC 0084 D8).
pub fn modules(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let has_src = root.join("src").is_dir();
    let base = if has_src {
        root.join("src")
    } else {
        root.to_path_buf()
    };
    let mut out = BTreeMap::new();
    collect(root, &base, "", &mut out)?;
    if has_src {
        for black_box in ["tests", "examples"] {
            collect(root, &root.join(black_box), black_box, &mut out)?;
        }
    }
    Ok(out)
}

fn collect(
    root: &Path,
    base: &Path,
    lead: &str,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), String> {
    let mut dirs = vec![base.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            if d == base && !lead.is_empty() {
                return Ok(());
            }
            return Err(format!("{}: cannot read", d.display()));
        };
        for e in entries {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != ".yir") {
                    dirs.push(p);
                }
                continue;
            }
            let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext, "yag" | "yn") || p == root.join(format!("package.{ext}")) {
                continue;
            }
            let rel = p
                .strip_prefix(base)
                .expect("under the base")
                .with_extension("");
            let name: Vec<String> = (!lead.is_empty())
                .then(|| lead.to_owned())
                .into_iter()
                .chain(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned()),
                )
                .collect();
            out.insert(name.join("."), p);
        }
    }
    Ok(())
}

/// A locked test: its path, hash and the line of `tests.lock` that names it.
pub struct Locked {
    pub path: String,
    pub hash: String,
    line: u32,
}

pub fn lock(root: &Path) -> Result<Vec<Locked>, String> {
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

/// The canonical Yan Core text of one item of `m`.
pub fn yan_text(m: &y::Module, it: &y::Item) -> String {
    yan_core::print(&y::Module {
        header: Vec::new(),
        path: m.path.clone(),
        items: vec![it.clone()],
    })
}

/// The test hash (R300): the test function's canonical Yan Core text, hashed.
fn test_hash(m: &y::Module, it: &y::Item) -> String {
    let text = yan_text(m, it);
    format!(
        "blake3:{}",
        blake3::hash(format!("yan test hash 1\0{text}").as_bytes()).to_hex()
    )
}

/// `assert failed at f.yag:7: \`x == 1\`; left is 0, right is 1`, from the panic's
/// `line N[: values]` message.
fn assert_failed(file: &str, src: &str, msg: &str) -> String {
    let (line, values) = msg.split_once(": ").unwrap_or((msg, ""));
    let Some(n) = line
        .strip_prefix("line ")
        .and_then(|n| n.parse::<usize>().ok())
    else {
        return format!("panic assert: {msg}");
    };
    let text = src.lines().nth(n - 1).unwrap_or("").trim();
    let text = text.strip_prefix("assert ").unwrap_or(text);
    let mut out = format!("assert failed at {file}:{n}: `{text}`");
    if !values.is_empty() {
        out += &format!("; {values}");
    }
    out
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

/// BLAKE3 of the running `yan` binary.
fn toolchain_hash() -> &'static str {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| {
        let exe = std::env::current_exe().and_then(std::fs::read);
        exe.map_or_else(|_| String::new(), |b| blake3::hash(&b).to_hex().to_string())
    })
}

/// Indices of the items `start` reaches through references, itself included, in order.
fn reach(hashes: &[Hashed], start: usize) -> Vec<usize> {
    let mut seen = vec![false; hashes.len()];
    let mut todo = vec![start];
    while let Some(i) = todo.pop() {
        if !std::mem::replace(&mut seen[i], true) {
            todo.extend(&hashes[i].refs);
        }
    }
    (0..hashes.len()).filter(|&i| seen[i]).collect()
}

/// Constructors of handlers that are pure functions of their arguments: the test handler set
/// of 7.3 and the `std.sim` hosts (RFC 0070 D1).
const SIM_HANDLERS: &[&str] = &[
    "sim",
    "mem_net",
    "fake_fs",
    "fake_env",
    "fake_proc",
    "virtual",
    "seeded",
    "never",
];

/// How a test binds capabilities.
enum Binds {
    /// No `with` in anything it reaches.
    None,
    /// Only simulated handlers, as sorted `name = constructor` bindings.
    Sim(Vec<String>),
    /// At least one other handler: an I/O test (R309).
    Real,
}

fn binds(m: &y::Module, reached: &[usize]) -> Binds {
    let mut sim = Vec::new();
    for &i in reached {
        let text = yan_text(m, &m.items[i]);
        // Canonical Core binds a handler built on an earlier `let _tN: T = path(...)` line.
        let built = |name: &str| {
            text.lines().find_map(|l| {
                let (lhs, rhs) = l.trim_start().strip_prefix("let ")?.split_once(" = ")?;
                (lhs.split(':').next() == Some(name)).then_some(rhs)
            })
        };
        for l in text.lines() {
            let Some(rest) = l.trim_start().strip_prefix("with ") else {
                continue;
            };
            let (name, rhs) = rest.split_once(" = ").unwrap_or((rest, ""));
            let rhs = rhs.trim_end_matches('{').trim();
            let call = built(rhs).unwrap_or(rhs);
            let ctor = call.split('(').next().unwrap_or("").trim();
            if !SIM_HANDLERS.contains(&ctor.rsplit('.').next().unwrap_or("")) {
                return Binds::Real;
            }
            sim.push(format!("{} = {ctor}", name.trim()));
        }
    }
    if sim.is_empty() {
        return Binds::None;
    }
    sim.sort();
    sim.dedup();
    Binds::Sim(sim)
}

/// The verdict cache key of 18.4, or None for an I/O test. A simulated test's key also holds
/// its handler bindings and the seed override (RFC 0070 D1).
fn cache_key(
    m: &y::Module,
    hashes: &[Hashed],
    reached: &[usize],
    test: &str,
    seed: Option<i128>,
) -> Option<String> {
    let bound = match binds(m, reached) {
        Binds::Real => return None,
        Binds::None => Vec::new(),
        Binds::Sim(b) => b,
    };
    if toolchain_hash().is_empty() {
        return None;
    }
    let mut h = blake3::Hasher::new();
    h.update(b"yan verdict key 1\0");
    h.update(test.as_bytes());
    for &i in reached {
        h.update(b"\0");
        h.update(hashes[i].identity.to_string().as_bytes());
    }
    for b in &bound {
        h.update(b"\0with\0");
        h.update(b.as_bytes());
    }
    if let Some(s) = seed {
        h.update(b"\0seed\0");
        h.update(&s.to_le_bytes());
    }
    h.update(b"\0");
    h.update(toolchain_hash().as_bytes());
    Some(h.finalize().to_hex().to_string())
}

/// The verdict cache: key to `{verdict, panic?, output}`.
struct Cache {
    file: PathBuf,
    entries: serde_json::Map<String, serde_json::Value>,
    dirty: bool,
}

impl Cache {
    fn open(file: &Path) -> Cache {
        let entries = std::fs::read_to_string(file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Cache {
            file: file.to_path_buf(),
            entries,
            dirty: false,
        }
    }

    fn get(&self, key: &str) -> Option<(Verdict, String)> {
        let e = self.entries.get(key)?;
        let output = e.get("output")?.as_str()?.to_owned();
        Some((Verdict::from_json(e)?, output))
    }

    fn put(&mut self, key: String, v: &Verdict, output: &str) {
        let mut e = v.json();
        e["output"] = output.into();
        self.entries.insert(key, e);
        self.dirty = true;
    }

    fn save(&self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let err = |e: std::io::Error| format!("{}: {e}", self.file.display());
        if let Some(d) = self.file.parent() {
            std::fs::create_dir_all(d).map_err(err)?;
        }
        let text = serde_json::to_string(&self.entries).expect("records serialize");
        std::fs::write(&self.file, text).map_err(err)
    }
}

/// The cache file of the package at `root`.
pub fn cache_file(root: &Path) -> PathBuf {
    root.join(".yan/cache/verdicts.json")
}

/// Run every test of the package at `root` that `filter` selects: `test_` functions and `ex`
/// lines (R309), reusing verdicts from `cache`.
pub fn run(
    root: &Path,
    filter: Option<&str>,
    gate: bool,
    cache: Option<&Path>,
    budget: Option<&mut Budget>,
) -> Result<Report, String> {
    run_sim(root, filter, gate, cache, budget, None)
}

/// The sweep seeds of a test are a function of its hash and an index, the same on every
/// machine (RFC 0070 D3).
fn derive_seed(test_hash: &str, index: u32) -> i128 {
    let mut h = blake3::Hasher::new();
    h.update(b"yan sim seed 1\0");
    h.update(test_hash.as_bytes());
    h.update(&index.to_le_bytes());
    let b = h.finalize();
    i128::from(u64::from_le_bytes(b.as_bytes()[..8].try_into().expect("8 bytes")) >> 1)
}

/// BLAKE3 prefix of a fault plan: the fired points, each as name and occurrence.
fn plan_hash(fired: &[(String, u64)]) -> String {
    let mut h = blake3::Hasher::new();
    for (n, k) in fired {
        h.update(format!("{n}#{k}\n").as_bytes());
    }
    h.finalize().to_hex()[..16].to_owned()
}

pub fn run_sim(
    root: &Path,
    filter: Option<&str>,
    gate: bool,
    cache: Option<&Path>,
    mut budget: Option<&mut Budget>,
    sim: Option<Sim>,
) -> Result<Report, String> {
    let locked = lock(root)?;
    let mut cache = cache.map(Cache::open);
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
        for d in db.graph_diagnostics() {
            if !diags.contains(&d) {
                diags.push(d);
            }
        }
        let m = match yan_db::load(&db, f) {
            Ok(Loaded::Ready(m, _, _)) => {
                let d = examples::remap(yan_ide::diagnostics(&db, f)?, &exs);
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
        let hashes = yan_core::hash::hash_module(&m);
        for (i, it) in m.items.iter().enumerate() {
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
            let hash = found[&id].0.clone();
            let reached = reach(&hashes, i);
            let simulated = matches!(binds(&m, &reached), Binds::Sim(_));
            let seeds: Vec<Option<i128>> = match sim {
                Some(Sim { seed: Some(s), .. }) if simulated => vec![Some(s)],
                Some(Sim { sweep, .. }) if simulated => {
                    (0..sweep).map(|n| Some(derive_seed(&hash, n))).collect()
                }
                _ => vec![None],
            };
            let (mut verdict, mut output, mut cached, mut failed_run) =
                (Verdict::Pass, String::new(), true, None);
            for seed in seeds {
                let key = cache
                    .as_ref()
                    .and_then(|_| cache_key(&m, &hashes, &reached, &hash, seed));
                let hit = key
                    .as_ref()
                    .and_then(|k| cache.as_ref().and_then(|c| c.get(k)))
                    // A failing simulated run reruns so its fault plan can be printed.
                    .filter(|(v, _)| *v == Verdict::Pass || !simulated);
                let (v, out) = match hit {
                    Some(v) => v,
                    None => {
                        cached = false;
                        let mut out = Vec::new();
                        let limits = budget
                            .as_ref()
                            .map_or(Limits::default(), |b| b.limits(Limits::default()));
                        let (start, mut steps) = (Instant::now(), 0);
                        let (r, fired) =
                            yan_interp::test_sim(&m, &fname, limits, &mut out, &mut steps, seed);
                        if let Some(b) = budget.as_mut() {
                            b.spend(steps, start.elapsed());
                        }
                        let v = match r {
                            Ok(true) => Verdict::Pass,
                            Ok(false) => Verdict::Fail("returned a failure".into()),
                            Err(Stop::Panic(p)) if p.kind == "assert" && ex.is_some() => {
                                Verdict::Fail(format!(
                                    "`{}` does not hold",
                                    ex.expect("an ex line").text
                                ))
                            }
                            Err(Stop::Panic(p)) if p.kind == "assert" => {
                                Verdict::Fail(assert_failed(&shown, &text, &p.message))
                            }
                            Err(Stop::Panic(p)) => {
                                Verdict::Fail(format!("panic({}): {}", p.kind, p.message))
                            }
                            Err(Stop::Limit(_)) => Verdict::Budget,
                            Err(Stop::Unsupported(what)) => {
                                return Err(format!("{id}: not supported yet: {what}"));
                            }
                        };
                        let out = String::from_utf8_lossy(&out).into_owned();
                        if v != Verdict::Pass && simulated {
                            failed_run = Some((seed, plan_hash(&fired)));
                        }
                        // A budget verdict depends on the session, not only the code (R676).
                        let metered = budget.is_some() && matches!(v, Verdict::Budget);
                        if let (Some(c), Some(k), false) = (cache.as_mut(), key, metered) {
                            c.put(k, &v, &out);
                        }
                        (v, out)
                    }
                };
                if v != Verdict::Pass {
                    (verdict, output) = (v, out);
                    break;
                }
                output = out;
            }
            let sim_fail = failed_run.and_then(|(s, p)| s.map(|s| (s, p)));
            tests.push(Record {
                approved: false,
                hash,
                id,
                verdict,
                output,
                cached,
                reach: reached.iter().map(|&j| hashes[j].key.clone()).collect(),
                sim: sim_fail,
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
                         approve it again with `yan approve tests`",
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
    if let Some(c) = &cache {
        c.save()?;
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
