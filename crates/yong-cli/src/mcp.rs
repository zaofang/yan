//! `yong mcp` (21.4): the agent server over stdio, newline-delimited JSON-RPC 2.0.
//! It serves `check`, `check_patch`, `apply_patch`, `autofix`, `sig`, `spec_check` and
//! `run_tests`, answered from one query database that lives as long as the server.

use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use yong_db::Database;
use yong_db::Loaded;
use yong_diag::{DEFAULT_MAX_ERRORS, Diagnostic, Severity, finish};
use yong_interp::{Limits, Stop};

use crate::examples::{self, Kind};
use crate::patch::{self, Ops};
use crate::test;

const PROTOCOL: &str = "2026-07-28";

pub fn serve(root: &Path) -> Result<(), String> {
    let db = RefCell::new(Database::new(false, false));
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => respond(root, &db, &req),
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
pub fn respond(root: &Path, db: &RefCell<Database>, req: &Value) -> Option<Value> {
    let id = req.get("id")?.clone();
    let params = &req["params"];
    let result = match req["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "yong", "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let args = &params["arguments"];
            match call(root, db, params["name"].as_str().unwrap_or(""), args) {
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

fn call(root: &Path, db: &RefCell<Database>, tool: &str, args: &Value) -> Result<Value, String> {
    let diagnose = |p: &Path, src: &str| {
        let mut db = db.borrow_mut();
        let f = db.file(p, src);
        yong_ide::diagnostics(&*db, f)
    };
    match tool {
        "check" => {
            let name = str_arg(args, "name")?;
            let (module, item) = split(root, name);
            let path = patch::module_path(root, &module);
            let src = read(&path)?;
            let mut db = db.borrow_mut();
            let f = db.file(&path, &src);
            let diags = match &item {
                Some(i) => yong_ide::item_diagnostics(&*db, f, i)?,
                None => yong_ide::diagnostics(&*db, f)?,
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
                    Ops::Plain(patch::module_path(root, module), text)
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
            let path = patch::module_path(root, module);
            let m = patch::parse(&path.display().to_string(), &read(&path)?)?;
            let budget = args["budget"].as_u64().map(|b| b as usize);
            let text = yong_ide::sig(&m, budget);
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "structuredContent": {"module": module, "text": text},
            }))
        }
        "spec_check" => {
            let module = str_arg(args, "module")?;
            let text = str_arg(args, "text")?;
            let diags = spec_check(&patch::module_path(root, module), text)?;
            Ok(diagnostics(
                finish(diags, DEFAULT_MAX_ERRORS),
                max_tokens(args),
            ))
        }
        "run_tests" => {
            let r = test::run(
                root,
                args["filter"].as_str(),
                args["gate"].as_bool() == Some(true),
            )?;
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
fn spec_check(path: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
    let file = path.display().to_string();
    let (src, exs) = examples::augment(&file, text, true);
    let mut db = Database::new(false, false);
    let f = db.file(path, &src);
    let mut diags: Vec<Diagnostic> = examples::remap(yong_ide::diagnostics(&db, f)?, &exs)
        .into_iter()
        .filter(|d| d.code != "H701")
        .collect();
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Ok(diags);
    }
    let m = match yong_db::load(&db, f) {
        Ok(Loaded::Ready(m, _)) => m.clone(),
        Ok(Loaded::Unsupported(what, _)) => return Err(format!("not supported yet: {what}")),
        Ok(Loaded::Stopped(d)) => return Ok(examples::remap(d.clone(), &exs)),
        Err(msg) => return Err(msg.clone()),
    };
    for x in exs.iter().filter(|x| matches!(x.kind, Kind::Pre)) {
        let mut out = Vec::new();
        let r = yong_interp::test(&m, &x.name, Limits::default(), &mut out);
        let d = match r {
            Ok(_) => continue,
            Err(Stop::Panic(k, _)) if k == "assert" => Diagnostic::error(
                EX_BREAKS_PRE,
                &file,
                x.span,
                &x.func,
                format!(
                    "the arguments of `ex` line {} break `pre` of `{}`",
                    x.k, x.func
                ),
            ),
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
    let path = patch::module_path(root, module);
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

/// `pricing.value` is item `value` of module `pricing` when `pricing.xin` exists and
/// `pricing/value.xin` does not.
fn split(root: &Path, name: &str) -> (String, Option<String>) {
    if let Some((m, i)) = name.rsplit_once('.')
        && !patch::module_path(root, name).is_file()
        && patch::module_path(root, m).is_file()
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
            "description": "Run test_ functions and ex lines; filter is a test id or a module or function prefix.",
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
        let d = RefCell::new(Database::new(false, false));
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(respond(Path::new("."), &d, &note).is_none());
        let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
        let r = respond(Path::new("."), &d, &list).unwrap();
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), 7);
    }
}
