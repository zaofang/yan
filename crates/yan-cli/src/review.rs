//! `yan review` (RFC 0023): compare two revisions of a package item by item and report what a
//! reviewer must look at.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use yan_core::ast::{FnDef, ItemKind, Param, Type};
use yan_db::{Database, Loaded};

use crate::{patch, test};

pub struct Finding {
    pub kind: &'static str,
    /// `module.item`, or a file for `tests_lock_changed`.
    pub item: String,
    pub detail: String,
    pub attention: bool,
    pub old: Option<String>,
    pub new: Option<String>,
    pub author: Option<String>,
}

pub struct Review {
    pub findings: Vec<Finding>,
    /// Items whose own text is unchanged but whose identity hash changed.
    pub affected: usize,
    /// Tests that reach no added, edited or affected item.
    pub untouched_tests: usize,
}

impl Review {
    pub fn needs_human(&self) -> bool {
        self.findings.iter().any(|f| f.attention)
    }

    pub fn json(&self) -> serde_json::Value {
        let findings: Vec<_> = self
            .findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "kind": f.kind,
                    "item": f.item,
                    "detail": f.detail,
                    "attention": f.attention,
                    "old": f.old,
                    "new": f.new,
                    "author": f.author,
                })
            })
            .collect();
        serde_json::json!({
            "findings": findings,
            "affected": self.affected,
            "untouched_tests": self.untouched_tests,
            "needs_human": self.needs_human(),
        })
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for f in &self.findings {
            let mark = if f.attention { "!" } else { " " };
            let author = f
                .author
                .as_ref()
                .map(|a| format!(" [{a}]"))
                .unwrap_or_default();
            let detail = if f.detail.is_empty() {
                String::new()
            } else {
                format!(": {}", f.detail)
            };
            out.push(format!("{mark} {} {}{detail}{author}", f.kind, f.item));
            for (label, text) in [("old", &f.old), ("new", &f.new)] {
                for (k, l) in text.iter().flat_map(|t| t.lines()).enumerate() {
                    let tag = if k == 0 { label } else { "   " };
                    out.push(format!("    {tag} {l}"));
                }
            }
        }
        out.push(self.summary());
        out
    }

    pub fn markdown(&self) -> String {
        let mut out = String::from("## yan review\n\n");
        if self.findings.is_empty() {
            out.push_str("No item changed.\n\n");
        }
        for (title, attention) in [("Needs a human", true), ("Changes", false)] {
            let fs: Vec<_> = self
                .findings
                .iter()
                .filter(|f| f.attention == attention)
                .collect();
            if fs.is_empty() {
                continue;
            }
            out.push_str(&format!("### {title}\n\n"));
            for f in fs {
                let author = f
                    .author
                    .as_ref()
                    .map(|a| format!(" ({a})"))
                    .unwrap_or_default();
                let detail = if f.detail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", f.detail)
                };
                out.push_str(&format!("- `{}` {}{detail}{author}\n", f.item, f.kind));
                if f.old.is_some() || f.new.is_some() {
                    out.push_str("\n  ```diff\n");
                    for (sign, text) in [("-", &f.old), ("+", &f.new)] {
                        for l in text.iter().flat_map(|t| t.lines()) {
                            out.push_str(&format!("  {sign} {l}\n"));
                        }
                    }
                    out.push_str("  ```\n");
                }
            }
            out.push('\n');
        }
        out.push_str(&self.summary());
        out.push('\n');
        out
    }

    fn summary(&self) -> String {
        format!(
            "{} items affected through callees; {} tests reach nothing changed",
            self.affected, self.untouched_tests
        )
    }
}

/// One side of the comparison: a package root on disk, and whether it is a temporary copy.
struct Tree {
    root: PathBuf,
    temp: bool,
}

