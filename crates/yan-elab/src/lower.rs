//! Function bodies: Yan Agent statements and expressions to Yan Core statements in A-normal form.
//!
//! Every compound expression becomes a typed `let` of a fresh `_tN`. A value that joins
//! control flow (`a if c else b`, `and`, `or`, `x else y`) becomes a join point whose body is
//! the rest of the enclosing block: the block is split where the join is made and closed
//! when the block ends.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::rc::Rc;

use yan_core::ast::{self as y, Arg, Atom, Conv, Literal, Place, Proj, Rhs, StmtKind};
use yan_diag::{Applicability, Span};
use yan_hir as j;

use crate::bounds::{self, Site};
use crate::ty::Axis;
use crate::{Cx, Globals, R, Sig, Ty, ffi, unsupported};

mod actor;
mod args;
mod arith;
mod array;
mod assign;
mod builtin;
mod call_expr;
mod calls;
mod cluster;
mod consume;
mod fallback;
mod last_use;
mod loops;
mod matching;
mod method;
mod narrow;
mod ptr;
pub(crate) mod reentrant;
mod rhs;
mod scalar;
mod std_calls;
mod stmt_form;
pub(crate) mod subscript;

/// List and dict methods whose receiver is `var` (R421).
const MUTATING: [&str; 11] = [
    "append", "pop", "insert", "extend", "remove", "clear", "sort", "reverse", "add", "discard",
    "update",
];

struct Local {
    ty: Ty,
}

struct Join {
    before: Vec<y::Stmt>,
    jp: y::Stmt,
    branch: y::Stmt,
}

struct Lower<'a, 'c, 's> {
    g: &'a Globals,
    cx: &'c mut Cx<'s>,
    sig: &'a Sig,
    item: String,
    out: Vec<y::Stmt>,
    joins: Vec<Vec<Join>>,
    scopes: Vec<Vec<(String, Option<Local>)>>,
    locals: HashMap<String, Local>,
    /// Names bound to a non-copy element of a list or array: a view, not an owner (E413 on a sink).
    index_views: HashSet<String>,
    /// Names whose block has ended; a later use needs function-level scope.
    gone: HashMap<String, Ty>,
    mutable: HashSet<String>,
    /// Each enclosing loop's label, with the name the source gave it.
    loops: Vec<(String, Option<String>)>,
    n: usize,
    /// Lowering a `become` call, whose arguments are exempt from E210 (SPEC_QUESTIONS).
    in_become: bool,
    /// The first parameter index whose argument N49 turned into a keyword one.
    keyworded_from: Option<usize>,
    /// Dict elements that `place` copied out; a mutating caller stores them back.
    dict_places: Vec<DictPlace>,
    /// Last use of each local, for E413 and `move` versus `copy`.
    last_use: last_use::LastUse,
    /// Names the body indexes or measures, which N37 collects when bound to an `iter`.
    indexed: HashSet<String>,
    ghosts: HashSet<String>,
    /// The parameter N05 took as the measure.
    inferred: Option<String>,
    /// User bindings and where each is made, for W812.
    bound: Vec<(String, Span)>,
    /// What each index expression indexes, for the bounds solver.
    sites: HashMap<Span, Site>,
    /// `&` of a field or element passes a copy; it is stored back after the statement.
    writeback: Vec<(String, Place, Span)>,
    /// The parameter type, argument type and span of each argument `args` lowered.
    arg_tys: Vec<(Ty, Ty, Span)>,
    /// The type the enclosing context expects of the call being lowered; infers a type
    /// parameter that only the return mentions (`e: iter[int] = iter.empty()`).
    want_ret: Option<(Ty, Span)>,
    /// Capabilities bound by an enclosing `with name = handler`.
    handlers: Vec<String>,
    /// Each call of a non-`par` callable local, for E716.
    fn_calls: Vec<(String, Span)>,
    /// `k = lambda ...` with no expected type: the target and lambda, lowered at the first call.
    pending: HashMap<String, (j::Expr, j::Expr)>,
    /// The name the current statement assigns, for W822.
    target: Option<String>,
    /// Inside an `unsafe` block (12.1).
    in_unsafe: bool,
    /// Lowering `gpu.launch`, the one place a `TileOrigin` is built and a kernel called.
    in_launch: bool,
    /// The handle a message of `actor.send` calls through, which may be a `SendOnly` (R975).
    sent_through: Option<String>,
    /// Stage-0 names: an instance's `Nat` parameters and `#for` targets (10.4).
    stage: Vec<(String, Sv)>,
    /// Optional locals narrowed by an `is None` test (R285): the source name, the local
    /// holding the unwrapped value, and the scope depth the narrowing ends with.
    narrow: Vec<(String, String, usize)>,
    /// The `post` clauses, checked at each return (8.4).
    posts: Vec<j::Expr>,
    /// Each `var` parameter `old(p)` reads, with the local holding its entry value.
    olds: Vec<(String, String)>,
    /// The parameters written as `View[T]`, and whether the function returns a view.
    views: Vec<String>,
    returns_view: bool,
    /// Types learned for empty `[]` and `{}` bindings, by the span of the literal (R808).
    hints: Rc<RefCell<HashMap<Span, Ty>>>,
    /// Names read after the block that bound them, with their types: the next lowering
    /// declares each before a statement that binds it on every path (R832).
    hoists: Rc<RefCell<HashMap<String, Ty>>>,
    /// Locals bound to an empty literal with no type yet, and that literal's span.
    open: HashMap<String, Span>,
}

/// A stage-0 value the elaborator folds itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Sv {
    Int(i128),
    Bool(bool),
}

impl Sv {
    fn atom(self, at: Span) -> (Atom, Ty) {
        match self {
            Sv::Int(n) => (Atom::Lit(Literal::Int(n.to_string()), at), Ty::Int),
            Sv::Bool(b) => (Atom::Lit(Literal::Bool(b), at), Ty::Bool),
        }
    }
}

/// `e` folded with the stage-0 names in `env`, when it is built from literals and those names.
pub(crate) fn fold(e: &j::Expr, env: &[(String, Sv)]) -> Option<Sv> {
    use j::ExprKind as K;
    let int = |e| match fold(e, env)? {
        Sv::Int(n) => Some(n),
        Sv::Bool(_) => None,
    };
    let bool_ = |e| match fold(e, env)? {
        Sv::Bool(b) => Some(b),
        Sv::Int(_) => None,
    };
    Some(match &e.kind {
        K::Lit(j::Lit::Int(t)) => Sv::Int(int_text(t).ok()?.parse().ok()?),
        K::Lit(j::Lit::Bool(b)) => Sv::Bool(*b),
        K::Name(n) => env.iter().rev().find(|(m, _)| m == n)?.1,
        K::Unary("-", x) => Sv::Int(int(x)?.checked_neg()?),
        K::Not(x) => Sv::Bool(!bool_(x)?),
        K::Binary("and", a, b) => Sv::Bool(bool_(a)? && bool_(b)?),
        K::Binary("or", a, b) => Sv::Bool(bool_(a)? || bool_(b)?),
        K::Binary(op, a, b) => {
            let (x, y) = (int(a)?, int(b)?);
            Sv::Int(match *op {
                "+" => x.checked_add(y)?,
                "-" => x.checked_sub(y)?,
                "*" => x.checked_mul(y)?,
                "//" if y != 0 => x.div_euclid(y),
                "%" if y != 0 => x.rem_euclid(y),
                _ => return None,
            })
        }
        K::Compare(op, a, b) => {
            let (x, y) = (fold(a, env)?, fold(b, env)?);
            Sv::Bool(match (*op, x, y) {
                ("==", x, y) => x == y,
                ("!=", x, y) => x != y,
                ("<", Sv::Int(x), Sv::Int(y)) => x < y,
                ("<=", Sv::Int(x), Sv::Int(y)) => x <= y,
                (">", Sv::Int(x), Sv::Int(y)) => x > y,
                (">=", Sv::Int(x), Sv::Int(y)) => x >= y,
                _ => return None,
            })
        }
        _ => return None,
    })
}

pub(crate) fn literal(l: &j::Lit) -> R<Literal> {
    Ok(match l {
        j::Lit::Int(s) => Literal::Int(int_text(s)?),
        j::Lit::Float(s) => Literal::Float(s.replace('_', "")),
        j::Lit::Str { prefix, value } if prefix.is_empty() || prefix == "r" => {
            Literal::Str(value.clone())
        }
        // Yan has no C string literal yet; the value is the text, typed `CStr` (SPEC_QUESTIONS).
        j::Lit::Str { prefix, value } if prefix == "c" => Literal::Str(value.clone()),
        j::Lit::Str { .. } => return unsupported("byte strings"),
        j::Lit::Char(c) => Literal::Char(*c),
        j::Lit::Bool(b) => Literal::Bool(*b),
        j::Lit::None => Literal::Unit,
    })
}

/// An integer literal typed as the binary float type `want` when its value is exact there (R656).
fn float_lit(text: &str, want: &Ty, at: Span) -> Option<Rhs> {
    let (precision, max) = match want {
        Ty::Float => (53, None),
        Ty::Fixed(n) => match *n {
            "f16" => (11, Some(65504)),
            "bf16" => (8, None),
            "f32" => (24, None),
            "f64" => (53, None),
            "f128" => (113, None),
            _ => return None,
        },
        _ => return None,
    };
    let v = text.parse::<i128>().ok()?.unsigned_abs();
    let odd = v.checked_shr(v.trailing_zeros()).unwrap_or(0);
    let exact = u128::BITS - odd.leading_zeros() <= precision && max.is_none_or(|m| v <= m);
    exact.then(|| Rhs::Atom(Atom::Lit(Literal::Float(format!("{text}.0")), at)))
}

fn int_text(s: &str) -> R<String> {
    let s = s.replace('_', "");
    let (radix, digits) = match s.get(..2) {
        Some("0x" | "0X") => (16, &s[2..]),
        Some("0o" | "0O") => (8, &s[2..]),
        Some("0b" | "0B") => (2, &s[2..]),
        _ => return Ok(s),
    };
    match i128::from_str_radix(digits, radix) {
        Ok(v) => Ok(v.to_string()),
        Err(_) => unsupported("integer literal out of range"),
    }
}

