//! `yan mcp` (21.4): the agent server over stdio, newline-delimited JSON-RPC 2.0.
//! It serves `check`, `check_patch`, `apply_patch`, `autofix`, `sig`, `spec_check` and
//! `run_tests`, answered from one query database that lives as long as the server. Code runs
//! only in a trusted root, under per-call and per-session budgets (R676).

use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use yan_db::Database;
use yan_db::Loaded;
use yan_diag::{DEFAULT_MAX_ERRORS, Diagnostic, Severity, finish};
use yan_interp::{Limits, Stop};

use crate::examples::{self, Kind};
use crate::patch::{self, Ops};
use crate::{test, trust};

const PROTOCOL: &str = "2026-07-28";

/// What code may spend: steps (calls and loop iterations), wall-clock time and heap bytes
/// above what the process held when the code started.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub steps: u64,
    pub wall: Duration,
    pub heap: usize,
}

const PER_CALL: Budget = Budget {
    steps: 200_000_000,
    wall: Duration::from_secs(60),
    heap: 1 << 30,
};

const PER_SESSION: Budget = Budget {
    steps: 2_000_000_000,
    wall: Duration::from_secs(600),
    heap: 1 << 30,
};

impl Budget {
    fn min(self, o: Budget) -> Budget {
        Budget {
            steps: self.steps.min(o.steps),
            wall: self.wall.min(o.wall),
            heap: self.heap.min(o.heap),
        }
    }

    /// The limits of one run, at most `default`'s steps.
    pub fn limits(&self, default: Limits) -> Limits {
        Limits {
            steps: self.steps.min(default.steps),
            heap: self.heap,
            deadline: Some(Instant::now() + self.wall),
            ..default
        }
    }

    pub fn spend(&mut self, steps: u64, wall: Duration) {
        self.steps = self.steps.saturating_sub(steps);
        self.wall = self.wall.saturating_sub(wall);
    }
}

/// One `yan mcp` session over the modules under `root`.
pub struct Server {
    root: PathBuf,
    db: RefCell<Database>,
    trusted: bool,
    left: RefCell<Budget>,
}

impl Server {
    pub fn new(root: &Path, trusted: bool) -> Server {
        Server {
            root: root.to_path_buf(),
            db: RefCell::new(session_db(trusted)),
            trusted,
            left: RefCell::new(PER_SESSION),
        }
    }

    /// The budget of one call, taken from the session's; `spend` settles it.
    fn call_budget(&self) -> Budget {
        PER_CALL.min(*self.left.borrow())
    }

    fn settle(&self, given: Budget, left: Budget) {
        self.left
            .borrow_mut()
            .spend(given.steps - left.steps, given.wall - left.wall);
    }

    /// `run_tests` and `query` run repository code; refused until `yan trust` (R676).
    fn require_trust(&self, tool: &str) -> Result<(), String> {
        if self.trusted {
            return Ok(());
        }
        Err(format!(
            "`{tool}` runs repository code, and {} is not trusted; run `yan trust` there first",
            self.root.display()
        ))
    }
}

fn session_db(trusted: bool) -> Database {
    let db = Database::new(false, false);
    if trusted { db } else { db.untrusted() }
}