impl Drop for Tree {
    fn drop(&mut self) {
        if self.temp {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// The package at `pkg` as of `rev`, copied to a temporary directory: its `.yag` and `.yn`
/// files and `tests.lock`.
fn checkout(pkg: &Path, rev: &str) -> Result<Tree, String> {
    let spec = format!("{rev}^{{commit}}");
    let commit = String::from_utf8_lossy(&git(pkg, &["rev-parse", "--verify", &spec])?)
        .trim()
        .to_owned();
    let root = std::env::temp_dir().join(format!("yan-review-{}-{commit}", std::process::id()));
    let tree = Tree { root, temp: true };
    let names = git(
        pkg,
        &["ls-tree", "-r", "-z", "--name-only", &commit, "--", "."],
    )?;
    for name in names.split(|&b| b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name);
        let wanted = name == "tests.lock" || name.ends_with(".yag") || name.ends_with(".yn");
        if !wanted || name.split('/').any(|c| c == ".yir") {
            continue;
        }
        let blob = git(pkg, &["show", &format!("{commit}:./{name}")])?;
        let dest = tree.root.join(&*name);
        let err = |e: std::io::Error| format!("{}: {e}", dest.display());
        std::fs::create_dir_all(dest.parent().expect("a file has a parent")).map_err(err)?;
        std::fs::write(&dest, blob).map_err(err)?;
    }
    Ok(tree)
}

/// What the review reads of one item.
struct Facts {
    text: String,
    identity: String,
    header: String,
    is_test: bool,
    is_extern: bool,
    is_pub: bool,
    contracts: Vec<String>,
    fns: BTreeMap<String, FnFacts>,
}

#[derive(Default, PartialEq)]
struct FnFacts {
    caps: BTreeSet<String>,
    fails: BTreeSet<String>,
    is_unsafe: bool,
}

fn fn_facts(fd: &FnDef) -> FnFacts {
    let caps = fd
        .params
        .iter()
        .filter_map(|p| match p {
            Param::Cap { ty, .. } => Some(ty.path.join(".")),
            Param::Value { .. } => None,
        })
        .collect();
    let fails = match &fd.fail {
        Some(Type::Union(ts)) => ts.iter().map(yan_core::type_text).collect(),
        Some(t) => BTreeSet::from([yan_core::type_text(t)]),
        None => BTreeSet::new(),
    };
    FnFacts {
        caps,
        fails,
        is_unsafe: fd.is_unsafe,
    }
}

fn is_pub(k: &ItemKind) -> bool {
    match k {
        ItemKind::Fn(f) | ItemKind::Gen(f) => f.is_pub,
        ItemKind::Record(r) => r.is_pub,
        ItemKind::Sum(s) => s.is_pub,
        ItemKind::Newtype(n) => n.is_pub,
        ItemKind::Trait(t) => t.is_pub,
        ItemKind::Const(c) => c.is_pub,
        ItemKind::Impl(_) | ItemKind::Test(_) | ItemKind::Extern(_) | ItemKind::Inst(_) => false,
    }
}

fn fns(k: &ItemKind) -> BTreeMap<String, FnFacts> {
    let own = |fs: &[FnDef]| -> BTreeMap<String, FnFacts> {
        fs.iter().map(|f| (f.path.join("."), fn_facts(f))).collect()
    };
    match k {
        ItemKind::Fn(f) | ItemKind::Gen(f) => BTreeMap::from([(String::new(), fn_facts(f))]),
        ItemKind::Impl(i) => own(&i.fns),
        ItemKind::Trait(t) => own(&t.sigs),
        _ => BTreeMap::new(),
    }
}

const SURFACE_CONTRACTS: [&str; 5] = ["pre ", "post ", "ex ", "invariant ", "decreases "];
const YAN_CONTRACTS: [&str; 3] = ["requires ", "ensures ", "decreases "];

fn contract_lines(text: &str, keywords: &[&str]) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| keywords.iter().any(|k| l.starts_with(k)))
        .map(str::to_owned)
        .collect()
}

/// A function's header is its text before the body; any other item is its whole text.
fn header(text: &str) -> String {
    let mut out = Vec::new();
    for l in text
        .lines()
        .filter(|l| !l.starts_with("module ") && !l.is_empty())
    {
        if l.trim() == "{" {
            break;
        }
        if let Some(sig) = l.strip_suffix(" {") {
            out.push(sig);
            break;
        }
        out.push(l);
    }
    out.join("\n")
}