pub(crate) fn walk<'e>(e: &'e j::Expr, f: &mut impl FnMut(&'e j::Expr)) {
    f(e);
    use j::ExprKind as K;
    match &e.kind {
        K::Tuple(xs) | K::List(xs) | K::Set(xs) => xs.iter().for_each(|x| walk(x, f)),
        K::Dict(kvs) => kvs.iter().for_each(|(k, v)| {
            walk(k, f);
            walk(v, f);
        }),
        K::Comp {
            elem,
            value,
            iter,
            cond,
            ..
        } => {
            walk(elem, f);
            value.iter().for_each(|v| walk(v, f));
            walk(iter, f);
            cond.iter().for_each(|c| walk(c, f));
        }
        K::Attr(b, ..)
        | K::Try(b)
        | K::Unary(_, b)
        | K::Not(b)
        | K::Lambda(_, b)
        | K::Reify(b)
        | K::Stage(b)
        | K::Cast(b, _) => walk(b, f),
        K::Call(c, args) => {
            walk(c, f);
            for a in args {
                match &a.kind {
                    j::ArgKind::Expr(x) | j::ArgKind::Ref(x) | j::ArgKind::Become(x) => walk(x, f),
                }
            }
        }
        K::Index(a, b) | K::Binary(_, a, b) | K::Compare(_, a, b) => {
            walk(a, f);
            walk(b, f);
        }
        K::Slice(a, parts, _) => {
            walk(a, f);
            parts.iter().flatten().for_each(|p| walk(p, f));
        }
        K::Cond { then, cond, els } => {
            walk(then, f);
            walk(cond, f);
            walk(els, f);
        }
        K::Fallback(a, fb) => {
            walk(a, f);
            match &**fb {
                j::Fallback::Value(x) | j::Fallback::Bind(_, x) | j::Fallback::Fail(x) => {
                    walk(x, f)
                }
                j::Fallback::Return(x) => x.iter().for_each(|x| walk(x, f)),
                j::Fallback::Break | j::Fallback::Continue | j::Fallback::Block(..) => {}
            }
        }
        K::FStr { pieces, .. } => {
            for p in pieces {
                if let j::FPiece::Expr(x, _) = p {
                    walk(x, f);
                }
            }
        }
        K::Name(_) | K::Lit(_) | K::Hole | K::Wild => {}
    }
}

fn temporary_receiver(what: &str) -> String {
    format!(
        "{what} changes its receiver, but the receiver is a temporary that is dropped after \
         the call; bind it to a local first"
    )
}

fn root(e: &j::Expr) -> Option<&str> {
    match &e.kind {
        j::ExprKind::Name(n) => Some(n),
        j::ExprKind::Attr(b, ..) | j::ExprKind::Index(b, _) => root(b),
        _ => None,
    }
}