pub fn serve(root: &Path) -> Result<(), String> {
    let server = Server::new(root, trust::is_trusted(root));
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => respond(&server, &req),
            Err(e) => Some(error(Value::Null, -32700, &e.to_string())),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")
                .and_then(|()| out.flush())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The reply to one message; notifications get none.
pub fn respond(server: &Server, req: &Value) -> Option<Value> {
    let id = req.get("id")?.clone();
    let params = &req["params"];
    let result = match req["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "yan", "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let args = &params["arguments"];
            match call(server, params["name"].as_str().unwrap_or(""), args) {
                Ok(r) => r,
                Err(msg) => json!({
                    "content": [{"type": "text", "text": msg}],
                    "isError": true,
                }),
            }
        }
        m => return Some(error(id, -32601, &format!("no method `{m}`"))),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn call(server: &Server, tool: &str, args: &Value) -> Result<Value, String> {
    let (root, db) = (server.root.as_path(), &server.db);
    // In the spec gate W806 is an error (21.1, R675).
    let diagnose = |p: &Path, src: &str| {
        let mut db = db.borrow_mut();
        let f = db.file(p, src);
        let mut diags = yan_ide::diagnostics(&*db, f)?;
        for d in diags.iter_mut().filter(|d| d.code == "W806") {
            d.severity = Severity::Error;
        }
        Ok(diags)
    };
    match tool {
        "check" => {
            let name = str_arg(args, "name")?;
            let (module, item) = split(root, name);
            let path = patch::module_path(root, &module)?;
            let src = read(&path)?;
            let mut db = db.borrow_mut();
            let f = db.file(&path, &src);
            let diags = match &item {
                Some(i) => yan_ide::item_diagnostics(&*db, f, i)?,
                None => yan_ide::diagnostics(&*db, f)?,
            };
            Ok(diagnostics(
                finish(diags, DEFAULT_MAX_ERRORS),
                max_tokens(args),
            ))
        }
        "check_patch" | "apply_patch" => {
            let p = args.get("patch").ok_or("missing `patch`")?.clone();
            let ops = match p {
                Value::String(text) => {
                    let module = str_arg(args, "module")?;
                    Ops::Plain(patch::module_path(root, module)?, text)
                }
                v => Ops::Json(v),
            };
            match patch::plan(root, ops, true, &diagnose)? {
                Err(diags) => Ok(diagnostics(diags, max_tokens(args))),
                Ok(applied) => {
                    let write = tool == "apply_patch";
                    if write {
                        patch::commit(root, &applied)?;
                    }
                    let hashes: serde_json::Map<String, Value> = applied
                        .hashes
                        .iter()
                        .map(|(n, h)| (n.clone(), h.clone().map_or(Value::Null, Value::String)))
                        .collect();
                    let verb = if write { "applied" } else { "applies" };
                    Ok(json!({
                        "content": [{"type": "text", "text": format!("the patch {verb}")}],
                        "structuredContent": {"ok": true, "written": write, "hashes": hashes},
                    }))
                }
            }
        }
        "autofix" => {
            let name = str_arg(args, "name")?;
            let (module, item) = split(root, name);
            autofix(root, &module, item.as_deref())
        }
        "sig" => {
            let module = str_arg(args, "module")?;
            let path = patch::module_path(root, module)?;
            let m = patch::parse(&path.display().to_string(), &read(&path)?)?;
            let budget = args["budget"].as_u64().map(|b| b as usize);
            let text = yan_ide::sig(&m, budget);
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "structuredContent": {"module": module, "text": text},
            }))
        }
        "spec_check" => {
            let module = str_arg(args, "module")?;
            let text = str_arg(args, "text")?;
            let given = server.call_budget();
            let mut left = given;
            let diags = spec_check(
                &patch::module_path(root, module)?,
                text,
                server.trusted,
                &mut left,
            );
            server.settle(given, left);
            let diags = diags?;
            Ok(diagnostics(
                finish(diags, DEFAULT_MAX_ERRORS),
                max_tokens(args),
            ))
        }
        "run_tests" => {
            server.require_trust(tool)?;
            let given = server.call_budget();
            let mut left = given;
            let r = test::run(
                root,
                args["filter"].as_str(),
                args["gate"].as_bool() == Some(true),
                Some(&test::cache_file(root)),
                Some(&mut left),
            );
            server.settle(given, left);
            let r = r?;
            let failed = r.failed(false);
            Ok(json!({
                "content": [{"type": "text", "text": r.lines().join("\n")}],
                "structuredContent": {"ok": !failed, "results": r.json()},
            }))
        }
        t => Err(format!("no tool `{t}`")),
    }
}