/// Tests are shown whole: their body is the contract.
fn shown(f: &Facts) -> String {
    if !f.is_test {
        return f.header.clone();
    }
    let lines: Vec<&str> = f
        .text
        .lines()
        .filter(|l| !l.starts_with("module "))
        .collect();
    lines.join("\n").trim().to_owned()
}

/// The items of every module of the package at `root`, keyed `module.key`. Modules that do not
/// elaborate are returned by name.
fn facts(root: &Path) -> Result<(BTreeMap<String, Facts>, Vec<String>), String> {
    let mut out = BTreeMap::new();
    let mut broken = Vec::new();
    for (name, file) in test::modules(root)? {
        let mut db = Database::new(false, false);
        let f = db.open(&file)?;
        let m = match yan_db::load(&db, f) {
            Ok(Loaded::Ready(m, _)) => m.clone(),
            _ => {
                broken.push(name);
                continue;
            }
        };
        let source = (file.extension().is_some_and(|e| e == "yag"))
            .then(|| std::fs::read_to_string(&file).ok())
            .flatten()
            .and_then(|src| patch::parse(&file.display().to_string(), &src).ok());
        for (it, h) in m.items.iter().zip(yan_core::hash::hash_module(&m)) {
            let mut bare = it.clone();
            bare.attrs.retain(|a| a.name != "doc");
            let text = test::yan_text(&m, &bare);
            let contracts = match &source {
                Some(src) => patch::find(&src.items, &h.key)
                    .map(|x| contract_lines(&patch::item_text(x), &SURFACE_CONTRACTS))
                    .unwrap_or_default(),
                None => contract_lines(&header(&text), &YAN_CONTRACTS),
            };
            let is_test = matches!(&it.kind, ItemKind::Fn(fd) if fd.path.join(".").starts_with("test_"))
                || matches!(it.kind, ItemKind::Test(_));
            out.insert(
                format!("{name}.{}", h.key),
                Facts {
                    header: header(&text),
                    text,
                    identity: h.identity.to_string(),
                    is_test,
                    is_extern: matches!(it.kind, ItemKind::Extern(_)),
                    is_pub: is_pub(&it.kind),
                    contracts,
                    fns: fns(&it.kind),
                },
            );
        }
    }
    Ok((out, broken))
}

fn finding(kind: &'static str, item: &str, detail: String, attention: bool) -> Finding {
    Finding {
        kind,
        item: item.to_owned(),
        detail,
        attention,
        old: None,
        new: None,
        author: None,
    }
}

/// Authority changes of one item (D3): capabilities, `fail` types, `unsafe` and `pub`.
fn authority(item: &str, old: Option<&Facts>, new: &Facts, out: &mut Vec<Finding>) {
    let none = BTreeMap::new();
    let old_fns = old.map_or(&none, |o| &o.fns);
    for (name, nf) in &new.fns {
        let of = old_fns.get(name);
        let at = if name.is_empty() {
            item.to_owned()
        } else {
            format!("{item}.{name}")
        };
        let empty = FnFacts::default();
        let of = of.unwrap_or(&empty);
        let diff = |a: &BTreeSet<String>, b: &BTreeSet<String>| {
            a.difference(b).cloned().collect::<Vec<_>>().join(", ")
        };
        for (kind, d, attention) in [
            ("capability_gained", diff(&nf.caps, &of.caps), true),
            ("fail_gained", diff(&nf.fails, &of.fails), true),
            ("capability_lost", diff(&of.caps, &nf.caps), false),
            ("fail_lost", diff(&of.fails, &nf.fails), false),
        ] {
            if !d.is_empty() {
                out.push(finding(kind, &at, d, attention));
            }
        }
        if nf.is_unsafe && !of.is_unsafe {
            out.push(finding("unsafe_gained", &at, String::new(), true));
        }
        if of.is_unsafe && !nf.is_unsafe && old.is_some() {
            out.push(finding("unsafe_lost", &at, String::new(), false));
        }
    }
    match old {
        Some(o) if o.is_pub && !new.is_pub => {
            out.push(finding("pub_lost", item, String::new(), false))
        }
        Some(o) if !o.is_pub && new.is_pub => {
            out.push(finding("pub_gained", item, String::new(), true))
        }
        None if new.is_pub => out.push(finding("pub_gained", item, String::new(), true)),
        _ => {}
    }
}