/// Exprs directly in a statement, for the mutability scan.
pub(crate) fn stmt_exprs(s: &j::Stmt) -> Vec<&j::Expr> {
    use j::StmtKind as K;
    match &s.kind {
        K::Expr(e) | K::Become(e) | K::Fail(e) | K::Assert(e) => vec![e],
        K::Assign { target, value, .. } | K::AugAssign { target, value, .. } => vec![target, value],
        K::Ghost { value, .. } | K::Bind { value, .. } => vec![value],
        K::Return(e) => e.iter().collect(),
        K::If { arms, .. } => arms.iter().map(|(c, _)| c).collect(),
        K::While { cond, .. } | K::StageIf { cond, .. } => vec![cond],
        K::StageFor { iter, .. } => vec![iter],
        K::For { iter, .. } => vec![iter],
        K::Match { scrutinee, .. } => vec![scrutinee],
        K::With { kind, .. } => match kind {
            j::WithKind::Bind(_, e)
            | j::WithKind::Deadline(e)
            | j::WithKind::Timeout(e)
            | j::WithKind::Supervisor(e) => vec![e],
        },
        K::Par {
            body: j::ParBody::For { iter, .. },
            ..
        } => vec![iter],
        K::Select(arms) => arms.iter().map(|a| &a.op).collect(),
        K::Asm { args, .. } => args
            .iter()
            .filter_map(|a| match a {
                j::AsmArg::Operand { value, .. } => Some(value),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn sub_blocks(s: &j::Stmt) -> Vec<&[j::Stmt]> {
    use j::StmtKind as K;
    match &s.kind {
        K::If { arms, els } => arms
            .iter()
            .map(|(_, b)| b.stmts.as_slice())
            .chain(els.iter().map(|b| b.stmts.as_slice()))
            .collect(),
        K::While { body, .. }
        | K::For { body, .. }
        | K::Unsafe(body)
        | K::StageFor { body, .. } => vec![body],
        K::StageIf { then, els, .. } => std::iter::once(then.as_slice())
            .chain(els.iter().map(Vec::as_slice))
            .collect(),
        K::Match { cases, .. } => cases.iter().map(|c| c.body.stmts.as_slice()).collect(),
        K::With { body, .. } => vec![&body.stmts],
        K::Par { body, .. } => match body {
            j::ParBody::Block(b) => vec![b],
            j::ParBody::For { body, .. } => vec![body],
        },
        K::Select(arms) => arms.iter().map(|a| a.body.stmts.as_slice()).collect(),
        _ => Vec::new(),
    }
}

/// Names used as `n[i]`, `n[a:b]`, `len(n)` or `n.len()`.
fn indexed_names(stmts: &[j::Stmt], out: &mut HashSet<String>) {
    use j::ExprKind as K;
    for s in stmts {
        for e in stmt_exprs(s) {
            walk(e, &mut |e| {
                let b = match &e.kind {
                    K::Index(b, _) | K::Slice(b, ..) => b,
                    K::Call(f, args) => match (&f.kind, args.as_slice()) {
                        (K::Name(n), [a]) if n == "len" => match &a.kind {
                            j::ArgKind::Expr(x) => x,
                            _ => return,
                        },
                        (K::Attr(b, m, _), []) if m == "len" => b,
                        _ => return,
                    },
                    _ => return,
                };
                if let K::Name(n) = &b.kind {
                    out.insert(n.clone());
                }
            });
        }
        for b in sub_blocks(s) {
            indexed_names(b, out);
        }
    }
}

/// W806: `if p == lit` conditions where `lit` is the literal an `ex` line passes as `p`, and
/// lookups keyed by one (R675): a `case lit` of `match p`, and `d[p]` or `d.get(p)` where `d`
/// is a dict literal, or a local assigned one, with the key `lit`.
fn special_cases(
    f: &j::FuncDef,
    contracts: &[j::Contract],
    stmts: &[j::Stmt],
) -> Vec<(Span, &'static str)> {
    let mut inputs: Vec<(&str, &j::Lit)> = Vec::new();
    for c in contracts {
        let j::Contract::Ex { expr, .. } = c else {
            continue;
        };
        let j::ExprKind::Compare(_, call, _) = &expr.kind else {
            continue;
        };
        let j::ExprKind::Call(callee, args) = &call.kind else {
            continue;
        };
        if !matches!(&callee.kind, j::ExprKind::Name(n) if *n == f.name) {
            continue;
        }
        let names = f.params.iter().filter_map(|p| match p {
            j::Param::Value { name, .. } => Some(name),
            _ => None,
        });
        for (p, a) in names.zip(args) {
            if let (None, j::ArgKind::Expr(e)) = (&a.name, &a.kind)
                && let j::ExprKind::Lit(l) = &e.kind
            {
                inputs.push((p, l));
            }
        }
    }
    const BRANCH: &str = "this branch special-cases an `ex` input";
    let mut out = Vec::new();
    if inputs.is_empty() {
        return out;
    }
    let is_input = |p: &str, l: &j::Lit| inputs.iter().any(|(q, v)| *q == p && *v == l);
    fn name(e: &j::Expr) -> Option<&str> {
        match &e.kind {
            j::ExprKind::Name(n) => Some(n),
            _ => None,
        }
    }
    let mut dicts: HashMap<&str, &[(j::Expr, j::Expr)]> = HashMap::new();
    let mut conds = Vec::new();
    let mut lookups: Vec<(&j::Expr, &j::Expr, Span)> = Vec::new();
    fn scan<'e>(
        stmts: &'e [j::Stmt],
        dicts: &mut HashMap<&'e str, &'e [(j::Expr, j::Expr)]>,
        conds: &mut Vec<&'e j::Expr>,
        lookups: &mut Vec<(&'e j::Expr, &'e j::Expr, Span)>,
        cases: &mut Vec<(&'e j::Expr, &'e j::Case)>,
    ) {
        for s in stmts {
            match &s.kind {
                j::StmtKind::If { arms, .. } => conds.extend(arms.iter().map(|(c, _)| c)),
                j::StmtKind::Match {
                    scrutinee,
                    cases: cs,
                } => cases.extend(cs.iter().map(|c| (scrutinee, c))),
                j::StmtKind::Assign { target, value, .. } => {
                    if let (j::ExprKind::Name(n), j::ExprKind::Dict(kvs)) =
                        (&target.kind, &value.kind)
                    {
                        dicts.insert(n, kvs);
                    }
                }
                _ => {}
            }
            for e in stmt_exprs(s) {
                walk(e, &mut |x| match &x.kind {
                    j::ExprKind::Index(d, k) => lookups.push((d, k, x.span)),
                    j::ExprKind::Call(c, args) => {
                        if let (j::ExprKind::Attr(d, m, _), [a]) = (&c.kind, args.as_slice())
                            && m == "get"
                            && let (None, j::ArgKind::Expr(k)) = (&a.name, &a.kind)
                        {
                            lookups.push((d, k, x.span));
                        }
                    }
                    _ => {}
                });
            }
            for b in sub_blocks(s) {
                scan(b, dicts, conds, lookups, cases);
            }
        }
    }
    let mut cases = Vec::new();
    scan(stmts, &mut dicts, &mut conds, &mut lookups, &mut cases);
    for c in conds {
        let j::ExprKind::Compare("==", a, b) = &c.kind else {
            continue;
        };
        let hit = |x: &j::Expr, y: &j::Expr| match (name(x), &y.kind) {
            (Some(n), j::ExprKind::Lit(l)) => is_input(n, l),
            _ => false,
        };
        if hit(a, b) || hit(b, a) {
            out.push((c.span, BRANCH));
        }
    }
    for (scrutinee, c) in cases {
        if let (
            Some(p),
            j::Pat::Lit(j::Expr {
                kind: j::ExprKind::Lit(l),
                ..
            }),
        ) = (name(scrutinee), &c.pat)
            && is_input(p, l)
        {
            out.push((c.span, BRANCH));
        }
    }
    for (d, k, at) in lookups {
        let Some(p) = name(k) else { continue };
        let kvs = match &d.kind {
            j::ExprKind::Dict(kvs) => Some(kvs.as_slice()),
            j::ExprKind::Name(n) => dicts.get(n.as_str()).copied(),
            _ => None,
        };
        if kvs.is_some_and(|kvs| {
            kvs.iter()
                .any(|(key, _)| matches!(&key.kind, j::ExprKind::Lit(l) if is_input(p, l)))
        }) {
            out.push((at, "this lookup is keyed by an `ex` input"));
        }
    }
    out
}

/// R421: a local is mutable when the function assigns it again, assigns through it, passes
/// it as `&x`, or calls a method with a `var` receiver on it.
fn mutables(
    g: &Globals,
    stmts: &[j::Stmt],
    assigned: &mut HashMap<String, usize>,
    out: &mut HashSet<String>,
) {
    for s in stmts {
        match &s.kind {
            j::StmtKind::Assign { target, .. } => {
                let names: Vec<&j::Expr> = match &target.kind {
                    j::ExprKind::Tuple(ts) => ts.iter().collect(),
                    _ => vec![target],
                };
                for t in names {
                    match &t.kind {
                        j::ExprKind::Name(n) => *assigned.entry(n.clone()).or_default() += 1,
                        _ => {
                            if let Some(r) = root(t) {
                                out.insert(r.to_owned());
                            }
                        }
                    }
                }
            }
            j::StmtKind::AugAssign { target, .. } => {
                if let Some(r) = root(target) {
                    out.insert(r.to_owned());
                }
            }
            j::StmtKind::Bind {
                mutable: true,
                target,
                ..
            } => {
                out.insert(target.clone());
            }
            _ => {}
        }
        for e in stmt_exprs(s) {
            walk(e, &mut |e| {
                if let j::ExprKind::Call(f, args) = &e.kind {
                    for a in args {
                        if let j::ArgKind::Ref(x) = &a.kind
                            && let Some(r) = root(x)
                        {
                            out.insert(r.to_owned());
                        }
                    }
                    if let j::ExprKind::Attr(recv, m, _) = &f.kind
                        && let Some(r) = root(recv)
                    {
                        let var_first = g
                            .fns
                            .get(m)
                            .and_then(|s| s.params.first())
                            .is_some_and(|p| p.conv == Conv::Inout);
                        if var_first
                            || MUTATING.contains(&m.as_str())
                            || crate::stdlib::mutates_receiver(m)
                        {
                            out.insert(r.to_owned());
                        }
                    }
                }
            });
        }
        for b in sub_blocks(s) {
            mutables(g, b, assigned, out);
        }
    }
}

fn declared_lets(stmts: &[j::Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        if let j::StmtKind::Bind {
            mutable: false,
            target,
            ..
        } = &s.kind
        {
            out.insert(target.clone());
        }
        for b in sub_blocks(s) {
            declared_lets(b, out);
        }
    }
}

fn ends(stmts: &[j::Stmt]) -> bool {
    stmts.last().is_some_and(is_hole)
        || matches!(
            stmts.last().map(|s| &s.kind),
            Some(
                j::StmtKind::Return(_)
                    | j::StmtKind::Fail(_)
                    | j::StmtKind::Become(_)
                    | j::StmtKind::Unreachable
            )
        )
}

/// `Ok(...)` or `Err(...)`.
fn is_outcome(e: &j::Expr) -> bool {
    matches!(&e.kind, j::ExprKind::Call(f, _) if matches!(&f.kind, j::ExprKind::Name(n) if n == "Ok" || n == "Err"))
}

/// Whether control can leave the end of `stmts` without a `return`, `fail` or `become`.
fn falls_through(stmts: &[j::Stmt]) -> bool {
    use j::StmtKind as K;
    if ends(stmts) {
        return false;
    }
    match stmts.last().map(|s| &s.kind) {
        Some(K::If {
            arms,
            els: Some(els),
        }) => arms.iter().any(|(_, b)| falls_through(&b.stmts)) || falls_through(&els.stmts),
        Some(K::Match { cases, .. }) => cases.iter().any(|c| falls_through(&c.body.stmts)),
        Some(K::With { body, .. }) => falls_through(&body.stmts),
        Some(K::Unsafe(b)) => falls_through(b),
        Some(K::While { cond, body, .. }) => {
            !matches!(cond.kind, j::ExprKind::Lit(j::Lit::Bool(true))) || breaks(body)
        }
        _ => true,
    }
}

/// Whether a loop body has a `break` that leaves this loop.
fn breaks(stmts: &[j::Stmt]) -> bool {
    use j::StmtKind as K;
    stmts.iter().any(|s| match &s.kind {
        K::Break | K::BreakTo(_) => true,
        K::While { .. } | K::For { .. } => false,
        _ => sub_blocks(s).into_iter().any(breaks),
    })
}

/// `...` or `return ...` as a statement.
fn is_hole(s: &j::Stmt) -> bool {
    matches!(&s.kind, j::StmtKind::Expr(e) | j::StmtKind::Return(Some(e)) if matches!(e.kind, j::ExprKind::Hole))
}

/// A bodiless Yan Core signature, as a trait declares it.
pub(crate) fn signature(name: &str, sig: &Sig, is_pub: bool, at: Span) -> y::FnDef {
    y::FnDef {
        is_pub,
        is_unsafe: false,
        path: vec![name.to_owned()],
        name_span: at,
        tparams: tparams(sig),
        params: sig
            .params
            .iter()
            .map(|p| y::Param::Value {
                comptime: false,
                conv: p.conv,
                name: p.name.clone(),
                ty: p.ty.core(),
            })
            .collect(),
        ret: sig.ret.core(),
        from: Vec::new(),
        fail: sig.fail.as_ref().map(Ty::core),
        contracts: Vec::new(),
        body: None,
    }
}

fn tparams(sig: &Sig) -> Vec<y::TParam> {
    sig.tparams
        .iter()
        .map(|(n, bs)| y::TParam {
            comptime: false,
            name: n.clone(),
            kind: y::Kind::Type,
            bounds: bs.iter().map(|b| vec![b.clone()]).collect(),
        })
        .collect()
}

pub(crate) fn function(
    g: &Globals,
    f: &j::FuncDef,
    sig: &Sig,
    is_pub: bool,
    cx: &mut Cx,
) -> R<y::FnDef> {
    let outer = g.scope.borrow().len();
    g.scope
        .borrow_mut()
        .extend(sig.tparams.iter().map(|(n, _)| n.clone()));
    let r = function_body(g, f, sig, is_pub, Vec::new(), cx);
    g.scope.borrow_mut().truncate(outer);
    r
}

type Hints = Rc<RefCell<HashMap<Span, Ty>>>;
type Hoists = Rc<RefCell<HashMap<String, Ty>>>;

/// Lowers `f`; when it binds empty `[]` or `{}` and a later use types them, lowers again with
/// those types, so `xs = []` then `xs.append(1)` needs no annotation.
fn function_body(
    g: &Globals,
    f: &j::FuncDef,
    sig: &Sig,
    is_pub: bool,
    stage: Vec<(String, Sv)>,
    cx: &mut Cx,
) -> R<y::FnDef> {
    let first = cx.diags.len();
    let hints = Rc::default();
    let hoists: Hoists = Rc::default();
    let mut r = function_body_with(g, f, sig, is_pub, stage.clone(), cx, (&hints, &hoists));
    let mut seen = (0, 0);
    loop {
        let now = (hints.borrow().len(), hoists.borrow().len());
        if now == seen || now == (0, 0) {
            return r;
        }
        seen = now;
        cx.diags.truncate(first);
        r = function_body_with(g, f, sig, is_pub, stage.clone(), cx, (&hints, &hoists));
    }
}

fn function_body_with(
    g: &Globals,
    f: &j::FuncDef,
    sig: &Sig,
    is_pub: bool,
    stage: Vec<(String, Sv)>,
    cx: &mut Cx,
    (hints, hoists): (&Hints, &Hoists),
) -> R<y::FnDef> {
    let Some(body) = &f.body else {
        return unsupported("functions without bodies");
    };
    let mut assigned = HashMap::new();
    let mut mutable = HashSet::new();
    let first_diag = cx.diags.len();
    mutables(g, &body.stmts, &mut assigned, &mut mutable);
    mutable.extend(assigned.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k));
    // Yan declares mutability (15.3): a `let` stays immutable, and assigning it is E403.
    let mut lets = HashSet::new();
    declared_lets(&body.stmts, &mut lets);
    mutable.retain(|n| !lets.contains(n));
    let mut l = Lower {
        g,
        cx,
        sig,
        item: f.name.clone(),
        out: Vec::new(),
        joins: Vec::new(),
        scopes: Vec::new(),
        locals: HashMap::new(),
        index_views: HashSet::new(),
        gone: HashMap::new(),
        mutable,
        loops: Vec::new(),
        n: 0,
        in_become: false,
        in_launch: false,
        sent_through: None,
        keyworded_from: None,
        dict_places: Vec::new(),
        last_use: last_use::LastUse::new(&body.stmts),
        indexed: HashSet::new(),
        ghosts: sig.ghosts.iter().map(|(_, n)| n.clone()).collect(),
        inferred: None,
        bound: Vec::new(),
        sites: HashMap::new(),
        writeback: Vec::new(),
        arg_tys: Vec::new(),
        want_ret: None,
        handlers: Vec::new(),
        fn_calls: Vec::new(),
        pending: HashMap::new(),
        target: None,
        in_unsafe: false,
        stage,
        narrow: Vec::new(),
        posts: Vec::new(),
        olds: Vec::new(),
        views: f
            .params
            .iter()
            .filter_map(|p| match p {
                j::Param::Value { name, ty, .. } if is_view(ty) => Some(name.clone()),
                _ => None,
            })
            .collect(),
        returns_view: f
            .ret
            .as_ref()
            .and_then(|r| r.ty.as_ref())
            .is_some_and(is_view),
        hints: Rc::clone(hints),
        hoists: Rc::clone(hoists),
        open: HashMap::new(),
    };
    indexed_names(&body.stmts, &mut l.indexed);
    l.infer_decreases(f, &body.stmts);
    let mut params: Vec<y::Param> = sig
        .caps
        .iter()
        .map(|c| y::Param::Cap {
            name: c.clone(),
            ty: y::Named {
                path: vec![sig.cap_trait(c)],
                args: Vec::new(),
            },
        })
        .collect();
    for p in &sig.params {
        if let Ty::Coll("Atomic", ts) = &p.ty
            && byte_size(g, &ts[0]).is_none_or(|n| n > 16)
        {
            let line =
                l.cx.src
                    .lines()
                    .nth(f.name_span.line as usize - 1)
                    .unwrap_or("");
            let col = line
                .find("Atomic[")
                .map_or(f.name_span.col, |k| k as u32 + 1);
            let at = Span {
                line: f.name_span.line,
                col,
                end_line: f.name_span.line,
                end_col: col + 6,
            };
            let item = l.item.clone();
            l.cx.err(
                "E418",
                at,
                &item,
                format!(
                    "`{}` has no lock-free form: it is wider than 16 bytes",
                    l.ty_text(&ts[0])
                ),
            );
        }
        if let Ty::Coll("Entity", ts) = &p.ty
            && !hashable(g, &ts[0])
        {
            let line =
                l.cx.src
                    .lines()
                    .nth(f.name_span.line as usize - 1)
                    .unwrap_or("");
            let col = line
                .find("Entity[")
                .map_or(f.name_span.col, |k| k as u32 + 1);
            let at = Span {
                line: f.name_span.line,
                col,
                end_line: f.name_span.line,
                end_col: col + 6,
            };
            let item = l.item.clone();
            l.cx.err(
                "E726",
                at,
                &item,
                format!(
                    "an entity key must implement `Hash`; `{}` does not",
                    l.ty_text(&ts[0])
                ),
            );
        }
        l.locals.insert(p.name.clone(), Local { ty: p.ty.clone() });
        params.push(y::Param::Value {
            comptime: false,
            conv: p.conv,
            name: p.name.clone(),
            ty: p.ty.core(),
        });
    }
    let mut contracts = Vec::new();
    let mut pre = Vec::new();
    let mut cost = None;
    for c in &body.contracts {
        let ghostly = |e: &j::Expr| {
            let mut hit = false;
            walk(
                e,
                &mut |x| hit |= matches!(&x.kind, j::ExprKind::Name(n) if l.ghosts.contains(n)),
            );
            hit
        };
        match c {
            // A contract on ghost values is a proof obligation only (8.7).
            j::Contract::Pre(e) | j::Contract::Post(e) if ghostly(e) => {}
            j::Contract::Pre(e) => match l.pure(e)? {
                Some(x) => contracts.push(y::Contract::Requires(x)),
                None => pre.push(e),
            },
            j::Contract::Decreases(e) => match l.pure(e)? {
                Some(x) => contracts.push(y::Contract::Decreases(vec![x])),
                None => return unsupported("a `decreases` measure that is not one operation"),
            },
            j::Contract::Post(e) => l.posts.push(e.clone()),
            j::Contract::Ex { .. } => {}
            j::Contract::Cost { steps, alloc } => {
                let steps = steps
                    .as_ref()
                    .map(|e| l.cost_bound(f, "steps", e))
                    .transpose()?;
                let alloc = alloc
                    .as_ref()
                    .map(|e| l.cost_bound(f, "alloc", e))
                    .transpose()?;
                cost = Some(y::Contract::Cost { steps, alloc });
            }
        }
    }
    // After the `requires` clauses, so the counter starts once the inputs are known good.
    contracts.extend(cost);
    if let Some(p) = l.inferred.clone() {
        contracts.push(y::Contract::Decreases(vec![y::Expr::Atom(Atom::Name(
            p,
            f.name_span,
        ))]));
    }
    if !l.posts.is_empty()
        && let Some(p) = sig.params.iter().find(|p| p.name == "out")
    {
        let item = l.item.clone();
        l.cx.err(
            "E230",
            p.span,
            &item,
            "`out` is the result in `post`; rename this parameter".into(),
        );
    }
    let block = l.block_with(|l| {
        for e in pre {
            let a = l.cond(e)?;
            l.cx.marks.push(crate::marks::Pending {
                span: e.span,
                kind: crate::marks::Kind::Assert,
                call: None,
                mark: yan_surface::Mark::FromPre,
            });
            l.emit(StmtKind::Assert(y::Expr::Atom(a)), e.span);
        }
        for p in olds(&l.posts) {
            let Some(t) = sig
                .params
                .iter()
                .find(|q| q.name == p)
                .map(|q| q.ty.clone())
            else {
                continue;
            };
            let place = Place {
                root: p.clone(),
                root_span: f.name_span,
                projs: Vec::new(),
            };
            let v = l.fresh("old");
            l.bind(&v, false, &t, Rhs::Copy(place), f.name_span, f.name_span);
            l.olds.push((p, v));
        }
        if !l.cx.yn && !matches!(sig.ret, Ty::Unit | Ty::Err) && falls_through(&body.stmts) {
            let at = body.stmts.last().map_or(f.name_span, |s| s.span);
            l.err(
                "E201",
                at,
                format!(
                    "`{}` can reach its end without returning its `{}`; add a `return`",
                    f.name, sig.ret
                ),
            );
        }
        l.stmts(&body.stmts)?;
        if matches!(body.stmts.as_slice(), [s] if is_hole(s)) && !matches!(sig.ret, Ty::Unit) {
            l.check_posts_of_hole(f.name_span)?;
        }
        if !ends(&body.stmts) && matches!(sig.ret, Ty::Unit) {
            let at = body.stmts.last().map_or(f.name_span, |s| s.span);
            l.ret(Atom::Lit(Literal::Unit, at), at)?;
        }
        Ok(())
    })?;
    for (sp, msg) in special_cases(f, &body.contracts, &body.stmts) {
        let item = l.item.clone();
        l.cx.warn("W806", sp, &item, msg.into());
    }
    let hole_body = matches!(body.stmts.as_slice(), [s] if is_hole(s));
    let unused: Vec<(String, Span)> = sig
        .params
        .iter()
        .filter(|_| !hole_body)
        .map(|p| (p.name.clone(), p.span))
        .chain(mem::take(&mut l.bound))
        .filter(|(n, _)| !n.starts_with('_') && n != "self" && !l.last_use.is_read(n))
        .collect();
    let item = l.item.clone();
    // A name the item could not resolve may be a misspelt use of an "unused" binding (R291).
    let unresolved = l.cx.diags[first_diag..].iter().any(|d| d.code == "E200");
    for b in bounds::check(&body.stmts, &l.sites, l.cx.release) {
        match b.code {
            "E501" => l.cx.err(b.code, b.span, &item, b.message),
            // Yan shows a kept check as an editor hint, not W820 (R372).
            "W820" if l.cx.yn => {}
            _ => l.cx.warn(b.code, b.span, &item, b.message),
        }
    }
    for (n, sp) in unused.into_iter().filter(|_| !unresolved) {
        let item = l.item.clone();
        // A misspelled use is E200 with `n` as its near name, not an unread `n` (R291).
        let misspelled = l.cx.diags.iter().any(|d| {
            d.item == item
                && d.code == "E200"
                && d.fixes.iter().any(|f| f["edits"][0]["text"] == n.as_str())
        });
        if misspelled {
            continue;
        }
        l.cx.warn(
            "W812",
            sp,
            &item,
            format!("`{n}` is never read; remove it or write `_` in its place"),
        );
        // Choice: a later assignment or a keyword argument may still use the name.
        l.cx.fix(Applicability::Choice, "name it `_`", sp, "_");
    }
    Ok(y::FnDef {
        is_pub,
        is_unsafe: false,
        path: vec![f.name.clone()],
        name_span: f.name_span,
        tparams: tparams(sig),
        params,
        ret: sig.ret.core(),
        from: sig.from.clone(),
        fail: sig.fail.as_ref().map(Ty::core),
        contracts,
        body: Some(block),
    })
}

/// A surface type name as HIR spells it: built-ins by their Yan Core names (16.3).
fn core_type_name(n: &str) -> String {
    [yan_names::Surface::Yag, yan_names::Surface::Yn]
        .into_iter()
        .find_map(|s| yan_names::type_name(n, s, yan_names::Surface::Yir))
        .unwrap_or(n)
        .to_owned()
}

fn call(path: &str, targs: Vec<y::TArg>, args: Vec<Arg>, span: Span) -> y::Call {
    y::Call {
        path: path.split('.').map(str::to_owned).collect(),
        span,
        targs,
        args,
    }
}

/// The std function and result type of `op` between time values (17.5).
/// The builtin functions `builtin` lowers, offered as near names (R651).
const BUILTINS: [&str; 28] = [
    "len",
    "sorted",
    "int",
    "float",
    "round",
    "abs",
    "str",
    "bigint",
    "list",
    "min",
    "max",
    "print",
    "evolve",
    "range",
    "deque",
    "heap",
    "sorted_dict",
    "sorted_set",
    "dict",
    "set",
    "enumerate",
    "zip",
    "sum",
    "any",
    "all",
    "reversed",
    "divmod",
    "pow",
];

fn time_op(c: &str, b: &Ty, op: &str) -> Option<(String, Ty)> {
    let Ty::Coll(b, _) = b else {
        return match (c, b, op) {
            ("Duration", Ty::Int, "*") => {
                Some(("std.time.duration.mul".into(), Ty::Coll("Duration", vec![])))
            }
            ("Duration", Ty::Int, "/") => {
                Some(("std.time.duration.div".into(), Ty::Coll("Duration", vec![])))
            }
            ("Duration", Ty::Int, "//") => Some((
                "std.time.duration.floordiv".into(),
                Ty::Coll("Duration", vec![]),
            )),
            ("Duration", Ty::Int, "%") => {
                Some(("std.time.duration.rem".into(), Ty::Coll("Duration", vec![])))
            }
            _ => None,
        };
    };
    // (left, right, operator, module, function, result)
    const OPS: &[(&str, &str, &str, &str, &str, &str)] = &[
        ("Duration", "Duration", "+", "duration", "add", "Duration"),
        ("Duration", "Duration", "-", "duration", "sub", "Duration"),
        ("Duration", "Duration", "/", "duration", "ratio", "float"),
        ("Duration", "Duration", "//", "duration", "quotient", "int"),
        ("Instant", "Duration", "+", "instant", "add", "Instant"),
        ("Instant", "Duration", "-", "instant", "sub", "Instant"),
        ("Instant", "Instant", "-", "instant", "since", "Duration"),
        ("MonoInstant", "Duration", "+", "mono", "add", "MonoInstant"),
        ("MonoInstant", "Duration", "-", "mono", "sub", "MonoInstant"),
        (
            "MonoInstant",
            "MonoInstant",
            "-",
            "mono",
            "since",
            "Duration",
        ),
        ("PlainDate", "Period", "+", "date", "add", "PlainDate"),
        (
            "PlainTime",
            "Duration",
            "+",
            "plain_time",
            "add",
            "PlainTime",
        ),
        (
            "PlainTime",
            "Duration",
            "-",
            "plain_time",
            "sub",
            "PlainTime",
        ),
        (
            "PlainDateTime",
            "Duration",
            "+",
            "plain_date_time",
            "add",
            "PlainDateTime",
        ),
        (
            "PlainDateTime",
            "Duration",
            "-",
            "plain_date_time",
            "sub",
            "PlainDateTime",
        ),
        (
            "PlainDateTime",
            "Period",
            "+",
            "plain_date_time",
            "add_period",
            "PlainDateTime",
        ),
    ];
    let &(_, _, _, module, f, rt) = OPS.iter().find(|r| (r.0, r.1, r.2) == (c, *b, op))?;
    let rt = match rt {
        "float" => Ty::Float,
        "int" => Ty::Int,
        t => Ty::Coll(crate::ty::collection(t).expect("a time type").0, vec![]),
    };
    Some((format!("std.time.{module}.{f}"), rt))
}

/// A value of `t` to declare a hoisted binding with; it is never read (R832).
fn zero(t: &Ty) -> Option<j::ExprKind> {
    use j::ExprKind as K;
    Some(match t {
        Ty::Int | Ty::Fixed(_) => K::Lit(j::Lit::Int("0".into())),
        Ty::Float => K::Lit(j::Lit::Float("0.0".into())),
        Ty::Bool => K::Lit(j::Lit::Bool(false)),
        Ty::Str => K::Lit(j::Lit::Str {
            prefix: String::new(),
            value: String::new(),
        }),
        Ty::Option(_) => K::Lit(j::Lit::None),
        Ty::List(_) => K::List(Vec::new()),
        Ty::Dict(..) => K::Dict(Vec::new()),
        _ => return None,
    })
}

/// Whether every path through `stmts` binds `n` or leaves the block.
fn binds_all(stmts: &[j::Stmt], n: &str) -> bool {
    use j::StmtKind as K;
    let named = |e: &j::Expr| match &e.kind {
        j::ExprKind::Name(m) => m == n,
        j::ExprKind::Tuple(ts) => ts
            .iter()
            .any(|t| matches!(&t.kind, j::ExprKind::Name(m) if m == n)),
        _ => false,
    };
    stmts.iter().any(|s| match &s.kind {
        K::Assign { target, .. } => named(target),
        K::Bind { target, .. } => target == n,
        K::Return(_) | K::Fail(_) | K::Become(_) | K::Unreachable | K::Break | K::Continue => true,
        K::If {
            arms,
            els: Some(els),
        } => arms.iter().all(|(_, b)| binds_all(&b.stmts, n)) && binds_all(&els.stmts, n),
        K::With { body, .. } => binds_all(&body.stmts, n),
        _ => false,
    })
}

fn is_empty_literal(e: &j::Expr) -> bool {
    match &e.kind {
        j::ExprKind::List(xs) => xs.is_empty(),
        j::ExprKind::Dict(kvs) => kvs.is_empty(),
        _ => false,
    }
}

fn let_arg(a: Atom) -> Arg {
    let s = a.span();
    Arg::Value(Conv::Let, a, s)
}

fn sink_arg(a: Atom) -> Arg {
    let s = a.span();
    Arg::Value(Conv::Sink, a, s)
}

fn int_op(op: &str) -> Option<(&'static str, Ty)> {
    Some(match op {
        "+" => ("add_checked_i64", Ty::Int),
        "-" => ("sub_checked_i64", Ty::Int),
        "*" => ("mul_checked_i64", Ty::Int),
        "//" => ("floordiv_i64", Ty::Int),
        "%" => ("mod_i64", Ty::Int),
        "/" => ("div_i64", Ty::Float),
        "&" => ("and_i64", Ty::Int),
        "|" => ("or_i64", Ty::Int),
        "^" => ("xor_i64", Ty::Int),
        _ => return None,
    })
}

/// `[[fill]align][width]` of a format spec as (fill, align, width); align is 0 left, 1 right,
/// 2 center, defaulting to right for numbers and left otherwise (SQ20).
fn pad_spec(spec: &str, t: &Ty) -> R<(char, u8, usize)> {
    let cs: Vec<char> = spec.chars().collect();
    let align_of = |c: char| match c {
        '<' => Some(0),
        '>' => Some(1),
        '^' => Some(2),
        _ => None,
    };
    let numeric = matches!(t, Ty::Int | Ty::Float | Ty::BigInt | Ty::Fixed(_));
    let (fill, align, rest) = match cs.as_slice() {
        [f, a, rest @ ..] if align_of(*a).is_some() => (*f, align_of(*a), rest),
        [a, rest @ ..] if align_of(*a).is_some() => (' ', align_of(*a), rest),
        rest => (' ', None, rest),
    };
    let digits: String = rest.iter().collect();
    let width = if digits.is_empty() {
        0
    } else {
        match digits.parse::<usize>() {
            Ok(w) if !digits.starts_with('0') => w,
            _ => return unsupported(format!("format specification `{spec}`")),
        }
    };
    Ok((fill, align.unwrap_or(u8::from(numeric)), width))
}

/// `p as name` at the top of a case: the pattern and the name bound to the whole scrutinee.
fn split_as(p: &j::Pat) -> (&j::Pat, Option<&str>) {
    match p {
        j::Pat::As(inner, n) => (inner, Some(n)),
        p => (p, None),
    }
}

/// An integer literal, possibly negated.
fn is_int_lit(e: &j::Expr) -> bool {
    match &e.kind {
        j::ExprKind::Lit(j::Lit::Int(_)) => true,
        j::ExprKind::Unary(op, x) => *op == "-" && is_int_lit(x),
        _ => false,
    }
}

fn is_integer(t: &Ty) -> bool {
    match t {
        Ty::Int | Ty::BigInt => true,
        Ty::Fixed(n) => !n.starts_with('f'),
        _ => false,
    }
}

fn bigint_op(op: &str) -> Option<&'static str> {
    Some(match op {
        "+" => "add_bigint",
        "-" => "sub_bigint",
        "*" => "mul_bigint",
        "//" => "floordiv_bigint",
        "%" => "mod_bigint",
        "&" => "and_bigint",
        "|" => "or_bigint",
        "^" => "xor_bigint",
        _ => return None,
    })
}

fn float_op(op: &str) -> Option<&'static str> {
    Some(match op {
        "+" => "add_f64",
        "-" => "sub_f64",
        "*" => "mul_f64",
        "/" => "div_f64",
        _ => return None,
    })
}