/// Phase 1 of the spec gate (21.1) for items whose bodies are `...`: their types resolve, each
/// `ex` line is well typed, and the arguments of each `ex` line satisfy `pre`. Nothing is written.
/// `pre` runs only in a trusted root, under `budget`.
fn spec_check(
    path: &Path,
    text: &str,
    trusted: bool,
    budget: &mut Budget,
) -> Result<Vec<Diagnostic>, String> {
    let file = path.display().to_string();
    let (src, exs) = examples::augment(&file, text, true);
    let mut db = session_db(trusted);
    let f = db.file(path, &src);
    let mut diags: Vec<Diagnostic> = examples::remap(yan_ide::diagnostics(&db, f)?, &exs)
        .into_iter()
        .filter(|d| d.code != "H701")
        .collect();
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Ok(diags);
    }
    let m = match yan_db::load(&db, f) {
        Ok(Loaded::Ready(m, _)) => m.clone(),
        Ok(Loaded::Stopped(d)) => return Ok(examples::remap(d.clone(), &exs)),
        Err(msg) => return Err(msg.clone()),
    };
    for x in exs
        .iter()
        .filter(|x| trusted && matches!(x.kind, Kind::Pre))
    {
        let mut out = Vec::new();
        let (start, mut steps) = (Instant::now(), 0);
        let r = yan_interp::test_metered(
            &m,
            &x.name,
            budget.limits(Limits::default()),
            &mut out,
            &mut steps,
        );
        budget.spend(steps, start.elapsed());
        let d = match r {
            Ok(_) => continue,
            Err(Stop::Panic(p)) if p.kind == "assert" => Diagnostic::error(
                EX_BREAKS_PRE,
                &file,
                x.span,
                &x.func,
                format!(
                    "the arguments of `ex` line {} break `pre` of `{}`",
                    x.k, x.func
                ),
            ),
            Err(Stop::Limit(what)) => {
                return Err(format!(
                    "`pre` of `{}` ran out of its {what} budget (R676)",
                    x.func
                ));
            }
            // `pre` that calls an unwritten body cannot be evaluated yet (21.1).
            Err(_) => continue,
        };
        diags.push(d.with_item(&x.func));
    }
    Ok(diags)
}

/// An `ex` line whose arguments break the function's `pre` (21.1, R621).
const EX_BREAKS_PRE: &str = "E619";

/// A diagnostics result: the line rendering for the model, the records for the harness (R295).
/// Under `max_tokens`, lines are kept while they fit (four bytes per token) and a last line
/// says how many were held back.
fn diagnostics(diags: Vec<Diagnostic>, max_tokens: Option<usize>) -> Value {
    let mut lines: Vec<String> = diags
        .iter()
        .enumerate()
        .map(|(k, d)| d.render_line(Some(k + 1)))
        .collect();
    if let Some(budget) = max_tokens {
        let mut used = 0;
        let keep = lines
            .iter()
            .take_while(|l| {
                used += l.len() / 4 + 1;
                used <= budget
            })
            .count();
        if keep < lines.len() {
            let held = lines.len() - keep;
            lines.truncate(keep);
            lines.push(format!("({held} more held back by max_tokens)"));
        }
    }
    let text = if lines.is_empty() {
        "no diagnostics".to_owned()
    } else {
        lines.join("\n")
    };
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {"diagnostics": diags},
    })
}