/// Compare `base` with `head`, a git revision or the working tree when None, for the package
/// at `pkg`.
pub fn review(pkg: &Path, base: &str, head: Option<&str>) -> Result<Review, String> {
    let base_tree = checkout(pkg, base)?;
    let head_tree = match head {
        Some(rev) => checkout(pkg, rev)?,
        None => Tree {
            root: pkg.to_path_buf(),
            temp: false,
        },
    };
    let (old, broken_base) = facts(&base_tree.root)?;
    let (new, broken) = facts(&head_tree.root)?;
    let locked = test::lock(&base_tree.root)?;
    let approved = |id: &str| locked.iter().any(|l| l.path == id);

    let mut findings = Vec::new();
    for m in &broken {
        findings.push(finding(
            "module_unchecked",
            m,
            "does not elaborate at HEAD".into(),
            true,
        ));
    }
    for m in broken_base.iter().filter(|m| !broken.contains(m)) {
        findings.push(finding(
            "module_unchecked",
            m,
            "does not elaborate at BASE".into(),
            false,
        ));
    }
    let mut changed = BTreeSet::new();
    let mut affected = 0;
    for (item, n) in &new {
        let o = old.get(item);
        match o {
            Some(o) if o.text == n.text => {
                if o.identity != n.identity {
                    affected += 1;
                    changed.insert(item.clone());
                }
                continue;
            }
            _ => {
                changed.insert(item.clone());
            }
        }
        let kind = match (o.is_some(), n.is_test) {
            (false, true) => "test_added",
            (false, false) => "added",
            (true, true) => "test_edited",
            (true, false) => "edited",
        };
        let frozen = kind == "test_edited" && approved(item);
        let old_shown = o.map(shown);
        let detail = if frozen {
            "frozen: an approved test"
        } else if old_shown.as_ref() == Some(&shown(n)) {
            "body only"
        } else {
            ""
        };
        let mut f = finding(kind, item, detail.into(), frozen);
        f.old = old_shown.filter(|old| *old != shown(n));
        f.new = Some(shown(n));
        findings.push(f);
        if n.is_extern {
            let what = if o.is_some() { "edited" } else { "added" };
            findings.push(finding("extern_changed", item, what.into(), true));
        }
        authority(item, o, n, &mut findings);
        let before = o.map(|o| o.contracts.clone()).unwrap_or_default();
        let frozen = !before.is_empty();
        for c in before.iter().filter(|c| !n.contracts.contains(c)) {
            findings.push(finding(
                "contract_removed",
                item,
                format!("frozen: {c}"),
                true,
            ));
        }
        for c in n.contracts.iter().filter(|c| !before.contains(c)) {
            let detail = if frozen {
                format!("frozen: {c}")
            } else {
                c.clone()
            };
            findings.push(finding("contract_added", item, detail, frozen));
        }
    }
    for (item, o) in &old {
        if new.contains_key(item) {
            continue;
        }
        let kind = if o.is_test { "test_removed" } else { "removed" };
        let frozen = o.is_test && approved(item);
        let mut f = finding(
            kind,
            item,
            if frozen {
                "frozen: an approved test".into()
            } else {
                String::new()
            },
            frozen,
        );
        f.old = Some(shown(o));
        if o.is_extern {
            findings.push(finding("extern_changed", item, "removed".into(), true));
        }
        findings.push(f);
    }
    let read = |t: &Tree| std::fs::read_to_string(t.root.join("tests.lock")).unwrap_or_default();
    if read(&base_tree) != read(&head_tree) {
        findings.push(finding(
            "tests_lock_changed",
            "tests.lock",
            "approvals moved".into(),
            true,
        ));
    }

    let report = test::run(
        &head_tree.root,
        None,
        false,
        Some(&test::cache_file(pkg)),
        None,
    )?;
    let mut untouched = 0;
    for t in &report.tests {
        let module = t.id.rsplit_once('.').map_or("", |(m, _)| m);
        if !t
            .reach
            .iter()
            .any(|k| changed.contains(&format!("{module}.{k}")))
        {
            untouched += 1;
            continue;
        }
        let ok = locked.iter().any(|l| l.path == t.id && l.hash == t.hash);
        let detail = format!(
            "{}, {}, {}",
            t.verdict.name(),
            if ok { "approved" } else { "proposed" },
            if t.cached { "cached" } else { "run" }
        );
        findings.push(finding(
            "test_verdict",
            &t.id,
            detail,
            t.verdict != test::Verdict::Pass,
        ));
    }

    let authors = provenance(pkg);
    for f in &mut findings {
        if let Some(n) = new.get(&f.item).filter(|_| !authors.is_empty()) {
            f.author = Some(
                authors
                    .get(&n.identity)
                    .cloned()
                    .unwrap_or("unknown".into()),
            );
        }
    }
    findings.sort_by_key(|f| (!f.attention, !f.kind.ends_with("_gained")));
    Ok(Review {
        findings,
        affected,
        untouched_tests: untouched,
    })
}

