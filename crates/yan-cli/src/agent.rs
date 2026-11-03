//! `yan agent`: the reference agent loop. It runs one task through the two phases of the spec
//! gate (21.1) as a client of the `yan mcp` tools (21.4), and writes a run record that the evaluation
//! harness (21.6) reads. The model is behind an adapter: `stub:FILE` replays scripted replies,
//! `cmd:PROGRAM` talks to a process that answers one JSON line per turn.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use yan_db::Database;
use yan_hir::ItemKind;

use crate::{mcp, patch};

pub struct Options {
    pub task: PathBuf,
    pub model: String,
    pub primer: Option<PathBuf>,
    pub shots: Option<PathBuf>,
    pub dir: Option<PathBuf>,
    pub module: String,
    pub spec_repairs: usize,
    pub body_repairs: usize,
    pub max_tokens: u64,
}

const SYSTEM: &str = "You write Yan Agent, the typed Python-shaped surface of Yan.";

const SPEC_ASK: &str = "Task: {prompt}\n\nPhase 1, the spec. Write the Yan Agent declarations the task \
needs and each function's def line, docstring, any pre and post lines and at least one ex line, \
with `...` as the rest of each body. Reply with one ```yag code block only.";

const BODY_ASK: &str = "Phase 2, the bodies. The spec is frozen: keep every line you wrote \
unchanged and replace each `...` body with the implementation. Reply with the whole file in one \
```yag code block only. If a frozen ex line or test is wrong, reply with one line \
`STOP: <reason>` instead.";

const REPAIR_ASK: &str = "{tool} reported:\n{report}\n\nReply with the whole corrected file in \
one ```yag code block only.";

#[derive(Default, Clone, Copy)]
struct Usage {
    input: u64,
    output: u64,
    reasoning: u64,
}

impl Usage {
    fn add(&mut self, u: Usage) {
        self.input += u.input;
        self.output += u.output;
        self.reasoning += u.reasoning;
    }

    fn json(self) -> Value {
        json!({"in": self.input, "out": self.output, "reasoning": self.reasoning})
    }
}

struct Reply {
    text: String,
    usage: Usage,
    model: Option<String>,
}

enum Model {
    Stub {
        replies: Vec<Value>,
        next: usize,
    },
    Cmd {
        child: Child,
        io: Lines,
        program: String,
    },
}

impl Model {
    fn open(spec: &str) -> Result<Model, String> {
        if let Some(file) = spec.strip_prefix("stub:") {
            let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
            let replies = v
                .as_array()
                .ok_or(format!("{file}: expected an array of replies"))?
                .clone();
            return Ok(Model::Stub { replies, next: 0 });
        }
        if let Some(cmd) = spec.strip_prefix("cmd:") {
            let mut words = cmd.split_whitespace();
            let program = words.next().ok_or("cmd: names no program")?;
            let (child, io) = spawn(Command::new(program).args(words))?;
            return Ok(Model::Cmd {
                child,
                io,
                program: cmd.to_owned(),
            });
        }
        Err(format!(
            "unknown model `{spec}`; use stub:FILE or cmd:PROGRAM"
        ))
    }

    fn id(&self) -> String {
        match self {
            Model::Stub { .. } => "stub".into(),
            Model::Cmd { program, .. } => format!("cmd:{program}"),
        }
    }