fn cmp_name(op: &str) -> Option<&'static str> {
    Some(match op {
        "==" => "eq",
        "!=" => "ne",
        "<" => "lt",
        "<=" => "le",
        ">" => "gt",
        ">=" => "ge",
        _ => return None,
    })
}

impl Lower<'_, '_, '_> {
    fn fresh(&mut self, p: &str) -> String {
        self.n += 1;
        format!("_{p}{}", self.n)
    }

    fn emit(&mut self, kind: StmtKind, span: Span) {
        self.out.push(y::Stmt { kind, span });
    }

    /// `t` as the module's surface spells it.
    fn ty_text(&self, t: &Ty) -> String {
        if self.cx.yn { t.yn() } else { t.to_string() }
    }

    fn err(&mut self, code: &str, span: Span, msg: String) {
        let item = self.item.clone();
        self.cx.err(code, span, &item, msg);
    }

    /// An E200 or E203 for the unknown `name` at `span`, with each near name as a `maybe`
    /// fix (R651).
    fn err_near<'n>(
        &mut self,
        code: &str,
        span: Span,
        msg: String,
        name: &str,
        candidates: impl IntoIterator<Item = &'n str>,
    ) {
        self.err(code, span, msg);
        for n in yan_diag::near(name, candidates) {
            self.cx
                .fix(Applicability::Choice, &format!("use `{n}`"), span, n);
        }
    }

    /// Value names visible here: locals, functions and constants.
    fn names_in_scope(&self) -> Vec<String> {
        self.locals
            .keys()
            .chain(self.g.fns.keys())
            .chain(self.g.consts.keys())
            .cloned()
            .collect()
    }

    /// A hole in statement position (21.2): H701 with the expected type, or E702 under
    /// release. It runs as `unreachable`, so a call to an unwritten body panics.
    fn hole(&mut self, at: Span) {
        let item = self.item.clone();
        let msg = format!("hole of type `{}`", self.sig.ret);
        if self.cx.release {
            self.cx
                .err("E702", at, &item, format!("{msg} in a release build"));
        } else {
            self.cx.note("H701", at, &item, msg);
        }
        self.emit(StmtKind::Unreachable, at);
    }

    fn expect(&mut self, got: &Ty, want: &Ty, at: Span) {
        if !got.fits(want) {
            self.err(
                "E201",
                at,
                format!(
                    "expected `{want}`, found `{got}`",
                    want = self.ty_text(want),
                    got = self.ty_text(got)
                ),
            );
        }
    }

    fn bind(&mut self, name: &str, mutable: bool, ty: &Ty, rhs: Rhs, span: Span, at: Span) {
        if let Rhs::Proj(p) = &rhs
            && !ty.is_copy()
            && p.projs.iter().any(|x| matches!(x, Proj::Index(_)))
        {
            self.index_views.insert(name.to_owned());
        }
        self.emit(
            StmtKind::Let {
                mutable,
                pat: y::Pat::Bind(name.to_owned()),
                pat_span: span,
                ty: ty.core(),
                rhs,
                rhs_span: at,
            },
            span,
        );
        self.declare(name, ty.clone());
    }

    fn declare(&mut self, name: &str, ty: Ty) {
        let prev = self.locals.insert(name.to_owned(), Local { ty });
        if let Some(s) = self.scopes.last_mut() {
            s.push((name.to_owned(), prev));
        }
    }

    fn tmp(&mut self, ty: &Ty, rhs: Rhs, at: Span) -> Atom {
        match rhs {
            Rhs::Atom(a) => return a,
            Rhs::Proj(p) if p.projs.is_empty() => return Atom::Name(p.root, p.root_span),
            _ => {}
        }
        let n = self.fresh("t");
        self.bind(&n, false, ty, rhs, at, at);
        Atom::Name(n, at)
    }

    /// Declare each name `s` binds on every path and a later statement reads, at its type's
    /// zero, so the branches assign it (R832).
    fn hoist(&mut self, s: &j::Stmt) -> R<()> {
        if !matches!(s.kind, j::StmtKind::If { .. } | j::StmtKind::With { .. }) {
            return Ok(());
        }
        let hs: Vec<(String, Ty)> = self
            .hoists
            .borrow()
            .iter()
            .filter(|(n, _)| !self.locals.contains_key(*n) && binds_all(std::slice::from_ref(s), n))
            .map(|(n, t)| (n.clone(), t.clone()))
            .collect();
        for (n, t) in hs {
            let e = zero(&t).expect("hoisted types have a zero");
            let e = j::Expr {
                kind: e,
                span: s.span,
                paren: false,
            };
            let (r, t) = self.rhs(&e, Some(&t))?;
            self.mutable.insert(n.clone());
            self.bind(&n, true, &t, r, s.span, s.span);
            self.cx.marks.push(crate::marks::Pending {
                span: s.span,
                kind: crate::marks::Kind::Stmt,
                call: None,
                mark: yan_surface::Mark::Hoisted,
            });
        }
        Ok(())
    }

    fn block_with(&mut self, f: impl FnOnce(&mut Self) -> R<()>) -> R<y::Block> {
        self.block_scoped(f, false)
    }

    /// `keep`: the block's own bindings stay visible after it, as a `par` branch's do (9.2).
    fn block_scoped(&mut self, f: impl FnOnce(&mut Self) -> R<()>, keep: bool) -> R<y::Block> {
        let saved = mem::take(&mut self.out);
        self.joins.push(Vec::new());
        self.scopes.push(Vec::new());
        let r = f(self);
        let mut body = mem::replace(&mut self.out, saved);
        for jn in self.joins.pop().expect("pushed").into_iter().rev() {
            let Join {
                mut before,
                mut jp,
                branch,
            } = jn;
            if let StmtKind::Jp { body: b, .. } = &mut jp.kind {
                b.stmts = body;
            }
            before.push(jp);
            before.push(branch);
            body = before;
        }
        let scope = self.scopes.pop().expect("pushed");
        let depth = self.scopes.len();
        self.narrow.retain(|n| n.2 <= depth);
        if keep {
            if let Some(s) = self.scopes.last_mut() {
                s.extend(scope);
            }
            r?;
            return Ok(y::Block { stmts: body });
        }
        for (name, prev) in scope.into_iter().rev() {
            match prev {
                Some(p) => {
                    self.locals.insert(name, p);
                }
                None => {
                    if let Some(l) = self.locals.remove(&name) {
                        self.gone.insert(name, l.ty);
                    }
                }
            }
        }
        r?;
        Ok(y::Block { stmts: body })
    }

    fn block(&mut self, stmts: &[j::Stmt]) -> R<y::Block> {
        self.block_with(|l| l.stmts(stmts))
    }

    /// Split the current block: `branch` jumps to a new join point with one parameter of the
    /// type it returns, and the rest of the block becomes the join point's body.
    fn join(
        &mut self,
        at: Span,
        branch: impl FnOnce(&mut Self, &str) -> R<(y::Stmt, Ty)>,
    ) -> R<(Atom, Ty)> {
        let k = self.fresh("k");
        let v = self.fresh("v");
        let (b, ty) = branch(self, &k)?;
        let before = mem::take(&mut self.out);
        let jp = y::Stmt {
            kind: StmtKind::Jp {
                name: k,
                params: vec![y::Param::Value {
                    comptime: false,
                    conv: Conv::Sink,
                    name: v.clone(),
                    ty: ty.core(),
                }],
                body: y::Block { stmts: Vec::new() },
            },
            span: at,
        };
        self.joins.last_mut().expect("inside a block").push(Join {
            before,
            jp,
            branch: b,
        });
        self.declare(&v, ty.clone());
        Ok((Atom::Name(v, at), ty))
    }

    /// A branch block that computes `e` and jumps to `k` with it.
    fn arm(&mut self, k: &str, e: &j::Expr, want: Option<&Ty>) -> R<(y::Block, Ty)> {
        let mut ty = Ty::Err;
        let b = self.block_with(|l| {
            let (a, t) = l.atom(e, want)?;
            if let Some(w) = want {
                l.expect(&t, w, e.span);
            }
            ty = t;
            l.jump(k, a, e.span);
            Ok(())
        })?;
        Ok((b, ty))
    }

    fn jump(&mut self, k: &str, a: Atom, at: Span) {
        self.emit(
            StmtKind::Jump {
                name: k.to_owned(),
                args: vec![a],
            },
            at,
        );
    }

    fn stmts(&mut self, stmts: &[j::Stmt]) -> R<()> {
        for (k, s) in stmts.iter().enumerate() {
            if k > 0
                && let Some(v) = guard_after_binding(&stmts[k - 1], s)
                && matches!(self.locals.get(v), Some(Local { ty: Ty::Option(_) }))
            {
                let item = self.item.clone();
                self.cx.warn(
                    "W824",
                    s.span,
                    &item,
                    format!("this guard on `{v}` is a fallback; write `{v} = ... else ...` (R429)"),
                );
            }
            self.hoist(s)?;
            let start = self.writeback.len();
            self.stmt(s)?;
            if self.writeback.len() > start {
                if ends(std::slice::from_ref(s)) {
                    return unsupported("`&` of a field or element in a statement that exits");
                }
                for (t, place, at) in self.writeback.split_off(start) {
                    self.emit(
                        StmtKind::Assign {
                            place,
                            rhs: Rhs::Atom(Atom::Name(t, at)),
                            rhs_span: at,
                        },
                        at,
                    );
                }
            }
        }
        Ok(())
    }

    fn label(&mut self) -> String {
        self.fresh("l")
    }

    fn stmt(&mut self, s: &j::Stmt) -> R<()> {
        self.stmt_inner(s).map_err(|u| u.at(s.span))
    }

    /// Types an open empty literal from the first statement that fixes its type.
    fn learn(&mut self, s: &j::Stmt) {
        use j::{ExprKind as E, StmtKind as K};
        let named = |e: &j::Expr| match &e.kind {
            E::Name(n) => Some(n.clone()),
            _ => None,
        };
        let (name, ty) = match &s.kind {
            K::Expr(j::Expr {
                kind: E::Call(f, args),
                ..
            }) => {
                let E::Attr(recv, m, _) = &f.kind else { return };
                let (Some(n), [a]) = (named(recv), args.as_slice()) else {
                    return;
                };
                let j::ArgKind::Expr(a) = &a.kind else { return };
                let Some(t) = self.probe(a) else { return };
                match m.as_str() {
                    "append" => (n, Ty::List(Box::new(t))),
                    "extend" if matches!(t, Ty::List(_)) => (n, t),
                    _ => return,
                }
            }
            K::Assign {
                target:
                    j::Expr {
                        kind: E::Index(d, k),
                        ..
                    },
                ty: None,
                value,
            } => {
                let Some(n) = named(d) else { return };
                let (Some(kt), Some(vt)) = (self.probe(k), self.probe(value)) else {
                    return;
                };
                (n, Ty::Dict(Box::new(kt), Box::new(vt)))
            }
            K::Return(Some(e)) => {
                let Some(n) = named(e) else { return };
                (n, self.sig.ret.clone())
            }
            _ => return,
        };
        let Some(&lit) = self.open.get(&name) else {
            return;
        };
        if matches!(ty, Ty::List(_) | Ty::Dict(..)) {
            self.open.remove(&name);
            self.hints.borrow_mut().insert(lit, ty);
        }
    }

    /// The type `e` lowers to; its code and any errors are discarded with the first pass.
    fn probe(&mut self, e: &j::Expr) -> Option<Ty> {
        let (start, diags) = (self.out.len(), self.cx.diags.len());
        let r = self.atom(e, None);
        self.out.truncate(start);
        self.cx.diags.truncate(diags);
        match r {
            Ok((_, Ty::Err)) | Err(_) => None,
            Ok((_, t)) => Some(t),
        }
    }
}

