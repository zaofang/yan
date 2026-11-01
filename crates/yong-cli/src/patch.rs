//! `yong patch` (18.3): apply a JSON patch, or a plain file of whole items, to a Xin module.

use std::path::{Path, PathBuf};

use serde_json::Value;
use yong_diag::{Diagnostic, Severity, Span};
use yong_hir::{Item, Module};

/// What a refused patch reports; the module is left unchanged.
pub type Refusal = Vec<Diagnostic>;

/// A source checker: the diagnostics of `src` as the module at `path`.
pub type Diagnose<'a> = &'a dyn Fn(&Path, &str) -> Result<Vec<Diagnostic>, String>;

/// A patch that applies: the module, its text before and after, and each touched item's new
/// hash (none when it was deleted).
pub struct Applied {
    pub target: PathBuf,
    pub before: String,
    pub after: String,
    pub hashes: Vec<(String, Option<String>)>,
}

/// The patch in file `patch`, applied to its module (written unless refused).
pub fn apply_file(
    patch: &Path,
    module: Option<&str>,
    check: bool,
    diagnose: Diagnose,
) -> Result<Result<Applied, Refusal>, String> {
    let text = read(patch)?;
    let dir = patch
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let ops = if patch.extension().is_some_and(|e| e == "json") {
        Ops::Json(serde_json::from_str(&text).map_err(|e| format!("{}: {e}", patch.display()))?)
    } else {
        let target = match module {
            Some(m) => module_path(dir, m),
            None => plain_target(patch, dir)?,
        };
        Ops::Plain(target, text)
    };
    let r = plan(dir, ops, check, diagnose)?;
    if let Ok(a) = &r {
        commit(dir, a)?;
    }
    Ok(r)
}

pub fn module_path(dir: &Path, module: &str) -> PathBuf {
    let stem = module.replace('.', "/");
    let xin = dir.join(format!("{stem}.xin"));
    // Legacy `.jian` modules until conformance/ is renamed to the Xin names.
    let jian = dir.join(format!("{stem}.jian"));
    if !xin.is_file() && jian.is_file() {
        jian
    } else {
        xin
    }
}

/// Writes an applied patch and appends it to the oplog (18.3 rule 7).
pub fn commit(dir: &Path, a: &Applied) -> Result<(), String> {
    std::fs::write(&a.target, &a.after).map_err(|e| format!("{}: {e}", a.target.display()))?;
    oplog(dir, &a.target.display().to_string(), &a.before, &a.after)
}

/// Checks `ops` against the module under `dir` without writing anything.
pub fn plan(
    dir: &Path,
    ops: Ops,
    check: bool,
    diagnose: Diagnose,
) -> Result<Result<Applied, Refusal>, String> {
    let target = match &ops {
        Ops::Json(v) => module_path(
            dir,
            v["module"].as_str().ok_or("the patch has no `module`")?,
        ),
        Ops::Plain(t, _) => t.clone(),
    };
    let file = target.display().to_string();
    let before = read(&target)?;
    let m = parse(&file, &before)?;
    let refuse = |code: &str, at: Span, msg: String| {
        Ok(Err(vec![Diagnostic::error(code, &file, at, "", msg)]))
    };
    let mut items = m.items.clone();
    let mut touched: Vec<String> = Vec::new();
    match ops {
        Ops::Plain(_, src) => {
            for it in parse(&file, &src)?.items {
                let name = it.kind.name().to_owned();
                match items.iter().position(|x| x.kind.name() == name) {
                    Some(k) => items[k] = it,
                    None => items.push(it),
                }
                touched.push(name);
            }
        }
        Ops::Json(v) => {
            let base = v["base"].as_object().cloned().unwrap_or_default();
            let stale: Vec<String> = base
                .iter()
                .filter(|(n, h)| {
                    find(&items, n).is_none_or(|it| Some(hash(it).as_str()) != h.as_str())
                })
                .map(|(n, _)| n.clone())
                .collect();
            if !stale.is_empty() {
                let mut d = Diagnostic::error(
                    "P001",
                    &file,
                    whole(),
                    "",
                    format!("the base hash of {} is not current", stale.join(", ")),
                );
                d.notes = stale
                    .iter()
                    .filter_map(|n| find(&items, n))
                    .map(|it| format!("current {}: {}", hash(it), item_text(it)))
                    .collect();
                return Ok(Err(vec![d]));
            }
            for s in strings(&v["replace"]) {
                for it in parse(&file, &s)?.items {
                    let name = it.kind.name().to_owned();
                    let Some(k) = items.iter().position(|x| x.kind.name() == name) else {
                        return refuse("P004", it.span, format!("no item `{name}` to replace"));
                    };
                    items[k] = it;
                    touched.push(name);
                }
            }
            for s in strings(&v["add"]) {
                for it in parse(&file, &s)?.items {
                    let name = it.kind.name().to_owned();
                    if let Some(old) = find(&items, &name) {
                        return refuse("P005", old.span, format!("`{name}` already exists"));
                    }
                    items.push(it);
                    touched.push(name);
                }
            }
            for name in strings(&v["delete"]) {
                let Some(k) = items.iter().position(|x| x.kind.name() == name) else {
                    return refuse("P004", whole(), format!("no item `{name}` to delete"));
                };
                items.remove(k);
                touched.push(name);
            }
            for e in v["edits"].as_array().into_iter().flatten() {
                let (Some(name), Some(search), Some(replace)) = (
                    e["item"].as_str(),
                    e["search"].as_str(),
                    e["replace"].as_str(),
                ) else {
                    return Err("an edit needs item, search and replace".into());
                };
                let Some(k) = items.iter().position(|x| x.kind.name() == name) else {
                    return refuse("P004", whole(), format!("no item `{name}` to edit"));
                };
                let text = item_text(&items[k]);
                let n = text.matches(search).count();
                if n != 1 {
                    return refuse(
                        "P002",
                        items[k].span,
                        format!("`{search}` matches {n} times in `{name}`; it must match once"),
                    );
                }
                let mut new = parse(&file, &text.replacen(search, replace, 1))?.items;
                if new.len() != 1 || new[0].kind.name() != name {
                    return refuse(
                        "P002",
                        items[k].span,
                        format!("the edit to `{name}` does not leave one item `{name}`"),
                    );
                }
                items[k] = new.remove(0);
                touched.push(name.to_owned());
            }
        }
    }
    for name in &touched {
        let old = find(&m.items, name);
        let new = find(&items, name);
        if let Some(why) = protected(old, new) {
            let at = old.or(new).map_or(whole(), |it| it.span);
            return refuse(
                "P006",
                at,
                format!("`{name}`: {why} without a human confirmation token"),
            );
        }
    }
    let after = yong_xin::print(&Module { items, ..m });
    if check {
        let was = diagnose(&target, &before)?;
        let introduced: Vec<Diagnostic> = diagnose(&target, &after)?
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .filter(|d| !was.iter().any(|w| w.code == d.code && w.item == d.item))
            .collect();
        if !introduced.is_empty() {
            let mut out = vec![Diagnostic::error(
                "P003",
                &file,
                whole(),
                "",
                format!("the patch introduces {} error(s)", introduced.len()),
            )];
            out.extend(introduced);
            return Ok(Err(out));
        }
    }
    let new = parse(&file, &after)?;
    let hashes = touched
        .into_iter()
        .map(|n| {
            let h = find(&new.items, &n).map(hash);
            (n, h)
        })
        .collect();
    Ok(Ok(Applied {
        target,
        before,
        after,
        hashes,
    }))
}

