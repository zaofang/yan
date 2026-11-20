//! `yan audit` (18.1): the claims a reviewer takes on trust: `unsafe fn` preconditions (6),
//! `unsafe impl`, extern blocks and their `safe`, `uses none` and `@access`/`@stack` claims (11.2,
//! 12, R627) and shields with their budget and call path (9.4, R812). Diffing lockfiles, `--stats` and `--policy` are not yet implemented.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use yan_core::ast::{
    Atom, Attr, Block, Call, Contract, Expr, FnDef, Item, ItemKind, Module, Pat, Rhs, Stmt,
    StmtKind, XItem,
};
use yan_db::closure::Node;
use yan_db::{Database, Loaded};

use crate::{closure, test};

pub struct Finding {
    pub kind: &'static str,
    /// `module.item`
    pub item: String,
    pub detail: String,
}

impl Finding {
    pub fn line(&self) -> String {
        if self.detail.is_empty() {
            format!("{} {}", self.kind, self.item)
        } else {
            format!("{} {}: {}", self.kind, self.item, self.detail)
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"kind": self.kind, "item": self.item, "detail": self.detail})
    }
}

/// Attributes that state a claim the checker cannot verify on an extern.
const CLAIM_ATTRS: [&str; 2] = ["access", "stack"];

fn claim_detail(a: &Attr) -> String {
    let args = a
        .args
        .as_ref()
        .map_or(String::new(), |v| format!("({} args)", v.len()));
    format!("@{}{args}", a.name)
}

fn preconditions(f: &FnDef) -> String {
    let pre: Vec<String> = f
        .contracts
        .iter()
        .filter_map(|c| match c {
            Contract::Requires(e) => Some(yan_core::expr_text(e)),
            _ => None,
        })
        .collect();
    if pre.is_empty() {
        "no preconditions".to_owned()
    } else {
        format!("requires {}", pre.join("; requires "))
    }
}

fn unsafe_fn(f: &FnDef, scope: &str, out: &mut Vec<Finding>) {
    if f.is_unsafe {
        out.push(Finding {
            kind: "unsafe_fn",
            item: format!("{scope}{}", f.path.join(".")),
            detail: preconditions(f),
        });
    }
}