/// The range of the Yan integer type `n`.
fn int_range(n: &str) -> (i128, i128) {
    match n {
        "i8" => (i8::MIN.into(), i8::MAX.into()),
        "i16" => (i16::MIN.into(), i16::MAX.into()),
        "i32" => (i32::MIN.into(), i32::MAX.into()),
        "i128" => (i128::MIN, i128::MAX),
        "u8" => (0, u8::MAX.into()),
        "u16" => (0, u16::MAX.into()),
        "u32" => (0, u32::MAX.into()),
        "u64" | "Addr" => (0, u64::MAX.into()),
        _ => (i64::MIN.into(), i64::MAX.into()),
    }
}

/// The type an `in` left operand must have for container type `ct`.
fn in_elem(ct: &Ty) -> Option<Ty> {
    match ct {
        Ty::List(e) | Ty::Set(e) | Ty::Dict(e, _) => Some((**e).clone()),
        Ty::Coll(n, a) if *n != "heap" => a.first().cloned(),
        _ => None,
    }
}

fn lit_ty(l: &j::Lit) -> Ty {
    match l {
        j::Lit::Int(_) => Ty::Int,
        j::Lit::Float(_) => Ty::Float,
        j::Lit::Str { prefix, .. } if prefix == "c" => Ty::Foreign("CStr", Vec::new()),
        j::Lit::Str { .. } => Ty::Str,
        j::Lit::Char(_) => Ty::Char,
        j::Lit::Bool(_) => Ty::Bool,
        j::Lit::None => Ty::Unit,
    }
}

