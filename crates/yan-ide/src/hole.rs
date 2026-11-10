//! `hole_context` (21.2, R997): what an agent needs to fill one typed hole, after Blinn et al.,
//! "Statically Contextualizing Large Language Models with Typed Holes" (OOPSLA 2024).

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use yan_diag::Span;
use yan_hir::{Block, FuncDef, Item, ItemKind, Module, Param, Stmt, StmtKind, TArg, Type};

use super::{TypeHit, header, search_type, single};

pub const DEFAULT_MAX_TOKENS: usize = 400;
const SEARCH_HITS: usize = 5;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Binding {
    pub name: String,
    /// `None` when the binding has no annotation and no inferred type is known.
    #[serde(rename = "type")]
    pub ty: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Definition {
    pub name: String,
    pub distance: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HoleContext {
    pub expected: String,
    pub bindings: Vec<Binding>,
    pub contracts: Vec<String>,
    pub examples: Vec<String>,
    pub search: Vec<String>,
    pub definitions: Vec<Definition>,
}

struct Found<'a> {
    func: &'a FuncDef,
    item: &'a Item,
    expected: Option<Type>,
    /// Bindings visible at the hole, declaration order.
    scope: Vec<(String, Option<Type>, u32)>,
}

/// The context of the hole at 1-based `line` and `col`, within `max_tokens` (four bytes each).
/// `inferred` maps `(name, line)` of an unannotated `let` to its inferred type text.
pub fn hole_context(
    m: &Module,
    line: u32,
    col: u32,
    max_tokens: usize,
    inferred: &HashMap<(String, u32), String>,
) -> Result<HoleContext, String> {
    let found = m
        .items
        .iter()
        .find_map(|it| find_in_item(it, line, col))
        .ok_or_else(|| format!("no hole at {line}:{col}"))?;
    let ret = found.func.ret.as_ref().and_then(|r| r.ty.clone());
    let expected = found
        .expected
        .clone()
        .or(ret)
        .ok_or_else(|| format!("the hole at {line}:{col} has no declared expected type"))?;
    let expected_text = yan_yag::print_type(&expected);

    let params = found.func.params.iter().filter_map(|p| match p {
        Param::Value { name, ty, span, .. } => Some((name.clone(), Some(ty.clone()), span.line)),
        Param::Pack { name, ty } => Some((name.clone(), Some(ty.clone()), 0)),
        Param::Slash => None,
    });
    let scope: Vec<_> = params.chain(found.scope.iter().cloned()).collect();
    // Nearest to the hole first; the last binding of a name shadows earlier ones.
    let mut seen = Vec::new();
    let mut bindings = Vec::new();
    for (name, ty, at) in scope.iter().rev() {
        if seen.contains(name) {
            continue;
        }
        seen.push(name.clone());
        let ty = ty
            .as_ref()
            .map(yan_yag::print_type)
            .or_else(|| inferred.get(&(name.clone(), *at)).cloned());
        bindings.push(Binding {
            name: name.clone(),
            ty,
        });
    }

    let head = header(&yan_yag::print(&single(found.item)));
    let (contracts, examples) = contracts_of(&head);

    let mut definitions = Vec::new();
    let mut named: BTreeMap<String, usize> = BTreeMap::new();
    type_names(&expected, &mut |n| {
        named.entry(n).or_insert(0);
    });
    for (i, (_, ty, _)) in scope.iter().rev().enumerate() {
        if let Some(t) = ty {
            type_names(t, &mut |n| {
                named.entry(n).or_insert(i + 1);
            });
        }
    }
    for (name, distance) in &named {
        let def = m.items.iter().find(|it| match &it.kind {
            ItemKind::Record(r) => &r.name == name,
            ItemKind::Type(t) => &t.name == name,
            _ => false,
        });
        if let Some(it) = def {
            definitions.push(Definition {
                name: name.clone(),
                distance: *distance,
                text: yan_yag::print(&single(it)).trim_end().to_owned(),
            });
        }
    }
    definitions.sort_by(|a, b| (a.distance, &a.name).cmp(&(b.distance, &b.name)));

    let search: Vec<String> = search_type(&expected_text)
        .unwrap_or_default()
        .into_iter()
        .take(SEARCH_HITS)
        .map(|TypeHit { name, signature }| format!("{name}: {signature}"))
        .collect();

    let mut ctx = HoleContext {
        expected: expected_text,
        bindings: Vec::new(),
        contracts: Vec::new(),
        examples: Vec::new(),
        search: Vec::new(),
        definitions: Vec::new(),
    };
    let mut used = ctx.expected.len() / 4 + 1;
    // Each list keeps its order and drops what does not fit; lists fill in priority order.
    let mut take = |from: Vec<String>, cost: &dyn Fn(&str) -> usize| -> Vec<bool> {
        from.iter()
            .map(|s| {
                let c = cost(s) / 4 + 1;
                let fits = used + c <= max_tokens;
                if fits {
                    used += c;
                }
                fits
            })
            .collect()
    };
    let keep = take(
        bindings
            .iter()
            .map(|b| format!("{}: {}", b.name, b.ty.as_deref().unwrap_or("?")))
            .collect(),
        &|s| s.len(),
    );
    ctx.bindings = pick(bindings, &keep);
    let keep = take(contracts.clone(), &|s| s.len());
    ctx.contracts = pick(contracts, &keep);
    let keep = take(examples.clone(), &|s| s.len());
    ctx.examples = pick(examples, &keep);
    let keep = take(definitions.iter().map(|d| d.text.clone()).collect(), &|s| {
        s.len()
    });
    ctx.definitions = pick(definitions, &keep);
    let keep = take(search.clone(), &|s| s.len());
    ctx.search = pick(search, &keep);
    Ok(ctx)
}

fn pick<T>(items: Vec<T>, keep: &[bool]) -> Vec<T> {
    items
        .into_iter()
        .zip(keep)
        .filter_map(|(x, &k)| k.then_some(x))
        .collect()
}

/// `pre`, `post`, `decreases` and `cost` lines, then `ex` lines, from the printed header.
fn contracts_of(head: &str) -> (Vec<String>, Vec<String>) {
    let (mut contracts, mut examples) = (Vec::new(), Vec::new());
    for l in head.lines().map(str::trim) {
        if l.starts_with("ex ") {
            examples.push(l.to_owned());
        } else if ["pre ", "post ", "decreases ", "cost "]
            .iter()
            .any(|k| l.starts_with(k))
        {
            contracts.push(l.to_owned());
        }
    }
    (contracts, examples)
}

fn type_names(t: &Type, f: &mut dyn FnMut(String)) {
    match t {
        Type::Named { path, args } => {
            if let [n] = path.as_slice() {
                f(n.clone());
            }
            for a in args {
                if let TArg::Type(t) = a {
                    type_names(t, f);
                }
            }
        }
        Type::Callable { params, ret, .. } => {
            params.iter().for_each(|p| type_names(p, f));
            type_names(ret, f);
        }
        Type::Call { .. } | Type::None => {}
    }
}

fn contains(s: Span, line: u32, col: u32) -> bool {
    (s.line, s.col) <= (line, col) && (line, col) < (s.end_line, s.end_col.max(s.col + 1))
}

fn find_in_item(it: &Item, line: u32, col: u32) -> Option<Found<'_>> {
    let funcs: Vec<&FuncDef> = match &it.kind {
        ItemKind::Func(f) | ItemKind::Lemma(f) => vec![f],
        ItemKind::Impl(i) => i.fns.iter().collect(),
        _ => return None,
    };
    funcs.into_iter().find_map(|f| {
        let body = f.body.as_ref()?;
        let mut scope = Vec::new();
        let (expected, _) = find_in_stmts(&body.stmts, line, col, &mut scope)?;
        Some(Found {
            func: f,
            item: it,
            expected,
            scope,
        })
    })
}