fn item_findings(module: &str, it: &Item, key: &str, out: &mut Vec<Finding>) {
    let at = format!("{module}.{key}");
    let mut push = |kind, item: String, detail: String| out.push(Finding { kind, item, detail });
    match &it.kind {
        ItemKind::Fn(f) | ItemKind::Gen(f) => unsafe_fn(f, &format!("{module}."), out),
        ItemKind::Trait(t) => t
            .sigs
            .iter()
            .for_each(|f| unsafe_fn(f, &format!("{at}."), out)),
        ItemKind::Impl(i) => {
            if i.is_unsafe {
                push("unsafe_impl", at.clone(), String::new());
            }
            i.fns
                .iter()
                .for_each(|f| unsafe_fn(f, &format!("{at}."), out));
        }
        ItemKind::Extern(e) => {
            push("extern", at.clone(), format!("abi {} lib {}", e.abi, e.lib));
            for x in &e.items {
                match x {
                    XItem::Fn {
                        attrs,
                        safe,
                        name,
                        uses_none,
                        ..
                    } => {
                        let name = format!("{at}.{name}");
                        for a in attrs
                            .iter()
                            .filter(|a| CLAIM_ATTRS.contains(&a.name.as_str()))
                        {
                            push("trusted_claim", name.clone(), claim_detail(a));
                        }
                        if *safe {
                            push("extern_safe", name.clone(), "trusted safe signature".into());
                        }
                        if *uses_none {
                            push("uses_none", name, "trusted: pure, deterministic".into());
                        }
                    }
                    XItem::Static {
                        safe: true, name, ..
                    } => push(
                        "extern_safe",
                        format!("{at}.{name}"),
                        "trusted safe static".into(),
                    ),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    for a in it
        .attrs
        .iter()
        .filter(|a| CLAIM_ATTRS.contains(&a.name.as_str()))
    {
        let kind = if matches!(it.kind, ItemKind::Extern(_)) {
            "trusted_claim"
        } else {
            "claim"
        };
        out.push(Finding {
            kind,
            item: at.clone(),
            detail: claim_detail(a),
        });
    }
}

/// Every function of a module with the name findings use: `module.fn`, `module.Type.fn` in an impl.
fn module_fns<'a>(module: &str, m: &'a Module, key_of: &[String]) -> Vec<(String, &'a FnDef)> {
    let mut out = Vec::new();
    for (it, key) in m.items.iter().zip(key_of) {
        let at = format!("{module}.{key}");
        match &it.kind {
            ItemKind::Fn(f) | ItemKind::Gen(f) => {
                out.push((format!("{module}.{}", f.path.join(".")), f));
            }
            ItemKind::Impl(i) => out.extend(
                i.fns
                    .iter()
                    .map(|f| (format!("{at}.{}", f.path.join(".")), f)),
            ),
            _ => {}
        }
    }
    out
}

/// Calls the statement itself makes, then every nested block and closure body.
fn visit_block(b: &Block, f: &mut dyn FnMut(&Block, usize, &Stmt)) {
    for (i, s) in b.stmts.iter().enumerate() {
        f(b, i, s);
        let rhs = |r: &Rhs, f: &mut dyn FnMut(&Block, usize, &Stmt)| {
            if let Rhs::Closure(c) = r {
                visit_block(&c.body, f);
            }
        };
        match &s.kind {
            StmtKind::Let { rhs: r, .. } | StmtKind::Assign { rhs: r, .. } | StmtKind::Rhs(r) => {
                rhs(r, f)
            }
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                visit_block(then, f);
                els.iter().for_each(|e| visit_block(e, f));
            }
            StmtKind::Match { arms, .. } => arms.iter().for_each(|(_, b)| visit_block(b, f)),
            StmtKind::Loop { body, .. }
            | StmtKind::Jp { body, .. }
            | StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::ComptimeFor { body, .. }
            | StmtKind::Unsafe(body) => visit_block(body, f),
            StmtKind::Par { blocks, .. } => blocks.iter().for_each(|b| visit_block(b, f)),
            _ => {}
        }
    }
}

fn rhs_call(r: &Rhs) -> Option<&Call> {
    match r {
        Rhs::Call(c) | Rhs::Try(c) | Rhs::Propagate { call: c, .. } => Some(c),
        _ => None,
    }
}

/// The Core `let` that binds `name` before statement `before` of `b`.
fn definition<'a>(b: &'a Block, before: usize, name: &str) -> Option<&'a Rhs> {
    b.stmts[..before].iter().rev().find_map(|s| match &s.kind {
        StmtKind::Let {
            pat: Pat::Bind(n),
            rhs,
            ..
        } if n == name => Some(rhs),
        _ => None,
    })
}

fn atom_text(b: &Block, before: usize, a: &Atom) -> String {
    match a {
        Atom::Name(n, _) => match definition(b, before, n) {
            Some(Rhs::Call(c)) => yan_core::expr_text(&Expr::Call(c.clone())),
            _ => yan_core::expr_text(&Expr::Atom(a.clone())),
        },
        Atom::Lit(..) => yan_core::expr_text(&Expr::Atom(a.clone())),
    }
}

/// The duration a shield was opened with: the second operand of the `mono.add` that built its
/// instant, else the instant's own definition.
fn budget(b: &Block, at_stmt: usize, at: &Atom) -> String {
    let Atom::Name(n, _) = at else {
        return atom_text(b, at_stmt, at);
    };
    match definition(b, at_stmt, n).and_then(rhs_call) {
        Some(c) if c.path.ends_with(&["mono".to_owned(), "add".to_owned()]) => {
            match c.args.get(1) {
                Some(yan_core::ast::Arg::Value(_, d, _)) => atom_text(b, at_stmt, d),
                _ => yan_core::expr_text(&Expr::Call(c.clone())),
            }
        }
        Some(c) => yan_core::expr_text(&Expr::Call(c.clone())),
        None => n.clone(),
    }
}