/// An argument after matching it to its parameter.
enum Given<'e> {
    /// The value, whether it is `&place`, its span, and whether it was passed by keyword.
    Expr(&'e j::Expr, bool, Span, bool),
    Recv(&'e j::Expr),
}

fn field_params(fs: &[crate::Field], at: Span) -> Vec<crate::P> {
    fs.iter()
        .map(|f| crate::P {
            name: f.name.clone(),
            conv: Conv::Sink,
            ty: f.ty.clone(),
            default: f.default.clone(),
            positional: false,
            span: at,
        })
        .collect()
}

/// The parameters that `old(p)` reads in `posts`.
fn olds(posts: &[j::Expr]) -> Vec<String> {
    let mut out = Vec::new();
    for e in posts {
        walk(e, &mut |e| {
            if let j::ExprKind::Call(f, args) = &e.kind
                && matches!(&f.kind, j::ExprKind::Name(o) if o == "old")
                && let [
                    j::Arg {
                        kind: j::ArgKind::Expr(x),
                        ..
                    },
                ] = args.as_slice()
                && let j::ExprKind::Name(p) = &x.kind
                && !out.contains(p)
            {
                out.push(p.clone());
            }
        });
    }
    out
}

/// `v` when `prev` binds it and `s` is `if v is None:` around one exit, with no other branch.
fn guard_after_binding<'s>(prev: &'s j::Stmt, s: &j::Stmt) -> Option<&'s str> {
    let j::StmtKind::Assign {
        target: j::Expr {
            kind: j::ExprKind::Name(v),
            ..
        },
        ..
    } = &prev.kind
    else {
        return None;
    };
    let j::StmtKind::If { arms, els: None } = &s.kind else {
        return None;
    };
    let [(cond, body)] = arms.as_slice() else {
        return None;
    };
    let j::ExprKind::Compare("is", x, none) = &cond.kind else {
        return None;
    };
    let guarded = matches!(&x.kind, j::ExprKind::Name(n) if n == v)
        && matches!(none.kind, j::ExprKind::Lit(j::Lit::None))
        && body.stmts.len() == 1
        && ends(&body.stmts);
    guarded.then_some(v.as_str())
}