/// The patch that canonical printing makes to the item, or to every item of the module: it
/// applies the lenient reader's normalizations (14.4), the only machine fixes so far.
fn autofix(root: &Path, module: &str, item: Option<&str>) -> Result<Value, String> {
    let path = patch::module_path(root, module)?;
    let src = read(&path)?;
    let m = patch::parse(&path.display().to_string(), &src)?;
    if let Some(i) = item
        && patch::find(&m.items, i).is_none()
    {
        return Err(format!("`{module}` has no item `{i}`"));
    }
    let lines: Vec<&str> = src.lines().collect();
    let starts: Vec<usize> = m
        .items
        .iter()
        .map(|it| (it.span.line as usize).saturating_sub(1 + it.decorators.len()))
        .collect();
    let mut base = serde_json::Map::new();
    let mut replace = Vec::new();
    for (k, it) in m.items.iter().enumerate() {
        let name = it.kind.name();
        if item.is_some_and(|i| i != name) {
            continue;
        }
        let end = starts.get(k + 1).copied().unwrap_or(lines.len());
        let written = lines[starts[k]..end].join("\n");
        let canonical = patch::item_text(it);
        if written.trim_end() != canonical.trim_end() {
            base.insert(name.to_owned(), Value::String(patch::hash(it)));
            replace.push(Value::String(canonical.trim_end().to_owned()));
        }
    }
    let p = json!({"module": module, "base": base, "replace": replace});
    let text = if replace.is_empty() {
        "nothing to fix".to_owned()
    } else {
        p.to_string()
    };
    Ok(json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {"patch": p},
    }))
}

/// `pricing.value` is item `value` of module `pricing` when `pricing.yag` exists and
/// `pricing/value.yag` does not.
fn split(root: &Path, name: &str) -> (String, Option<String>) {
    if let Some((m, i)) = name.rsplit_once('.')
        && !patch::module_path(root, name).is_ok_and(|p| p.is_file())
        && patch::module_path(root, m).is_ok_and(|p| p.is_file())
    {
        return (m.to_owned(), Some(i.to_owned()));
    }
    (name.to_owned(), None)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key].as_str().ok_or_else(|| format!("missing `{key}`"))
}

fn max_tokens(args: &Value) -> Option<usize> {
    args["max_tokens"].as_u64().map(|n| n as usize)
}