type Scope = Vec<(String, Option<Type>, u32)>;

/// The hole's own expected type (`None`: the enclosing function's return type) when `line:col`
/// is a hole in `stmts`; `scope` gains the bindings made before it on the way down.
fn find_in_stmts(
    stmts: &[Stmt],
    line: u32,
    col: u32,
    scope: &mut Scope,
) -> Option<(Option<Type>, ())> {
    let hole = |e: &yan_hir::Expr| {
        matches!(e.kind, yan_hir::ExprKind::Hole) && contains(e.span, line, col)
    };
    for s in stmts {
        let before = scope.len();
        let hit = match &s.kind {
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) if hole(e) => Some(None),
            StmtKind::Bind { ty, value, .. } if hole(value) => Some(ty.clone()),
            StmtKind::Assign { ty, value, .. } if hole(value) => Some(ty.clone()),
            StmtKind::If { arms, els } => arms
                .iter()
                .map(|(_, b)| b)
                .chain(els.as_ref())
                .find_map(|b: &Block| find_in_stmts(&b.stmts, line, col, scope))
                .map(|(t, _)| t),
            StmtKind::While { body, .. } | StmtKind::For { body, .. } => {
                find_in_stmts(body, line, col, scope).map(|(t, _)| t)
            }
            StmtKind::Match { cases, .. } => cases
                .iter()
                .find_map(|c| find_in_stmts(&c.body.stmts, line, col, scope))
                .map(|(t, _)| t),
            StmtKind::With { body, .. } => {
                find_in_stmts(&body.stmts, line, col, scope).map(|(t, _)| t)
            }
            _ => None,
        };
        if let Some(t) = hit {
            return Some((t, ()));
        }
        scope.truncate(before);
        match &s.kind {
            StmtKind::Bind {
                target,
                ty,
                target_span,
                ..
            } => scope.push((target.clone(), ty.clone(), target_span.line)),
            StmtKind::Assign {
                target:
                    yan_hir::Expr {
                        kind: yan_hir::ExprKind::Name(n),
                        span,
                        ..
                    },
                ty,
                ..
            } if !scope.iter().any(|(x, _, _)| x == n) => {
                scope.push((n.clone(), ty.clone(), span.line))
            }
            _ => {}
        }
    }
    None
}