/// Authors from `.yan/provenance.jsonl` (21.5), keyed by item hash. Empty without the file.
fn provenance(root: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(root.join(".yan/provenance.jsonl")).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|r| {
            let hash = r
                .get("item_hash")?
                .as_str()?
                .trim_start_matches("b3:")
                .to_owned();
            let author = match r.get("author")? {
                serde_json::Value::String(s) => s.clone(),
                a => format!(
                    "model({}, {})",
                    a.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                    a.get("version").and_then(|v| v.as_str()).unwrap_or("?")
                ),
            };
            Some((hash, author))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .expect("git runs")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    const BASE: &str = "def add(a: int, b: int) -> int:\n    pre a >= 0\n    return a + b\n\n\
        def shout(s: str) -> str:\n    return s\n\n\
        def test_add():\n    assert add(a=1, b=2) == 3\n\n\
        def test_other():\n    assert 1 == 1\n";

    #[test]
    fn header_stops_at_the_body() {
        assert_eq!(
            header("module m\n\nfn f() -> Unit {\n    ret ()\n}"),
            "fn f() -> Unit"
        );
        assert_eq!(
            header("fn f() -> Unit\n    requires true\n{\n    ret ()\n}"),
            "fn f() -> Unit\n    requires true"
        );
    }

    #[test]
    fn reports_authority_contracts_and_frozen_tests() {
        let dir = std::env::temp_dir().join(format!("yan-review-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("m.yag"), BASE).expect("write");
        let first = test::run(&dir, None, false, None, None).expect("tests run");
        let h = &first
            .tests
            .iter()
            .find(|t| t.id == "m.test_add")
            .expect("test")
            .hash;
        let lock = format!("format = 1\n[[test]]\npath = \"m.test_add\"\nhash = \"{h}\"\n");
        std::fs::write(dir.join("tests.lock"), lock).expect("write");
        sh(&dir, &["init", "-q"]);
        sh(&dir, &["add", "-A"]);
        sh(&dir, &["commit", "-qm", "base"]);
        let head = BASE
            .replace("pre a >= 0", "pre a >= 1")
            .replace(
                "def shout(s: str) -> str:\n",
                "def shout(s: str) -> str uses io:\n    io.print(s)\n",
            )
            .replace("== 3", "== 4");
        std::fs::write(dir.join("m.yag"), head).expect("write");

        let r = review(&dir, "HEAD", None).expect("review runs");
        let _ = std::fs::remove_dir_all(&dir);
        let has = |kind: &str, item: &str| {
            r.findings
                .iter()
                .find(|f| f.kind == kind && f.item == item)
                .map(|f| (f.attention, f.detail.clone()))
        };
        assert_eq!(
            has("capability_gained", "m.shout"),
            Some((true, "Io".into()))
        );
        assert_eq!(r.findings[0].kind, "capability_gained");
        assert_eq!(
            has("contract_removed", "m.add"),
            Some((true, "frozen: pre a >= 0".into()))
        );
        assert_eq!(
            has("test_edited", "m.test_add"),
            Some((true, "frozen: an approved test".into()))
        );
        assert_eq!(
            has("test_verdict", "m.test_add"),
            Some((true, "fail, proposed, run".into()))
        );
        assert_eq!(has("test_verdict", "m.test_other"), None);
        assert_eq!(r.untouched_tests, 1);
        assert!(r.needs_human());
    }
}
