//! `yan why <item> <fact>`: the shortest chain of statements and callees that makes an item need a
//! capability, a `fail` member or a panic kind (18.4, R612).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::process::ExitCode;

use yan_core::ast::{
    Arg, Atom, Block, Call, FnDef, ItemKind, Module, Param, Place, Proj, Rhs, StmtKind, Type,
};
use yan_diag::Span;

const PANIC_KINDS: [&str; 9] = [
    "overflow",
    "div_zero",
    "contract",
    "exclusivity",
    "bounds",
    "shape",
    "unreachable",
    "assert",
    "deadlock",
];

/// Panic kinds the analysis finds from Yan Core. The others have no statement form in it.
const TRACED_KINDS: [&str; 6] = [
    "overflow",
    "div_zero",
    "contract",
    "bounds",
    "unreachable",
    "assert",
];

#[derive(Clone, Debug, PartialEq)]
enum Fact {
    Cap(String),
    Fail(String),
    Panic(&'static str),
}

struct Step {
    span: Span,
    text: String,
}

enum Ev {
    Call {
        call: Call,
        span: Span,
        propagated: bool,
    },
    Fail(Atom, Span),
    Assert(Span),
    Unreachable(Span),
    Index(Span),
    Capture(String, Span),
}

type Fns<'a> = HashMap<String, &'a FnDef>;

fn module_fns(m: &Module) -> Fns<'_> {
    m.items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Fn(f) | ItemKind::Gen(f) => Some((f.path.join("."), f)),
            _ => None,
        })
        .collect()
}

/// The `fail` members of each std function, by `std.<module>.<name>`.
fn std_fails() -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for (module, _, _, m) in yan_db::std_modules() {
        for item in &m.items {
            if let yan_hir::ItemKind::Func(f) = &item.kind {
                let fails = f.ret.iter().flat_map(|r| &r.fail).filter_map(hir_name);
                out.insert(format!("std.{module}.{}", f.name), fails.collect());
            }
        }
    }
    out
}

fn hir_name(t: &yan_hir::Type) -> Option<String> {
    match t {
        yan_hir::Type::Named { path, .. } => path.last().cloned(),
        _ => None,
    }
}