/// One `shield` finding per shield in the package, with the shortest chain of direct calls from a
/// function nothing in the package calls (R812). Calls through traits and function values are not
/// followed.
fn shields(modules: &[(String, Module, Vec<String>)]) -> Vec<Finding> {
    let fns: Vec<(String, &FnDef)> = modules
        .iter()
        .flat_map(|(n, m, keys)| module_fns(n, m, keys))
        .collect();
    let known: BTreeMap<&str, usize> = fns
        .iter()
        .enumerate()
        .map(|(i, (n, _))| (n.as_str(), i))
        .collect();
    let mut callers = vec![0usize; fns.len()];
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); fns.len()];
    let mut found: Vec<(usize, String)> = Vec::new();
    for (i, (name, f)) in fns.iter().enumerate() {
        let Some(body) = &f.body else { continue };
        let module = modules
            .iter()
            .map(|(n, ..)| n.as_str())
            .filter(|n| name.starts_with(&format!("{n}.")))
            .max_by_key(|n| n.len())
            .unwrap_or("");
        visit_block(body, &mut |b, at, s| {
            if let StmtKind::Deadline {
                at: a,
                shield: true,
                ..
            } = &s.kind
            {
                found.push((i, budget(b, at, a)));
            }
            let call = match &s.kind {
                StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) => {
                    rhs_call(rhs)
                }
                StmtKind::Become(c) => Some(c),
                _ => None,
            };
            if let Some(c) = call {
                let path = c.path.join(".");
                let target = known
                    .get(path.as_str())
                    .or_else(|| known.get(format!("{module}.{path}").as_str()));
                if let Some(&t) = target.filter(|&&t| t != i && !edges[i].contains(&t)) {
                    edges[i].push(t);
                    callers[t] += 1;
                }
            }
        });
    }
    let mut parent: Vec<Option<usize>> = vec![None; fns.len()];
    let mut seen: Vec<bool> = callers.iter().map(|&c| c == 0).collect();
    let mut queue: VecDeque<usize> = (0..fns.len()).filter(|&i| seen[i]).collect();
    while let Some(i) = queue.pop_front() {
        for &t in &edges[i] {
            if !seen[t] {
                seen[t] = true;
                parent[t] = Some(i);
                queue.push_back(t);
            }
        }
    }
    found
        .into_iter()
        .map(|(i, budget)| {
            let mut chain = vec![fns[i].0.as_str()];
            let mut at = i;
            while let Some(p) = parent[at] {
                chain.push(fns[p].0.as_str());
                at = p;
            }
            chain.reverse();
            Finding {
                kind: "shield",
                item: fns[i].0.clone(),
                detail: format!("budget {budget}; path {}", chain.join(" -> ")),
            }
        })
        .collect()
}