    fn chat(&mut self, system: &str, msgs: &[Value], max_tokens: u64) -> Result<Reply, String> {
        let v = match self {
            Model::Stub { replies, next } => {
                let r = replies
                    .get(*next)
                    .cloned()
                    .ok_or(format!("the stub has no reply {}", *next + 1))?;
                *next += 1;
                match r {
                    Value::String(text) => json!({"text": text}),
                    r => r,
                }
            }
            Model::Cmd { io, .. } => io.ask(&json!({
                "system": system,
                "messages": msgs,
                "max_tokens": max_tokens,
            }))?,
        };
        if let Some(e) = v["error"].as_str() {
            return Err(format!("model: {e}"));
        }
        let n = |k: &str| v["usage"][k].as_u64().unwrap_or(0);
        Ok(Reply {
            text: v["text"]
                .as_str()
                .ok_or("model reply has no `text`")?
                .to_owned(),
            usage: Usage {
                input: n("in"),
                output: n("out"),
                reasoning: n("reasoning"),
            },
            model: v["model"].as_str().map(str::to_owned),
        })
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        if let Model::Cmd { child, .. } = self {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A child process spoken to in newline-delimited JSON.
struct Lines {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Lines {
    fn ask(&mut self, msg: &Value) -> Result<Value, String> {
        writeln!(self.stdin, "{msg}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        if self
            .stdout
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("the child process closed its output".into());
        }
        serde_json::from_str(&line).map_err(|e| format!("bad reply `{}`: {e}", line.trim()))
    }
}

fn spawn(cmd: &mut Command) -> Result<(Child, Lines), String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("{cmd:?}: {e}"))?;
    let io = Lines {
        stdin: child.stdin.take().expect("piped"),
        stdout: BufReader::new(child.stdout.take().expect("piped")),
    };
    Ok((child, io))
}

/// The `yan mcp` server for the work directory, spoken to in its own JSON-RPC messages but
/// in this process.
struct Mcp {
    root: PathBuf,
    db: RefCell<Database>,
    id: u64,
    calls: Vec<Value>,
}

impl Mcp {
    fn start(dir: &Path) -> Result<Mcp, String> {
        let mut m = Mcp {
            root: dir.to_path_buf(),
            db: RefCell::new(Database::new(false, false)),
            id: 0,
            calls: Vec::new(),
        };
        m.request("initialize", json!({"protocolVersion": "2026-07-28"}))?;
        Ok(m)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.id += 1;
        let req = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        let r = mcp::respond(&self.root, &self.db, &req).ok_or("yan mcp gave no reply")?;
        match r.get("error") {
            Some(e) => Err(format!("yan mcp: {e}")),
            None => Ok(r["result"].clone()),
        }
    }

    /// One tool call: its text for the model and its structured result; recorded for the run.
    fn call(&mut self, tool: &str, args: Value) -> Result<Called, String> {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}))?;
        let text = r["content"][0]["text"].as_str().unwrap_or("").to_owned();
        if r["isError"].as_bool() == Some(true) {
            return Err(format!("{tool}: {text}"));
        }
        let data = r["structuredContent"].clone();
        let codes: Vec<Value> = data["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d.get("code").cloned())
            .collect();
        let mut rec = json!({"tool": tool, "codes": codes});
        if tool == "run_tests" {
            rec["failed"] = data["results"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| t["kind"] == "test" && t["verdict"] != "pass")
                .map(|t| t["id"].clone())
                .collect();
        }
        if tool == "autofix" {
            rec["replaced"] = json!(data["patch"]["replace"].as_array().map_or(0, Vec::len));
        }
        self.calls.push(rec);
        Ok(Called { text, data })
    }
}

struct Called {
    text: String,
    data: Value,
}