fn members(t: &Option<Type>) -> Vec<String> {
    match t {
        Some(Type::Named(n)) => n.path.last().cloned().into_iter().collect(),
        Some(Type::Union(ms)) => ms
            .iter()
            .filter_map(|m| match m {
                Type::Named(n) => n.path.last().cloned(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn cap_params(f: &FnDef) -> impl Iterator<Item = &str> {
    f.params.iter().filter_map(|p| match p {
        Param::Cap { name, .. } => Some(name.as_str()),
        Param::Value { .. } => None,
    })
}

fn events(f: &FnDef) -> (Vec<Ev>, HashMap<String, Type>) {
    let mut evs = Vec::new();
    let mut lets = HashMap::new();
    if let Some(b) = &f.body {
        block(b, &mut evs, &mut lets);
    }
    (evs, lets)
}

fn place_indexes(p: &Place) -> bool {
    p.projs.iter().any(|x| matches!(x, Proj::Index(_)))
}

fn rhs(r: &Rhs, span: Span, evs: &mut Vec<Ev>, lets: &mut HashMap<String, Type>) {
    match r {
        Rhs::Call(c) | Rhs::Try(c) => evs.push(Ev::Call {
            call: c.clone(),
            span: c.span,
            propagated: false,
        }),
        Rhs::Propagate { call, .. } => evs.push(Ev::Call {
            call: call.clone(),
            span: call.span,
            propagated: true,
        }),
        Rhs::Proj(p) | Rhs::Copy(p) | Rhs::Move(p) if place_indexes(p) => evs.push(Ev::Index(span)),
        Rhs::Closure(c) => {
            for (_, n, s) in &c.captures {
                evs.push(Ev::Capture(n.clone(), *s));
            }
            block(&c.body, evs, lets);
        }
        _ => {}
    }
}

fn block(b: &Block, evs: &mut Vec<Ev>, lets: &mut HashMap<String, Type>) {
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Let {
                pat,
                ty,
                rhs: r,
                rhs_span,
                ..
            } => {
                if let yan_core::ast::Pat::Bind(n) = pat {
                    lets.insert(n.clone(), ty.clone());
                }
                rhs(r, *rhs_span, evs, lets);
            }
            StmtKind::Assign {
                place,
                rhs: r,
                rhs_span,
                ..
            } => {
                if place_indexes(place) {
                    evs.push(Ev::Index(*rhs_span));
                }
                rhs(r, *rhs_span, evs, lets);
            }
            StmtKind::Rhs(r) => rhs(r, s.span, evs, lets),
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                block(then, evs, lets);
                if let Some(e) = els {
                    block(e, evs, lets);
                }
            }
            StmtKind::Match { arms, .. } => arms.iter().for_each(|(_, b)| block(b, evs, lets)),
            StmtKind::Loop { body, .. }
            | StmtKind::Jp { body, .. }
            | StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::ComptimeFor { body, .. } => block(body, evs, lets),
            StmtKind::Unsafe(b) => block(b, evs, lets),
            StmtKind::Par { blocks, .. } => blocks.iter().for_each(|b| block(b, evs, lets)),
            StmtKind::Become(c) => evs.push(Ev::Call {
                call: c.clone(),
                span: c.span,
                propagated: false,
            }),
            StmtKind::Fail(a) => evs.push(Ev::Fail(a.clone(), s.span)),
            StmtKind::Assert(_) => evs.push(Ev::Assert(s.span)),
            StmtKind::Unreachable => evs.push(Ev::Unreachable(s.span)),
            _ => {}
        }
    }
}

fn is_overflow_op(path: &[String]) -> bool {
    let [name] = path else { return false };
    [
        "add_checked_",
        "sub_checked_",
        "mul_checked_",
        "neg_checked_",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

fn is_div_op(path: &[String]) -> bool {
    let [name] = path else { return false };
    ["floordiv_", "mod_"].iter().any(|p| name.starts_with(p))
}

struct Analysis<'a> {
    fns: Fns<'a>,
    std: HashMap<String, Vec<String>>,
}

/// What a function reaches directly for a fact, and the module functions it must go through.
struct Node {
    leaf: Option<Step>,
    edges: Vec<(Step, String, String)>,
}

impl Analysis<'_> {
    /// `tracked` is the capability's name inside this function, for a capability fact.
    fn node(&self, f: &FnDef, fact: &Fact, tracked: &str) -> Node {
        let (evs, lets) = events(f);
        let mut node = Node {
            leaf: None,
            edges: Vec::new(),
        };
        let mut leaf = |span: Span, text: String| {
            node.leaf.get_or_insert(Step { span, text });
        };
        let mut edges = Vec::new();
        for ev in evs {
            match (ev, fact) {
                (Ev::Capture(n, span), Fact::Cap(_)) if n == tracked => {
                    leaf(span, format!("a closure captures `{n}`"));
                }
                (
                    Ev::Call {
                        call,
                        span,
                        propagated,
                    },
                    _,
                ) => {
                    let name = call.path.join(".");
                    let callee = self.fns.get(&name);
                    match fact {
                        Fact::Cap(_) => {
                            let passed = call.args.iter().enumerate().find_map(|(i, a)| match a {
                                Arg::Cap(n, _) if n == tracked => Some(i),
                                _ => None,
                            });
                            let Some(i) = passed else { continue };
                            match callee.and_then(|c| cap_params(c).nth(cap_index(&call, i))) {
                                Some(inner) => edges.push((
                                    Step {
                                        span,
                                        text: format!("calls `{name}`, which uses `{inner}`"),
                                    },
                                    name,
                                    inner.to_owned(),
                                )),
                                None => leaf(span, format!("calls `{name}` with `{tracked}`")),
                            }
                        }
                        Fact::Fail(m) if propagated => {
                            let has = match callee {
                                Some(c) => members(&c.fail).contains(m),
                                None => self.std.get(&name).is_some_and(|v| v.contains(m)),
                            };
                            if has {
                                let step = Step {
                                    span,
                                    text: format!("propagates `{m}` from `{name}`"),
                                };
                                if callee.is_some() {
                                    edges.push((step, name, String::new()));
                                } else {
                                    leaf(step.span, step.text);
                                }
                            }
                        }
                        Fact::Panic(k) => {
                            if *k == "overflow" && is_overflow_op(&call.path) {
                                leaf(span, format!("`{name}` overflows"));
                            } else if *k == "div_zero" && is_div_op(&call.path) {
                                leaf(span, format!("`{name}` divides by a divisor that may be 0"));
                            } else if *k == "contract"
                                && callee.is_some_and(|c| !c.contracts.is_empty())
                            {
                                leaf(span, format!("`{name}` has a contract the call must meet"));
                            } else if callee.is_some() {
                                edges.push((
                                    Step {
                                        span,
                                        text: format!("calls `{name}`"),
                                    },
                                    name,
                                    String::new(),
                                ));
                            }
                        }
                        Fact::Fail(_) => {}
                    }
                }
                (Ev::Fail(a, span), Fact::Fail(m)) => {
                    if let Atom::Name(n, _) = &a
                        && members(&lets.get(n).cloned()).contains(m)
                    {
                        leaf(span, format!("fails with `{m}`"));
                    }
                }
                (Ev::Assert(span), Fact::Panic("assert")) => leaf(span, "`assert` may fail".into()),
                (Ev::Unreachable(span), Fact::Panic("unreachable")) => {
                    leaf(span, "`unreachable` is reached".into());
                }
                (Ev::Index(span), Fact::Panic("bounds")) => {
                    leaf(span, "an index may be out of bounds".into());
                }
                _ => {}
            }
        }
        node.edges = edges;
        node
    }
}

/// The position among the callee's capability parameters of argument `i`.
fn cap_index(call: &Call, i: usize) -> usize {
    call.args[..i]
        .iter()
        .filter(|a| matches!(a, Arg::Cap(..)))
        .count()
}

/// Breadth-first from `item`: the first function with a direct cause ends the shortest chain.
fn chain(a: &Analysis, item: &FnDef, fact: &Fact, tracked: &str) -> Vec<Step> {
    type Key = (String, String);
    let start: Key = (item.path.join("."), tracked.to_owned());
    let mut seen: HashSet<Key> = HashSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start, Vec::<Step>::new())]);
    while let Some(((name, tr), steps)) = queue.pop_front() {
        let Some(f) = a.fns.get(&name) else { continue };
        let node = a.node(f, fact, &tr);
        if let Some(leaf) = node.leaf {
            return steps.into_iter().chain([leaf]).collect();
        }
        for (step, callee, inner) in node.edges {
            let key = (callee, inner);
            if seen.insert(key.clone()) {
                let mut next: Vec<Step> = steps
                    .iter()
                    .map(|s| Step {
                        span: s.span,
                        text: s.text.clone(),
                    })
                    .collect();
                next.push(step);
                queue.push_back((key, next));
            }
        }
    }
    Vec::new()
}