/// One `foreign_lib` finding per extern library in the closure of the package at `root`, with the
/// shortest dependency path that brings it in (RFC 0079 D3). Libraries are spelled as a ceiling spells
/// them, so the line can be pasted into `capabilities`.
fn foreign_libraries(root: &Path) -> Vec<Finding> {
    let packages = crate::graph::packages(root);
    let nodes: Vec<Node> = packages.iter().map(closure::from_package).collect();
    let Some(start) = packages.iter().position(|p| p.role == yan_db::Role::Root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in closure::reach(&nodes, start) {
        let at = *path.last().unwrap_or(&start);
        for lib in &packages[at].foreign.libs {
            let route: Vec<&str> = path.iter().map(|&i| nodes[i].name.as_str()).collect();
            out.push(Finding {
                kind: "foreign_lib",
                item: closure::entry(&nodes[at], &lib.name, at == start),
                detail: format!("link {}, via {}", lib.link, route.join(" -> ")),
            });
        }
    }
    out
}

/// One `unconfined` finding per package of the closure that declares no `capabilities`: with no
/// ceiling it may use every capability, which R673 reports as granting every capability.
fn unconfined_packages(root: &Path) -> Vec<Finding> {
    let packages = crate::graph::packages(root);
    let nodes: Vec<Node> = packages.iter().map(closure::from_package).collect();
    let Some(start) = packages.iter().position(|p| p.role == yan_db::Role::Root) else {
        return Vec::new();
    };
    closure::reach(&nodes, start)
        .iter()
        .filter_map(|path| {
            let at = *path.last().unwrap_or(&start);
            let p = &packages[at];
            let route: Vec<&str> = path.iter().map(|&i| nodes[i].name.as_str()).collect();
            p.manifest.capabilities.is_none().then(|| Finding {
                kind: "unconfined",
                item: p
                    .name
                    .clone()
                    .unwrap_or_else(|| p.root.display().to_string()),
                detail: format!(
                    "no `capabilities`, so no ceiling, via {}",
                    route.join(" -> ")
                ),
            })
        })
        .collect()
}

/// Findings for every module of the package at `root`, in module order. Modules that do not
/// elaborate are reported as `unchecked`.
pub fn audit(root: &Path) -> Result<Vec<Finding>, String> {
    let mut out = unconfined_packages(root);
    out.extend(foreign_libraries(root));
    let mut loaded = Vec::new();
    for (name, file) in test::modules(root)? {
        let mut db = Database::new(false, false);
        let f = db.open(&file)?;
        let m = match yan_db::load(&db, f) {
            Ok(Loaded::Ready(m, _, _)) => m.clone(),
            _ => {
                out.push(Finding {
                    kind: "unchecked",
                    item: name,
                    detail: "module does not elaborate; its claims are not listed".into(),
                });
                continue;
            }
        };
        let keys: Vec<String> = yan_core::hash::hash_module(&m)
            .into_iter()
            .map(|h| h.key)
            .collect();
        for (it, key) in m.items.iter().zip(&keys) {
            item_findings(&name, it, key, &mut out);
        }
        loaded.push((name, m, keys));
    }
    out.extend(shields(&loaded));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(src: &str) -> Vec<String> {
        let (m, d) = yan_core::parse("m.yir", src);
        let m = m.unwrap();
        assert!(d.is_empty(), "{d:?}");
        let mut out = Vec::new();
        for (it, h) in m.items.iter().zip(yan_core::hash::hash_module(&m)) {
            item_findings("m", it, &h.key, &mut out);
        }
        out.iter().map(Finding::line).collect()
    }

    #[test]
    fn lists_access_claim_on_extern_fn() {
        let src = "module m\n\nunsafe extern \"C\" lib m link \"libm\" {\n    @access(a = rows(0), out = elementwise) safe fn scale(a: Array[f64]) -> Array[f64]\n}\n";
        let got = findings(src);
        assert!(
            got.iter()
                .any(|l| l.starts_with("trusted_claim") && l.contains("@access(2 args)")),
            "{got:?}"
        );
    }

    #[test]
    fn lists_unsafe_extern_and_claims() {
        let src = "module m\n\nunsafe fn poke(let p: i64) -> Unit\n    requires ge_i64(let p, let 0)\n{\n    ret ()\n}\n\nunsafe extern \"C\" lib m link \"libm\" {\n    safe fn cos(x: f64) -> f64 uses none\n    unsafe fn printf(fmt: Ptr[u8], ...) -> i32\n}\n";
        let got = findings(src);
        assert!(
            got.iter()
                .any(|l| l.starts_with("unsafe_fn m.poke: requires")),
            "{got:?}"
        );
        assert!(
            got.iter()
                .any(|l| l.starts_with("extern_safe") && l.contains("cos"))
        );
        assert!(
            got.iter()
                .any(|l| l.starts_with("uses_none") && l.contains("cos"))
        );
        assert!(
            !got.iter()
                .any(|l| l.contains("printf") && l.starts_with("extern_safe"))
        );
    }
    #[test]
    fn lists_shield_with_budget_and_call_path() {
        let src = "module m\n\nfn hold(let d: Duration, let t0: MonoInstant) -> Unit {\n    let _t1: MonoInstant = std.time.mono.add(let t0, let d)\n    shield _t1 {\n    }\n}\n\nfn mid(let d: Duration, let t0: MonoInstant) -> Unit {\n    hold(let d, let t0)\n}\n\nfn main(let d: Duration, let t0: MonoInstant) -> Unit {\n    mid(let d, let t0)\n}\n";
        let (m, d) = yan_core::parse("m.yir", src);
        let m = m.unwrap();
        assert!(d.is_empty(), "{d:?}");
        let keys = yan_core::hash::hash_module(&m)
            .into_iter()
            .map(|h| h.key)
            .collect();
        let got: Vec<String> = shields(&[("m".into(), m, keys)])
            .iter()
            .map(Finding::line)
            .collect();
        assert_eq!(
            got,
            ["shield m.hold: budget d; path m.main -> m.mid -> m.hold"]
        );
    }
}