fn read(p: &PathBuf) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn tools() -> Value {
    let diags_out = json!({
        "type": "object",
        "properties": {"diagnostics": {"type": "array", "items": {"type": "object"}}},
    });
    let patch_out = json!({
        "type": "object",
        "properties": {
            "ok": {"type": "boolean"},
            "written": {"type": "boolean"},
            "hashes": {"type": "object"},
            "diagnostics": {"type": "array", "items": {"type": "object"}},
        },
    });
    let patch_in = json!({
        "type": "object",
        "properties": {
            "patch": {"description": "an 18.3 patch object, or the text of whole items"},
            "module": {"type": "string", "description": "the module a plain text patch applies to"},
            "max_tokens": {"type": "integer"},
        },
        "required": ["patch"],
    });
    json!([
        {
            "name": "check",
            "description": "Check a module, or one item as module.item; returns diagnostics.",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}, "max_tokens": {"type": "integer"}},
                "required": ["name"],
            },
            "outputSchema": diags_out,
        },
        {
            "name": "check_patch",
            "description": "Check an 18.3 patch without writing; diagnostics, or success with the new hashes.",
            "inputSchema": patch_in,
            "outputSchema": patch_out,
        },
        {
            "name": "apply_patch",
            "description": "As check_patch, then write the module.",
            "inputSchema": patch_in,
            "outputSchema": patch_out,
        },
        {
            "name": "autofix",
            "description": "The patch that applies every machine-applicable fix to an item (module.item) or a module.",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
            },
            "outputSchema": {"type": "object", "properties": {"patch": {"type": "object"}}},
        },
        {
            "name": "spec_check",
            "description": "Phase 1 of the spec gate: items whose bodies are `...`; their types, their ex lines and pre for each ex line. Writes nothing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": {"type": "string"},
                    "text": {"type": "string", "description": "whole items with `...` bodies"},
                    "max_tokens": {"type": "integer"},
                },
                "required": ["module", "text"],
            },
            "outputSchema": diags_out,
        },
        {
            "name": "run_tests",
            "description": "Run test_ functions and ex lines; filter is a test id or a module or function prefix. Runs only in a root trusted with `yan trust`, under step, heap and wall-clock budgets.",
            "inputSchema": {
                "type": "object",
                "properties": {"filter": {"type": "string"}, "gate": {"type": "boolean"}},
            },
            "outputSchema": {
                "type": "object",
                "properties": {"ok": {"type": "boolean"}, "results": {"type": "array"}},
            },
        },
        {
            "name": "sig",
            "description": "Signatures, contracts and examples of a module, within an optional token budget.",
            "inputSchema": {
                "type": "object",
                "properties": {"module": {"type": "string"}, "budget": {"type": "integer"}},
                "required": ["module"],
            },
            "outputSchema": {
                "type": "object",
                "properties": {"module": {"type": "string"}, "text": {"type": "string"}},
            },
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_get_no_reply() {
        let d = Server::new(Path::new("."), true);
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(respond(&d, &note).is_none());
        let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
        let r = respond(&d, &list).unwrap();
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), 7);
    }

    #[test]
    fn module_names_stay_under_the_root() {
        let dir = std::env::temp_dir().join(format!("yan-mcp-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let outside = dir.join("outside");
        let root = dir.join("root");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(outside.join("v.yag"), "def f() -> int:\n    return 1\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let abs = outside.join("v").display().to_string().replace('/', ".");
        let d = Server::new(&root, true);
        for name in [abs.as_str(), "..v", "a..b", "link.v"] {
            let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "check", "arguments": {"name": name}}});
            let r = respond(&d, &req).unwrap();
            assert_eq!(r["result"]["isError"], true, "{name}");
        }
    }

    fn tool(s: &Server, name: &str, args: Value) -> Value {
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}});
        respond(s, &req).unwrap()["result"].clone()
    }

    fn package(tag: &str, src: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yan-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.yag"), src).unwrap();
        dir
    }

    #[test]
    fn untrusted_roots_run_no_code() {
        let dir = package("untrusted", "def test_k():\n    assert 1 + 2 == 3\n");

        let s = Server::new(&dir, false);
        let r = tool(&s, "run_tests", json!({}));
        assert_eq!(r["isError"], true);
        assert!(
            r["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("yan trust")
        );
        let stage0 = |trusted| {
            let mut db = session_db(trusted);
            let f = db.file(&dir.join("s.yn"), "let k: int = #(1 + 2)\n");
            let diags = yan_ide::diagnostics(&db, f).unwrap();
            diags.iter().any(|d| d.code == "E261")
        };
        assert!(stage0(false));
        assert!(!stage0(true));
        let r = tool(&Server::new(&dir, true), "run_tests", json!({}));
        assert_eq!(r["structuredContent"]["ok"], true, "{r}");
    }

    #[test]
    fn budgets_stop_runaway_tests() {
        let dir = package(
            "budget",
            "def test_spin():\n    n = 0\n    while True:\n        n += 1\n",
        );
        let s = Server::new(&dir, true);
        *s.left.borrow_mut() = Budget {
            steps: 10_000,
            ..PER_SESSION
        };
        let r = tool(&s, "run_tests", json!({}));
        assert_eq!(r["structuredContent"]["ok"], false, "{r}");
        assert_eq!(s.left.borrow().steps, 0);
        *s.left.borrow_mut() = Budget {
            wall: Duration::from_millis(50),
            ..PER_SESSION
        };
        let t = Instant::now();
        tool(&s, "run_tests", json!({}));
        assert!(t.elapsed() < Duration::from_secs(5));
        assert_eq!(s.left.borrow().wall, Duration::ZERO);
        std::fs::write(
            dir.join("m.yag"),
            "def test_grow():\n    xs = [0]\n    while True:\n        xs = xs + xs\n",
        )
        .unwrap();
        *s.left.borrow_mut() = Budget {
            heap: 1 << 20,
            ..PER_SESSION
        };
        let r = tool(&s, "run_tests", json!({}));
        assert!(
            r["content"][0]["text"].as_str().unwrap().contains("budget"),
            "{r}"
        );
    }

    #[test]
    fn w806_is_an_error_in_the_gate() {
        let dir = package(
            "w806",
            "pub def double(x: int) -> int:\n    \"\"\"Twice x.\"\"\"\n    ex double(3) == 6\n    ...\n",
        );
        let s = Server::new(&dir, true);
        let body = "pub def double(x: int) -> int:\n    \"\"\"Twice x.\"\"\"\n    ex double(3) == 6\n    if x == 3:\n        return 6\n    return x * 2\n";
        let r = tool(&s, "check_patch", json!({"module": "m", "patch": body}));
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("P003") && text.contains("W806"), "{text}");
        let lookup = body.replace(
            "    if x == 3:\n        return 6\n    return x * 2\n",
            "    table = {3: 6}\n    return table[x]\n",
        );
        let r = tool(&s, "check_patch", json!({"module": "m", "patch": lookup}));
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("W806") && text.contains("lookup"), "{text}");
    }

    #[test]
    fn attenuators_narrow_handlers() {
        let dir = package(
            "attenuate",
            "def describe(c: Command) -> str fail ProcError uses proc:\n    out = proc.run(c)?\n    return f\"ran {out.code}\"\n\ndef try_run(p: str) -> str uses proc:\n    return describe(Command(p)) else e -> f\"{e.reason}\"\n\ndef home_and_a() -> str uses env:\n    return f\"{env.var('HOME')} {env.var('A')} {env.vars()}\"\n\ndef test_env_only():\n    with env = fake_env(vars={\"HOME\": \"/h\", \"A\": \"1\"}):\n        with env = env.only(vars=[\"HOME\"]):\n            assert home_and_a() == \"/h None {'HOME': '/h'}\"\n            with env = env.only(vars=[\"A\"]):\n                assert home_and_a() == \"None None {}\"\n\ndef test_proc_only():\n    none = \"\"\n    ok = Output(code=0, signal=None, stdout=none.bytes(), stderr=none.bytes())\n    with proc = fake_proc({\"git\": ok, \"rm\": ok.copy()}):\n        with proc = proc.only(programs=[\"git\"]):\n            assert try_run(\"git\") == \"ran 0\"\n            assert try_run(\"rm\") == \"PermissionDenied()\"\n\ndef test_fs_sub_and_only():\n    with fs = fake_fs({\"/r/a/x.txt\": \"X\", \"/r/b/y.txt\": \"Y\"}):\n        with fs = fs.sub(\"/r/a\"):\n            assert (fs.read(\"x.txt\") else _ -> \"\") == \"X\"\n            assert (fs.read(\"../b/y.txt\") else e -> e.reason) == \"not granted\"\n            assert (fs.read(\"/r/b/y.txt\") else e -> e.reason) == \"not granted\"\n        with fs = fs.only(read=[\"/r/b\"]):\n            assert (fs.read(\"/r/b/y.txt\") else _ -> \"\") == \"Y\"\n            assert (fs.read(\"/r/a/x.txt\") else e -> e.reason) == \"not granted\"\n            assert (fs.read(\"/r/b/../a/x.txt\") else e -> e.reason) == \"not granted\"\n",
        );
        let r = tool(&Server::new(&dir, true), "run_tests", json!({}));
        assert_eq!(r["structuredContent"]["ok"], true, "{r}");
    }

    #[test]
    fn e326_for_another_capabilitys_attenuation() {
        let dir = package(
            "e326",
            "def f() -> str uses proc:\n    with env = proc.only(programs=[]):\n        return \"x\"\n",
        );
        let r = tool(&Server::new(&dir, true), "check", json!({"name": "m"}));
        assert!(
            r["content"][0]["text"].as_str().unwrap().contains("E326"),
            "{r}"
        );
    }

    #[test]
    fn e201_for_a_handler_of_another_capability() {
        let dir = package(
            "withtype",
            "def f() -> int uses rand:\n    with rand = fixed_time(Instant.epoch()):\n        return 1\n\ndef g() -> int uses rand:\n    with rand = seeded(1):\n        return 1\n",
        );
        let r = tool(&Server::new(&dir, true), "check", json!({"name": "m"}));
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("E201") && text.contains("Rand"), "{text}");
        assert_eq!(text.matches("E201").count(), 1, "{text}");
    }
}