fn resolve(f: &FnDef, fact: &str) -> Result<Fact, String> {
    if cap_params(f).any(|c| c == fact) {
        Ok(Fact::Cap(fact.to_owned()))
    } else if let Some(k) = PANIC_KINDS.iter().find(|k| **k == fact) {
        if TRACED_KINDS.contains(k) {
            Ok(Fact::Panic(k))
        } else {
            Err(format!(
                "panic kind `{fact}` is not traced from Yan Core yet"
            ))
        }
    } else if members(&f.fail).iter().any(|m| m == fact) {
        Ok(Fact::Fail(fact.to_owned()))
    } else {
        Err(format!(
            "`{fact}` is not a capability, `fail` member or panic kind `{}` declares or can raise",
            f.path.join(".")
        ))
    }
}

/// The lines of the chain, or the empty-chain report: the declaration is unneeded.
pub fn run(file: &str, m: &Module, item: &str, fact: &str) -> Result<Vec<String>, String> {
    let fns = module_fns(m);
    let def = *fns
        .get(item)
        .ok_or_else(|| format!("no function `{item}` in {file}"))?;
    let kind = resolve(def, fact)?;
    let a = Analysis {
        fns,
        std: std_fails(),
    };
    let steps = chain(&a, def, &kind, fact);
    if steps.is_empty() {
        return Err(format!(
            "`{item}` does not need `{fact}`: the declaration is unneeded; remove it"
        ));
    }
    Ok(steps
        .into_iter()
        .map(|s| format!("{file}:{}:{}: {}", s.span.line, s.span.col, s.text))
        .collect())
}