pub enum Ops {
    Json(Value),
    /// A plain file of whole items and the module it patches.
    Plain(PathBuf, String),
}

/// A plain file patch names no module: without `--module` it applies to the one other Xin
/// module beside it.
fn plain_target(patch: &Path, dir: &Path) -> Result<PathBuf, String> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|e| e == "xin" || e == "jian")
                && p.file_name() != patch.file_name()
        })
        .collect();
    match found.len() {
        1 => Ok(found.remove(0)),
        n => Err(format!(
            "{}: name the module with --module; {n} modules are beside the patch",
            patch.display()
        )),
    }
}

/// R300 and 18.3: test functions and contract clauses are protected.
fn protected(old: Option<&Item>, new: Option<&Item>) -> Option<&'static str> {
    let is_test = |it: &Item| it.kind.name().starts_with("test_");
    if let Some(o) = old
        && is_test(o)
        && new.is_none_or(|n| item_text(n) != item_text(o))
    {
        return Some("it changes a test function");
    }
    let contracts = |it: Option<&Item>| -> Vec<String> {
        it.map(item_text)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| {
                ["pre ", "post ", "ex ", "invariant ", "decreases "]
                    .iter()
                    .any(|k| l.starts_with(k))
            })
            .map(str::to_owned)
            .collect()
    };
    (old.is_some() && contracts(old) != contracts(new)).then_some("it changes a contract clause")
}

pub fn find<'m>(items: &'m [Item], name: &str) -> Option<&'m Item> {
    items.iter().find(|it| it.kind.name() == name)
}

pub fn item_text(it: &Item) -> String {
    yong_xin::print(&Module {
        header: Vec::new(),
        attrs: Vec::new(),
        doc: None,
        items: vec![it.clone()],
    })
}

/// The item hash of 18.3: BLAKE3 of its canonical text.
pub fn hash(it: &Item) -> String {
    format!("b3:{}", blake3::hash(item_text(it).as_bytes()).to_hex())
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect()
}

pub fn parse(file: &str, src: &str) -> Result<Module, String> {
    let (m, diags) = yong_xin::parse(file, src, false);
    match m {
        Some(m) if !diags.iter().any(|d| d.severity == Severity::Error) => Ok(m),
        _ => Err(format!("{file} does not parse")),
    }
}

fn read(p: &Path) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn whole() -> Span {
    Span {
        line: 1,
        col: 1,
        end_line: 1,
        end_col: 1,
    }
}

/// 18.3 rule 7: every applied patch is appended to `.yong/oplog.jsonl`.
fn oplog(dir: &Path, file: &str, before: &str, after: &str) -> Result<(), String> {
    use std::io::Write;
    let log = dir.join(".yong");
    std::fs::create_dir_all(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let line = serde_json::json!({"file": file, "before": before, "after": after});
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log.join("oplog.jsonl"))
        .and_then(|mut f| writeln!(f, "{line}"))
        .map_err(|e| format!("oplog: {e}"))
}