impl Called {
    fn errors(&self) -> bool {
        self.data["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|d| d["severity"] == "error")
    }
}

/// A tool error for code the toolchain cannot judge yet; the run ends with `unsupported` set.
const UNSUPPORTED: &str = "not supported yet";

/// The run record (21.6): schema 1.
pub fn run(o: &Options) -> Result<(Value, bool), String> {
    let task: Value = serde_json::from_str(
        &std::fs::read_to_string(&o.task).map_err(|e| format!("{}: {e}", o.task.display()))?,
    )
    .map_err(|e| format!("{}: {e}", o.task.display()))?;
    let id = task["id"].as_str().unwrap_or("task").to_owned();
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        || id.starts_with('.')
    {
        return Err(format!(
            "{}: task id `{id}` must be letters, digits, `_`, `-` and `.`",
            o.task.display()
        ));
    }
    let prompt = task["prompt"]
        .as_str()
        .ok_or(format!("{}: the task has no `prompt`", o.task.display()))?;
    let dir = match &o.dir {
        Some(d) => {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
            d.clone()
        }
        None => fresh_dir(&id)?,
    };
    let system = system_prompt(o)?;
    let mut model = Model::open(&o.model)?;
    let mut mcp = Mcp::start(&dir)?;
    let mut msgs: Vec<Value> = Vec::new();
    let mut rounds: Vec<Value> = Vec::new();
    let mut totals: BTreeMap<&str, Usage> = BTreeMap::new();
    let mut model_id = model.id();
    let mut ask = |model: &mut Model,
                   msgs: &mut Vec<Value>,
                   totals: &mut BTreeMap<&str, Usage>,
                   phase: &'static str,
                   content: String|
     -> Result<(String, Usage), String> {
        msgs.push(json!({"role": "user", "content": content}));
        let r = model.chat(&system, msgs, o.max_tokens)?;
        msgs.push(json!({"role": "assistant", "content": r.text}));
        totals.entry(phase).or_default().add(r.usage);
        if let Some(m) = r.model {
            model_id = m;
        }
        Ok((r.text, r.usage))
    };

    // Phase 1: the spec.
    let mut content = SPEC_ASK.replace("{prompt}", prompt);
    let mut spec = None;
    for n in 0..=o.spec_repairs {
        let (text, usage) = ask(&mut model, &mut msgs, &mut totals, "spec", content)?;
        let code = extract(&text);
        let r = match mcp.call("spec_check", json!({"module": o.module, "text": code})) {
            Err(e) if e.contains(UNSUPPORTED) => {
                rounds.push(round("spec", n, usage, &mut mcp, Some(&e), None));
                let mut rec = record(&id, &model_id, "spec_failed", None, &rounds, &totals);
                rec["unsupported"] = json!(e);
                return Ok((finish(rec, &system, &msgs, None, None), false));
            }
            r => r?,
        };
        let ok = !r.errors();
        let report = (!ok).then_some(r.text.as_str());
        rounds.push(round("spec", n, usage, &mut mcp, report, None));
        if ok {
            spec = Some(code);
            break;
        }
        content = REPAIR_ASK
            .replace("{tool}", "spec_check")
            .replace("{report}", &r.text);
    }
    let Some(spec) = spec else {
        let rec = record(&id, &model_id, "spec_failed", None, &rounds, &totals);
        return Ok((finish(rec, &system, &msgs, None, None), false));
    };
    // The spec is accepted here and frozen for phase 2; a person accepts it outside the harness.
    let target = patch::module_path(&dir, &o.module)?;
    std::fs::write(&target, &spec).map_err(|e| format!("{}: {e}", target.display()))?;
    let frozen = patch::parse(&target.display().to_string(), &spec)?;

    // Phase 2: the bodies.
    let mut content = BODY_ASK.to_owned();
    let mut status = "body_failed";
    let mut green_at = None;
    let mut unsupported = None;
    for n in 0..=o.body_repairs {
        let (text, usage) = ask(&mut model, &mut msgs, &mut totals, "body", content)?;
        if let Some(why) = text.lines().find_map(|l| l.trim().strip_prefix("STOP:")) {
            rounds.push(round("body", n, usage, &mut mcp, None, Some(why.trim())));
            status = "stopped";
            break;
        }
        let code = extract(&text);
        let (tool, report) = match frozen_changes(&frozen, &code, &target) {
            Some(report) => ("the spec gate", Some(report)),
            None => match body_round(&mut mcp, &o.module, &code) {
                // The toolchain cannot judge this body yet: a run result, not a harness failure.
                Err(e) if e.contains(UNSUPPORTED) => {
                    rounds.push(round("body", n, usage, &mut mcp, Some(&e), None));
                    unsupported = Some(e);
                    break;
                }
                r => r?,
            },
        };
        rounds.push(round("body", n, usage, &mut mcp, report.as_deref(), None));
        let Some(report) = report else {
            status = "green";
            green_at = Some(n + 1);
            break;
        };
        content = REPAIR_ASK
            .replace("{tool}", tool)
            .replace("{report}", &report);
    }
    let mut rec = record(&id, &model_id, status, green_at, &rounds, &totals);
    if let Some(e) = unsupported {
        rec["unsupported"] = json!(e);
    }
    let last = std::fs::read_to_string(&target).ok();
    Ok((
        finish(rec, &system, &msgs, Some(&spec), last.as_deref()),
        status == "green",
    ))
}

/// A new directory under the system temp dir with an unpredictable name; `create_dir` fails
/// rather than reuse a path someone else made first.
fn fresh_dir(id: &str) -> Result<PathBuf, String> {
    use std::hash::{BuildHasher, RandomState};
    let tag = RandomState::new().hash_one(std::time::SystemTime::now());
    let dir = std::env::temp_dir().join(format!("yan-agent-{id}-{tag:016x}"));
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Apply the body, close what autofix can, run the tests; the report for the model when the
/// round is not green.
fn body_round(
    mcp: &mut Mcp,
    module: &str,
    code: &str,
) -> Result<(&'static str, Option<String>), String> {
    let r = mcp.call("apply_patch", json!({"module": module, "patch": code}))?;
    if r.data["ok"] != true {
        return Ok(("apply_patch", Some(r.text)));
    }
    let fix = mcp.call("autofix", json!({"name": module}))?;
    let p = &fix.data["patch"];
    if p["replace"].as_array().is_some_and(|r| !r.is_empty()) {
        mcp.call("apply_patch", json!({"patch": p}))?;
    }
    let t = mcp.call("run_tests", json!({"filter": module}))?;
    Ok(("run_tests", (t.data["ok"] != true).then_some(t.text)))
}

/// A report when the body reply changes the frozen spec: an item of the spec missing, or one
/// whose header, docstring or contracts differ.
fn frozen_changes(frozen: &yan_hir::Module, code: &str, target: &Path) -> Option<String> {
    let new = match patch::parse(&target.display().to_string(), code) {
        Ok(m) => m,
        Err(e) => return Some(e),
    };
    let changed: Vec<&str> = frozen
        .items
        .iter()
        .filter(|old| {
            let Some(it) = patch::find(&new.items, old.kind.name()) else {
                return true;
            };
            let mut it = it.clone();
            if let (ItemKind::Func(n), ItemKind::Func(o)) = (&mut it.kind, &old.kind)
                && let (Some(nb), Some(ob)) = (&mut n.body, &o.body)
            {
                nb.stmts.clone_from(&ob.stmts);
            }
            patch::item_text(&it) != patch::item_text(old)
        })
        .map(|it| it.kind.name())
        .collect();
    (!changed.is_empty()).then(|| {
        format!(
            "the spec is frozen (21.1); these items differ from the accepted spec or are \
             missing: {}. Keep their def lines, docstrings and contracts as accepted.",
            changed.join(", ")
        )
    })
}

/// One model turn and the tool calls that judged it; `report` is what the model is shown next.
fn round(
    phase: &str,
    n: usize,
    usage: Usage,
    mcp: &mut Mcp,
    report: Option<&str>,
    stop: Option<&str>,
) -> Value {
    let mut r = json!({
        "phase": phase,
        "round": n,
        "usage": usage.json(),
        "tools": std::mem::take(&mut mcp.calls),
        "ok": report.is_none() && stop.is_none(),
        "report": report,
    });
    if let Some(s) = stop {
        r["stop"] = json!(s);
    }
    r
}

fn record(
    id: &str,
    model: &str,
    status: &str,
    green_at: Option<usize>,
    rounds: &[Value],
    totals: &BTreeMap<&str, Usage>,
) -> Value {
    let t = |p: &str| totals.get(p).copied().unwrap_or_default().json();
    json!({
        "schema": 1,
        "task": id,
        "model": model,
        "toolchain": {"version": env!("CARGO_PKG_VERSION"), "hash": toolchain_hash()},
        "status": status,
        "rounds_to_green": green_at,
        "tokens": {"spec": t("spec"), "body": t("body")},
        "rounds": rounds,
    })
}

/// The texts the harness recounts tokens from (21.6): the prompt, every turn, the accepted
/// spec and the module as the run left it.
fn finish(
    mut rec: Value,
    system: &str,
    msgs: &[Value],
    spec: Option<&str>,
    last: Option<&str>,
) -> Value {
    rec["system"] = json!(system);
    rec["messages"] = json!(msgs);
    rec["spec"] = json!(spec);
    rec["final"] = json!(last);
    rec
}

fn toolchain_hash() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(exe).ok()?;
    Some(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

fn system_prompt(o: &Options) -> Result<String, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let mut s = match &o.primer {
        Some(p) => read(p)?,
        None => SYSTEM.to_owned(),
    };
    if let Some(dir) = &o.shots {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "yag"))
            .collect();
        files.sort();
        s.push_str("\n\nExamples:");
        for f in files {
            s.push_str(&format!("\n\n```yag\n{}```", read(&f)?));
        }
    }
    Ok(s)
}