pub fn command(item: &str, fact: &str) -> ExitCode {
    let Some((module, name)) = item.rsplit_once('.') else {
        return crate::tool_failure(&format!("`{item}` is not `module.item`"));
    };
    let src = Path::new("src");
    let root = if src.is_dir() { src } else { Path::new(".") };
    let path = match crate::patch::module_path(root, module) {
        Ok(p) => p,
        Err(m) => return crate::tool_failure(&m),
    };
    let mut db = yan_db::Database::new(false, false);
    let file = path.display().to_string();
    let m = match crate::checked(&mut db, &path) {
        Ok(m) => m,
        Err(code) => return code,
    };
    match run(&file, m, name, fact) {
        Ok(lines) => {
            lines.iter().for_each(|l| println!("{l}"));
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("yan: {msg}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    const SRC: &str = "class Oops:\n    n: int\n\ndef leaf(n: int) -> int fail Oops uses fs, io:\n    assert n > 0\n    txt = fs.read(\"x\") else \"x\"\n    io.print(txt)\n    if n > 3:\n        fail Oops(n)\n    return n // 2\n\npub def mid(n: int) -> int fail Oops uses fs, io:\n    \"\"\"Doc.\"\"\"\n    return leaf(n)?\n\npub def idle(n: int) -> int uses fs:\n    \"\"\"Doc.\"\"\"\n    return n\n";

    fn lines(item: &str, fact: &str) -> Result<Vec<String>, String> {
        let dir = std::env::temp_dir().join(format!("yan-why-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.yag");
        std::fs::write(&path, SRC).unwrap();
        let mut db = yan_db::Database::new(false, false);
        let core = crate::checked(&mut db, &path).unwrap_or_else(|_| panic!("checks"));
        super::run("m.yag", core, item, fact)
    }

    #[test]
    fn capability_chain_goes_through_callees() {
        let l = lines("mid", "fs").unwrap();
        assert_eq!(l.len(), 2, "{l:?}");
        assert!(l[0].contains("calls `leaf`") && l[1].contains("std.fs.read"));
    }

    #[test]
    fn fail_member_chain_ends_at_the_fail() {
        let l = lines("mid", "Oops").unwrap();
        assert!(
            l[0].contains("propagates `Oops` from `leaf`") && l[1].contains("fails with `Oops`")
        );
    }

    #[test]
    fn panic_kinds() {
        assert!(lines("leaf", "assert").unwrap()[0].contains("assert"));
        assert!(lines("leaf", "div_zero").is_ok());
        assert!(lines("mid", "shape").unwrap_err().contains("not traced"));
    }

    #[test]
    fn an_unneeded_declaration_has_an_empty_chain() {
        assert!(lines("idle", "fs").unwrap_err().contains("unneeded"));
        assert!(lines("idle", "nope").is_err());
    }
}