fn calls_self(stmts: &[j::Stmt], name: &str) -> bool {
    let mut found = false;
    for s in stmts {
        for e in stmt_exprs(s) {
            walk(e, &mut |e| {
                if let j::ExprKind::Call(f, _) = &e.kind
                    && matches!(&f.kind, j::ExprKind::Name(n) if n == name)
                {
                    found = true;
                }
            });
        }
        found |= sub_blocks(s).into_iter().any(|b| calls_self(b, name));
    }
    found
}

impl Lower<'_, '_, '_> {
    /// The first read of `n` reachable from the read at `at` before `n` is reassigned.
    fn next_use(&self, n: &str, at: Span) -> Option<Span> {
        self.last_use.next_use(n, at)
    }

    /// N05: a recursive function without `decreases` whose body matches a sum-typed parameter
    /// recurses structurally; the lenient reader takes that parameter as the measure.
    fn infer_decreases(&mut self, f: &j::FuncDef, stmts: &[j::Stmt]) {
        let has = f.body.as_ref().is_some_and(|b| {
            b.contracts
                .iter()
                .any(|c| matches!(c, j::Contract::Decreases(_)))
        });
        if has || self.cx.strict || !calls_self(stmts, &f.name) {
            return;
        }
        for s in stmts {
            if let j::StmtKind::Match { scrutinee, .. } = &s.kind
                && let j::ExprKind::Name(p) = &scrutinee.kind
                && self.sig.params.iter().any(|q| {
                    q.name == *p && matches!(&q.ty, Ty::Data(d) if self.g.sums.contains_key(d))
                })
            {
                let item = self.item.clone();
                self.cx.note(
                    "N05",
                    f.name_span,
                    &item,
                    format!("`decreases {p}` inferred: structural recursion on `{p}`"),
                );
                self.inferred = Some(p.clone());
                return;
            }
        }
    }
}

/// Names of functions called anywhere in `stmts`.
/// The first `fail` or `?` in `stmts`, outside lambdas.
fn first_failure(stmts: &[j::Stmt]) -> Option<Span> {
    for s in stmts {
        if matches!(s.kind, j::StmtKind::Fail(_)) {
            return Some(s.span);
        }
        let mut found = None;
        for e in stmt_exprs(s) {
            walk(e, &mut |e| {
                if found.is_none() && matches!(e.kind, j::ExprKind::Try(_)) {
                    found = Some(e.span);
                }
            });
        }
        if found.is_some() {
            return found;
        }
        if let Some(sp) = sub_blocks(s).into_iter().find_map(first_failure) {
            return Some(sp);
        }
    }
    None
}

/// Receivers of calls such as `io.print(...)` that name a std capability, with their spans.
pub(crate) fn cap_uses(stmts: &[j::Stmt], out: &mut Vec<(String, Span)>) {
    for s in stmts {
        for e in stmt_exprs(s) {
            walk(e, &mut |e| {
                if let j::ExprKind::Call(f, _) = &e.kind
                    && let j::ExprKind::Attr(b, _, _) = &f.kind
                    && let j::ExprKind::Name(c) = &b.kind
                    && crate::stdlib::STD.caps.contains(&crate::cap_type(c))
                {
                    out.push((c.clone(), b.span));
                }
            });
        }
        for b in sub_blocks(s) {
            cap_uses(b, out);
        }
    }
}

pub(crate) fn callees(stmts: &[j::Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        for e in stmt_exprs(s) {
            expr_callees(e, out);
        }
        for b in sub_blocks(s) {
            callees(b, out);
        }
    }
}

/// N39: a Python `math` name that Yan spells another way, as the expression it stands for.
fn math_alias(
    m: &str,
    module: &j::Expr,
    name_at: Span,
    args: &[j::Arg],
    at: Span,
) -> Option<(j::Expr, String)> {
    let ex = |kind| j::Expr {
        kind,
        span: at,
        paren: false,
    };
    let math = |f: &str, args: Vec<j::Arg>| {
        ex(j::ExprKind::Call(
            Box::new(ex(j::ExprKind::Attr(
                Box::new(module.clone()),
                f.to_owned(),
                name_at,
            ))),
            args,
        ))
    };
    // The operands get the name's span, so a mark joins the division, which has `at`.
    let math_at = |f: &str, args: Vec<j::Arg>| j::Expr {
        span: name_at,
        ..math(f, args)
    };
    let free = |f: &str, args: Vec<j::Arg>| {
        ex(j::ExprKind::Call(
            Box::new(ex(j::ExprKind::Name(f.to_owned()))),
            args,
        ))
    };
    Some(match m {
        "abs" | "fabs" => (free("abs", args.to_vec()), "abs(...)".into()),
        "isnan" | "isinf" | "isfinite" => {
            let f = format!("is_{}", &m[2..]);
            (math(&f, args.to_vec()), format!("math.{f}(...)"))
        }
        "log" if args.len() == 2 && args.iter().all(|a| a.name.is_none()) => (
            ex(j::ExprKind::Binary(
                "/",
                Box::new(math_at("log", vec![args[0].clone()])),
                Box::new(math_at("log", vec![args[1].clone()])),
            )),
            "math.log(x) / math.log(base)".into(),
        ),
        _ => return None,
    })
}

/// The callees of a body, contracts included: a `post all(...)` calls `all` too.
pub(crate) fn body_callees(b: &j::Body, out: &mut HashSet<String>) {
    callees(&b.stmts, out);
    for c in &b.contracts {
        match c {
            j::Contract::Pre(e) | j::Contract::Post(e) | j::Contract::Decreases(e) => {
                expr_callees(e, out);
            }
            j::Contract::Cost { steps, alloc } => {
                for e in steps.iter().chain(alloc) {
                    expr_callees(e, out);
                }
            }
            j::Contract::Ex { expr, with } => {
                expr_callees(expr, out);
                for (_, e) in with {
                    expr_callees(e, out);
                }
            }
        }
    }
}

fn expr_callees(e: &j::Expr, out: &mut HashSet<String>) {
    walk(e, &mut |e| {
        if matches!(e.kind, j::ExprKind::Binary("**", ..))
            || matches!(&e.kind, j::ExprKind::Call(f, _) if matches!(&f.kind, j::ExprKind::Name(n) if n == "pow"))
        {
            out.insert("int_pow".into());
            out.insert("bigint_pow".into());
        }
        if let j::ExprKind::FStr { pieces, .. } = &e.kind
            && pieces
                .iter()
                .any(|p| matches!(p, j::FPiece::Expr(_, Some(_))))
        {
            out.insert("fmt_pad".into());
        }
        if let j::ExprKind::Call(f, _) = &e.kind {
            match &f.kind {
                j::ExprKind::Name(n) | j::ExprKind::Attr(_, n, _) => {
                    // `max(xs)` lowers to the iterator's `max_nonempty` (R658).
                    if n == "min" || n == "max" {
                        out.insert(format!("{n}_nonempty"));
                    }
                    // N37 may collect an `iter` from a source.
                    if matches!(n.as_str(), "chars" | "iter" | "graphemes" | "list") {
                        out.insert("collect".into());
                    }
                    out.insert(n.clone());
                }
                _ => {}
            }
        }
    });
}

/// The type a literal is checked against: `U` where `U | None` is expected (R794).
fn unoption(want: Option<&Ty>) -> Option<&Ty> {
    let mut w = want;
    while let Some(Ty::Option(t)) = w {
        w = Some(t);
    }
    w
}

/// A collection's type arguments, which its std methods take in the same order.
fn has_param(t: &Ty) -> bool {
    match t {
        Ty::Param(_) => true,
        Ty::List(t) | Ty::Set(t) | Ty::Option(t) => has_param(t),
        Ty::Dict(a, b) | Ty::Result(a, b) => has_param(a) || has_param(b),
        Ty::Tuple(ts) | Ty::Coll(_, ts) => ts.iter().any(has_param),
        Ty::Fn(_, ps, r) => has_param(r) || ps.iter().any(has_param),
        _ => false,
    }
}

fn coll_targs(t: &Ty) -> Vec<y::TArg> {
    match t {
        Ty::Coll(_, ts) => ts.iter().map(Ty::targ).collect(),
        _ => Vec::new(),
    }
}

/// The key and value types of a `dict` or `sorted_dict`.
struct DictPlace {
    map: Ty,
    root: String,
    root_span: Span,
    key: Atom,
    value: String,
}

fn map_kv(t: &Ty) -> Option<(Ty, Ty)> {
    match t {
        Ty::Dict(k, v) => Some(((**k).clone(), (**v).clone())),
        Ty::Coll("sorted_dict", ts) => Some((ts[0].clone(), ts[1].clone())),
        _ => None,
    }
}

const DATE: u8 = 1;
const TIME: u8 = 2;
const ZONE: u8 = 4;