/// The first fenced code block of a reply, or the whole reply.
fn extract(text: &str) -> String {
    let body = text.split_once("```").and_then(|(_, rest)| {
        let (_, code) = rest.split_once('\n')?;
        Some(code.split_once("```").map_or(code, |(c, _)| c))
    });
    format!("{}\n", body.unwrap_or(text).trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "```yag\npub def clamp_all(xs: list[int], lo: int, hi: int) -> list[int]:\n    \"\"\"Clamp every element of xs into [lo, hi].\"\"\"\n    pre lo <= hi\n    ex clamp_all([1, 5, 9], lo=2, hi=6) == [2, 5, 6]\n    ...\n```";

    fn body(stmts: &str) -> String {
        SPEC.replace("    ...\n", stmts)
    }

    /// Runs the stub `replies` in a fresh directory.
    fn go(name: &str, replies: Vec<Value>) -> (Value, bool) {
        let dir =
            std::env::temp_dir().join(format!("yan-agent-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let task = dir.join("task.json");
        std::fs::write(&task, r#"{"id": "t", "prompt": "clamp_all"}"#).unwrap();
        let stub = dir.join("replies.json");
        std::fs::write(&stub, Value::Array(replies).to_string()).unwrap();
        let o = Options {
            task,
            model: format!("stub:{}", stub.display()),
            primer: None,
            shots: None,
            dir: Some(dir.join("work")),
            module: "task".into(),
            spec_repairs: 2,
            body_repairs: 3,
            max_tokens: 100,
        };
        run(&o).unwrap()
    }

    fn oks(rec: &Value) -> Vec<(String, bool)> {
        rec["rounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["phase"].as_str().unwrap().to_owned(), r["ok"] == true))
            .collect()
    }

    #[test]
    fn repairs_until_green() {
        let fixed = "    out: list[int] = []\n    for x in xs:\n        out.append(min(max(x, lo), hi))\n    return out\n";
        let (rec, green) = go(
            "green",
            vec![
                json!({"text": SPEC.replace("lo=2, hi=6", "lo=7, hi=6"), "usage": {"in": 10, "out": 4}}),
                json!(SPEC),
                json!(body("    return xs\n").replace("pre lo <= hi", "pre lo < hi")),
                json!(body("    return xs\n")),
                json!({"text": body(fixed), "usage": {"in": 30, "out": 8}}),
            ],
        );
        assert!(green);
        let s = |p: &str, ok| (p.to_owned(), ok);
        assert_eq!(
            oks(&rec),
            [
                s("spec", false),
                s("spec", true),
                s("body", false),
                s("body", false),
                s("body", true)
            ]
        );
        assert_eq!(rec["rounds_to_green"], 3);
        assert_eq!(rec["tokens"]["spec"]["in"], 10);
        assert_eq!(rec["tokens"]["body"]["out"], 8);
        assert_eq!(
            rec["rounds"][3]["tools"][2]["failed"],
            json!(["task.clamp_all#ex1"])
        );
        assert!(rec["final"].as_str().unwrap().contains("out.append"));
    }

    #[test]
    fn stop_ends_the_run() {
        let (rec, green) = go(
            "stop",
            vec![json!(SPEC), json!("STOP: the ex line is wrong")],
        );
        assert!(!green);
        assert_eq!(rec["status"], "stopped");
        assert_eq!(rec["rounds"][1]["stop"], "the ex line is wrong");
    }

    #[test]
    fn spec_repairs_run_out() {
        let bad = json!(SPEC.replace("[1, 5, 9]", "\"x\""));
        let (rec, green) = go("spec", vec![bad.clone(), bad.clone(), bad]);
        assert!(!green);
        assert_eq!(rec["status"], "spec_failed");
        assert_eq!(rec["rounds"][0]["tools"][0]["codes"], json!(["E201"]));
    }

    #[test]
    fn a_task_id_cannot_leave_the_temp_dir() {
        let dir = std::env::temp_dir().join(format!("yan-agent-id-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let task = dir.join("task.json");
        std::fs::write(&task, r#"{"id": "x/../../y", "prompt": "p"}"#).unwrap();
        let o = Options {
            task,
            model: "stub:none".into(),
            primer: None,
            shots: None,
            dir: None,
            module: "task".into(),
            spec_repairs: 0,
            body_repairs: 0,
            max_tokens: 1,
        };
        assert!(run(&o).unwrap_err().contains("task id"));
    }

    #[test]
    fn extract_takes_the_first_block() {
        assert_eq!(
            extract("x\n```yag\ndef f():\n    ...\n```\ny"),
            "def f():\n    ...\n"
        );
        assert_eq!(extract("def f():\n    ...\n"), "def f():\n    ...\n");
    }
}