/// Each Jiff `strtime` directive in `fmt` with the fields it needs, or the first unknown one.
fn directives(fmt: &str) -> Result<Vec<(String, u8)>, String> {
    let cs: Vec<char> = fmt.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < cs.len()
            && (matches!(cs[i], '_' | '-' | '0' | '^' | '#' | '.' | ':') || cs[i].is_ascii_digit())
        {
            i += 1;
        }
        let text: String = cs[start..(i + 1).min(cs.len())].iter().collect();
        let need = match cs.get(i) {
            Some('%' | 'n' | 't') => 0,
            Some(
                'A' | 'a' | 'B' | 'b' | 'h' | 'C' | 'D' | 'd' | 'e' | 'F' | 'G' | 'g' | 'j' | 'm'
                | 'q' | 'U' | 'u' | 'V' | 'W' | 'w' | 'x' | 'Y' | 'y',
            ) => DATE,
            Some(
                'f' | 'H' | 'I' | 'k' | 'l' | 'M' | 'N' | 'P' | 'p' | 'R' | 'r' | 'S' | 'T' | 'X',
            ) => TIME,
            Some('c') => DATE | TIME,
            Some('Q' | 's' | 'Z' | 'z') => ZONE,
            _ => return Err(text),
        };
        out.push((text, need));
        i += 1;
    }
    Ok(out)
}

/// Std units (11.4): name, dimension, scale to the SI base unit, and for a duration the
/// `Duration` constructor and multiplier an `int` count goes through.
/// A `Duration` constructor and the multiplier its `int` count takes.
type DurationCtor = Option<(&'static str, i64)>;

const UNITS: &[(&str, &str, f64, DurationCtor)] = &[
    ("ns", "time", 1e-9, Some(("nanos", 1))),
    ("us", "time", 1e-6, Some(("micros", 1))),
    ("ms", "time", 1e-3, Some(("millis", 1))),
    ("s", "time", 1.0, Some(("seconds", 1))),
    ("min", "time", 60.0, Some(("minutes", 1))),
    ("h", "time", 3600.0, Some(("hours", 1))),
    ("day", "time", 86400.0, Some(("hours", 24))),
    ("week", "time", 604800.0, Some(("hours", 168))),
    ("m", "length", 1.0, None),
    ("km", "length", 1e3, None),
    ("cm", "length", 1e-2, None),
    ("mm", "length", 1e-3, None),
    ("kg", "mass", 1.0, None),
    ("g", "mass", 1e-3, None),
    ("A", "current", 1.0, None),
    ("K", "temperature", 1.0, None),
    ("mol", "amount", 1.0, None),
    ("cd", "luminosity", 1.0, None),
];

/// Operations whose result depends on other tasks, so they need `uses nondet` (6.5, E709).
const NONDET_OPS: &[&str] = &[
    "std.sync.arc.strong_count",
    "std.chan.sender.share",
    "std.chan.receiver.share",
    "std.actor.share",
];

/// A type holding an `Rc` or `Weak`, whose counts are not atomic (6.4).
fn not_send(t: &y::Type) -> bool {
    match t {
        y::Type::Named(n) => {
            matches!(n.path.last().map(String::as_str), Some("Rc" | "Weak"))
                || n.args
                    .iter()
                    .any(|a| matches!(a, y::TArg::Type(t) if not_send(t)))
        }
        y::Type::Tuple(ts) | y::Type::Union(ts) => ts.iter().any(not_send),
        _ => false,
    }
}

/// The size in bytes of a plain value of `t`, when it has a fixed one (5.1).
fn byte_size(g: &Globals, t: &Ty) -> Option<usize> {
    Some(match t {
        Ty::Int | Ty::Float => 8,
        Ty::Bool => 1,
        Ty::Char => 4,
        Ty::Unit => 0,
        Ty::Fixed(n) => {
            n.trim_start_matches(|c: char| c.is_alphabetic())
                .parse::<usize>()
                .ok()?
                / 8
        }
        Ty::Tuple(ts) => ts.iter().map(|t| byte_size(g, t)).sum::<Option<usize>>()?,
        Ty::Data(n) => g
            .records
            .get(n)?
            .iter()
            .map(|f| byte_size(g, &f.ty))
            .sum::<Option<usize>>()?,
        _ => return None,
    })
}

/// Whether values of `t` can key a hashed map or an entity (`Hash`; floats have none).
fn hashable(g: &Globals, t: &Ty) -> bool {
    match t {
        Ty::Int | Ty::BigInt | Ty::Bool | Ty::Str | Ty::Char | Ty::Unit | Ty::Err => true,
        Ty::Fixed(n) => !n.starts_with('f') && !n.starts_with("bf"),
        Ty::Tuple(ts) => ts.iter().all(|t| hashable(g, t)),
        Ty::Option(t) | Ty::List(t) => hashable(g, t),
        Ty::Data(n) => g
            .records
            .get(n)
            .is_none_or(|fs| fs.iter().all(|f| hashable(g, &f.ty))),
        _ => false,
    }
}

/// Whether `t` can cross a node boundary (`Wire`, 9.8): no callables, handles or capabilities.
fn wire(t: &Ty) -> bool {
    match t {
        Ty::Fn(..) | Ty::Foreign(..) => false,
        Ty::Coll(n, ts) => {
            !matches!(
                *n,
                "Rc" | "Arc"
                    | "Cell"
                    | "Atomic"
                    | "Sender"
                    | "Receiver"
                    | "Group"
                    | "iter"
                    | "Secret"
                    | "TcpListener"
                    | "TcpStream"
            ) && ts.iter().all(wire)
        }
        Ty::List(t) | Ty::Set(t) | Ty::Option(t) => wire(t),
        Ty::Dict(k, v) | Ty::Result(k, v) => wire(k) && wire(v),
        Ty::Tuple(ts) | Ty::Union(ts) => ts.iter().all(wire),
        _ => true,
    }
}

pub(crate) fn is_view(t: &j::Type) -> bool {
    matches!(t, j::Type::Named { path, .. } if path.len() == 1 && path[0] == "View")
}

/// The source text a one-line span covers.
fn span_text(src: &str, at: Span) -> Option<&str> {
    if at.line != at.end_line {
        return None;
    }
    let line = src.lines().nth(at.line as usize - 1)?;
    line.get(at.col as usize - 1..at.end_col as usize - 1)
}

/// std.session (9.5, R634): each operation calls the function the protocol generated for the
/// endpoint's current state, so its type is the state's.
impl Lower<'_, '_, '_> {
    fn session_connect(
        &mut self,
        k: &j::Expr,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let j::ExprKind::Name(n) = &k.kind else {
            return unsupported("`session.connect` of a protocol that is not a name");
        };
        let t = self.g.ty(
            &j::Type::Named {
                path: vec![n.clone()],
                args: Vec::new(),
            },
            self.cx,
            k.span,
        )?;
        let Ty::Data(p) = &t else {
            if t != Ty::Err {
                self.err("E201", k.span, format!("`{n}` is not a session protocol"));
            }
            return Ok(err);
        };
        let Some(p) = self.g.protocols.get(p) else {
            self.err("E201", k.span, format!("`{n}` is not a session protocol"));
            return Ok(err);
        };
        let f = p.connect_fn();
        self.user_call(&f, None, args, at)
    }

    fn session_op(&mut self, op: &str, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        // Failing like the operation would keeps a `?` after it quiet.
        let err = (
            Rhs::Atom(Atom::Lit(Literal::Unit, at)),
            Ty::Err,
            Some(Ty::Err),
        );
        let want = if op == "send" { 2 } else { 1 };
        let Some(xs) = self.positional(args, want, &format!("session.{op}"), at)? else {
            return Ok(err);
        };
        let ep = xs[0];
        let t = match &ep.kind {
            j::ExprKind::Name(n) if self.locals.contains_key(n) => self.locals[n].ty.clone(),
            // An unknown name: lowering reports it.
            j::ExprKind::Name(_) => {
                self.atom(ep, None)?;
                return Ok(err);
            }
            _ => return unsupported(format!("`session.{op}` of an endpoint that is not a local")),
        };
        let found = match &t {
            Ty::Data(d) => self.g.endpoints.get(d).cloned(),
            _ => None,
        };
        let Some(e) = found else {
            if t != Ty::Err {
                let shown = self.ty_text(&t);
                self.err(
                    "E201",
                    ep.span,
                    format!("`session.{op}` takes a session endpoint, not a `{shown}`"),
                );
            }
            return Ok(err);
        };
        let p = &self.g.protocols[&e.protocol];
        let state = &p.states[e.role][e.state];
        let me = p.state_type(e.role, e.state);
        let allowed = match state {
            crate::session::State::Send(bs) => {
                let ls: Vec<String> = bs.iter().map(|b| format!("`{}`", b.1)).collect();
                format!("sending {}", ls.join(" or "))
            }
            crate::session::State::Recv(from, _) => format!("`recv` from `{}`", p.roles[*from]),
            crate::session::State::End => "only `close`".to_owned(),
        };
        let refuse = |s: &mut Self, what: String| {
            s.err(
                "E727",
                at,
                format!("{what}, but endpoint state `{me}` allows {allowed}"),
            );
            Ok(err.clone())
        };
        match (op, state) {
            ("send", crate::session::State::Send(bs)) => {
                let (label, payload) = match &xs[1].kind {
                    j::ExprKind::Call(l, a) if let j::ExprKind::Name(l) = &l.kind => {
                        (l.clone(), a.clone())
                    }
                    j::ExprKind::Name(l) => (l.clone(), Vec::new()),
                    _ => {
                        self.err(
                            "E201",
                            xs[1].span,
                            "`session.send` sends a message written `Label(value)`".into(),
                        );
                        return Ok(err);
                    }
                };
                let Some(b) = bs.iter().find(|b| b.1 == label) else {
                    return refuse(self, format!("`session.send` of `{label}`"));
                };
                if payload.len() != usize::from(b.2.is_some()) {
                    let n = usize::from(b.2.is_some());
                    self.err(
                        "E201",
                        xs[1].span,
                        format!("`{label}` carries {n} value(s), not {}", payload.len()),
                    );
                    return Ok(err);
                }
                let f = p.op_fn(e.role, e.state, &format!("send_{label}"));
                let mut call_args = vec![args[0].clone()];
                call_args.extend(payload);
                self.user_call(&f, None, &call_args, at)
            }
            ("recv", crate::session::State::Recv(..)) | ("close", crate::session::State::End) => {
                let f = p.op_fn(e.role, e.state, op);
                self.user_call(&f, None, args, at)
            }
            _ => refuse(self, format!("`session.{op}`")),
        }
    }
}
