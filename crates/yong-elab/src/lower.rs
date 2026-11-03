//! Function bodies: Xin statements and expressions to Yong statements in A-normal form.
//!
//! Every compound expression becomes a typed `let` of a fresh `_tN`. A value that joins
//! control flow (`a if c else b`, `and`, `or`, `x else y`) becomes a join point whose body is
//! the rest of the enclosing block: the block is split where the join is made and closed
//! when the block ends.

use std::collections::{HashMap, HashSet};
use std::mem;

use yong_core::ast::{self as y, Arg, Atom, Conv, Literal, Place, Proj, Rhs, StmtKind};
use yong_diag::{Applicability, Span};
use yong_hir as j;

use crate::bounds::{self, Site};
use crate::{Cx, Globals, R, Sig, Ty, cap_type, ffi, unsupported};

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
    /// Names whose block has ended; a later use needs function-level scope.
    gone: HashSet<String>,
    mutable: HashSet<String>,
    /// Each enclosing loop's label, with the name the source gave it.
    loops: Vec<(String, Option<String>)>,
    n: usize,
    /// Lowering a `become` call, whose arguments are exempt from E210 (SPEC_QUESTIONS).
    in_become: bool,
    /// Dict elements that `place` copied out; a mutating caller stores them back.
    dict_places: Vec<DictPlace>,
    /// Every read of each name, for last-use moves (E413).
    uses: HashMap<String, Vec<Span>>,
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
    /// The locals bound outside each enclosing loop, for W822.
    loop_outer: Vec<HashSet<String>>,
    /// The name the current statement assigns, for W822.
    target: Option<String>,
    /// Inside an `unsafe` block (12.1).
    in_unsafe: bool,
    /// Stage-0 names: an instance's `Nat` parameters and `#for` targets (10.4).
    stage: Vec<(String, Sv)>,
    /// Optional locals narrowed by an `is None` test (R285): the source name, the local
    /// holding the unwrapped value, and the scope depth the narrowing ends with.
    narrow: Vec<(String, String, usize)>,
    /// The `post` clauses, checked at each return (8.4).
    posts: Vec<j::Expr>,
    /// Each `var` parameter `old(p)` reads, with the local holding its entry value.
    olds: Vec<(String, String)>,
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
        // Yong has no C string literal yet; the value is the text, typed `CStr` (SPEC_QUESTIONS).
        j::Lit::Str { prefix, value } if prefix == "c" => Literal::Str(value.clone()),
        j::Lit::Str { .. } => return unsupported("byte strings"),
        j::Lit::Char(c) => Literal::Char(*c),
        j::Lit::Bool(b) => Literal::Bool(*b),
        j::Lit::None => Literal::Unit,
    })
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

pub(crate) fn walk(e: &j::Expr, f: &mut impl FnMut(&j::Expr)) {
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
        | K::Is { value: b, .. }
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
        K::Ghost { value, .. } | K::Bind { value, .. } | K::LetElse { value, .. } => vec![value],
        K::Return(e) => e.iter().collect(),
        K::If { arms, .. } => arms.iter().map(|(c, _)| c).collect(),
        K::While { cond, .. } | K::StageIf { cond, .. } => vec![cond],
        K::StageFor { iter, .. } => vec![iter],
        K::For { iter, .. } => vec![iter],
        K::Match { scrutinee, .. } => vec![scrutinee],
        K::Par {
            body: j::ParBody::For { iter, .. },
            ..
        } => vec![iter],
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
        K::With { body, .. } | K::LetElse { els: body, .. } => vec![&body.stmts],
        K::Par { body, .. } => match body {
            j::ParBody::Block(b) => vec![b],
            j::ParBody::For { body, .. } => vec![body],
        },
        _ => Vec::new(),
    }
}

/// W806: `if p == lit` conditions where `lit` is the literal an `ex` line passes as `p`.
fn special_cases(f: &j::FuncDef, contracts: &[j::Contract], stmts: &[j::Stmt]) -> Vec<Span> {
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
    let mut out = Vec::new();
    if inputs.is_empty() {
        return out;
    }
    fn conds<'e>(stmts: &'e [j::Stmt], out: &mut Vec<&'e j::Expr>) {
        for s in stmts {
            if let j::StmtKind::If { arms, .. } = &s.kind {
                out.extend(arms.iter().map(|(c, _)| c));
            }
            for b in sub_blocks(s) {
                conds(b, out);
            }
        }
    }
    let mut cs = Vec::new();
    conds(stmts, &mut cs);
    for c in cs {
        let j::ExprKind::Compare("==", a, b) = &c.kind else {
            continue;
        };
        let hit = |x: &j::Expr, y: &j::Expr| {
            matches!((&x.kind, &y.kind), (j::ExprKind::Name(n), j::ExprKind::Lit(l))
                if inputs.iter().any(|(p, v)| p == n && *v == l))
        };
        if hit(a, b) || hit(b, a) {
            out.push(c.span);
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

/// A bodiless Yong signature, as a trait declares it.
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

fn function_body(
    g: &Globals,
    f: &j::FuncDef,
    sig: &Sig,
    is_pub: bool,
    stage: Vec<(String, Sv)>,
    cx: &mut Cx,
) -> R<y::FnDef> {
    let Some(body) = &f.body else {
        return unsupported("functions without bodies");
    };
    let mut assigned = HashMap::new();
    let mut mutable = HashSet::new();
    let first_diag = cx.diags.len();
    mutables(g, &body.stmts, &mut assigned, &mut mutable);
    mutable.extend(assigned.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k));
    // Wen declares mutability (15.3): a `let` stays immutable, and assigning it is E403.
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
        gone: HashSet::new(),
        mutable,
        loops: Vec::new(),
        n: 0,
        in_become: false,
        dict_places: Vec::new(),
        uses: HashMap::new(),
        ghosts: HashSet::new(),
        inferred: None,
        bound: Vec::new(),
        sites: HashMap::new(),
        writeback: Vec::new(),
        arg_tys: Vec::new(),
        want_ret: None,
        handlers: Vec::new(),
        fn_calls: Vec::new(),
        pending: HashMap::new(),
        loop_outer: Vec::new(),
        target: None,
        in_unsafe: false,
        stage,
        narrow: Vec::new(),
        posts: Vec::new(),
        olds: Vec::new(),
    };
    names_used(&body.stmts, &mut l.uses);
    l.infer_decreases(f, &body.stmts);
    let mut params: Vec<y::Param> = sig
        .caps
        .iter()
        .map(|c| y::Param::Cap {
            name: c.clone(),
            ty: y::Named {
                path: vec![cap_type(c)],
                args: Vec::new(),
            },
        })
        .collect();
    for p in &sig.params {
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
    for c in &body.contracts {
        match c {
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
        }
    }
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
        if !l.cx.wen && !matches!(sig.ret, Ty::Unit | Ty::Err) && falls_through(&body.stmts) {
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
    for sp in special_cases(f, &body.contracts, &body.stmts) {
        let item = l.item.clone();
        l.cx.warn(
            "W806",
            sp,
            &item,
            "this branch special-cases an `ex` input".into(),
        );
    }
    let hole_body = matches!(body.stmts.as_slice(), [s] if is_hole(s));
    let unused: Vec<(String, Span)> = sig
        .params
        .iter()
        .filter(|_| !hole_body)
        .map(|p| (p.name.clone(), p.span))
        .chain(mem::take(&mut l.bound))
        .filter(|(n, _)| {
            !n.starts_with('_') && n != "self" && l.uses.get(n).is_none_or(Vec::is_empty)
        })
        .collect();
    let item = l.item.clone();
    // A name the item could not resolve may be a misspelt use of an "unused" binding (R291).
    let unresolved = l.cx.diags[first_diag..].iter().any(|d| d.code == "E200");
    for b in bounds::check(&body.stmts, &l.sites, l.cx.release) {
        match b.code {
            "E501" => l.cx.err(b.code, b.span, &item, b.message),
            // Wen shows a kept check as an editor hint, not W820 (R372).
            "W820" if l.cx.wen => {}
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
        from: Vec::new(),
        fail: sig.fail.as_ref().map(Ty::core),
        contracts,
        body: Some(block),
    })
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
const BUILTINS: [&str; 26] = [
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
        if self.cx.wen { t.wen() } else { t.to_string() }
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
        for n in yong_diag::near(name, candidates) {
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
        if let Rhs::Atom(a) = rhs {
            return a;
        }
        let n = self.fresh("t");
        self.bind(&n, false, ty, rhs, at, at);
        Atom::Name(n, at)
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
                    self.locals.remove(&name);
                    self.gone.insert(name);
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
                    conv: Conv::Let,
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

    fn stmt_inner(&mut self, s: &j::Stmt) -> R<()> {
        use j::StmtKind as K;
        let at = s.span;
        match &s.kind {
            K::Pass | K::Reveal(_) => {}
            K::Expr(e) | K::Return(Some(e)) if matches!(e.kind, j::ExprKind::Hole) => {
                self.hole(e.span);
            }
            K::Ghost { name, .. } => {
                self.ghosts.insert(name.clone());
            }
            K::Expr(j::Expr {
                kind: j::ExprKind::Stage(x),
                ..
            }) if matches!(&x.kind, j::ExprKind::Call(f, _) if matches!(&f.kind, j::ExprKind::Name(n) if n == "compile_error" || n == "compile_warn")) =>
            {
                let j::ExprKind::Call(f, args) = &x.kind else {
                    unreachable!("matched")
                };
                let msg = match args.as_slice() {
                    [
                        j::Arg {
                            kind:
                                j::ArgKind::Expr(j::Expr {
                                    kind: j::ExprKind::Lit(j::Lit::Str { value, .. }),
                                    ..
                                }),
                            ..
                        },
                    ] => value.clone(),
                    _ => return unsupported("`compile_error` with other than one string literal"),
                };
                let item = self.item.clone();
                if matches!(&f.kind, j::ExprKind::Name(n) if n == "compile_error") {
                    self.cx.err("E270", at, &item, msg);
                } else {
                    self.cx.warn("W816", at, &item, msg);
                }
            }
            K::Expr(e) => {
                let (rhs, t) = self.rhs(e, None)?;
                if !matches!(t, Ty::Unit | Ty::Never | Ty::Err) && matches!(rhs, Rhs::Call(_)) {
                    self.err(
                        "E312",
                        at,
                        format!("a result of type `{t}` is discarded", t = self.ty_text(&t)),
                    );
                }
                if !matches!(rhs, Rhs::Atom(_)) {
                    self.emit(StmtKind::Rhs(rhs), at);
                }
            }
            K::Assign { target, ty, value } => self.assign(target, ty.as_ref(), value, at)?,
            K::Bind {
                target,
                target_span,
                ty,
                value,
                ..
            } => {
                if self.locals.contains_key(target) {
                    self.err(
                        "E230",
                        *target_span,
                        format!("`{target}` is already bound; pick another name"),
                    );
                    return Ok(());
                }
                let t = j::Expr {
                    kind: j::ExprKind::Name(target.clone()),
                    span: *target_span,
                    paren: false,
                };
                self.assign(&t, ty.as_ref(), value, at)?;
            }
            K::LetElse { .. } => return unsupported("let-else"),
            K::AugAssign { target, op, value } => {
                let op: &'static str = op.trim_end_matches('=');
                if !["+", "-", "*", "/", "//", "%"].contains(&op) {
                    return unsupported(format!("`{op}=`"));
                }
                let bin = j::Expr {
                    kind: j::ExprKind::Binary(
                        op,
                        Box::new(target.clone()),
                        Box::new(value.clone()),
                    ),
                    span: value.span,
                    paren: false,
                };
                if let j::ExprKind::Index(d, k) = &target.kind
                    && let Some(Ty::Dict(kt, vt)) = self.ty_of_local(d)
                {
                    self.insert_default(d, k, &kt, &vt, at)?;
                }
                self.assign(target, None, &bin, at)?;
            }
            K::Return(e) => {
                let want = self.sig.ret.clone();
                let a = match e {
                    None => Atom::Lit(Literal::Unit, at),
                    Some(e) => {
                        let (a, t) = self.atom(e, Some(&want))?;
                        self.expect(&t, &want, e.span);
                        a
                    }
                };
                self.ret(a, at)?;
            }
            K::Fail(e) => {
                let Some(want) = self.sig.fail.clone() else {
                    return unsupported("`fail` in a function without a failure type");
                };
                let (a, t) = self.atom(e, Some(&want))?;
                self.expect(&t, &want, e.span);
                self.emit(StmtKind::Fail(a), at);
            }
            K::Become(e) => {
                self.in_become = true;
                let r = self.rhs(e, None);
                self.in_become = false;
                let (rhs, _) = r?;
                let Rhs::Call(c) = rhs else {
                    return unsupported("`become` of a constructor");
                };
                self.emit(StmtKind::Become(c), at);
            }
            K::Break | K::Continue | K::BreakTo(_) | K::ContinueTo(_) => {
                let label = match &s.kind {
                    K::BreakTo(n) | K::ContinueTo(n) => self
                        .loops
                        .iter()
                        .rev()
                        .find(|(_, name)| name.as_ref() == Some(n))
                        .map(|(l, _)| l.clone()),
                    _ => self.loops.last().map(|(l, _)| l.clone()),
                };
                let Some(label) = label else {
                    return unsupported("`break` outside its loop");
                };
                let kind = if matches!(s.kind, K::Break | K::BreakTo(_)) {
                    StmtKind::Break { label, value: None }
                } else {
                    StmtKind::Continue { label }
                };
                self.emit(kind, at);
            }
            K::Assert(e) => {
                let a = self.cond(e)?;
                self.emit(StmtKind::Assert(y::Expr::Atom(a)), at);
            }
            K::Unreachable => self.emit(StmtKind::Unreachable, at),
            K::If { arms, els } => self.if_(arms, els.as_ref(), at)?,
            K::While {
                label: name,
                cond,
                body,
                ..
            } => {
                let label = self.label();
                self.loops.push((label.clone(), name.clone()));
                self.loop_outer.push(self.locals.keys().cloned().collect());
                let b = self.block_with(|l| {
                    let c = l.cond(cond)?;
                    let stop = y::Block {
                        stmts: vec![y::Stmt {
                            kind: StmtKind::Break {
                                label: label.clone(),
                                value: None,
                            },
                            span: at,
                        }],
                    };
                    l.emit(
                        StmtKind::If {
                            cond: c,
                            then: y::Block { stmts: Vec::new() },
                            els: Some(stop),
                        },
                        at,
                    );
                    for (x, inner) in l.narrowings(cond, true) {
                        l.narrow_to(&x, inner, cond.span);
                    }
                    l.stmts(body)
                });
                self.loops.pop();
                self.loop_outer.pop();
                let body = b?;
                self.emit(
                    StmtKind::Loop {
                        label,
                        invariants: Vec::new(),
                        decreases: Vec::new(),
                        body,
                    },
                    at,
                );
            }
            K::For {
                label,
                target,
                iter,
                body,
                ..
            } => self.for_(label.as_deref(), target, iter, at, |l| l.stmts(body))?,
            K::Match { scrutinee, cases } => self.match_(scrutinee, cases, at)?,
            K::Par {
                concurrent,
                limit: None,
                body: j::ParBody::Block(b),
            } => {
                let mut blocks = Vec::new();
                let mut called: Vec<String> = Vec::new();
                for s in b {
                    let start = self.fn_calls.len();
                    blocks.push(self.block_scoped(|l| l.stmts(std::slice::from_ref(s)), true)?);
                    let mine: Vec<(String, Span)> = self.fn_calls[start..].to_vec();
                    for (n, sp) in &mine {
                        if called.contains(n) {
                            self.err(
                                "E716",
                                *sp,
                                format!("`{n}` is not `par`; it is called from two branches"),
                            );
                        }
                    }
                    called.extend(mine.into_iter().map(|(n, _)| n));
                }
                self.emit(
                    StmtKind::Par {
                        concurrent: *concurrent,
                        blocks,
                    },
                    at,
                );
            }
            K::Par { .. } => return unsupported("`par for` and `par(limit=...)`"),
            K::With {
                kind: j::WithKind::Bind(name, h),
                body,
            } => {
                let (a, _) = self.atom(h, None)?;
                self.handlers.push(name.clone());
                let b = self.block(&body.stmts);
                self.handlers.pop();
                self.emit(
                    StmtKind::With {
                        name: name.clone(),
                        dyn_: false,
                        handler: a,
                        body: b?,
                    },
                    at,
                );
            }
            K::With { .. } => return unsupported("`with deadline` and `with timeout`"),
            K::Unsafe(body) => {
                if !self.cx.ffi {
                    self.err(
                        "E901",
                        at,
                        "`unsafe` belongs in an ffi module, under the package's `ffi/` directory"
                            .into(),
                    );
                }
                let outer = mem::replace(&mut self.in_unsafe, true);
                let b = self.block(body);
                self.in_unsafe = outer;
                self.emit(StmtKind::Unsafe(b?), at);
            }
            K::Asm { template, args } => self.asm(template, args, at)?,
            K::StageIf { cond, then, els } => {
                let Some(Sv::Bool(c)) = fold(cond, &self.stage) else {
                    return unsupported("a `#if` condition that is not a stage-0 constant");
                };
                match (c, els) {
                    (true, _) => self.stmts(then)?,
                    (false, Some(b)) => self.stmts(b)?,
                    (false, None) => {}
                }
            }
            K::StageFor { target, iter, body } => {
                let j::ExprKind::List(xs) = &iter.kind else {
                    return unsupported("`#for` over a sequence other than a list literal");
                };
                for x in xs {
                    let Some(v) = fold(x, &self.stage) else {
                        return unsupported("a `#for` element that is not a stage-0 constant");
                    };
                    self.stage.push((target.clone(), v));
                    let r = self.stmts(body);
                    self.stage.pop();
                    r?;
                }
            }
        }
        Ok(())
    }

    fn if_(&mut self, arms: &[(j::Expr, j::Block)], els: Option<&j::Block>, at: Span) -> R<()> {
        let (cond, body) = &arms[0];
        let c = self.cond(cond)?;
        let yes = self.narrowings(cond, true);
        let no = self.narrowings(cond, false);
        let then = self.block_with(|l| {
            for (x, inner) in yes.clone() {
                l.narrow_to(&x, inner, cond.span);
            }
            l.stmts(&body.stmts)
        })?;
        let narrow_else = |l: &mut Self| {
            for (x, inner) in no.clone() {
                l.narrow_to(&x, inner, cond.span);
            }
        };
        let (els, els_exits) = if arms.len() > 1 {
            let b = self.block_with(|l| {
                narrow_else(l);
                l.if_(&arms[1..], els, at)
            })?;
            (Some(b), false)
        } else {
            match els {
                Some(e) => {
                    let b = self.block_with(|l| {
                        narrow_else(l);
                        l.stmts(&e.stmts)
                    })?;
                    (Some(b), ends(&e.stmts))
                }
                None => (None, false),
            }
        };
        self.emit(StmtKind::If { cond: c, then, els }, at);
        // A branch that exits narrows the rest of the block by the other branch's test.
        if ends(&body.stmts) && arms.len() == 1 {
            narrow_else(self);
        }
        if els_exits {
            for (x, inner) in yes {
                self.narrow_to(&x, inner, cond.span);
            }
        }
        Ok(())
    }

    /// A hole body never returns, so its `post` lines would go unchecked until the body is
    /// written, after the spec is frozen. Check them against `out` in a block that is dropped.
    fn check_posts_of_hole(&mut self, at: Span) -> R<()> {
        if self.posts.is_empty() {
            return Ok(());
        }
        let ty = self.sig.ret.clone();
        let posts = self.posts.clone();
        self.block_with(|l| {
            let out = l.fresh("out");
            l.bind(
                &out,
                false,
                &ty,
                Rhs::Atom(Atom::Lit(Literal::Unit, at)),
                at,
                at,
            );
            let depth = l.scopes.len();
            l.narrow.push(("out".into(), out.clone(), depth));
            for p in &posts {
                l.cond(p)?;
            }
            l.narrow.retain(|m| m.1 != out || m.0 != "out");
            Ok(())
        })?;
        Ok(())
    }

    /// Return `a`, after checking each `post` with `out` naming it.
    fn ret(&mut self, a: Atom, at: Span) -> R<()> {
        if self.posts.is_empty() {
            self.emit(StmtKind::Ret(a), at);
            return Ok(());
        }
        let ty = self.sig.ret.clone();
        let out = match &a {
            Atom::Name(n, _) if self.locals.contains_key(n) => n.clone(),
            _ => {
                let n = self.fresh("out");
                self.bind(&n, false, &ty, Rhs::Atom(a), at, at);
                n
            }
        };
        let depth = self.scopes.len();
        self.narrow.push(("out".into(), out.clone(), depth));
        for p in self.posts.clone() {
            let c = self.cond(&p)?;
            self.emit(StmtKind::Assert(y::Expr::Atom(c)), p.span);
        }
        self.narrow.retain(|m| m.1 != out || m.0 != "out");
        self.emit(StmtKind::Ret(Atom::Name(out, at)), at);
        Ok(())
    }

    /// A condition: a `bool`, or under the lenient reader a value tested by its type (N15).
    fn cond(&mut self, e: &j::Expr) -> R<Atom> {
        let (a, t) = self.atom(e, Some(&Ty::Bool))?;
        if matches!(t, Ty::Bool | Ty::Err | Ty::Never) || self.cx.strict {
            self.expect(&t, &Ty::Bool, e.span);
            return Ok(a);
        }
        if matches!(t, Ty::List(_)) {
            self.sites.insert(e.span, Site::List);
        }
        let Some((test, how)) = self.truth(a, &t, e.span)? else {
            self.expect(&t, &Ty::Bool, e.span);
            return Ok(Atom::Lit(Literal::Bool(false), e.span));
        };
        let item = self.item.clone();
        self.cx.note(
            "N15",
            e.span,
            &item,
            format!(
                "a `{t}` as a condition normalized to {how}",
                t = self.ty_text(&t)
            ),
        );
        Ok(test)
    }

    /// The truth test of `a: t` that N15 writes out, and how it is spelled.
    fn truth(&mut self, a: Atom, t: &Ty, at: Span) -> R<Option<(Atom, &'static str)>> {
        let zero = |s: &str| {
            Atom::Lit(
                if s.starts_with('f') {
                    Literal::Float("0.0".into())
                } else {
                    Literal::Int("0".into())
                },
                at,
            )
        };
        let (r, how) = match t {
            Ty::Int | Ty::Float | Ty::Fixed(_) => {
                let s = t.scalar().expect("a scalar");
                let c = call(
                    &format!("ne_{s}"),
                    vec![],
                    vec![let_arg(a), let_arg(zero(s))],
                    at,
                );
                (Rhs::Call(c), "`n != 0`")
            }
            Ty::BigInt => {
                let z = call("std.prelude.bigint", vec![], vec![let_arg(zero("i64"))], at);
                let z = self.tmp(&Ty::BigInt, Rhs::Call(z), at);
                let c = call("ne_bigint", vec![], vec![let_arg(a), let_arg(z)], at);
                (Rhs::Call(c), "`n != 0`")
            }
            Ty::Str => {
                let empty = Atom::Lit(Literal::Str(String::new()), at);
                let c = call(
                    "std.cmp.ne",
                    vec![t.targ()],
                    vec![let_arg(a), let_arg(empty)],
                    at,
                );
                (Rhs::Call(c), "`s != \"\"`")
            }
            Ty::List(_) | Ty::Set(_) | Ty::Dict(..) | Ty::Coll(..) => {
                let targs = match t {
                    Ty::List(e) | Ty::Set(e) => vec![e.targ()],
                    Ty::Dict(k, v) => vec![k.targ(), v.targ()],
                    _ => coll_targs(t),
                };
                let path = format!("std.{}.len", t.module().expect("a collection"));
                let n = self.tmp(
                    &Ty::Int,
                    Rhs::Call(call(&path, targs, vec![let_arg(a)], at)),
                    at,
                );
                let c = call("gt_i64", vec![], vec![let_arg(n), let_arg(zero("i64"))], at);
                (Rhs::Call(c), "`len(xs) > 0`")
            }
            Ty::Option(_) => {
                let (v, _) = self.is_some(a, at)?;
                return Ok(Some((v, "`x is not None`")));
            }
            _ => return Ok(None),
        };
        Ok(Some((self.tmp(&Ty::Bool, r, at), how)))
    }

    /// Whether the optional `a` holds a value.
    fn is_some(&mut self, a: Atom, at: Span) -> R<(Atom, Ty)> {
        self.join(at, |l, k| {
            let arm = |l: &mut Self, b: bool| {
                l.block_with(|l| {
                    l.jump(k, Atom::Lit(Literal::Bool(b), at), at);
                    Ok(())
                })
            };
            let some = y::Pat::Ctor {
                path: vec!["Some".into()],
                fields: Some((vec![("value".into(), y::Pat::Wild)], false)),
            };
            let arms = vec![
                (some, arm(l, true)?),
                (y::Pat::Bind("None".into()), arm(l, false)?),
            ];
            Ok((
                y::Stmt {
                    kind: StmtKind::Match { scrutinee: a, arms },
                    span: at,
                },
                Ty::Bool,
            ))
        })
    }

    /// The local an optional `n` is narrowed to here.
    fn narrowed(&self, n: &str) -> Option<&str> {
        self.narrow
            .iter()
            .rev()
            .find(|m| m.0 == n)
            .map(|m| m.1.as_str())
    }

    /// The optional locals that `cond` being `holds` proves present, with their value types:
    /// `x is not None` when it holds, `x is None` when it does not, through `and` and `or`.
    fn narrowings(&self, cond: &j::Expr, holds: bool) -> Vec<(String, Ty)> {
        use j::ExprKind as K;
        match &cond.kind {
            K::Compare(op @ ("is" | "is not"), x, none)
                if matches!(none.kind, K::Lit(j::Lit::None)) && (*op == "is not") == holds =>
            {
                match &x.kind {
                    K::Name(n) if self.narrowed(n).is_none() => match self.locals.get(n) {
                        Some(Local {
                            ty: Ty::Option(inner),
                        }) => vec![(n.clone(), (**inner).clone())],
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                }
            }
            K::Not(x) => self.narrowings(x, !holds),
            K::Binary("and", a, b) if holds => {
                let mut v = self.narrowings(a, true);
                v.extend(self.narrowings(b, true));
                v
            }
            K::Binary("or", a, b) if !holds => {
                let mut v = self.narrowings(a, false);
                v.extend(self.narrowings(b, false));
                v
            }
            _ => Vec::new(),
        }
    }

    /// From here to the end of the block, `x` names its unwrapped value.
    fn narrow_to(&mut self, x: &str, inner: Ty, at: Span) {
        let o = self.fresh("n");
        let (v, _) = self
            .join(at, |l, k| {
                let some = y::Pat::Ctor {
                    path: vec!["Some".into()],
                    fields: Some((vec![("value".into(), y::Pat::Bind(o.clone()))], false)),
                };
                let some_b = l.block_with(|l| {
                    l.declare(&o, inner.clone());
                    l.jump(k, Atom::Name(o.clone(), at), at);
                    Ok(())
                })?;
                let none_b = y::Block {
                    stmts: vec![y::Stmt {
                        kind: StmtKind::Unreachable,
                        span: at,
                    }],
                };
                Ok((
                    y::Stmt {
                        kind: StmtKind::Match {
                            scrutinee: Atom::Name(x.to_owned(), at),
                            arms: vec![(some, some_b), (y::Pat::Bind("None".into()), none_b)],
                        },
                        span: at,
                    },
                    inner.clone(),
                ))
            })
            .expect("lowering a narrowing cannot fail");
        let Atom::Name(v, _) = v else {
            unreachable!("a join value is a name")
        };
        let depth = self.scopes.len();
        self.narrow.push((x.to_owned(), v, depth));
    }

    /// Bind a `for` target, one name or a tuple of names, to `cur` of type `ty`.
    fn bind_target(&mut self, target: &[String], ty: &Ty, cur: Rhs, at: Span, span: Span) {
        match (target, ty) {
            ([n], _) => {
                let m = self.mutable.contains(n);
                self.bind(n, m, ty, cur, at, span);
            }
            (ns, Ty::Tuple(ts)) if ns.len() == ts.len() => {
                let e = self.tmp(ty, cur, at);
                let Atom::Name(e, _) = e else {
                    unreachable!("a projection is bound")
                };
                for (k, (n, t)) in ns.iter().zip(ts).enumerate() {
                    let p = Rhs::Proj(Place {
                        root: e.clone(),
                        root_span: at,
                        projs: vec![Proj::Field(format!("_{k}"), at)],
                    });
                    let m = self.mutable.contains(n);
                    self.bind(n, m, t, p, at, at);
                }
            }
            _ => {
                self.err(
                    "E201",
                    at,
                    format!(
                        "cannot unpack `{ty}` into {} names",
                        target.len(),
                        ty = self.ty_text(ty)
                    ),
                );
            }
        }
    }

    /// `for` over an `iter[T]`: pull until it is done (R515).
    #[allow(clippy::too_many_arguments)]
    fn for_iter(
        &mut self,
        label: String,
        name: Option<&str>,
        target: &[String],
        it: Atom,
        e: Ty,
        span: Span,
        at: Span,
        body: impl FnOnce(&mut Self) -> R<()>,
    ) -> R<()> {
        self.loops.push((label.clone(), name.map(str::to_owned)));
        self.loop_outer.push(self.locals.keys().cloned().collect());
        let b = self.block_with(|l| {
            let done = call(
                "std.iter.ops.done",
                vec![e.targ()],
                vec![let_arg(it.clone())],
                at,
            );
            let done = l.tmp(&Ty::Bool, Rhs::Call(done), at);
            let brk = y::Stmt {
                kind: StmtKind::Break {
                    label: label.clone(),
                    value: None,
                },
                span: at,
            };
            l.emit(
                StmtKind::If {
                    cond: done,
                    then: y::Block { stmts: vec![brk] },
                    els: None,
                },
                at,
            );
            let cur = call(
                "std.iter.ops.current",
                vec![e.targ()],
                vec![let_arg(it)],
                at,
            );
            l.bind_target(target, &e, Rhs::Call(cur), at, span);
            body(l)
        });
        self.loops.pop();
        self.loop_outer.pop();
        let body = b?;
        self.emit(
            StmtKind::Loop {
                label,
                invariants: Vec::new(),
                decreases: Vec::new(),
                body,
            },
            at,
        );
        Ok(())
    }

    fn for_(
        &mut self,
        name: Option<&str>,
        target: &[String],
        iter: &j::Expr,
        at: Span,
        body: impl FnOnce(&mut Self) -> R<()>,
    ) -> R<()> {
        let label = self.label();
        let i = self.fresh("i");
        let one = Atom::Lit(Literal::Int("1".into()), at);
        let is_range = matches!(&iter.kind, j::ExprKind::Call(f, _)
            if matches!(&f.kind, j::ExprKind::Name(n) if n == "range" && !self.locals.contains_key(n)));
        // `src` is the list and its element type; `None` iterates the counter itself.
        let (src, stop, step) = if is_range {
            let j::ExprKind::Call(_, args) = &iter.kind else {
                unreachable!("matched above")
            };
            let mut xs = Vec::new();
            for a in args {
                let (j::ArgKind::Expr(e), None) = (&a.kind, &a.name) else {
                    return unsupported("`range` with a keyword or reference argument");
                };
                let (x, t) = self.atom(e, Some(&Ty::Int))?;
                self.expect(&t, &Ty::Int, e.span);
                xs.push(x);
            }
            let zero = Atom::Lit(Literal::Int("0".into()), at);
            let (start, stop, step) = match xs.len() {
                1 => (zero, xs.remove(0), one.clone()),
                2 => {
                    let stop = xs.remove(1);
                    (xs.remove(0), stop, one.clone())
                }
                3 if matches!(&xs[2], Atom::Lit(Literal::Int(s), _) if !s.starts_with('-') && s != "0") =>
                {
                    let step = xs.remove(2);
                    let stop = xs.remove(1);
                    (xs.remove(0), stop, step)
                }
                3 => return unsupported("`range` with a step that is not a positive literal"),
                _ => {
                    self.err("E202", iter.span, "`range` takes 1 to 3 arguments".into());
                    return Ok(());
                }
            };
            self.bind(&i, true, &Ty::Int, Rhs::Atom(start), at, at);
            (None, stop, step)
        } else {
            let (mut xs, t) = self.atom(iter, None)?;
            if let Ty::Coll("iter", ts) = &t {
                let e = ts[0].clone();
                return self.for_iter(label, name, target, xs, e, iter.span, at, body);
            }
            let mut len_path = "std.list.len".to_owned();
            let e = match &t {
                Ty::List(e) => (**e).clone(),
                Ty::Coll("heap", _) => {
                    self.err(
                        "E201",
                        iter.span,
                        "a `heap` is not iterable; use `h.into_sorted()`".into(),
                    );
                    return Ok(());
                }
                // Keys in ascending order, as a list.
                Ty::Coll("sorted_dict", ts) => {
                    let keys = call(
                        "std.collections.sorted_dict.keys",
                        coll_targs(&t),
                        vec![let_arg(xs)],
                        at,
                    );
                    xs = self.tmp(&Ty::List(Box::new(ts[0].clone())), Rhs::Call(keys), at);
                    ts[0].clone()
                }
                Ty::Coll(_, ts) => {
                    len_path = format!("std.{}.len", t.module().expect("a collection"));
                    ts[0].clone()
                }
                Ty::Str if self.cx.strict => {
                    let s = self.cx.text(iter.span);
                    self.err(
                        "E504",
                        iter.span,
                        format!("a `str` is not iterable; use `{s}.chars()`"),
                    );
                    return Ok(());
                }
                Ty::Str => {
                    let item = self.item.clone();
                    self.cx.note(
                        "N21",
                        iter.span,
                        &item,
                        "`for c in s` normalized to `for c in s.chars()`".into(),
                    );
                    let chars = call("std.text.chars", vec![], vec![let_arg(xs)], at);
                    let it = Ty::Coll("iter", vec![Ty::Char]);
                    let it = self.tmp(&it, Rhs::Call(chars), at);
                    return self.for_iter(label, name, target, it, Ty::Char, iter.span, at, body);
                }
                Ty::Err => return Ok(()),
                t => return unsupported(format!("iterating a `{t}`", t = self.ty_text(t))),
            };
            let zero = Atom::Lit(Literal::Int("0".into()), at);
            self.bind(&i, true, &Ty::Int, Rhs::Atom(zero), at, at);
            let len = call(&len_path, vec![e.targ()], vec![let_arg(xs.clone())], at);
            let n = self.tmp(&Ty::Int, Rhs::Call(len), at);
            (Some((xs, e)), n, one)
        };
        self.loops.push((label.clone(), name.map(str::to_owned)));
        self.loop_outer.push(self.locals.keys().cloned().collect());
        let b = self.block_with(|l| {
            let ge = call(
                "ge_i64",
                vec![],
                vec![let_arg(Atom::Name(i.clone(), at)), let_arg(stop)],
                at,
            );
            let done = l.tmp(&Ty::Bool, Rhs::Call(ge), at);
            let brk = y::Stmt {
                kind: StmtKind::Break {
                    label: label.clone(),
                    value: None,
                },
                span: at,
            };
            l.emit(
                StmtKind::If {
                    cond: done,
                    then: y::Block { stmts: vec![brk] },
                    els: None,
                },
                at,
            );
            let (cur, ty) = match src {
                None => (Rhs::Atom(Atom::Name(i.clone(), at)), Ty::Int),
                Some((Atom::Name(xs, sp), e)) => (
                    Rhs::Proj(Place {
                        root: xs,
                        root_span: sp,
                        projs: vec![Proj::Index(Atom::Name(i.clone(), at))],
                    }),
                    e,
                ),
                Some(_) => return unsupported("iterating a literal"),
            };
            l.bind_target(target, &ty, cur, at, iter.span);
            let next = call(
                "add_checked_i64",
                vec![],
                vec![let_arg(Atom::Name(i.clone(), at)), let_arg(step)],
                at,
            );
            l.emit(
                StmtKind::Assign {
                    place: Place {
                        root: i.clone(),
                        root_span: at,
                        projs: Vec::new(),
                    },
                    rhs: Rhs::Call(next),
                    rhs_span: at,
                },
                at,
            );
            body(l)
        });
        self.loops.pop();
        self.loop_outer.pop();
        let body = b?;
        self.emit(
            StmtKind::Loop {
                label,
                invariants: Vec::new(),
                decreases: Vec::new(),
                body,
            },
            at,
        );
        Ok(())
    }

    /// A loop that fills a fresh collection. Its element type is known only once the element
    /// is lowered inside the loop, so the collection's `let` is patched afterwards.
    /// `par` comprehensions run in source order, which is what their results observe (12.2).
    fn comprehension(&mut self, e: &j::Expr) -> R<(Rhs, Ty)> {
        let j::ExprKind::Comp {
            kind,
            elem,
            value,
            target,
            iter,
            cond,
            ..
        } = &e.kind
        else {
            unreachable!("called on a comprehension")
        };
        let (kind, value, cond, at) = (*kind, value.as_deref(), cond.as_deref(), e.span);
        let acc = self.fresh("c");
        let new = match kind {
            j::CompKind::List => "std.list.new",
            j::CompKind::Set => "std.set.new",
            j::CompKind::Dict => "std.dict.new",
        };
        self.bind(
            &acc,
            true,
            &Ty::Err,
            Rhs::Call(call(new, vec![], vec![], at)),
            at,
            at,
        );
        let slot = self.out.len() - 1;
        let mut found: Option<(Ty, Option<Ty>)> = None;
        let acc_arg = Arg::Value(Conv::Inout, Atom::Name(acc.clone(), at), at);
        self.for_(None, target, iter, at, |l| {
            let mut push = |l: &mut Self| -> R<()> {
                let (x, xt) = l.atom(elem, None)?;
                let (path, mut args, mut targs) = match kind {
                    j::CompKind::List => {
                        ("std.list.append", vec![acc_arg.clone()], vec![xt.targ()])
                    }
                    j::CompKind::Set => ("std.set.add", vec![acc_arg.clone()], vec![xt.targ()]),
                    j::CompKind::Dict => {
                        ("std.dict.insert", vec![acc_arg.clone()], vec![xt.targ()])
                    }
                };
                let vt = match (kind, value) {
                    (j::CompKind::Dict, Some(v)) => {
                        let (va, vt) = l.atom(v, None)?;
                        targs.push(vt.targ());
                        args.push(let_arg(x));
                        args.push(sink_arg(va));
                        Some(vt)
                    }
                    _ => {
                        args.push(sink_arg(x));
                        None
                    }
                };
                found = Some((xt, vt));
                l.emit(StmtKind::Rhs(Rhs::Call(call(path, targs, args, at))), at);
                Ok(())
            };
            match cond {
                None => push(l),
                Some(c) => {
                    let (ca, ct) = l.atom(c, Some(&Ty::Bool))?;
                    l.expect(&ct, &Ty::Bool, c.span);
                    let then = l.block_with(push)?;
                    l.emit(
                        StmtKind::If {
                            cond: ca,
                            then,
                            els: None,
                        },
                        at,
                    );
                    Ok(())
                }
            }
        })?;
        let ty = match (kind, found) {
            (_, None) => Ty::Err,
            (j::CompKind::List, Some((x, _))) => Ty::List(Box::new(x)),
            (j::CompKind::Set, Some((x, _))) => Ty::Set(Box::new(x)),
            (j::CompKind::Dict, Some((k, v))) => {
                Ty::Dict(Box::new(k), Box::new(v.unwrap_or(Ty::Err)))
            }
        };
        let targs = match &ty {
            Ty::List(x) | Ty::Set(x) => vec![x.targ()],
            Ty::Dict(k, v) => vec![k.targ(), v.targ()],
            _ => vec![],
        };
        if let StmtKind::Let {
            ty: t,
            rhs: Rhs::Call(c),
            ..
        } = &mut self.out[slot].kind
        {
            *t = ty.core();
            c.targs = targs;
        }
        self.declare(&acc, ty.clone());
        Ok((Rhs::Atom(Atom::Name(acc, at)), ty))
    }

    fn match_(&mut self, scrutinee: &j::Expr, cases: &[j::Case], at: Span) -> R<()> {
        let (a, t) = self.atom(scrutinee, None)?;
        let catch_all = |p: &j::Pat| match p {
            j::Pat::Wild => true,
            j::Pat::Path { path, args: None } => {
                path.len() == 1 && !self.g.variant_of.contains_key(&path[0])
            }
            _ => false,
        };
        if let Ty::Data(d) = &t
            && self.g.sums.contains_key(d)
            && let Some(c) = cases
                .iter()
                .find(|c| matches!(c.pat, j::Pat::Wild) && c.span != at)
        {
            if self.cx.wen {
                let item = self.item.clone();
                self.cx.warn(
                    "W811",
                    c.span,
                    &item,
                    format!("`_` on the sum `{d}`; name each variant"),
                );
            } else {
                self.err(
                    "E502",
                    c.span,
                    format!("`case _` on the sum `{d}`; name each variant"),
                );
            }
        }
        let scalar = matches!(t, Ty::Int | Ty::Float | Ty::Str | Ty::Char | Ty::Fixed(_));
        let bools = |b: bool| {
            cases.iter().any(|c| {
                matches!(&c.pat, j::Pat::Lit(e) if matches!(e.kind, j::ExprKind::Lit(j::Lit::Bool(x)) if x == b))
            })
        };
        let covered = cases.iter().any(|c| c.guard.is_none() && catch_all(&c.pat))
            || (matches!(t, Ty::Bool) && bools(true) && bools(false));
        if (scalar || matches!(t, Ty::Bool)) && !covered {
            self.err(
                "E503",
                at,
                format!("this `match` on `{t}` needs `case _`", t = self.ty_text(&t)),
            );
        }
        if cases.iter().any(|c| c.guard.is_some()) {
            return self.guarded_match(a, &t, cases, at);
        }
        let mut arms = Vec::new();
        for c in cases {
            let mut pat = y::Pat::Wild;
            let b = self.block_with(|l| {
                let (p, binds) = l.pat(&c.pat, &t, at)?;
                pat = p;
                for (n, ty) in binds {
                    l.declare(&n, ty);
                }
                l.stmts(&c.body.stmts)
            })?;
            arms.push((pat, b));
        }
        self.emit(StmtKind::Match { scrutinee: a, arms }, at);
        Ok(())
    }

    /// Yong's `match` has no guards, so a guarded one is a block that tries each case in turn:
    /// a one-case match per case whose arm runs the body when the guard holds and leaves.
    fn guarded_match(&mut self, a: Atom, t: &Ty, cases: &[j::Case], at: Span) -> R<()> {
        let done = self.label();
        let leave = |at| y::Stmt {
            kind: StmtKind::Break {
                label: done.clone(),
                value: None,
            },
            span: at,
        };
        let b = self.block_with(|l| {
            for c in cases {
                let mut pat = y::Pat::Wild;
                let arm = l.block_with(|l| {
                    let (p, binds) = l.pat(&c.pat, t, at)?;
                    pat = p;
                    for (n, ty) in binds {
                        l.declare(&n, ty);
                    }
                    let body = l.block_with(|l| {
                        l.stmts(&c.body.stmts)?;
                        if !ends(&c.body.stmts) {
                            l.out.push(leave(c.span));
                        }
                        Ok(())
                    })?;
                    match &c.guard {
                        None => l.out.extend(body.stmts),
                        Some(g) => {
                            let (cond, gt) = l.atom(g, Some(&Ty::Bool))?;
                            l.expect(&gt, &Ty::Bool, g.span);
                            l.emit(
                                StmtKind::If {
                                    cond,
                                    then: body,
                                    els: None,
                                },
                                c.span,
                            );
                        }
                    }
                    Ok(())
                })?;
                let rest = y::Block { stmts: Vec::new() };
                l.emit(
                    StmtKind::Match {
                        scrutinee: a.clone(),
                        arms: vec![(pat, arm), (y::Pat::Wild, rest)],
                    },
                    c.span,
                );
            }
            l.out.push(leave(at));
            Ok(())
        })?;
        self.emit(
            StmtKind::Loop {
                label: done.clone(),
                invariants: Vec::new(),
                decreases: Vec::new(),
                body: b,
            },
            at,
        );
        Ok(())
    }

    fn pat(&mut self, p: &j::Pat, ty: &Ty, at: Span) -> R<(y::Pat, Vec<(String, Ty)>)> {
        Ok(match p {
            j::Pat::Wild => (y::Pat::Wild, Vec::new()),
            j::Pat::Lit(e) => {
                let (l, lt) = match &e.kind {
                    j::ExprKind::Lit(l) => (literal(l)?, lit_ty(l)),
                    j::ExprKind::Unary("-", x) => match &x.kind {
                        j::ExprKind::Lit(j::Lit::Int(s)) => {
                            (Literal::Int(format!("-{}", int_text(s)?)), Ty::Int)
                        }
                        _ => return unsupported("this literal pattern"),
                    },
                    _ => return unsupported("this literal pattern"),
                };
                self.expect(&lt, ty, e.span);
                (y::Pat::Lit(l), Vec::new())
            }
            j::Pat::Path { path, args } if path.len() == 1 => {
                let n = &path[0];
                let fields: Option<Vec<(String, Ty)>> = match (ty, n.as_str()) {
                    (Ty::Option(_), "None") if args.is_none() => Some(Vec::new()),
                    (Ty::Option(t), "Some") => Some(vec![("value".into(), (**t).clone())]),
                    (Ty::Result(t, _), "Ok") => Some(vec![("value".into(), (**t).clone())]),
                    (Ty::Result(_, e), "Err") => Some(vec![("error".into(), (**e).clone())]),
                    (Ty::Data(s), _) if self.g.variant_of.get(n) == Some(s) => self.g.sums[s]
                        .iter()
                        .find(|(v, _)| v == n)
                        .map(|(_, fs)| fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect()),
                    (Ty::Data(r), _) if r == n && self.g.records.contains_key(r) => Some(
                        self.g.records[r]
                            .iter()
                            .map(|f| (f.name.clone(), f.ty.clone()))
                            .collect(),
                    ),
                    _ => None,
                };
                match (args, fields) {
                    (None, Some(fs)) if fs.is_empty() => (y::Pat::Bind(n.clone()), Vec::new()),
                    (None, _) => (y::Pat::Bind(n.clone()), vec![(n.clone(), ty.clone())]),
                    (Some(_), None) if matches!(ty, Ty::Err) => (y::Pat::Wild, Vec::new()),
                    (Some(_), None) => {
                        self.err(
                            "E201",
                            at,
                            format!("pattern `{n}` cannot match a `{ty}`", ty = self.ty_text(ty)),
                        );
                        (y::Pat::Wild, Vec::new())
                    }
                    (Some(ps), Some(fs)) if fs.is_empty() && ps.is_empty() => {
                        (y::Pat::Bind(n.clone()), Vec::new())
                    }
                    (Some(ps), Some(fs)) => {
                        let mut pairs = Vec::new();
                        let mut binds = Vec::new();
                        for (k, (name, sub)) in ps.iter().enumerate() {
                            let f = match name {
                                Some(name) => fs.iter().find(|(f, _)| f == name),
                                None => fs.get(k),
                            };
                            let Some((fname, fty)) = f else {
                                self.err(
                                    "E203",
                                    at,
                                    format!("`{n}` has no field for this pattern"),
                                );
                                continue;
                            };
                            let (sp, mut b) = self.pat(sub, fty, at)?;
                            pairs.push((fname.clone(), sp));
                            binds.append(&mut b);
                        }
                        let rest = pairs.len() < fs.len();
                        (
                            y::Pat::Ctor {
                                path: vec![n.clone()],
                                fields: Some((pairs, rest)),
                            },
                            binds,
                        )
                    }
                }
            }
            j::Pat::Tuple(ps) => {
                let Ty::Tuple(ts) = ty else {
                    self.err(
                        "E201",
                        at,
                        format!(
                            "a tuple pattern cannot match a `{ty}`",
                            ty = self.ty_text(ty)
                        ),
                    );
                    return Ok((y::Pat::Wild, Vec::new()));
                };
                if ts.len() != ps.len() {
                    self.err(
                        "E201",
                        at,
                        format!(
                            "a {}-tuple pattern cannot match a `{ty}`",
                            ps.len(),
                            ty = self.ty_text(ty)
                        ),
                    );
                    return Ok((y::Pat::Wild, Vec::new()));
                }
                let mut out = Vec::new();
                let mut binds = Vec::new();
                for (p, t) in ps.iter().zip(ts) {
                    let (sp, mut b) = self.pat(p, t, at)?;
                    out.push(sp);
                    binds.append(&mut b);
                }
                (y::Pat::Tuple(out), binds)
            }
            _ => return unsupported("qualified and `as` patterns"),
        })
    }

    fn assign(
        &mut self,
        target: &j::Expr,
        ann: Option<&j::Type>,
        value: &j::Expr,
        at: Span,
    ) -> R<()> {
        let ann = ann.map(|t| self.g.ty(t, self.cx, at)).transpose()?;
        match &target.kind {
            j::ExprKind::Tuple(ts) => {
                let mut vals = Vec::new();
                match &value.kind {
                    j::ExprKind::Tuple(vs) if vs.len() == ts.len() => {
                        // R216: snapshot names too, so `a, b = b, a` swaps.
                        for v in vs {
                            let (a, t) = self.atom(v, None)?;
                            let a = match a {
                                Atom::Name(..) => {
                                    let n = self.fresh("t");
                                    self.bind(&n, false, &t, Rhs::Atom(a), v.span, v.span);
                                    Atom::Name(n, v.span)
                                }
                                lit => lit,
                            };
                            vals.push((a, t, v.span));
                        }
                    }
                    _ => {
                        let (a, t) = self.atom(value, None)?;
                        let Ty::Tuple(tys) = &t else {
                            if !matches!(t, Ty::Err) {
                                self.err(
                                    "E201",
                                    value.span,
                                    format!(
                                        "cannot unpack `{t}` into {} targets",
                                        ts.len(),
                                        t = self.ty_text(&t)
                                    ),
                                );
                            }
                            return Ok(());
                        };
                        if tys.len() != ts.len() {
                            self.err(
                                "E201",
                                value.span,
                                format!(
                                    "cannot unpack `{t}` into {} targets",
                                    ts.len(),
                                    t = self.ty_text(&t)
                                ),
                            );
                            return Ok(());
                        }
                        let Atom::Name(root, sp) = a else {
                            unreachable!("tuples are bound")
                        };
                        for (k, ty) in tys.iter().enumerate() {
                            let p = Rhs::Proj(Place {
                                root: root.clone(),
                                root_span: sp,
                                projs: vec![Proj::Field(format!("_{k}"), sp)],
                            });
                            let a = self.tmp(ty, p, value.span);
                            vals.push((a, ty.clone(), value.span));
                        }
                    }
                }
                for (t, (a, ty, sp)) in ts.iter().zip(vals) {
                    self.store(t, None, Rhs::Atom(a), ty, sp)?;
                }
                Ok(())
            }
            j::ExprKind::Name(n) => {
                let want = ann
                    .clone()
                    .or_else(|| self.locals.get(n).map(|l| l.ty.clone()));
                // Captures are copied at creation, so creating the closure at its first call is
                // the same only when nothing it captures is reassigned.
                if let (None, j::ExprKind::Lambda(ps, body)) = (&want, &value.kind)
                    && !self.mutable.contains(n)
                {
                    let mut stable = true;
                    walk(body, &mut |e| {
                        if let j::ExprKind::Name(m) = &e.kind
                            && !ps.contains(m)
                            && self.mutable.contains(m)
                        {
                            stable = false;
                        }
                    });
                    if stable {
                        self.pending
                            .insert(n.clone(), (target.clone(), value.clone()));
                        return Ok(());
                    }
                }
                let outer = self.target.replace(n.clone());
                let r = self.rhs(value, want.as_ref());
                self.target = outer;
                let (r, t) = r?;
                if let Some(a) = &ann
                    && *a == t
                    && !self.locals.contains_key(n)
                    && self.inferred_alone(value, &t)
                {
                    let sp = self.annotation_span(target.span);
                    let item = self.item.clone();
                    self.cx.warn(
                        "W823",
                        sp,
                        &item,
                        format!(
                            "`{n}` is inferred as `{t}`; remove the annotation",
                            t = self.ty_text(&t)
                        ),
                    );
                    let colon_to_type = Span {
                        col: target.span.end_col,
                        ..sp
                    };
                    self.cx.fix(
                        Applicability::Auto,
                        "remove the annotation",
                        colon_to_type,
                        "",
                    );
                }
                self.store(target, ann, r, t, value.span)
            }
            _ => {
                // An element's type types a literal stored into it: `ba[0] = 66` is a `uint8`.
                let want = match &target.kind {
                    j::ExprKind::Index(b, _) => match self.ty_of_local(b) {
                        Some(Ty::List(x)) => Some(*x),
                        Some(Ty::Coll("bytearray", _)) => Some(Ty::Fixed("u8")),
                        _ => None,
                    },
                    _ => None,
                };
                let (r, t) = self.rhs(value, want.as_ref())?;
                self.store(target, ann, r, t, value.span)
            }
        }
    }

    fn store(&mut self, target: &j::Expr, ann: Option<Ty>, r: Rhs, t: Ty, at: Span) -> R<()> {
        if let Rhs::Atom(Atom::Name(x, _)) = &r
            && !t.is_copy()
            && self.locals.contains_key(x)
            && !x.starts_with('_')
            && let Some(later) = self.next_use(x, at)
        {
            self.err(
                "E413",
                at,
                format!("`{x}` is used again later; bind `{x}.copy()`"),
            );
            self.cx
                .fix(Applicability::Auto, "copy", at, &format!("{x}.copy()"));
            self.cx.label(later, "used again");
        }
        match &target.kind {
            j::ExprKind::Name(n) if n == "_" => {
                if !matches!(r, Rhs::Atom(_)) {
                    self.emit(
                        StmtKind::Let {
                            mutable: false,
                            pat: y::Pat::Wild,
                            pat_span: target.span,
                            ty: t.core(),
                            rhs: r,
                            rhs_span: at,
                        },
                        target.span,
                    );
                }
            }
            j::ExprKind::Name(n) if ann.is_none() && self.locals.contains_key(n) => {
                self.narrow.retain(|m| m.0 != *n);
                let want = self.locals[n].ty.clone();
                let (r, t) = self.inject(r, t, Some(&want), at);
                self.expect(&t, &want, at);
                self.emit(
                    StmtKind::Assign {
                        place: Place {
                            root: n.clone(),
                            root_span: target.span,
                            projs: Vec::new(),
                        },
                        rhs: r,
                        rhs_span: at,
                    },
                    target.span,
                );
            }
            j::ExprKind::Name(n) => {
                if self.gone.contains(n) {
                    return unsupported("a binding made again after its block ended");
                }
                let (r, ty) = match ann {
                    Some(a) => {
                        let (r, t) = self.inject(r, t, Some(&a), at);
                        self.expect(&t, &a, at);
                        (r, a)
                    }
                    None => (r, t),
                };
                let m = self.mutable.contains(n);
                // An unread `try` result is E311 from the checker, not W812.
                let tried = matches!(r, Rhs::Try(_));
                self.bind(n, m, &ty, r, target.span, at);
                if !tried {
                    self.bound.push((n.clone(), target.span));
                }
            }
            j::ExprKind::Index(d, k) if self.ty_of_local(d).as_ref().and_then(map_kv).is_some() => {
                let dt = self.ty_of_local(d).expect("matched");
                let (kt, vt) = map_kv(&dt).expect("matched");
                let j::ExprKind::Name(dn) = &d.kind else {
                    unreachable!("a local")
                };
                let (ka, kty) = self.atom(k, Some(&kt))?;
                self.expect(&kty, &kt, k.span);
                self.expect(&t, &vt, at);
                let v = self.tmp(&t, r, at);
                let c = call(
                    &format!("std.{}.insert", dt.module().expect("a map")),
                    vec![kt.targ(), vt.targ()],
                    vec![
                        Arg::Value(Conv::Inout, Atom::Name(dn.clone(), d.span), d.span),
                        Arg::Value(Conv::Let, ka, k.span),
                        Arg::Value(Conv::Sink, v, at),
                    ],
                    target.span,
                );
                self.emit(StmtKind::Rhs(Rhs::Call(c)), target.span);
            }
            j::ExprKind::Attr(..) | j::ExprKind::Index(..) => {
                if let j::ExprKind::Index(b, _) = &target.kind
                    && let Some(Ty::Coll("bytes", _)) = self.ty_of_local(b)
                {
                    self.err(
                        "E201",
                        target.span,
                        "`bytes` is immutable; use a `bytearray`".into(),
                    );
                    return Ok(());
                }
                self.dict_places.clear();
                let (p, pt) = self.place(target)?;
                let stores = mem::take(&mut self.dict_places);
                self.expect(&t, &pt, at);
                self.emit(
                    StmtKind::Assign {
                        place: p,
                        rhs: r,
                        rhs_span: at,
                    },
                    target.span,
                );
                self.store_dict_places(stores, target.span);
            }
            j::ExprKind::Wild => self.emit(StmtKind::Rhs(r), target.span),
            _ => return unsupported("this assignment target"),
        }
        Ok(())
    }

    /// `d[k] op= v` inserts the value type's default first when `k` is missing (R438).
    fn insert_default(&mut self, d: &j::Expr, k: &j::Expr, kt: &Ty, vt: &Ty, at: Span) -> R<()> {
        if !matches!(k.kind, j::ExprKind::Name(_) | j::ExprKind::Lit(_)) {
            return unsupported("`d[k] op= v` with a computed key");
        }
        let j::ExprKind::Name(dn) = &d.kind else {
            unreachable!("a local")
        };
        let lit = |l| Atom::Lit(l, at);
        let zero = match vt {
            Ty::Int => lit(Literal::Int("0".into())),
            Ty::Float => lit(Literal::Float("0.0".into())),
            Ty::Str => lit(Literal::Str(String::new())),
            _ => {
                return unsupported(format!(
                    "`d[k] op= v` on a `{vt}` value",
                    vt = self.ty_text(vt)
                ));
            }
        };
        let (ka, kty) = self.atom(k, Some(kt))?;
        self.expect(&kty, kt, k.span);
        let c = call(
            "std.dict.setdefault",
            vec![kt.targ(), vt.targ()],
            vec![
                Arg::Value(Conv::Inout, Atom::Name(dn.clone(), d.span), d.span),
                Arg::Value(Conv::Let, ka, k.span),
                Arg::Value(Conv::Sink, zero, at),
            ],
            at,
        );
        self.tmp(vt, Rhs::Call(c), at);
        Ok(())
    }

    fn ty_of_local(&self, e: &j::Expr) -> Option<Ty> {
        match &e.kind {
            j::ExprKind::Name(n) => {
                let n = self.narrowed(n).unwrap_or(n);
                self.locals.get(n).map(|l| l.ty.clone())
            }
            _ => None,
        }
    }

    fn place(&mut self, e: &j::Expr) -> R<(Place, Ty)> {
        self.place_inner(e).map_err(|u| u.at(e.span))
    }

    fn place_inner(&mut self, e: &j::Expr) -> R<(Place, Ty)> {
        match &e.kind {
            j::ExprKind::Name(n) if self.narrowed(n).is_some() => {
                let m = self.narrowed(n).expect("checked").to_owned();
                let t = self.locals[&m].ty.clone();
                Ok((
                    Place {
                        root: m,
                        root_span: e.span,
                        projs: Vec::new(),
                    },
                    t,
                ))
            }
            j::ExprKind::Name(n) if self.locals.contains_key(n) => Ok((
                Place {
                    root: n.clone(),
                    root_span: e.span,
                    projs: Vec::new(),
                },
                self.locals[n].ty.clone(),
            )),
            j::ExprKind::Attr(b, f, fsp) => {
                let (mut p, t) = self.place(b)?;
                let ft = match &t {
                    Ty::Err => Ty::Err,
                    Ty::Data(r) if self.g.records.contains_key(r) => {
                        match self.g.records[r].iter().find(|fd| fd.name == *f) {
                            Some(fd) => fd.ty.clone(),
                            None => {
                                let names = self.g.records[r].iter().map(|fd| fd.name.as_str());
                                let msg = format!("`{r}` has no field `{f}`");
                                self.err_near("E203", *fsp, msg, f, names);
                                Ty::Err
                            }
                        }
                    }
                    t => {
                        self.err(
                            "E203",
                            *fsp,
                            format!("`{t}` has no field `{f}`", t = self.ty_text(t)),
                        );
                        Ty::Err
                    }
                };
                p.projs.push(Proj::Field(f.clone(), *fsp));
                Ok((p, ft))
            }
            j::ExprKind::Index(b, i) => {
                let (mut p, t) = self.place(b)?;
                match &t {
                    Ty::List(_) | Ty::Coll("deque" | "bytes" | "bytearray", _) => {
                        let et = match t {
                            Ty::Coll("bytes" | "bytearray", _) => Ty::Fixed("u8"),
                            _ => t.elem().cloned().expect("an element type"),
                        };
                        self.sites.insert(e.span, Site::List);
                        // N16: the parser kept `xs[-k]`; it means `xs[len(xs) - k]`.
                        let from_end;
                        let i = match &i.kind {
                            j::ExprKind::Lit(j::Lit::Int(s)) if s.starts_with('-') => {
                                let x = |kind| j::Expr {
                                    kind,
                                    span: i.span,
                                    paren: false,
                                };
                                let len = x(j::ExprKind::Call(
                                    Box::new(x(j::ExprKind::Name("len".into()))),
                                    vec![j::Arg {
                                        name: None,
                                        kind: j::ArgKind::Expr((**b).clone()),
                                        span: b.span,
                                    }],
                                ));
                                from_end = x(j::ExprKind::Binary("+", Box::new(len), i.clone()));
                                &from_end
                            }
                            _ => &**i,
                        };
                        let (ia, it) = self.atom(i, Some(&Ty::Int))?;
                        self.expect(&it, &Ty::Int, i.span);
                        p.projs.push(Proj::Index(ia));
                        Ok((p, et))
                    }
                    Ty::Tuple(ts) => {
                        let k = match &i.kind {
                            j::ExprKind::Lit(j::Lit::Int(s)) => s.parse::<usize>().ok(),
                            _ => None,
                        };
                        match k.and_then(|k| ts.get(k).map(|t| (k, t.clone()))) {
                            Some((k, t)) => {
                                p.projs.push(Proj::Field(format!("_{k}"), i.span));
                                Ok((p, t))
                            }
                            None => {
                                self.err(
                                    "E201",
                                    i.span,
                                    format!(
                                        "a `{t}` is indexed by a literal below {}",
                                        ts.len(),
                                        t = self.ty_text(&t)
                                    ),
                                );
                                Ok((p, Ty::Err))
                            }
                        }
                    }
                    Ty::Str => {
                        let s = self.cx.text(e.span);
                        self.err(
                            "E504",
                            e.span,
                            format!("`str` has no indexing; use `{s}.chars()`"),
                        );
                        Ok((p, Ty::Err))
                    }
                    Ty::Err => Ok((p, Ty::Err)),
                    t if p.projs.is_empty() && map_kv(t).is_some() => {
                        let (kt, vt) = map_kv(t).expect("matched");
                        self.sites.insert(e.span, Site::Dict);
                        let (ka, kty) = self.atom(i, Some(&kt))?;
                        self.expect(&kty, &kt, i.span);
                        let path = format!("std.{}.index", t.module().expect("a map"));
                        let root = Atom::Name(p.root.clone(), p.root_span);
                        let c = call(
                            &path,
                            vec![kt.targ(), vt.targ()],
                            vec![let_arg(root), let_arg(ka.clone())],
                            e.span,
                        );
                        let v = self.fresh("t");
                        self.bind(&v, true, &vt, Rhs::Call(c), e.span, e.span);
                        self.dict_places.push(DictPlace {
                            map: t.clone(),
                            root: p.root,
                            root_span: p.root_span,
                            key: ka,
                            value: v.clone(),
                        });
                        let place = Place {
                            root: v,
                            root_span: e.span,
                            projs: Vec::new(),
                        };
                        Ok((place, vt))
                    }
                    t => unsupported(format!("indexing a `{t}` as a place", t = self.ty_text(t))),
                }
            }
            _ => {
                let (a, t) = self.atom(e, None)?;
                match a {
                    Atom::Name(n, sp) => Ok((
                        Place {
                            root: n,
                            root_span: sp,
                            projs: Vec::new(),
                        },
                        t,
                    )),
                    lit @ Atom::Lit(..) => {
                        let v = self.fresh("t");
                        self.bind(&v, true, &t, Rhs::Atom(lit), e.span, e.span);
                        let place = Place {
                            root: v,
                            root_span: e.span,
                            projs: Vec::new(),
                        };
                        Ok((place, t))
                    }
                }
            }
        }
    }

    /// Stores the dict elements `place` copied out back into their dicts.
    fn store_dict_places(&mut self, places: Vec<DictPlace>, at: Span) {
        for d in places.into_iter().rev() {
            let (kt, vt) = map_kv(&d.map).expect("a map");
            let c = call(
                &format!("std.{}.insert", d.map.module().expect("a map")),
                vec![kt.targ(), vt.targ()],
                vec![
                    Arg::Value(Conv::Inout, Atom::Name(d.root, d.root_span), at),
                    Arg::Value(Conv::Let, d.key, at),
                    Arg::Value(Conv::Sink, Atom::Name(d.value, at), at),
                ],
                at,
            );
            self.emit(StmtKind::Rhs(Rhs::Call(c)), at);
        }
    }

    fn atom(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Atom, Ty)> {
        let (r, t) = self.rhs(e, want)?;
        let (r, t) = self.inject(r, t, want, e.span);
        Ok((self.tmp(&t, r, e.span), t))
    }

    /// The type of the integer literal `text`: the expected integer type when its range holds
    /// the value (3.4), else `int`. A value outside the expected type's range is E201.
    fn int_lit_ty(&mut self, text: &str, want: Option<&Ty>, at: Span) -> Ty {
        let t = match want {
            Some(w @ Ty::Fixed(n)) if !n.starts_with('f') => w.clone(),
            _ => Ty::Int,
        };
        let fits = text.parse::<i128>().ok().is_some_and(|v| {
            let (lo, hi) = int_range(t.scalar().expect("an integer type"));
            lo <= v && v <= hi
        });
        if !fits {
            let hint = if t == Ty::Int { "; use `bigint`" } else { "" };
            self.err(
                "E201",
                at,
                format!("`{text}` does not fit in `{t}`{hint}", t = self.ty_text(&t)),
            );
            return Ty::Err;
        }
        t
    }

    /// The `bigint` of decimal `text`, built from `int` pieces of at most 18 digits.
    fn bigint_lit(&mut self, text: &str, at: Span) -> Rhs {
        let (neg, digits) = match text.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, text),
        };
        let piece = |l: &mut Self, d: &str| {
            let d = d.trim_start_matches('0');
            let lit = Atom::Lit(Literal::Int(if d.is_empty() { "0" } else { d }.into()), at);
            let c = call("std.prelude.bigint", vec![], vec![let_arg(lit)], at);
            l.tmp(&Ty::BigInt, Rhs::Call(c), at)
        };
        let head = digits.len() % 18;
        let (first, rest) = digits.split_at(if head == 0 {
            18.min(digits.len())
        } else {
            head
        });
        let mut acc = piece(self, first);
        if !rest.is_empty() {
            let base = piece(self, "1000000000000000000");
            for k in (0..rest.len()).step_by(18) {
                let shifted = call(
                    "mul_bigint",
                    vec![],
                    vec![let_arg(acc), let_arg(base.clone())],
                    at,
                );
                let shifted = self.tmp(&Ty::BigInt, Rhs::Call(shifted), at);
                let p = piece(self, &rest[k..k + 18]);
                let sum = call("add_bigint", vec![], vec![let_arg(shifted), let_arg(p)], at);
                acc = self.tmp(&Ty::BigInt, Rhs::Call(sum), at);
            }
        }
        if !neg {
            return Rhs::Atom(acc);
        }
        let z = piece(self, "0");
        Rhs::Call(call(
            "sub_bigint",
            vec![],
            vec![let_arg(z), let_arg(acc)],
            at,
        ))
    }

    /// A `T` where `T | None` is expected is `Some` of it.
    fn inject(&mut self, r: Rhs, t: Ty, want: Option<&Ty>, at: Span) -> (Rhs, Ty) {
        let Some(Ty::Option(inner)) = want else {
            return (r, t);
        };
        if matches!(t, Ty::Option(_) | Ty::Err | Ty::Never) || !t.fits(inner) {
            return (r, t);
        }
        let a = self.tmp(&t, r, at);
        let ctor = y::Ctor::Named {
            path: vec!["Some".into()],
            targs: Vec::new(),
            fields: vec![("value".into(), a)],
        };
        (Rhs::Ctor(ctor), Ty::Option(Box::new(t)))
    }

    fn rhs(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Rhs, Ty)> {
        self.rhs_inner(e, want).map_err(|u| u.at(e.span))
    }

    fn rhs_inner(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Rhs, Ty)> {
        use j::ExprKind as K;
        let at = e.span;
        Ok(match &e.kind {
            K::Name(n) if self.stage.iter().any(|(m, _)| m == n) => {
                let v = self
                    .stage
                    .iter()
                    .rev()
                    .find(|(m, _)| m == n)
                    .expect("checked")
                    .1;
                let (a, t) = v.atom(at);
                (Rhs::Atom(a), t)
            }
            K::Call(f, args)
                if matches!(&f.kind, K::Name(o) if o == "old")
                    && let [
                        j::Arg {
                            name: None,
                            kind: j::ArgKind::Expr(x),
                            ..
                        },
                    ] = args.as_slice()
                    && let K::Name(p) = &x.kind
                    && let Some((_, v)) = self.olds.iter().find(|(q, _)| q == p) =>
            {
                let v = v.clone();
                let t = self.locals[&v].ty.clone();
                (Rhs::Atom(Atom::Name(v, at)), t)
            }
            K::Stage(x) => match fold(x, &self.stage) {
                Some(v) => {
                    let (a, t) = v.atom(at);
                    (Rhs::Atom(a), t)
                }
                None => {
                    return unsupported("stage-0 expressions in a function body beyond constants");
                }
            },
            K::Name(n) if self.narrowed(n).is_some() => {
                let m = self.narrowed(n).expect("checked").to_owned();
                self.uses.entry(n.clone()).or_default().push(at);
                let t = self.locals[&m].ty.clone();
                (Rhs::Atom(Atom::Name(m, at)), t)
            }
            K::Name(n) => {
                if let Some(l) = self.locals.get(n) {
                    (Rhs::Atom(Atom::Name(n.clone(), at)), l.ty.clone())
                } else if let Some(t) = self.g.consts.get(n) {
                    (Rhs::Atom(Atom::Name(n.clone(), at)), t.clone())
                } else if let Some(s) = self.g.variant_of.get(n) {
                    (Rhs::Atom(Atom::Name(n.clone(), at)), Ty::Data(s.clone()))
                } else if self.pending.contains_key(n) {
                    return unsupported("a lambda with no expected type used before a call");
                } else if let Some(sig) = self.g.fns.get(n)
                    && sig.tparams.is_empty()
                    && sig.caps.is_empty()
                    && sig.fail.is_none()
                    && sig.params.iter().all(|p| p.conv == Conv::Let)
                {
                    // A function used as a value is the lambda that calls it.
                    let names: Vec<String> =
                        (0..sig.params.len()).map(|k| format!("_{n}_{k}")).collect();
                    let name = |x: &str| j::Expr {
                        kind: K::Name(x.to_owned()),
                        span: at,
                        paren: false,
                    };
                    let args = names
                        .iter()
                        .zip(&sig.params)
                        .map(|(x, p)| j::Arg {
                            name: (!p.positional).then(|| p.name.clone()),
                            kind: j::ArgKind::Expr(name(x)),
                            span: at,
                        })
                        .collect();
                    let body = j::Expr {
                        kind: K::Call(Box::new(name(n)), args),
                        span: at,
                        paren: false,
                    };
                    let ty = Ty::Fn(
                        false,
                        sig.params.iter().map(|p| p.ty.clone()).collect(),
                        Box::new(sig.ret.clone()),
                    );
                    let (r, _) = self.lambda(&names, &body, Some(&ty))?;
                    (r, ty)
                } else if self.gone.contains(n) {
                    return unsupported("a binding used after the block that made it");
                } else if self.ghosts.contains(n) {
                    self.err(
                        "E604",
                        at,
                        format!("`{n}` is ghost; it exists only in proofs"),
                    );
                    (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)
                } else {
                    let names = self.names_in_scope();
                    let names = names.iter().map(String::as_str);
                    self.err_near("E200", at, format!("cannot find `{n}`"), n, names);
                    (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)
                }
            }
            K::Lit(j::Lit::None) => match want {
                Some(t @ Ty::Option(_)) => (Rhs::Atom(Atom::Name("None".into(), at)), t.clone()),
                _ => (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Unit),
            },
            K::Lit(l) => {
                if let j::Lit::Str { value, .. } = l
                    && !unicode_normalization::is_nfc(value)
                {
                    let item = self.item.clone();
                    self.cx.warn(
                        "W840",
                        at,
                        &item,
                        "this string is not in NFC; normalize it (R338)".into(),
                    );
                }
                if let (j::Lit::Int(s), Some(Ty::BigInt)) = (l, want) {
                    return Ok((self.bigint_lit(&int_text(s)?, at), Ty::BigInt));
                }
                let t = match (l, want) {
                    (j::Lit::Int(s), _) => self.int_lit_ty(&int_text(s)?, want, at),
                    (j::Lit::Float(_), Some(w @ Ty::Fixed(n))) if n.starts_with('f') => w.clone(),
                    _ => lit_ty(l),
                };
                (Rhs::Atom(Atom::Lit(literal(l)?, at)), t)
            }
            K::FStr {
                template: false,
                pieces,
            } => self.fstr(pieces, at)?,
            K::Tuple(xs) => {
                let wants: Vec<Option<Ty>> = match want {
                    Some(Ty::Tuple(ts)) if ts.len() == xs.len() => {
                        ts.iter().cloned().map(Some).collect()
                    }
                    _ => vec![None; xs.len()],
                };
                let mut atoms = Vec::new();
                let mut tys = Vec::new();
                for (x, w) in xs.iter().zip(wants) {
                    let (a, t) = self.atom(x, w.as_ref())?;
                    atoms.push(a);
                    tys.push(t);
                }
                (Rhs::Ctor(y::Ctor::Tuple(atoms)), Ty::Tuple(tys))
            }
            K::List(xs) => {
                let mut et = match want {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                let mut atoms = Vec::new();
                for x in xs {
                    let (a, t) = self.atom(x, et.as_ref())?;
                    match &et {
                        Some(w) => self.expect(&t, w, x.span),
                        None => et = Some(t),
                    }
                    atoms.push(a);
                }
                let Some(et) = et else {
                    self.err("E241", at, "annotate the type of this empty list".into());
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
                };
                let lt = Ty::List(Box::new(et.clone()));
                let l = self.fresh("t");
                let new = call("std.list.new", vec![et.targ()], vec![], at);
                self.bind(&l, true, &lt, Rhs::Call(new), at, at);
                for a in atoms {
                    let sp = a.span();
                    let push = call(
                        "std.list.push",
                        vec![et.targ()],
                        vec![
                            Arg::Value(Conv::Inout, Atom::Name(l.clone(), at), at),
                            Arg::Value(Conv::Sink, a, sp),
                        ],
                        sp,
                    );
                    self.emit(StmtKind::Rhs(Rhs::Call(push)), sp);
                }
                (Rhs::Atom(Atom::Name(l, at)), lt)
            }
            K::Dict(kvs) => {
                let (mut kt, mut vt) = match want {
                    Some(Ty::Dict(k, v)) => (Some((**k).clone()), Some((**v).clone())),
                    _ => (None, None),
                };
                let mut atoms = Vec::new();
                for (k, v) in kvs {
                    let (ka, t) = self.atom(k, kt.as_ref())?;
                    match &kt {
                        Some(w) => self.expect(&t, w, k.span),
                        None => kt = Some(t),
                    }
                    let (va, t) = self.atom(v, vt.as_ref())?;
                    match &vt {
                        Some(w) => self.expect(&t, w, v.span),
                        None => vt = Some(t),
                    }
                    atoms.push((ka, va));
                }
                let (Some(kt), Some(vt)) = (kt, vt) else {
                    self.err("E241", at, "annotate the type of this empty dict".into());
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
                };
                let dt = Ty::Dict(Box::new(kt.clone()), Box::new(vt.clone()));
                let d = self.fresh("t");
                let targs = vec![kt.targ(), vt.targ()];
                let new = call("std.dict.new", targs.clone(), vec![], at);
                self.bind(&d, true, &dt, Rhs::Call(new), at, at);
                for (k, v) in atoms {
                    let sp = k.span();
                    let ins = call(
                        "std.dict.insert",
                        targs.clone(),
                        vec![
                            Arg::Value(Conv::Inout, Atom::Name(d.clone(), at), at),
                            Arg::Value(Conv::Sink, k, sp),
                            Arg::Value(Conv::Sink, v, sp),
                        ],
                        sp,
                    );
                    self.emit(StmtKind::Rhs(Rhs::Call(ins)), sp);
                }
                (Rhs::Atom(Atom::Name(d, at)), dt)
            }
            K::Attr(b, m, _)
                if let K::Name(module) = &b.kind
                    && !self.locals.contains_key(module)
                    && let Some(v) = crate::stdlib::konst(&format!("std.{module}.{m}")) =>
            {
                self.rhs(v, want)?
            }
            K::Attr(b, m, _)
                if let K::Name(module) = &b.kind
                    && !self.locals.contains_key(module)
                    && self.std_variant(module, m) =>
            {
                (
                    Rhs::Atom(Atom::Name(m.clone(), at)),
                    Ty::Data(self.g.variant_of[m].clone()),
                )
            }
            K::Attr(..) => {
                let (p, t) = self.place(e)?;
                (Rhs::Proj(p), t)
            }
            K::Index(b, k) => {
                let (p, t) = self.place(b)?;
                match map_kv(&t) {
                    Some((kt, vt)) => {
                        self.sites.insert(e.span, Site::Dict);
                        let d = self.tmp(&t, Rhs::Proj(p), b.span);
                        let (ka, kty) = self.atom(k, Some(&kt))?;
                        self.expect(&kty, &kt, k.span);
                        let path = format!("std.{}.index", t.module().expect("a map"));
                        let c = call(
                            &path,
                            vec![kt.targ(), vt.targ()],
                            vec![let_arg(d), let_arg(ka)],
                            at,
                        );
                        (Rhs::Call(c), vt)
                    }
                    _ => {
                        let (p, t) = self.place(e)?;
                        (Rhs::Proj(p), t)
                    }
                }
            }
            K::Call(f, args) => {
                let (r, t, fail) = self.call_expr(f, args, at, want)?;
                match (fail, r) {
                    (None, r) => (r, t),
                    (Some(fl), Rhs::Call(c)) => match want {
                        Some(w @ Ty::Result(..)) => {
                            let got = Ty::Result(Box::new(t), Box::new(fl));
                            self.expect(&got, w, at);
                            (Rhs::Try(c), got)
                        }
                        _ => {
                            self.err(
                                "E315",
                                at,
                                "the failure of this call is not handled; add `?` or `else`".into(),
                            );
                            (Rhs::Call(c), t)
                        }
                    },
                    (Some(_), r) => (r, t),
                }
            }
            K::Try(x) => {
                let j::ExprKind::Call(f, args) = &x.kind else {
                    return unsupported("`?` on a value that is not a call");
                };
                let (r, t, fail) = self.call_expr(f, args, x.span, None)?;
                match (fail, r) {
                    (Some(e), Rhs::Call(c)) => {
                        if self.sig.fail.is_none() {
                            self.err(
                                "E310",
                                at,
                                format!(
                                    "`?` passes a `{e}` failure up, but `{}` cannot fail; \
                                     declare `fail {e}` on it or handle the failure with `else`",
                                    self.item
                                ),
                            );
                        }
                        // `?` injects any failure into `Error` with no `From` (R432).
                        let into_error = e != Ty::Data("Error".into())
                            && self.sig.fail == Some(Ty::Data("Error".into()))
                            && self.g.std_records.contains("Error");
                        let from =
                            self.sig.fail.as_ref().and_then(|own| {
                                self.g.froms.get(&(e.to_string(), own.to_string()))
                            });
                        let via = match from {
                            Some(f) => Some(vec![f.clone()]),
                            None => into_error
                                .then(|| vec!["std".into(), "error".into(), "wrap".into()]),
                        };
                        (Rhs::Propagate { call: c, via }, t)
                    }
                    _ => return unsupported("`?` on an optional or on a call that cannot fail"),
                }
            }
            K::Unary(op, x) => match (*op, &x.kind) {
                ("-", K::Lit(j::Lit::Int(s))) if want == Some(&Ty::BigInt) => (
                    self.bigint_lit(&format!("-{}", int_text(s)?), at),
                    Ty::BigInt,
                ),
                ("-", K::Lit(j::Lit::Int(s))) => {
                    let text = format!("-{}", int_text(s)?);
                    let t = self.int_lit_ty(&text, want, at);
                    (Rhs::Atom(Atom::Lit(Literal::Int(text), at)), t)
                }
                ("-", K::Lit(j::Lit::Float(s))) => (
                    Rhs::Atom(Atom::Lit(
                        Literal::Float(format!("-{}", s.replace('_', ""))),
                        at,
                    )),
                    Ty::Float,
                ),
                ("+", _) => self.rhs(x, want)?,
                ("-", _) => {
                    let (a, t) = self.atom(x, want)?;
                    let c = match t {
                        Ty::Int => call(
                            "sub_checked_i64",
                            vec![],
                            vec![let_arg(Atom::Lit(Literal::Int("0".into()), at)), let_arg(a)],
                            at,
                        ),
                        Ty::Float => call(
                            "mul_f64",
                            vec![],
                            vec![
                                let_arg(Atom::Lit(Literal::Float("-1.0".into()), at)),
                                let_arg(a),
                            ],
                            at,
                        ),
                        Ty::BigInt => {
                            let z = self.bigint_lit("0", at);
                            let z = self.tmp(&Ty::BigInt, z, at);
                            call("sub_bigint", vec![], vec![let_arg(z), let_arg(a)], at)
                        }
                        Ty::Coll("Duration", _) => {
                            call("std.time.duration.neg", vec![], vec![let_arg(a)], at)
                        }
                        Ty::Err => return Ok((Rhs::Atom(a), Ty::Err)),
                        t => {
                            self.err(
                                "E201",
                                x.span,
                                format!("cannot negate a `{t}`", t = self.ty_text(&t)),
                            );
                            return Ok((Rhs::Atom(a), Ty::Err));
                        }
                    };
                    (Rhs::Call(c), t)
                }
                ("~", _) => {
                    let (a, t) = self.atom(x, want)?;
                    match t {
                        Ty::Int | Ty::BigInt => {}
                        Ty::Fixed(n) if !n.starts_with('f') => {}
                        Ty::Err => return Ok((Rhs::Atom(a), Ty::Err)),
                        t => {
                            self.err("E201", x.span, format!("`~` on a `{t}`"));
                            return Ok((Rhs::Atom(a), Ty::Err));
                        }
                    }
                    let p = format!("not_{}", t.scalar().expect("an integer"));
                    (Rhs::Call(call(&p, vec![], vec![let_arg(a)], at)), t)
                }
                _ => return unsupported(format!("unary `{op}`")),
            },
            K::Not(x) => {
                let a = self.cond(x)?;
                (
                    Rhs::Call(call("not_bool", vec![], vec![let_arg(a)], at)),
                    Ty::Bool,
                )
            }
            K::Binary(op @ ("and" | "or"), a, b) => {
                let x = self.cond(a)?;
                let op = *op;
                let narrow = self.narrowings(a, op == "and");
                let (v, t) = self.join(at, |l, k| {
                    let rest = l.block_with(|l| {
                        for (n, inner) in narrow {
                            l.narrow_to(&n, inner, a.span);
                        }
                        let c = l.cond(b)?;
                        l.jump(k, c, b.span);
                        Ok(())
                    })?;
                    let short = y::Block {
                        stmts: vec![y::Stmt {
                            kind: StmtKind::Jump {
                                name: k.to_owned(),
                                args: vec![Atom::Lit(Literal::Bool(op == "or"), at)],
                            },
                            span: at,
                        }],
                    };
                    let (then, els) = if op == "and" {
                        (rest, short)
                    } else {
                        (short, rest)
                    };
                    Ok((
                        y::Stmt {
                            kind: StmtKind::If {
                                cond: x,
                                then,
                                els: Some(els),
                            },
                            span: at,
                        },
                        Ty::Bool,
                    ))
                })?;
                (Rhs::Atom(v), t)
            }
            K::Binary(op, a, b) => self.arith(op, a, b, at, want)?,
            K::Compare(op, a, b) => self.compare(op, a, b, at)?,
            K::Cond { then, cond, els } => {
                let c = self.cond(cond)?;
                let (v, t) = self.join(at, |l, k| {
                    let (tb, tt) = l.arm(k, then, want)?;
                    let (eb, _) = l.arm(k, els, Some(want.unwrap_or(&tt)))?;
                    Ok((
                        y::Stmt {
                            kind: StmtKind::If {
                                cond: c,
                                then: tb,
                                els: Some(eb),
                            },
                            span: at,
                        },
                        want.cloned().unwrap_or(tt),
                    ))
                })?;
                (Rhs::Atom(v), t)
            }
            K::Fallback(x, fb) => self.fallback(x, fb, at, want)?,
            K::FStr { .. } => return unsupported("t-strings"),
            K::Hole => return unsupported("typed holes"),
            K::Comp { .. } => self.comprehension(e)?,
            K::Slice(b, bounds, _) => {
                let (xs, t) = self.atom(b, None)?;
                let et = match &t {
                    Ty::List(e) => (**e).clone(),
                    Ty::Err => return Ok((Rhs::Atom(xs), Ty::Err)),
                    Ty::Str if self.cx.strict => {
                        let s = self.cx.text(b.span);
                        self.err("E504", at, format!("a slice of a `str`; use `{s}.chars()`"));
                        return Ok((Rhs::Atom(xs), Ty::Err));
                    }
                    Ty::Str => return unsupported("slices of a `str`"),
                    t => {
                        self.err(
                            "E201",
                            b.span,
                            format!("cannot slice a `{t}`", t = self.ty_text(t)),
                        );
                        return Ok((Rhs::Atom(xs), Ty::Err));
                    }
                };
                let opt = Ty::Option(Box::new(Ty::Int));
                let mut args = vec![let_arg(xs)];
                for x in bounds {
                    let a = match x {
                        Some(x) => {
                            let (a, xt) = self.atom(x, Some(&opt))?;
                            self.expect(&xt, &opt, x.span);
                            a
                        }
                        None => Atom::Name("None".into(), at),
                    };
                    args.push(let_arg(a));
                }
                let c = call("std.list.slice_clamped", vec![et.targ()], args, at);
                (Rhs::Call(c), t)
            }
            K::Lambda(ps, body) => self.lambda(ps, body, want)?,
            K::Cast(x, j::Type::Named { path, args })
                if args.is_empty()
                    && matches!(path.as_slice(), [n] if n == "int" || n == "float") =>
            {
                let arg = j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr((**x).clone()),
                    span: x.span,
                };
                return self.builtin(&path[0], &[arg], at, want);
            }
            K::Cast(..) => return unsupported("`as` to a fixed-width type"),
            K::Is { .. } => return unsupported("`is` with a pattern"),
            // Wen's `try f(x)`: the call's failure as a `Result` value, as Xin reifies a failing
            // call bound at a `Result` type.
            K::Reify(x) => {
                let j::ExprKind::Call(f, args) = &x.kind else {
                    return unsupported("`try` of anything but a call");
                };
                let want = match want {
                    Some(Ty::Result(ok, _)) => Some(ok.as_ref().clone()),
                    _ => None,
                };
                match self.call_expr(f, args, x.span, want.as_ref())? {
                    (Rhs::Call(c), t, Some(fl)) => {
                        (Rhs::Try(c), Ty::Result(Box::new(t), Box::new(fl)))
                    }
                    (r, t, None) => {
                        self.err(
                            "E201",
                            at,
                            format!(
                                "`try` of a call that cannot fail; it returns `{t}`",
                                t = self.ty_text(&t)
                            ),
                        );
                        (r, t)
                    }
                    _ => return unsupported("`try` of this call"),
                }
            }
            K::Set(_) => return unsupported("set literals"),
            K::Wild => return unsupported("`_` as a value"),
        })
    }

    /// A lambda becomes a closure; its parameter types come from the expected callable type.
    /// Locals capture by copy, `var` parameters by reference and capabilities by name (4.6).
    fn lambda(&mut self, names: &[String], body: &j::Expr, want: Option<&Ty>) -> R<(Rhs, Ty)> {
        // An optional callable parameter (`key: ... | None`) takes a lambda as its callable.
        let want = match want {
            Some(Ty::Option(f)) => Some(&**f),
            w => w,
        };
        let (par, ps, ret) = match want {
            Some(Ty::Fn(par, ps, ret)) if ps.len() == names.len() && !ps.iter().any(has_param) => {
                (*par, ps.clone(), (!has_param(ret)).then(|| (**ret).clone()))
            }
            _ => return unsupported("a lambda without a known callable type"),
        };
        let mut captures: Vec<(Conv, String, Span)> = Vec::new();
        walk(body, &mut |e| {
            let j::ExprKind::Name(n) = &e.kind else {
                return;
            };
            if names.contains(n) || captures.iter().any(|(_, m, _)| m == n) {
                return;
            }
            let inout = self
                .sig
                .params
                .iter()
                .any(|p| p.name == *n && p.conv == Conv::Inout);
            if inout {
                captures.push((Conv::Inout, n.clone(), e.span));
            } else if self.locals.contains_key(n)
                || self.sig.caps.contains(n)
                || self.handlers.contains(n)
            {
                captures.push((Conv::Let, n.clone(), e.span));
            }
        });
        let mut rt = Ty::Err;
        let block = self.block_with(|l| {
            for (n, t) in names.iter().zip(&ps) {
                l.declare(n, t.clone());
            }
            let (a, t) = l.atom(body, ret.as_ref())?;
            if let Some(r) = &ret {
                l.expect(&t, r, body.span);
            }
            rt = ret.clone().unwrap_or(t);
            l.emit(StmtKind::Ret(a), body.span);
            Ok(())
        })?;
        let params = names
            .iter()
            .zip(&ps)
            .map(|(n, t)| y::Param::Value {
                comptime: false,
                conv: Conv::Let,
                name: n.clone(),
                ty: t.core(),
            })
            .collect();
        let clo = y::Closure {
            captures,
            params,
            ret: rt.core(),
            uses: None,
            fail: None,
            body: block,
        };
        Ok((Rhs::Closure(Box::new(clo)), Ty::Fn(par, ps, Box::new(rt))))
    }

    fn fstr(&mut self, pieces: &[j::FPiece], at: Span) -> R<(Rhs, Ty)> {
        let mut parts = Vec::new();
        for p in pieces {
            match p {
                j::FPiece::Lit(s) if s.is_empty() => {}
                j::FPiece::Lit(s) => parts.push(Atom::Lit(Literal::Str(s.clone()), at)),
                j::FPiece::Expr(_, Some(_)) => return unsupported("format specifications"),
                j::FPiece::Expr(e, None) => {
                    let (a, t) = self.atom(e, None)?;
                    parts.push(self.show(a, &t, e.span));
                }
            }
        }
        let mut it = parts.into_iter();
        let Some(mut acc) = it.next() else {
            return Ok((
                Rhs::Atom(Atom::Lit(Literal::Str(String::new()), at)),
                Ty::Str,
            ));
        };
        for x in it {
            let c = call(
                "std.text.concat",
                vec![],
                vec![let_arg(acc), let_arg(x)],
                at,
            );
            acc = self.tmp(&Ty::Str, Rhs::Call(c), at);
        }
        Ok((Rhs::Atom(acc), Ty::Str))
    }

    fn show(&mut self, a: Atom, t: &Ty, at: Span) -> Atom {
        if matches!(t, Ty::Str) {
            return a;
        }
        let c = call("std.fmt.show", vec![t.targ()], vec![let_arg(a)], at);
        self.tmp(&Ty::Str, Rhs::Call(c), at)
    }

    fn fallback(
        &mut self,
        x: &j::Expr,
        fb: &j::Fallback,
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        // The scrutinee, the success type, and the failure type (`None` for an optional).
        let (s, ok_ty, err_ty) = match &x.kind {
            j::ExprKind::Call(f, args) => {
                let (r, t, fail) = self.call_expr(f, args, x.span, None)?;
                match (fail, r) {
                    (Some(e), Rhs::Call(c)) => {
                        let rt = Ty::Result(Box::new(t.clone()), Box::new(e.clone()));
                        (self.tmp(&rt, Rhs::Try(c), x.span), t, Some(e))
                    }
                    (_, r) => match t {
                        Ty::Option(inner) => (
                            self.tmp(&Ty::Option(inner.clone()), r, x.span),
                            *inner,
                            None,
                        ),
                        t => {
                            self.err(
                                "E201",
                                x.span,
                                format!(
                                    "`else` needs an optional or a fallible call, found `{t}`",
                                    t = self.ty_text(&t)
                                ),
                            );
                            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
                        }
                    },
                }
            }
            _ => {
                let (a, t) = self.atom(x, None)?;
                match t {
                    Ty::Option(inner) => (a, *inner, None),
                    Ty::Err => return Ok((Rhs::Atom(a), Ty::Err)),
                    t => {
                        self.err(
                            "E201",
                            x.span,
                            format!(
                                "`else` needs an optional or a fallible call, found `{t}`",
                                t = self.ty_text(&t)
                            ),
                        );
                        return Ok((Rhs::Atom(a), Ty::Err));
                    }
                }
            }
        };
        let want = want.cloned().unwrap_or_else(|| ok_ty.clone());
        let fb = fb.clone();
        let (v, t) = self.join(at, |l, k| {
            let ok = l.fresh("o");
            let ok_pat = y::Pat::Ctor {
                path: vec![if err_ty.is_some() { "Ok" } else { "Some" }.into()],
                fields: Some((vec![("value".into(), y::Pat::Bind(ok.clone()))], false)),
            };
            let ok_block = l.block_with(|l| {
                l.declare(&ok, ok_ty.clone());
                l.expect(&ok_ty, &want, x.span);
                l.jump(k, Atom::Name(ok.clone(), at), at);
                Ok(())
            })?;
            let (err_name, err_pat) = match (&fb, &err_ty) {
                (j::Fallback::Bind(n, _) | j::Fallback::Block(Some(n), _), Some(_)) => (
                    Some(n.clone()),
                    y::Pat::Ctor {
                        path: vec!["Err".into()],
                        fields: Some((vec![("error".into(), y::Pat::Bind(n.clone()))], false)),
                    },
                ),
                (_, Some(_)) => (
                    None,
                    y::Pat::Ctor {
                        path: vec!["Err".into()],
                        fields: Some((vec![("error".into(), y::Pat::Wild)], false)),
                    },
                ),
                (_, None) => (None, y::Pat::Bind("None".into())),
            };
            let err_block = l.block_with(|l| {
                if let (Some(n), Some(e)) = (&err_name, &err_ty) {
                    l.declare(n, e.clone());
                }
                match &fb {
                    j::Fallback::Value(h) | j::Fallback::Bind(_, h) => {
                        let (a, t) = l.atom(h, Some(&want))?;
                        l.expect(&t, &want, h.span);
                        l.jump(k, a, h.span);
                    }
                    j::Fallback::Return(e) => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Return(e.clone()),
                        span: at,
                    })?,
                    j::Fallback::Fail(e) => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Fail(e.clone()),
                        span: at,
                    })?,
                    j::Fallback::Break => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Break,
                        span: at,
                    })?,
                    j::Fallback::Continue => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Continue,
                        span: at,
                    })?,
                    j::Fallback::Block(_, stmts) => {
                        let exits = matches!(
                            stmts.last().map(|s| &s.kind),
                            Some(
                                j::StmtKind::Return(_)
                                    | j::StmtKind::Fail(_)
                                    | j::StmtKind::Become(_)
                                    | j::StmtKind::Unreachable
                                    | j::StmtKind::Break
                                    | j::StmtKind::Continue
                                    | j::StmtKind::BreakTo(_)
                                    | j::StmtKind::ContinueTo(_)
                            )
                        );
                        if !exits {
                            return unsupported("a fallback block that does not exit");
                        }
                        l.stmts(stmts)?;
                    }
                }
                Ok(())
            })?;
            Ok((
                y::Stmt {
                    kind: StmtKind::Match {
                        scrutinee: s,
                        arms: vec![(ok_pat, ok_block), (err_pat, err_block)],
                    },
                    span: at,
                },
                want.clone(),
            ))
        })?;
        Ok((Rhs::Atom(v), t))
    }

    fn arith(
        &mut self,
        op: &str,
        a: &j::Expr,
        b: &j::Expr,
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        let (x, tx) = self.atom(a, want.filter(|w| !matches!(w, Ty::Float) || op != "/"))?;
        if matches!(op, "<<" | ">>") && is_integer(&tx) {
            // The amount is any integer type; a `bigint` shifts by an `int`.
            let hint = if tx == Ty::BigInt {
                Ty::Int
            } else {
                tx.clone()
            };
            let (n, nt) = self.atom(b, Some(&hint))?;
            if nt == Ty::Err {
                return Ok((Rhs::Atom(x), Ty::Err));
            }
            if !is_integer(&nt) || nt == Ty::BigInt {
                self.err(
                    "E201",
                    b.span,
                    format!("a shift amount must be an integer, not `{nt}`"),
                );
                return Ok((Rhs::Atom(x), Ty::Err));
            }
            let f = if op == "<<" { "shl" } else { "shr" };
            let p = format!("{f}_{}", tx.scalar().expect("an integer"));
            return Ok((
                Rhs::Call(call(&p, vec![], vec![let_arg(x), let_arg(n)], at)),
                tx,
            ));
        }
        let (y_, ty) = self.atom(b, Some(&tx))?;
        if op == "+" && matches!(tx, Ty::Str | Ty::List(_)) && self.not_last_use(a, at) {
            let item = self.item.clone();
            self.cx.warn(
                "W822",
                at,
                &item,
                "`+` in a loop copies its left operand, which is not at its last use (R388)".into(),
            );
        }
        let args = vec![let_arg(x.clone()), let_arg(y_)];
        let (path, rt, targs) = match (&tx, &ty) {
            (Ty::Err, _) | (_, Ty::Err) => return Ok((Rhs::Atom(x), Ty::Err)),
            (Ty::Int, Ty::Int) => match int_op(op) {
                Some((p, t)) => (p.to_owned(), t, vec![]),
                None => return unsupported(format!("`{op}` on `int`")),
            },
            (Ty::Float, Ty::Float) => match float_op(op) {
                Some(p) => (p.to_owned(), Ty::Float, vec![]),
                None => return unsupported(format!("`{op}` on `float`")),
            },
            (Ty::BigInt, Ty::BigInt) => match bigint_op(op) {
                Some(p) => (p.to_owned(), Ty::BigInt, vec![]),
                None => return unsupported(format!("`{op}` on `bigint`")),
            },
            (Ty::Fixed(n), Ty::Fixed(m)) if n == m && op != "/" => {
                let p = if n.starts_with('f') {
                    float_op(op).map(|p| p.replace("f64", n))
                } else {
                    int_op(op).map(|(p, _)| p.replace("i64", n))
                };
                match p {
                    Some(p) => (p, tx.clone(), vec![]),
                    None => {
                        return unsupported(format!("`{op}` on `{tx}`", tx = self.ty_text(&tx)));
                    }
                }
            }
            (Ty::Coll(c, _), _) if let Some((p, rt)) = time_op(c, &ty, op) => (p, rt, vec![]),
            (Ty::Data(z), Ty::Coll(c @ ("Duration" | "Period"), _))
                if z == "ZonedDateTime" && (op == "+" || (op == "-" && *c == "Duration")) =>
            {
                let f = match (*c, op) {
                    ("Period", _) => "zoned_add_period",
                    (_, "+") => "zoned_add",
                    _ => "zoned_sub",
                };
                (format!("std.time.{f}"), tx.clone(), vec![])
            }
            (Ty::Str, Ty::Str) if op == "+" => ("std.text.concat".to_owned(), Ty::Str, vec![]),
            (Ty::List(e), Ty::List(_)) if op == "+" => {
                ("std.list.concat".to_owned(), tx.clone(), vec![e.targ()])
            }
            (Ty::Option(_), _) | (_, Ty::Option(_)) => {
                let t = if matches!(tx, Ty::Option(_)) {
                    &tx
                } else {
                    &ty
                };
                self.err(
                    "E201",
                    at,
                    format!("`{op}` on a `{t}`, which may be `None`; unwrap it first with `else`"),
                );
                return Ok((Rhs::Atom(x), Ty::Err));
            }
            _ => {
                self.err(
                    "E201",
                    at,
                    format!(
                        "`{op}` between `{tx}` and `{ty}`",
                        ty = self.ty_text(&ty),
                        tx = self.ty_text(&tx)
                    ),
                );
                return Ok((Rhs::Atom(x), Ty::Err));
            }
        };
        Ok((Rhs::Call(call(&path, targs, args, at)), rt))
    }

    /// Lowering `value` with no expected type gives `t`: a literal of its default type, a local,
    /// or a call of a non-generic function of this module.
    fn inferred_alone(&self, value: &j::Expr, t: &Ty) -> bool {
        match &value.kind {
            j::ExprKind::Lit(l) => !matches!(l, j::Lit::None) && lit_ty(l) == *t,
            j::ExprKind::Name(n) => self.locals.get(n).is_some_and(|l| l.ty == *t),
            j::ExprKind::Call(f, _) => matches!(&f.kind, j::ExprKind::Name(n)
                if self.g.fns.get(n).is_some_and(|s| s.tparams.is_empty() && s.ret == *t)),
            _ => false,
        }
    }

    /// The type after `name:` on the line of `name`.
    fn annotation_span(&self, name: Span) -> Span {
        let line = self
            .cx
            .src
            .lines()
            .nth(name.line as usize - 1)
            .unwrap_or("");
        let from = (name.end_col as usize).saturating_sub(1).min(line.len());
        let rest = &line[from..];
        let skip = rest.find(':').map_or(0, |k| {
            k + 1 + rest[k + 1..].len() - rest[k + 1..].trim_start().len()
        });
        let col = (from + skip + 1) as u32;
        let end = line[from + skip..]
            .find(" =")
            .map_or(col, |k| col + k as u32);
        Span {
            col,
            end_col: end,
            end_line: name.line,
            ..name
        }
    }

    /// `a` names a local the loop reads again: it is bound outside the loop and this statement
    /// does not reassign it, or it is read later.
    fn not_last_use(&self, a: &j::Expr, at: Span) -> bool {
        let (j::ExprKind::Name(n), Some(outer)) = (&a.kind, self.loop_outer.last()) else {
            return false;
        };
        self.locals.contains_key(n)
            && self.target.as_ref() != Some(n)
            && (outer.contains(n) || self.used_after(n, at))
    }

    /// The side of `f(x) == Ok(v)` that is not the constructor: a fallible call is its
    /// `Result`, so the constructor gets an expected type (R309 `ex` lines compare this way).
    fn operand(&mut self, e: &j::Expr) -> R<(Atom, Ty)> {
        let j::ExprKind::Call(f, args) = &e.kind else {
            return self.atom(e, None);
        };
        let (r, t, fail) = self.call_expr(f, args, e.span, None)?;
        let (r, t) = match (fail, r) {
            (Some(fl), Rhs::Call(c)) => (Rhs::Try(c), Ty::Result(Box::new(t), Box::new(fl))),
            (_, r) => (r, t),
        };
        Ok((self.tmp(&t, r, e.span), t))
    }

    fn compare(&mut self, op: &str, a: &j::Expr, b: &j::Expr, at: Span) -> R<(Rhs, Ty)> {
        let none = |e: &j::Expr| matches!(e.kind, j::ExprKind::Lit(j::Lit::None));
        if matches!(op, "is" | "is not") {
            if !none(b) {
                return unsupported("`is` with other than `None`");
            }
            let (x, t) = self.atom(a, None)?;
            let present = match t {
                Ty::Option(_) => self.is_some(x, at)?.0,
                Ty::Err => return Ok((Rhs::Atom(x), Ty::Bool)),
                Ty::Unit => {
                    self.err(
                        "E201",
                        a.span,
                        "this is always `None`; annotate the binding with its type, as \
                         `x: int | None = None`"
                            .into(),
                    );
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
                }
                t => {
                    self.err(
                        "E201",
                        a.span,
                        format!("a `{t}` is never `None`", t = self.ty_text(&t)),
                    );
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
                }
            };
            if op == "is not" {
                return Ok((Rhs::Atom(present), Ty::Bool));
            }
            let c = call("not_bool", vec![], vec![let_arg(present)], at);
            return Ok((Rhs::Call(c), Ty::Bool));
        }
        if matches!(op, "in" | "not in") {
            let (x, _) = self.atom(a, None)?;
            let (c, ct) = self.atom(b, None)?;
            let (path, targs) = match &ct {
                Ty::List(e) => ("std.list.contains".to_owned(), vec![e.targ()]),
                Ty::Set(e) => ("std.set.contains".to_owned(), vec![e.targ()]),
                Ty::Dict(k, v) => ("std.dict.contains_key".to_owned(), vec![k.targ(), v.targ()]),
                Ty::Coll(n, _) if *n != "heap" => {
                    let m = if *n == "sorted_dict" {
                        "contains_key"
                    } else {
                        "contains"
                    };
                    let module = ct.module().expect("a collection");
                    (format!("std.{module}.{m}"), coll_targs(&ct))
                }
                Ty::Str => ("std.text.contains".to_owned(), vec![]),
                Ty::Err => return Ok((Rhs::Atom(x), Ty::Err)),
                t => {
                    self.err(
                        "E201",
                        b.span,
                        format!("`in` on a `{t}`", t = self.ty_text(t)),
                    );
                    return Ok((Rhs::Atom(x), Ty::Err));
                }
            };
            let r = Rhs::Call(call(&path, targs, vec![let_arg(c), let_arg(x)], at));
            if op == "in" {
                return Ok((r, Ty::Bool));
            }
            let t = self.tmp(&Ty::Bool, r, at);
            return Ok((
                Rhs::Call(call("not_bool", vec![], vec![let_arg(t)], at)),
                Ty::Bool,
            ));
        }
        let Some(name) = cmp_name(op) else {
            return unsupported(format!("`{op}`"));
        };
        let (x, tx, y_, ty) = if is_outcome(a) && !is_outcome(b) {
            let (y_, ty) = self.operand(b)?;
            let (x, tx) = self.atom(a, Some(&ty))?;
            (x, tx, y_, ty)
        } else {
            let (x, tx) = if is_outcome(b) {
                self.operand(a)?
            } else {
                self.atom(a, None)?
            };
            let (y_, ty) = self.atom(b, Some(&tx))?;
            (x, tx, y_, ty)
        };
        if matches!(tx, Ty::Err) || matches!(ty, Ty::Err) {
            return Ok((Rhs::Atom(x), Ty::Bool));
        }
        if !ty.fits(&tx) && !tx.fits(&ty) {
            self.err(
                "E201",
                b.span,
                format!(
                    "cannot compare `{tx}` with `{ty}`",
                    ty = self.ty_text(&ty),
                    tx = self.ty_text(&tx)
                ),
            );
            return Ok((Rhs::Atom(x), Ty::Bool));
        }
        if matches!(name, "lt" | "le" | "gt" | "ge") && !self.implements(&tx, "Ord") {
            self.err(
                "E221",
                at,
                format!("`{tx}` does not implement `Ord`", tx = self.ty_text(&tx)),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
        }
        let args = vec![let_arg(x), let_arg(y_)];
        let c = match tx.scalar() {
            Some("bool") if !matches!(name, "eq" | "ne") => {
                self.err("E201", at, format!("`{op}` on `bool`"));
                return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
            }
            Some(s) => call(&format!("{name}_{s}"), vec![], args, at),
            None => call(&format!("std.cmp.{name}"), vec![tx.targ()], args, at),
        };
        Ok((Rhs::Call(c), Ty::Bool))
    }
}

/// The range of the Yong integer type `n`.
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

impl Lower<'_, '_, '_> {
    /// Lower a call. Returns the right-hand side, its success type and its failure type.
    fn call_expr(
        &mut self,
        f: &j::Expr,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        self.want_ret = want.map(|w| (w.clone(), at));
        match &f.kind {
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cli" && !self.locals.contains_key(c))
                    && matches!(op.as_str(), "parse" | "parse_args") =>
            {
                self.cli_call(op, k, args, at)
            }
            j::ExprKind::Index(b, k) if matches!(&b.kind, j::ExprKind::Name(n) if self.g.templates.contains_key(n)) =>
            {
                let j::ExprKind::Name(n) = &b.kind else {
                    unreachable!("matched")
                };
                let Some(v) = fold(k, &self.stage) else {
                    return unsupported("a stage-0 argument that is not a constant");
                };
                let (name, sig) = self.instance(n, v, at)?;
                self.call_sig(&name, &sig, None, args, at)
            }
            j::ExprKind::Name(n)
                if !self.locals.contains_key(n) && self.pending.contains_key(n) =>
            {
                let mut vals = Vec::new();
                let mut tys = Vec::new();
                for a in args {
                    let (j::ArgKind::Expr(e), None) = (&a.kind, &a.name) else {
                        return unsupported("keyword, `&` or `become` arguments to a lambda");
                    };
                    let (x, t) = self.atom(e, None)?;
                    vals.push(let_arg(x));
                    tys.push(t);
                }
                let (target, value) = self.pending.remove(n).expect("checked");
                // An unbound parameter as the return type: the lambda's body decides it.
                let want = Ty::Fn(false, tys, Box::new(Ty::Param(String::new())));
                let (r, t) = self.rhs(&value, Some(&want))?;
                let Ty::Fn(_, _, ret) = t.clone() else {
                    unreachable!("a lambda")
                };
                self.store(&target, None, r, t, value.span)?;
                self.fn_calls.push((n.clone(), f.span));
                Ok((Rhs::Call(call(n, vec![], vals, at)), *ret, None))
            }
            j::ExprKind::Name(n)
                if matches!(self.locals.get(n), Some(Local { ty: Ty::Fn(..) })) =>
            {
                let t = self.locals[n].ty.clone();
                self.call_local(n, t, f.span, args, at)
            }
            j::ExprKind::Name(n) if !self.locals.contains_key(n) => {
                if self.g.fns.contains_key(n) {
                    return self.user_call(n, None, args, at);
                }
                if let Some(fs) = self.g.records.get(n) {
                    let ps = field_params(fs, at);
                    let r = self.ctor(n, &ps, args, at)?;
                    return Ok((r, Ty::Data(n.clone()), None));
                }
                if let Some(s) = self.g.variant_of.get(n).cloned() {
                    let fs = &self.g.sums[&s]
                        .iter()
                        .find(|(v, _)| v == n)
                        .expect("declared")
                        .1;
                    let ps = field_params(fs, at);
                    if ps.is_empty() && args.is_empty() {
                        return Ok((Rhs::Atom(Atom::Name(n.clone(), at)), Ty::Data(s), None));
                    }
                    let r = self.ctor(n, &ps, args, at)?;
                    return Ok((r, Ty::Data(s), None));
                }
                let (r, t) = self.builtin(n, args, at, want)?;
                Ok((r, t, None))
            }
            j::ExprKind::Attr(b, m, name_at) => {
                if let j::ExprKind::Name(c) = &b.kind
                    && !self.locals.contains_key(c)
                    && (self.sig.caps.contains(c) || self.handlers.contains(c) || c == "io")
                {
                    return self.cap_call(c, m, args, at);
                }
                if let j::ExprKind::Name(module) = &b.kind
                    && !self.locals.contains_key(module)
                    && self.std_variant(module, m)
                {
                    let f = j::Expr {
                        kind: j::ExprKind::Name(m.clone()),
                        span: *name_at,
                        paren: false,
                    };
                    return self.call_expr(&f, args, at, want);
                }
                // `bytes.from_list` and `bytearray.from_bytes` name their type's declaring module.
                if let j::ExprKind::Name(module) = &b.kind
                    && !self.locals.contains_key(module)
                    && let module = match module.as_str() {
                        "bytes" => "encoding",
                        "bytearray" => "encoding.bytearray",
                        "Duration" => "time.duration.of",
                        "Instant" => "time.instant",
                        "PlainDate" => "time.date",
                        "PlainTime" => "time.plain_time",
                        "PlainDateTime" => "time.plain_date_time",
                        "Period" => "time.period",
                        "TimeZone" => "time.zone",
                        "ZonedDateTime" => "time",
                        m => m,
                    }
                    && let Some(d) = crate::stdlib::decl(&format!("std.{module}.{m}"))
                    && !d.cap
                {
                    return self.module_call(module, m, args, at);
                }
                if let (j::ExprKind::Call(f, fargs), "context") = (&b.kind, m.as_str()) {
                    return self.context(f, fargs, b.span, args, at);
                }
                self.method(b, m, *name_at, args, at)
            }
            _ => {
                let (a, t) = self.atom(f, None)?;
                match (a, &t) {
                    (Atom::Name(n, _), Ty::Fn(..)) => {
                        self.call_local(&n, t.clone(), f.span, args, at)
                    }
                    _ => unsupported("calling a value"),
                }
            }
        }
    }

    fn user_call(
        &mut self,
        name: &str,
        recv: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        if self.g.deprecated.contains(name) {
            let item = self.item.clone();
            self.cx
                .warn("W815", at, &item, format!("`{name}` is deprecated"));
        }
        let Some(sig) = self.g.fns.get(name) else {
            return unsupported(format!("`{name}`"));
        };
        if let Some(&(safe, variadic)) = self.g.externs.get(name) {
            if !safe && !self.in_unsafe {
                self.err(
                    "E903",
                    at,
                    format!("`{name}` is an unsafe extern function; call it inside `unsafe`"),
                );
            }
            if variadic {
                return self.variadic_call(name, sig, args, at);
            }
        }
        // A prelude intrinsic typed here is still called by its std path.
        let path = format!("std.prelude.{name}");
        let name = match crate::stdlib::decl(&path) {
            Some(d) if d.intrinsic => &path,
            _ => name,
        };
        self.call_sig(name, sig, recv, args, at)
    }

    /// E542: a literal `strftime` or `strptime` pattern may only use the directives its civil
    /// type has fields for (14.2).
    fn pattern_check(&mut self, ty: &str, fmt: Option<&j::Arg>) {
        let has = match ty {
            "PlainDate" => DATE,
            "PlainTime" => TIME,
            "PlainDateTime" => DATE | TIME,
            _ => return,
        };
        let Some(j::Arg {
            kind: j::ArgKind::Expr(e),
            ..
        }) = fmt
        else {
            return;
        };
        let j::ExprKind::Lit(j::Lit::Str { prefix, value }) = &e.kind else {
            return;
        };
        if !matches!(prefix.as_str(), "" | "r") {
            return;
        }
        let msg = match directives(value) {
            Err(d) => format!("`{d}` is not a format directive"),
            Ok(ds) => match ds.into_iter().find(|(_, need)| need & !has != 0) {
                Some((d, _)) => format!("`{d}` does not apply to a `{ty}`"),
                None => return,
            },
        };
        self.err("E542", e.span, msg);
    }

    /// E540: `PlainDate` and `PlainTime` called with integer literals only must name a real
    /// date or time (14.2).
    fn civil_literal(&mut self, name: &str, args: &[j::Arg], at: Span) {
        let (params, mut vals): (&[&str], Vec<Option<i128>>) = match name {
            "PlainDate" => (&["y", "m", "d"], vec![None; 3]),
            "PlainTime" => (&["h", "mi", "s", "ns"], vec![None, None, Some(0), Some(0)]),
            _ => return,
        };
        for (i, a) in args.iter().enumerate() {
            let j::ArgKind::Expr(e) = &a.kind else { return };
            let j::ExprKind::Lit(j::Lit::Int(t)) = &e.kind else {
                return;
            };
            let Some(v) = int_text(t).ok().and_then(|s| s.parse().ok()) else {
                return;
            };
            let k = match &a.name {
                None => i,
                Some(n) => match params.iter().position(|p| p == n) {
                    Some(k) => k,
                    None => return,
                },
            };
            let Some(slot) = vals.get_mut(k) else { return };
            *slot = Some(v);
        }
        let Some(v) = vals.into_iter().collect::<Option<Vec<i128>>>() else {
            return;
        };
        let ok = match name {
            "PlainDate" => {
                let leap = v[0] % 4 == 0 && (v[0] % 100 != 0 || v[0] % 400 == 0);
                let dim = match v[1] {
                    2 if leap => 29,
                    2 => 28,
                    4 | 6 | 9 | 11 => 30,
                    _ => 31,
                };
                (-9999..=9999).contains(&v[0])
                    && (1..=12).contains(&v[1])
                    && (1..=dim).contains(&v[2])
            }
            _ => {
                (0..24).contains(&v[0])
                    && (0..60).contains(&v[1])
                    && (0..60).contains(&v[2])
                    && (0..1_000_000_000).contains(&v[3])
            }
        };
        if !ok {
            let shown: Vec<String> = v.iter().map(i128::to_string).collect();
            let what = if name == "PlainDate" { "date" } else { "time" };
            self.err(
                "E540",
                at,
                format!("`{name}({})` is not a valid {what}", shown.join(", ")),
            );
        }
    }

    /// The instance of template `n` for stage-0 argument `v`, lowered on first use (10.4).
    /// Its errors are E272, noted with the instantiation site.
    fn instance(&mut self, n: &str, v: Sv, at: Span) -> R<(String, Sig)> {
        let shown = match v {
            Sv::Int(i) => i.to_string(),
            Sv::Bool(b) => b.to_string(),
        };
        let name = format!("{n}__{}", shown.replace('-', "m"));
        if let Some(s) = self.g.instances.borrow().get(&name) {
            return Ok((name, s.clone()));
        }
        let (def, param) = self.g.templates[n].clone();
        let mut f = def.clone();
        f.name = name.clone();
        f.tparams.retain(|t| !matches!(t, j::TParam::Value { .. }));
        let sig = crate::sig(self.g, &f, self.cx)?;
        self.g
            .instances
            .borrow_mut()
            .insert(name.clone(), sig.clone());
        let start = self.cx.diags.len();
        let fd = function_body(self.g, &f, &sig, false, vec![(param, v)], self.cx)?;
        let site = format!("in `{n}[{shown}]`, instantiated at {}:{}", at.line, at.col);
        for d in &mut self.cx.diags[start..] {
            if d.severity == yong_diag::Severity::Error && d.code != "E270" {
                d.code = "E272".into();
            }
            d.item = n.to_owned();
            d.notes.push(site.clone());
        }
        self.g.generated.borrow_mut().push(y::Item {
            attrs: Vec::new(),
            kind: y::ItemKind::Fn(fd),
            span: f.name_span,
        });
        Ok((name, sig))
    }

    /// A call of a C variadic: the arguments past the declared ones get the default argument
    /// promotions, so none may be a type that promotes (12.3, E905).
    fn variadic_call(
        &mut self,
        name: &str,
        sig: &Sig,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let n = sig.params.len().min(args.len());
        let (rhs, t, fail) = self.call_sig(name, sig, None, &args[..n], at)?;
        let Rhs::Call(mut c) = rhs else {
            return Ok((rhs, t, fail));
        };
        for a in &args[n..] {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported("`&` arguments to a C variadic");
            };
            let (v, vt) = self.atom(e, None)?;
            if matches!(
                vt,
                Ty::Fixed("f32" | "i8" | "i16" | "u8" | "u16") | Ty::Bool
            ) {
                let to = if vt == Ty::Fixed("f32") {
                    "f64"
                } else {
                    "c_int"
                };
                self.err(
                    "E905",
                    e.span,
                    format!(
                        "a `{vt}` passed to a C variadic is promoted; convert it to `{to}`",
                        vt = self.ty_text(&vt)
                    ),
                );
            }
            c.args.push(let_arg(v));
        }
        Ok((Rhs::Call(c), t, fail))
    }

    /// `asm(...)` (12.9): operands are scalars or `Vec` (E910) in registers of the target (E911).
    fn asm(&mut self, template: &str, args: &[j::AsmArg], at: Span) -> R<()> {
        let mut operands = Vec::new();
        for a in args {
            operands.push(match a {
                j::AsmArg::Operand { dir, reg, value } => {
                    let reg = match reg {
                        j::AsmReg::Class(n) => {
                            if !ffi::register_ok(true, n) {
                                self.err(
                                    "E911",
                                    value.span,
                                    format!("no register class `{n}` on this target"),
                                );
                            }
                            y::AsmReg::Class(n.clone())
                        }
                        j::AsmReg::Explicit(n, sp) => {
                            if !ffi::register_ok(false, n) {
                                self.err("E911", *sp, format!("no register `{n}` on this target"));
                            }
                            y::AsmReg::Explicit(n.clone())
                        }
                    };
                    let (v, t) = self.atom(value, None)?;
                    let vec = matches!(&t, Ty::Coll(..)) && t.to_string().starts_with("Vec");
                    if !(t.scalar().is_some_and(|s| s != "bigint")
                        || matches!(t, Ty::Char | Ty::Err)
                        || vec)
                    {
                        self.err(
                            "E910",
                            value.span,
                            format!(
                                "an `asm` operand is a scalar or `Vec`, not `{t}`",
                                t = self.ty_text(&t)
                            ),
                        );
                    }
                    let dir = match dir.as_str() {
                        "in" => y::AsmDir::In,
                        "out" => y::AsmDir::Out,
                        "inout" => y::AsmDir::Inout,
                        "lateout" => y::AsmDir::Lateout,
                        d => return unsupported(format!("`asm` operand direction `{d}`")),
                    };
                    y::AsmOperand::Reg { dir, reg, value: v }
                }
                j::AsmArg::Typed { .. } => return unsupported("typed `asm` outputs"),
                j::AsmArg::ClobberAbi(s) => y::AsmOperand::ClobberAbi(s.clone()),
                j::AsmArg::Option(n) => y::AsmOperand::Options(vec![n.clone()]),
            });
        }
        self.emit(
            StmtKind::Asm {
                template: template.to_owned(),
                operands,
            },
            at,
        );
        Ok(())
    }

    /// A call of `path` with signature `sig`. A generic callee's type arguments are inferred
    /// from the argument types, and each must implement its bounds (E221).
    fn call_sig(
        &mut self,
        path: &str,
        sig: &Sig,
        recv: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let mut out: Vec<Arg> = sig.caps.iter().map(|c| Arg::Cap(c.clone(), at)).collect();
        let want = self.want_ret.take().filter(|(_, sp)| *sp == at);
        let outer = mem::take(&mut self.arg_tys);
        let vals = self.args(&sig.params, recv, args, at);
        let got = mem::replace(&mut self.arg_tys, outer);
        let Some(vals) = vals? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        out.extend(vals);
        let mut map = Vec::new();
        // A default typed at its parameter's own type (`key=None`) says nothing about the
        // parameters it mentions.
        let given = |w: &Ty, t: &Ty| w != t || !sig.tparams.iter().any(|(n, _)| w.mentions(n));
        for (w, t, _) in &got {
            if given(w, t) {
                w.unify(t, &mut map);
            }
        }
        if let Some((w, _)) = &want {
            sig.ret.unify(w, &mut map);
        }
        // An omitted `key: Callable[[T], K] | None` is the identity: `K` is `T` (R514).
        for p in &sig.params {
            let Ty::Option(f) = &p.ty else { continue };
            let Ty::Fn(_, ps, r) = &**f else { continue };
            let ([Ty::Param(t)], Ty::Param(k)) = (ps.as_slice(), &**r) else {
                continue;
            };
            if got.iter().any(|(w, t, _)| w == &p.ty && given(w, t)) {
                continue;
            }
            let Some(tv) = map.iter().find(|(n, _)| n == t).map(|(_, v)| v.clone()) else {
                continue;
            };
            match map.iter().find(|(n, _)| n == k) {
                None => map.push((k.clone(), tv)),
                Some((_, kv)) if *kv != tv && !matches!(kv, Ty::Err) && !matches!(tv, Ty::Err) => {
                    let msg = format!(
                        "expected `{tv}`, found `{kv}`",
                        tv = self.ty_text(&tv),
                        kv = self.ty_text(kv)
                    );
                    self.err("E201", at, msg);
                }
                Some(_) => {}
            }
        }
        for (n, bounds) in &sig.tparams {
            let Some((_, t)) = map.iter().find(|(m, _)| m == n) else {
                continue;
            };
            for b in bounds {
                if !self.implements(t, b) {
                    let sp = got
                        .iter()
                        .find(|(w, _, _)| w.mentions(n))
                        .map_or(at, |(_, _, sp)| *sp);
                    self.err(
                        "E221",
                        sp,
                        format!("`{t}` does not implement `{b}`", t = self.ty_text(t)),
                    );
                }
            }
        }
        let targs = sig
            .tparams
            .iter()
            .map(|(n, _)| {
                map.iter()
                    .find(|(m, _)| m == n)
                    .map_or(Ty::Err, |(_, t)| t.clone())
                    .targ()
            })
            .collect();
        let targs = if sig.tparams.is_empty() {
            vec![]
        } else {
            targs
        };
        Ok((
            Rhs::Call(call(path, targs, out, at)),
            sig.ret.subst(&map),
            sig.fail.as_ref().map(|f| f.subst(&map)),
        ))
    }

    /// The signature of method `m` on `t` from an `impl`, or from a bound of a type parameter.
    fn trait_method(&self, t: &Ty, m: &str) -> Option<Sig> {
        if let Some(s) = self.g.methods.get(&(t.to_string(), m.to_owned())) {
            return Some(s.clone());
        }
        let Ty::Param(p) = t else {
            return None;
        };
        let (_, bounds) = self.sig.tparams.iter().find(|(n, _)| n == p)?;
        let map = [("Self".to_owned(), t.clone())];
        bounds.iter().find_map(|b| {
            let (_, s) = self.g.traits.get(b)?.iter().find(|(n, _)| n == m)?;
            let mut s = s.clone();
            for q in &mut s.params {
                q.ty = q.ty.subst(&map);
            }
            s.ret = s.ret.subst(&map);
            Some(s)
        })
    }

    /// `t` implements trait `b`: an `impl`, or a type parameter bounded by it.
    fn implements(&self, t: &Ty, b: &str) -> bool {
        match t {
            Ty::Err => true,
            Ty::Param(p) => self
                .sig
                .tparams
                .iter()
                .any(|(n, bs)| n == p && bs.iter().any(|x| x == b)),
            Ty::Int | Ty::Float | Ty::BigInt | Ty::Fixed(_) if b == "Num" => true,
            Ty::Int | Ty::Float | Ty::BigInt | Ty::Fixed(_) | Ty::Str | Ty::Char | Ty::Bool
                if matches!(b, "Eq" | "Ord" | "Hash") =>
            {
                !(b == "Hash" && matches!(t, Ty::Float | Ty::Fixed("f32" | "f64")))
            }
            Ty::Tuple(ts) if matches!(b, "Eq" | "Ord" | "Hash") => {
                ts.iter().all(|t| self.implements(t, b))
            }
            Ty::List(t) if matches!(b, "Eq" | "Ord") => self.implements(t, b),
            Ty::Coll("bytes", _) => matches!(b, "Eq" | "Ord" | "Hash"),
            Ty::Coll("bytearray", _) => b == "Eq",
            Ty::Coll("Secret", ts) if b == "Eq" => self.implements(&ts[0], b),
            Ty::Coll("Path" | "WindowsPath", _) if matches!(b, "Eq" | "Ord" | "Hash") => true,
            Ty::Str | Ty::Coll("Path", _) if b == "AsPath" => true,
            Ty::Coll("Duration" | "Instant", _) => matches!(b, "Eq" | "Ord" | "Hash"),
            Ty::Coll("MonoInstant", _) => matches!(b, "Eq" | "Ord"),
            Ty::Coll("PlainDate" | "PlainTime" | "PlainDateTime", _) => {
                matches!(b, "Eq" | "Ord" | "Hash")
            }
            Ty::Coll("Period" | "TimeZone", _) => matches!(b, "Eq" | "Hash"),
            t => self.g.impls.contains(&(b.to_owned(), t.to_string())),
        }
    }

    fn ctor(&mut self, name: &str, ps: &[crate::P], args: &[j::Arg], at: Span) -> R<Rhs> {
        let Some(vals) = self.args(ps, None, args, at)? else {
            return Ok(Rhs::Atom(Atom::Lit(Literal::Unit, at)));
        };
        let fields = ps
            .iter()
            .zip(vals)
            .map(|(p, a)| match a {
                Arg::Value(_, a, _) => (p.name.clone(), a),
                Arg::Cap(..) => unreachable!("constructors take no capabilities"),
            })
            .collect();
        Ok(Rhs::Ctor(y::Ctor::Named {
            path: vec![name.to_owned()],
            targs: Vec::new(),
            fields,
        }))
    }

    /// Match arguments to parameters (14.1 Calls) and lower them in parameter order.
    /// `None` when an arity error was reported.
    fn args(
        &mut self,
        ps: &[crate::P],
        recv: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
    ) -> R<Option<Vec<Arg>>> {
        let mut slots: Vec<Option<Given>> = (0..ps.len()).map(|_| None).collect();
        let mut next = 0;
        if let Some(r) = recv {
            if ps.is_empty() {
                self.err("E202", at, "this function takes no receiver".into());
                return Ok(None);
            }
            slots[0] = Some(Given::Recv(r));
            next = 1;
        }
        for a in args {
            let (e, is_ref) = match &a.kind {
                j::ArgKind::Expr(e) => (e, false),
                j::ArgKind::Ref(e) => (e, true),
                j::ArgKind::Become(_) => return unsupported("`become` arguments"),
            };
            let k = match &a.name {
                None => {
                    if next >= ps.len() {
                        self.err("E202", a.span, format!("expected {} arguments", ps.len()));
                        return Ok(None);
                    }
                    next += 1;
                    next - 1
                }
                Some(n) => match ps.iter().position(|p| p.name == *n) {
                    Some(k) if slots[k].is_none() && !ps[k].positional => k,
                    _ => {
                        self.err("E202", at, format!("unexpected argument `{n}`"));
                        return Ok(None);
                    }
                },
            };
            // A keyword argument is reported at its value: `&` sits just before the place.
            let sp = match (&a.name, is_ref) {
                (None, _) => a.span,
                (Some(_), false) => e.span,
                (Some(_), true) => Span {
                    col: e.span.col.saturating_sub(1),
                    ..e.span
                },
            };
            slots[k] = Some(Given::Expr(e, is_ref, sp, a.name.is_some()));
        }
        // E210: parameters that share a type take keyword arguments.
        for (k, slot) in slots.iter().enumerate() {
            let Some(Given::Expr(e, _, sp, keyword)) = slot else {
                continue;
            };
            let p = &ps[k];
            let shared = ps.iter().enumerate().any(|(i, q)| {
                i != k && q.ty == p.ty && !q.positional && !(recv.is_some() && i == 0)
            });
            let exempt = matches!(&e.kind, j::ExprKind::Lit(_))
                || matches!(&e.kind, j::ExprKind::Unary("-", x) if matches!(x.kind, j::ExprKind::Lit(_)))
                || matches!(&e.kind, j::ExprKind::Name(n) if *n == p.name);
            if shared && !*keyword && !exempt && !p.positional && !self.in_become {
                self.err(
                    "E210",
                    *sp,
                    format!(
                        "pass `{}` by keyword: another parameter has type `{}`",
                        p.name, p.ty
                    ),
                );
                break;
            }
        }
        let mut elems: Vec<(String, Atom)> = Vec::new();
        // Lambdas go last, so their parameter types can come from the other arguments.
        let lambda = |s: &Option<Given>| matches!(s, Some(Given::Expr(e, ..)) if matches!(e.kind, j::ExprKind::Lambda(..)));
        let mut order: Vec<usize> = (0..ps.len()).filter(|&k| !lambda(&slots[k])).collect();
        order.extend((0..ps.len()).filter(|&k| lambda(&slots[k])));
        let mut done: Vec<Option<Arg>> = (0..ps.len()).map(|_| None).collect();
        for k in order {
            let (p, slot) = (&ps[k], slots[k].take());
            let out = &mut done[k];
            let (e, is_ref, sp) = match slot {
                Some(Given::Expr(e, r, sp, _)) => (e, r, sp),
                Some(Given::Recv(e)) => (e, p.conv == Conv::Inout, e.span),
                None => match &p.default {
                    Some(d) => {
                        let (a, t) = self.atom(&d.clone(), Some(&p.ty))?;
                        self.expect(&t, &p.ty, at);
                        self.arg_tys.push((p.ty.clone(), t.clone(), at));
                        *out = Some(Arg::Value(p.conv, a, at));
                        continue;
                    }
                    None => {
                        self.err("E202", at, format!("missing argument `{}`", p.name));
                        return Ok(None);
                    }
                },
            };
            match (p.conv, is_ref) {
                (Conv::Inout, true) => {
                    if root(e).is_none() {
                        self.err("E403", sp, temporary_receiver("this call"));
                    }
                    let (pl, t) = self.place(e)?;
                    self.expect(&t, &p.ty, sp);
                    self.arg_tys.push((p.ty.clone(), t.clone(), sp));
                    if pl.projs.is_empty() {
                        *out = Some(Arg::Value(
                            Conv::Inout,
                            Atom::Name(pl.root, pl.root_span),
                            sp,
                        ));
                        continue;
                    }
                    // Two elements of one list may alias: the inserted check panics if they do.
                    if let [Proj::Index(i)] = pl.projs.as_slice() {
                        for (root, j) in &elems {
                            if *root == pl.root {
                                let c = call(
                                    "exclusive_check",
                                    vec![],
                                    vec![let_arg(j.clone()), let_arg(i.clone())],
                                    sp,
                                );
                                self.emit(StmtKind::Rhs(Rhs::Call(c)), sp);
                            }
                        }
                        elems.push((pl.root.clone(), i.clone()));
                    }
                    let n = self.fresh("w");
                    self.bind(&n, true, &t, Rhs::Proj(pl.clone()), sp, sp);
                    self.writeback.push((n.clone(), pl, sp));
                    *out = Some(Arg::Value(Conv::Inout, Atom::Name(n, sp), sp));
                }
                (Conv::Inout, false) => {
                    self.err(
                        "E404",
                        sp,
                        format!("`{}` is a `var` parameter; pass `&place`", p.name),
                    );
                    let marked = format!("&{}", self.cx.text(sp));
                    self.cx.fix(Applicability::Auto, "pass `&`", sp, &marked);
                }
                (_, true) => {
                    self.err(
                        "E405",
                        sp,
                        format!("`{}` is not a `var` parameter; remove `&`", p.name),
                    );
                    self.cx.fix(Applicability::Auto, "remove `&`", sp, "");
                }
                (conv, false) => {
                    let mut map = Vec::new();
                    for (w, t, _) in &self.arg_tys {
                        w.unify(t, &mut map);
                    }
                    let w = p.ty.subst(&map);
                    // An unbound type parameter is no expectation: `first(["a"])` must bind `T`
                    // from the literal, not type the literal by `T`. Lambdas still need it.
                    let lambda = matches!(e.kind, j::ExprKind::Lambda(..));
                    let w = (!matches!(w, Ty::Coll("iter", _))
                        && (lambda || !self.unbound_param(&w)))
                    .then_some(w);
                    let (a, t) = self.atom(e, w.as_ref())?;
                    let (a, t) = self.as_iter(&p.ty, a, t, sp)?;
                    self.expect(&t, &p.ty, sp);
                    self.arg_tys.push((p.ty.clone(), t.clone(), sp));
                    if let (Conv::Sink, Atom::Name(n, _)) = (conv, &a)
                        && !t.is_copy()
                        && self.locals.contains_key(n)
                        && let Some(later) = self.next_use(n, e.span)
                    {
                        self.err(
                            "E413",
                            sp,
                            format!("`{n}` is used again later; pass `{n}.copy()`"),
                        );
                        let copied = format!("{}.copy()", self.cx.text(sp));
                        self.cx.fix(Applicability::Auto, "copy", sp, &copied);
                        self.cx.label(later, "used again");
                    }
                    *out = Some(Arg::Value(conv, a, sp));
                }
            }
        }
        Ok(Some(done.into_iter().flatten().collect()))
    }

    /// Whether `t` mentions a type parameter of the callee that is not yet bound; the caller's
    /// own type parameters are fixed types here.
    fn unbound_param(&self, t: &Ty) -> bool {
        fn free(t: &Ty, scope: &[String]) -> bool {
            match t {
                Ty::Param(n) => !scope.contains(n),
                Ty::List(t) | Ty::Set(t) | Ty::Option(t) => free(t, scope),
                Ty::Dict(a, b) | Ty::Result(a, b) => free(a, scope) || free(b, scope),
                Ty::Tuple(ts) | Ty::Coll(_, ts) => ts.iter().any(|t| free(t, scope)),
                Ty::Fn(_, ps, r) => free(r, scope) || ps.iter().any(|t| free(t, scope)),
                _ => false,
            }
        }
        free(t, &self.g.scope.borrow())
    }

    /// A collection passed where `iter[T]` is wanted is passed as its `.iter()` (R511).
    fn as_iter(&mut self, want: &Ty, a: Atom, t: Ty, sp: Span) -> R<(Atom, Ty)> {
        let iterable = matches!(t, Ty::Str | Ty::List(_) | Ty::Set(_) | Ty::Dict(..))
            || matches!(t, Ty::Coll(c, _) if c != "iter" && c != "heap");
        let (Ty::Coll("iter", _), true, Atom::Name(n, _)) = (want, iterable, &a) else {
            return Ok((a, t));
        };
        let name = j::Expr {
            kind: j::ExprKind::Name(n.clone()),
            span: sp,
            paren: false,
        };
        let f = j::Expr {
            kind: j::ExprKind::Attr(Box::new(name), "iter".into(), sp),
            span: sp,
            paren: false,
        };
        let call = j::Expr {
            kind: j::ExprKind::Call(Box::new(f), vec![]),
            span: sp,
            paren: false,
        };
        self.atom(&call, None)
    }

    fn positional<'e>(
        &mut self,
        args: &'e [j::Arg],
        n: usize,
        what: &str,
        at: Span,
    ) -> R<Option<Vec<&'e j::Expr>>> {
        let mut out = Vec::new();
        for a in args {
            match (&a.name, &a.kind) {
                (None, j::ArgKind::Expr(e)) => out.push(e),
                _ => return unsupported(format!("keyword or reference arguments to `{what}`")),
            }
        }
        if out.len() != n {
            self.err(
                "E202",
                at,
                format!(
                    "`{what}` takes {n} argument{}",
                    if n == 1 { "" } else { "s" }
                ),
            );
            return Ok(None);
        }
        Ok(Some(out))
    }

    /// `deque(xs, maxlen=n)`, `heap(xs)`, `sorted_dict(d)`, `sorted_set(xs)`, each argument optional (R488).
    fn collection(
        &mut self,
        n: &str,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        let (name, arity) = crate::ty::collection(n).expect("a collection name");
        let (mut src, mut maxlen) = (None, None);
        for a in args {
            match (a.name.as_deref(), &a.kind) {
                (None, j::ArgKind::Expr(e)) if src.is_none() => src = Some(e),
                (Some("maxlen"), j::ArgKind::Expr(e)) | (None, j::ArgKind::Expr(e))
                    if name == "deque" && maxlen.is_none() =>
                {
                    maxlen = Some(e)
                }
                _ => {
                    self.err("E202", a.span, format!("unexpected argument to `{n}`"));
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
                }
            }
        }
        let wanted = match want {
            Some(Ty::Coll(m, ts)) if *m == name => Some(ts.clone()),
            _ => None,
        };
        let src_ty = |ts: &[Ty]| match ts {
            [k, v] => Ty::Dict(Box::new(k.clone()), Box::new(v.clone())),
            [t] => Ty::List(Box::new(t.clone())),
            _ => Ty::Err,
        };
        let mut out = Vec::new();
        let ts = match src {
            Some(e) => {
                let (a, t) = self.atom(e, wanted.as_deref().map(src_ty).as_ref())?;
                let ts = match (&t, arity) {
                    (Ty::Dict(k, v), 2) => vec![(**k).clone(), (**v).clone()],
                    (Ty::List(x), 1) => vec![(**x).clone()],
                    (Ty::Err, _) => return Ok((Rhs::Atom(a), Ty::Err)),
                    _ => {
                        self.err(
                            "E201",
                            e.span,
                            format!("`{n}` does not take a `{t}`", t = self.ty_text(&t)),
                        );
                        return Ok((Rhs::Atom(a), Ty::Err));
                    }
                };
                out.push(sink_arg(a));
                ts
            }
            None => match wanted {
                Some(ts) => ts,
                None => {
                    self.err("E241", at, format!("annotate the type of this empty {n}"));
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
                }
            },
        };
        if let Some(e) = maxlen {
            if out.is_empty() {
                let new = call("std.list.new", vec![ts[0].targ()], vec![], at);
                let xs = self.tmp(&src_ty(&ts), Rhs::Call(new), at);
                out.push(sink_arg(xs));
            }
            let (a, t) = self.atom(e, Some(&Ty::Int))?;
            self.expect(&t, &Ty::Int, e.span);
            out.push(let_arg(a));
        }
        let path = format!("std.prelude.{name}");
        let targs = ts.iter().map(Ty::targ).collect();
        Ok((Rhs::Call(call(&path, targs, out, at)), Ty::Coll(name, ts)))
    }

    fn builtin(&mut self, n: &str, args: &[j::Arg], at: Span, want: Option<&Ty>) -> R<(Rhs, Ty)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err);
        match n {
            "len" | "sorted" | "int" | "float" | "round" | "abs" | "str" | "bigint" | "list" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(err);
                };
                let (a, t) = self.atom(xs[0], None)?;
                let (path, targs, ret) = match (n, &t) {
                    (_, Ty::Err) => return Ok((Rhs::Atom(a), Ty::Err)),
                    ("len", Ty::List(e)) => ("std.list.len", vec![e.targ()], Ty::Int),
                    ("len", Ty::Set(e)) => ("std.set.len", vec![e.targ()], Ty::Int),
                    ("len", Ty::Dict(k, v)) => ("std.dict.len", vec![k.targ(), v.targ()], Ty::Int),
                    ("len", Ty::Coll(..)) => {
                        let path = format!("std.{}.len", t.module().expect("a collection"));
                        let c = call(&path, coll_targs(&t), vec![let_arg(a)], at);
                        return Ok((Rhs::Call(c), Ty::Int));
                    }
                    ("len", Ty::Str) if self.cx.strict => {
                        let s = self.cx.text(xs[0].span);
                        self.err(
                            "E504",
                            at,
                            format!("`len` of a `str`; use `{s}.char_count()` or `{s}.byte_len()`"),
                        );
                        return Ok(err);
                    }
                    ("len", Ty::Str) => {
                        let item = self.item.clone();
                        self.cx.note(
                            "N21",
                            at,
                            &item,
                            "`len(s)` normalized to `s.char_count()`".into(),
                        );
                        ("std.text.char_count", vec![], Ty::Int)
                    }
                    ("sorted", Ty::List(e)) => ("std.prelude.sorted", vec![e.targ()], t.clone()),
                    ("list", Ty::List(e)) => {
                        let c = call("std.prelude.list", vec![e.targ()], vec![sink_arg(a)], at);
                        return Ok((Rhs::Call(c), t.clone()));
                    }
                    ("bigint", Ty::Int) => ("std.prelude.bigint", vec![], Ty::BigInt),
                    ("bigint", Ty::BigInt) => return Ok((Rhs::Atom(a), t)),
                    ("int", Ty::Int) | ("float", Ty::Float) => return Ok((Rhs::Atom(a), t)),
                    ("int", Ty::Float) => ("std.prelude.int", vec![], Ty::Int),
                    ("float", Ty::Int) => ("std.prelude.float", vec![], Ty::Float),
                    ("round", Ty::Float) => ("std.prelude.round", vec![], Ty::Int),
                    ("abs", Ty::Int | Ty::Float) => ("std.prelude.abs", vec![t.targ()], t.clone()),
                    ("str", _) => return Ok((Rhs::Atom(self.show(a, &t, at)), Ty::Str)),
                    _ => {
                        self.err(
                            "E201",
                            xs[0].span,
                            format!("`{n}` does not take a `{t}`", t = self.ty_text(&t)),
                        );
                        return Ok(err);
                    }
                };
                Ok((Rhs::Call(call(path, targs, vec![let_arg(a)], at)), ret))
            }
            "min" | "max" => {
                let Some(xs) = self.positional(args, 2, n, at)? else {
                    return Ok(err);
                };
                let (a, t) = self.atom(xs[0], None)?;
                let (b, u) = self.atom(xs[1], Some(&t))?;
                self.expect(&u, &t, xs[1].span);
                let path = format!("std.prelude.{n}");
                Ok((
                    Rhs::Call(call(
                        &path,
                        vec![t.targ()],
                        vec![let_arg(a), let_arg(b)],
                        at,
                    )),
                    t,
                ))
            }
            "print" => {
                let (r, t, _) = self.cap_call("io", "print", args, at)?;
                Ok((r, t))
            }
            "evolve" => {
                let Some(first) = args.first().filter(|a| a.name.is_none()) else {
                    self.err("E202", at, "`evolve` takes a record first".into());
                    return Ok(err);
                };
                let j::ArgKind::Expr(c) = &first.kind else {
                    return unsupported("`evolve` of a reference");
                };
                let (p, t) = self.place(c)?;
                let Ty::Data(r) = &t else {
                    self.err(
                        "E201",
                        c.span,
                        format!("`evolve` takes a record, found `{t}`", t = self.ty_text(&t)),
                    );
                    return Ok(err);
                };
                let Some(fs) = self.g.records.get(r) else {
                    self.err(
                        "E201",
                        c.span,
                        format!("`evolve` takes a record, found `{t}`", t = self.ty_text(&t)),
                    );
                    return Ok(err);
                };
                let fs: Vec<(String, Ty)> =
                    fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
                let v = self.fresh("t");
                self.bind(&v, true, &t, Rhs::Copy(p), at, at);
                for a in &args[1..] {
                    let (Some(name), j::ArgKind::Expr(e)) = (&a.name, &a.kind) else {
                        self.err("E202", a.span, "`evolve` takes fields by keyword".into());
                        continue;
                    };
                    let Some((_, ft)) = fs.iter().find(|(f, _)| f == name) else {
                        let names = fs.iter().map(|(f, _)| f.as_str());
                        let msg = format!("`{r}` has no field `{name}`");
                        self.err_near("E203", a.span, msg, name, names);
                        continue;
                    };
                    let (x, xt) = self.atom(e, Some(ft))?;
                    self.expect(&xt, ft, e.span);
                    self.emit(
                        StmtKind::Assign {
                            place: Place {
                                root: v.clone(),
                                root_span: at,
                                projs: vec![Proj::Field(name.clone(), a.span)],
                            },
                            rhs: Rhs::Atom(x),
                            rhs_span: e.span,
                        },
                        a.span,
                    );
                }
                Ok((Rhs::Atom(Atom::Name(v, at)), t))
            }
            "Ok" | "Err" | "Some" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(err);
                };
                let (field, inner) = match (n, want) {
                    ("Ok", Some(Ty::Result(t, _))) => ("value", (**t).clone()),
                    ("Err", Some(Ty::Result(_, e))) => ("error", (**e).clone()),
                    ("Some", Some(Ty::Option(t))) => ("value", (**t).clone()),
                    _ => return unsupported(format!("`{n}(...)` without an expected type")),
                };
                let (a, t) = self.atom(xs[0], Some(&inner))?;
                self.expect(&t, &inner, xs[0].span);
                Ok((
                    Rhs::Ctor(y::Ctor::Named {
                        path: vec![n.to_owned()],
                        targs: Vec::new(),
                        fields: vec![(field.to_owned(), a)],
                    }),
                    want.cloned().expect("matched above"),
                ))
            }
            "range" => {
                let n = args.len();
                if !(1..=2).contains(&n) {
                    self.err(
                        "E202",
                        at,
                        format!("`range` takes 1 or 2 arguments, {n} given"),
                    );
                    return Ok(err);
                }
                let Some(xs) = self.positional(args, n, "range", at)? else {
                    return Ok(err);
                };
                let mut out = Vec::new();
                for x in xs {
                    let (a, t) = self.atom(x, Some(&Ty::Int))?;
                    self.expect(&t, &Ty::Int, x.span);
                    out.push(let_arg(a));
                }
                let c = call("std.prelude.range", vec![], out, at);
                Ok((Rhs::Call(c), Ty::List(Box::new(Ty::Int))))
            }
            "deque" | "heap" | "sorted_dict" | "sorted_set" => self.collection(n, args, at, want),
            "Secret" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(err);
                };
                let inner = match want {
                    Some(Ty::Coll("Secret", ts)) => ts.first().cloned(),
                    _ => None,
                };
                let (a, t) = self.atom(xs[0], inner.as_ref())?;
                let c = call("std.prelude.Secret", vec![t.targ()], vec![sink_arg(a)], at);
                Ok((Rhs::Call(c), Ty::Coll("Secret", vec![t])))
            }
            "dict" | "set" | "enumerate" | "zip" | "reversed" | "isinstance" | "hash" | "repr"
            | "divmod" | "pow" | "chr" | "ord" | "atan2" | "hypot" => unsupported(format!("`{n}`")),
            // std.test is in scope without an import (assumption A7).
            _ if let Some((path, p)) = ["prelude", "test"].iter().find_map(|m| {
                let path = format!("std.{m}.{n}");
                let p = crate::stdlib::decl(&path)?.plain()?;
                Some((path, p))
            }) =>
            {
                if p.fail.is_some() {
                    return unsupported(format!("`{path}`, a std function that can fail"));
                }
                self.civil_literal(n, args, at);
                let (r, t, _) = self.std_call(&path, p, Vec::new(), args, n, at)?;
                Ok((r, t))
            }
            _ => {
                let mut names: Vec<&str> = self.g.fns.keys().map(String::as_str).collect();
                names.extend(BUILTINS);
                names.extend(crate::stdlib::members("prelude"));
                names.extend(crate::stdlib::members("test"));
                self.err_near("E200", at, format!("cannot find function `{n}`"), n, names);
                for a in args {
                    if let j::ArgKind::Expr(e) | j::ArgKind::Ref(e) = &a.kind {
                        self.atom(e, None)?;
                    }
                }
                Ok(err)
            }
        }
    }

    /// A call of the callable bound to the local `n`, of type `t`.
    fn call_local(
        &mut self,
        n: &str,
        t: Ty,
        f_span: Span,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let Ty::Fn(par, ps, ret) = t else {
            unreachable!("a callable")
        };
        if args.len() != ps.len() || args.iter().any(|a| a.name.is_some()) {
            self.err(
                "E202",
                at,
                format!("`{n}` takes {} positional arguments", ps.len()),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        }
        let mut out = Vec::new();
        for (a, p) in args.iter().zip(&ps) {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported("`&` or `become` arguments to a callable");
            };
            let (x, t) = self.atom(e, Some(p))?;
            self.expect(&t, p, a.span);
            out.push(let_arg(x));
        }
        if !par {
            self.fn_calls.push((n.to_owned(), f_span));
        }
        Ok((Rhs::Call(call(n, vec![], out, at)), *ret, None))
    }

    /// `f(...).context(msg)`: the call's failure as an `Error` with `msg` added (R432).
    fn context(
        &mut self,
        f: &j::Expr,
        fargs: &[j::Arg],
        fat: Span,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let (r, t, fail) = self.call_expr(f, fargs, fat, None)?;
        let (Some(e), Rhs::Call(c)) = (fail, r) else {
            return unsupported("`.context` on a call that cannot fail");
        };
        if !self.g.std_records.contains("Error") {
            return unsupported("`.context` where the module defines its own `Error`");
        }
        let Some(xs) = self.positional(args, 1, "context", at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let (msg, mt) = self.atom(xs[0], Some(&Ty::Str))?;
        self.expect(&mt, &Ty::Str, xs[0].span);
        let rt = Ty::Result(Box::new(t.clone()), Box::new(e.clone()));
        let tried = self.tmp(&rt, Rhs::Try(c), fat);
        let c = call(
            "std.error.context",
            vec![t.targ(), e.targ()],
            vec![sink_arg(tried), let_arg(msg)],
            at,
        );
        Ok((Rhs::Call(c), t, Some(Ty::Data("Error".into()))))
    }

    /// `bits.f(x, ...)` as the typed `<f>_<type>` operation for `x`'s integer type.
    fn bits_call(&mut self, f: &str, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        let n = if matches!(f, "rotate_left" | "rotate_right" | "wrapping_shl") {
            2
        } else {
            1
        };
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let Some(xs) = self.positional(args, n, f, at)? else {
            return Ok(err);
        };
        let (x, t) = self.atom(xs[0], None)?;
        if t == Ty::Err {
            return Ok(err);
        }
        let big_ok = matches!(f, "count_ones" | "bit_width");
        if !is_integer(&t) || (t == Ty::BigInt && !big_ok) {
            self.err(
                "E201",
                xs[0].span,
                format!("`bits.{f}` does not take a `{t}`"),
            );
            return Ok(err);
        }
        let mut out = vec![let_arg(x)];
        if let Some(e) = xs.get(1) {
            let (a, nt) = self.atom(e, Some(&Ty::Int))?;
            if !is_integer(&nt) || nt == Ty::BigInt {
                if nt != Ty::Err {
                    self.err(
                        "E201",
                        e.span,
                        format!("`bits.{f}` needs an integer amount, not `{nt}`"),
                    );
                }
                return Ok(err);
            }
            out.push(let_arg(a));
        }
        let rt = match f {
            "is_power_of_two" => Ty::Bool,
            "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" | "bit_width" => {
                Ty::Int
            }
            _ => t.clone(),
        };
        let p = format!("{f}_{}", t.scalar().expect("an integer"));
        Ok((Rhs::Call(call(&p, vec![], out, at)), rt, None))
    }

    /// `module.f(...)`, a free std function called by its module's name, as `math.sqrt(x)`.
    fn module_call(
        &mut self,
        module: &str,
        f: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        if module == "bits" {
            return self.bits_call(f, args, at);
        }
        let path = format!("std.{module}.{f}");
        if crate::stdlib::decl(&path).is_some_and(|d| d.typed_at_call()) {
            return self.user_call(&path, None, args, at);
        }
        let Some(mut p) = crate::stdlib::decl(&path).and_then(|d| d.plain()) else {
            return unsupported(format!("`{path}`"));
        };
        if f == "strptime"
            && let Some(n) = match module {
                "time.date" => Some("PlainDate"),
                "time.plain_time" => Some("PlainTime"),
                "time.plain_date_time" => Some("PlainDateTime"),
                _ => None,
            }
        {
            self.pattern_check(n, args.get(1));
        }
        // A literal pattern is checked here, so the call cannot fail (R484).
        if path == "std.regex.compile"
            && let [
                j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(e),
                    ..
                },
            ] = args
            && let j::ExprKind::Lit(j::Lit::Str { prefix, value }) = &e.kind
            && matches!(prefix.as_str(), "" | "r")
        {
            if let Err(reason) = yong_std::regex::build(value) {
                self.err(
                    "E545",
                    e.span,
                    format!("invalid regular expression: {reason}"),
                );
            }
            p.fail = None;
        }
        self.std_call(&path, p, Vec::new(), args, f, at)
    }

    /// An operation of a std capability (`io.print(s)`, `fs.read(p)`), declared in its module.
    fn cap_call(
        &mut self,
        cap: &str,
        op: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let path = format!("std.{cap}.{op}");
        if path == "std.rand.choice" || path == "std.rand.shuffle" {
            return self.rand_list_op(cap, op, args, at);
        }
        if cap == "log" && matches!(op, "debug" | "info" | "warn" | "error" | "span") {
            return self.log_call(op, args, at);
        }
        let Some(p) = crate::stdlib::decl(&path)
            .filter(|d| d.cap)
            .and_then(|d| d.plain())
        else {
            return unsupported(format!("capability operation `{cap}.{op}`"));
        };
        self.std_call(&path, p, vec![Arg::Cap(cap.to_owned(), at)], args, op, at)
    }

    /// `log.info(msg, key=value, ...)` as `log.event(InfoLevel(), msg, [("key", show(value))])`,
    /// and `log.span(name, key=value)` as `log.span(name, [...])`.
    fn log_call(&mut self, op: &str, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let mut msg = None;
        let mut fields = Vec::new();
        for a in args {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported(format!("a reference argument to `log.{op}`"));
            };
            match &a.name {
                None if msg.is_none() => msg = Some(e.clone()),
                None => {
                    self.err(
                        "E202",
                        a.span,
                        format!("`log.{op}` fields are keyword arguments"),
                    );
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
                }
                Some(n) => fields.push(ex(j::ExprKind::Tuple(vec![
                    ex(j::ExprKind::Lit(j::Lit::Str {
                        prefix: String::new(),
                        value: n.clone(),
                    })),
                    ex(j::ExprKind::FStr {
                        template: false,
                        pieces: vec![j::FPiece::Expr(e.clone(), None)],
                    }),
                ]))),
            }
        }
        let Some(msg) = msg else {
            self.err("E202", at, format!("`log.{op}` needs a message"));
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let arg = |e| j::Arg {
            name: None,
            kind: j::ArgKind::Expr(e),
            span: at,
        };
        let mut synth = Vec::new();
        let f = if op == "span" {
            "span"
        } else {
            let level = format!("{}{}Level", op[..1].to_uppercase(), &op[1..]);
            synth.push(arg(ex(j::ExprKind::Call(
                Box::new(ex(j::ExprKind::Name(level))),
                Vec::new(),
            ))));
            "event"
        };
        synth.push(arg(msg));
        synth.push(arg(ex(j::ExprKind::List(fields))));
        let path = format!("std.log.{f}");
        let p = crate::stdlib::decl(&path)
            .and_then(|d| d.plain())
            .expect("std.log declares the operation");
        self.std_call(&path, p, vec![Arg::Cap("log".into(), at)], &synth, op, at)
    }

    /// `cli.parse_args[T](args)` with `T`'s schema, and `cli.parse[T]()` over `env.args()`.
    fn cli_call(
        &mut self,
        op: &str,
        t: &j::Expr,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let j::ExprKind::Name(n) = &t.kind else {
            return unsupported(format!("`cli.{op}` of a type that is not a name"));
        };
        // After an error the call is still lowered, so what follows it checks.
        let schema = if self.g.clis.contains_key(n) {
            // An error here was reported at the derive.
            crate::cli::schema(self.g, n).unwrap_or_default()
        } else {
            self.err("E201", t.span, format!("`{n}` does not derive `Cli`"));
            String::new()
        };
        let xs = if op == "parse" {
            if !args.is_empty() {
                self.err(
                    "E202",
                    at,
                    "`cli.parse` takes no arguments; it reads `env.args()`".into(),
                );
                return Ok(err);
            }
            let (r, t, _) = self.cap_call("env", "args", &[], at)?;
            self.tmp(&t, r, at)
        } else {
            let Some(xs) = self.positional(args, 1, "parse_args", at)? else {
                return Ok(err);
            };
            let want = Ty::List(Box::new(Ty::Str));
            let (a, t) = self.atom(xs[0], Some(&want))?;
            self.expect(&t, &want, xs[0].span);
            a
        };
        let rt = Ty::Data(n.clone());
        let c = call(
            "std.cli.parse_args",
            vec![rt.targ()],
            vec![let_arg(xs), let_arg(Atom::Lit(Literal::Str(schema), at))],
            at,
        );
        Ok((Rhs::Call(c), rt, Some(Ty::Data("CliError".into()))))
    }

    /// `rand.choice(xs)` and `rand.shuffle(&xs)`, the generic operations of `Rand`.
    fn rand_list_op(
        &mut self,
        cap: &str,
        op: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let [a] = args else {
            self.err("E202", at, format!("`rand.{op}` takes one list"));
            return Ok(err);
        };
        let path = format!("std.rand.{op}");
        let mut out = vec![Arg::Cap(cap.to_owned(), at)];
        let (t, ret) = match (op, &a.kind) {
            ("choice", j::ArgKind::Expr(e)) => {
                let (x, t) = self.atom(e, None)?;
                out.push(let_arg(x));
                let el = match &t {
                    Ty::List(el) => (**el).clone(),
                    _ => Ty::Err,
                };
                (t, el)
            }
            ("shuffle", j::ArgKind::Ref(e)) => {
                let (pl, t) = self.place(e)?;
                if !pl.projs.is_empty() {
                    return unsupported("`rand.shuffle` of a projection");
                }
                out.push(Arg::Value(
                    Conv::Inout,
                    Atom::Name(pl.root, pl.root_span),
                    a.span,
                ));
                (t, Ty::Unit)
            }
            ("shuffle", _) => {
                self.err("E404", a.span, "`rand.shuffle` takes `&place`".into());
                return Ok(err);
            }
            _ => {
                self.err(
                    "E405",
                    a.span,
                    "`rand.choice` takes the list, not `&`".into(),
                );
                return Ok(err);
            }
        };
        let Ty::List(el) = &t else {
            if t != Ty::Err {
                self.err(
                    "E201",
                    a.span,
                    format!("`rand.{op}` takes a list, not `{t}`"),
                );
            }
            return Ok(err);
        };
        let targs = vec![el.targ()];
        Ok((Rhs::Call(call(&path, targs, out, at)), ret, None))
    }

    /// `module.v` names variant `v` of a std sum type declared in `module`, as `math.Up`.
    fn std_variant(&self, module: &str, v: &str) -> bool {
        self.g
            .variant_of
            .get(v)
            .is_some_and(|s| crate::stdlib::sum_module(s) == Some(module))
    }

    /// The arguments of a std call matched to `names`: positional, then by keyword, then the
    /// declared defaults. `None` after reporting a mismatch.
    fn bind_std<'e>(
        &mut self,
        args: &'e [j::Arg],
        names: &[(&str, Option<&'e j::Expr>)],
        what: &str,
        at: Span,
    ) -> R<Option<Vec<&'e j::Expr>>> {
        let mut out: Vec<Option<&j::Expr>> = vec![None; names.len()];
        let mut next = 0;
        for a in args {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported(format!("reference arguments to `{what}`"));
            };
            let i = match &a.name {
                None => next,
                Some(n) => match names.iter().position(|(p, _)| p == n) {
                    Some(i) => i,
                    None => {
                        self.err("E202", a.span, format!("`{what}` has no parameter `{n}`"));
                        return Ok(None);
                    }
                },
            };
            if i >= names.len() || out[i].is_some() {
                self.err("E202", a.span, format!("too many arguments to `{what}`"));
                return Ok(None);
            }
            out[i] = Some(e);
            next = i + 1;
        }
        let mut xs = Vec::new();
        for (x, (n, default)) in out.into_iter().zip(names) {
            match x.or(*default) {
                Some(e) => xs.push(e),
                None => {
                    self.err("E202", at, format!("`{what}` needs argument `{n}`"));
                    return Ok(None);
                }
            }
        }
        Ok(Some(xs))
    }

    /// A call of a non-generic std function; `lead` are the arguments before the written ones.
    fn std_call(
        &mut self,
        path: &str,
        p: crate::stdlib::Plain,
        mut lead: Vec<Arg>,
        args: &[j::Arg],
        what: &str,
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let Some(xs) = self.bind_std(args, &p.names, what, at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        for (x, (take, pt)) in xs.into_iter().zip(&p.params) {
            let (a, t) = self.atom(x, Some(pt))?;
            self.expect(&t, pt, x.span);
            lead.push(Arg::Value(
                if *take { Conv::Sink } else { Conv::Let },
                a,
                x.span,
            ));
        }
        Ok((Rhs::Call(call(path, vec![], lead, at)), p.ret, p.fail))
    }

    fn method(
        &mut self,
        b: &j::Expr,
        m: &str,
        name_at: Span,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        if m == "copy" && args.is_empty() {
            let (p, t) = self.place(b)?;
            return Ok((Rhs::Copy(p), t, None));
        }
        // A prelude function that shares its name with an iter method (`zip`, `max`): the
        // receiver's type picks. Both take the receiver by `let`, so a temporary is enough.
        if self.g.fns.contains_key(m)
            && crate::stdlib::decl(&format!("std.prelude.{m}")).is_some()
            && crate::stdlib::decl(&format!("std.iter.ops.{m}")).is_some()
        {
            let (a, t) = self.atom(b, None)?;
            let Atom::Name(n, _) = a else {
                return unsupported(format!("`.{m}()` on a literal"));
            };
            let recv = j::Expr {
                kind: j::ExprKind::Name(n),
                span: b.span,
                paren: false,
            };
            let path = match t {
                Ty::Coll("iter", _) => format!("std.iter.ops.{m}"),
                _ => m.to_owned(),
            };
            return self.user_call(&path, Some(&recv), args, at);
        }
        if self.g.fns.contains_key(m) {
            if let Some(module) = self.ty_of_local(b).as_ref().and_then(Ty::module)
                && crate::stdlib::decl(&format!("std.{module}.{m}")).is_some()
            {
                self.cx.warn(
                    "W819",
                    at,
                    &self.item.clone(),
                    format!("this module's `{m}` hides `std.{module}.{m}`"),
                );
            }
            return self.user_call(m, Some(b), args, at);
        }
        self.dict_places.clear();
        let (p, t) = self.place(b)?;
        let mut stores = mem::take(&mut self.dict_places);
        if let Some(sig) = self.trait_method(&t, m) {
            return self.call_sig(m, &sig, Some(b), args, at);
        }
        let e = || Ty::Err;
        if t == Ty::Err {
            return Ok((Rhs::Proj(p), e(), None));
        }
        // `std.text` declares `upper` and `lower` on `str` only; a `char` goes through its text.
        if t == Ty::Char && matches!(m, "upper" | "lower") && args.is_empty() {
            let c = if p.projs.is_empty() {
                Atom::Name(p.root, p.root_span)
            } else {
                self.tmp(&t, Rhs::Proj(p), b.span)
            };
            let text = call("std.fmt.show", vec![t.targ()], vec![let_arg(c)], b.span);
            let s = self.tmp(&Ty::Str, Rhs::Call(text), b.span);
            let c = call(&format!("std.text.{m}"), vec![], vec![let_arg(s)], at);
            return Ok((Rhs::Call(c), Ty::Str, None));
        }
        let std_class = match &t {
            Ty::Data(n) if self.g.std_records.contains(n) => crate::stdlib::class_module(n),
            _ => None,
        };
        let Some(module) = t.module().or(std_class) else {
            self.err(
                "E200",
                at,
                format!(
                    "no function `{m}` takes a `{t}` first",
                    t = self.ty_text(&t)
                ),
            );
            return Ok((Rhs::Proj(p), e(), None));
        };
        if m == "strftime"
            && let Ty::Coll(n, _) = &t
        {
            self.pattern_check(n, args.first());
        }
        let path = format!("std.{module}.{m}");
        let Some(d) = crate::stdlib::decl(&path) else {
            let names = crate::stdlib::members(module);
            self.err_near(
                "E203",
                name_at,
                format!("`{t}` has no method `{m}`", t = self.ty_text(&t)),
                m,
                names,
            );
            return Ok((Rhs::Proj(p), e(), None));
        };
        if !d.intrinsic || d.typed_at_call() {
            return self.user_call(&path, Some(b), args, at);
        }
        // A fallible method (`b.decode_utf8()`) is typed as its plain signature, which carries
        // the failure type.
        if d.def.ret.as_ref().is_some_and(|r| !r.fail.is_empty())
            && let Some(mut plain) = d.plain()
        {
            let (_, self_ty) = plain.params.remove(0);
            plain.names.remove(0);
            self.expect(&t, &self_ty, b.span);
            let a = if p.projs.is_empty() {
                Atom::Name(p.root, p.root_span)
            } else {
                self.tmp(&t, Rhs::Proj(p), b.span)
            };
            return self.std_call(&path, plain, vec![let_arg(a)], args, m, at);
        }
        let Some(crate::stdlib::Method {
            inout,
            params,
            ret,
            targs,
        }) = d.method(&t)
        else {
            return unsupported(format!("`{path}` on `{t}`", t = self.ty_text(&t)));
        };
        let targs: Vec<_> = targs.iter().map(Ty::targ).collect();
        let mut writeback = None;
        let recv = if inout {
            if root(b).is_none() {
                self.err("E403", b.span, temporary_receiver(&format!("`{m}`")));
            }
            if p.projs.is_empty() {
                Arg::Value(Conv::Inout, Atom::Name(p.root, p.root_span), b.span)
            } else {
                // The method runs on a copy that is stored back.
                let v = self.fresh("t");
                self.bind(&v, true, &t, Rhs::Copy(p.clone()), b.span, b.span);
                writeback = Some((p, v.clone()));
                Arg::Value(Conv::Inout, Atom::Name(v, b.span), b.span)
            }
        } else {
            let a = if p.projs.is_empty() {
                Atom::Name(p.root, p.root_span)
            } else {
                self.tmp(&t, Rhs::Proj(p), b.span)
            };
            let_arg(a)
        };
        // Trailing parameters that default to `None` may be left out (`s.split()`).
        let defaults: Vec<_> = d
            .def
            .params
            .iter()
            .filter_map(|p| match p {
                j::Param::Value { default, .. } => Some(default),
                _ => None,
            })
            .skip(1)
            .collect();
        let none_defaults = defaults
            .iter()
            .rev()
            .take_while(|d| {
                matches!(
                    d,
                    Some(j::Expr {
                        kind: j::ExprKind::Lit(j::Lit::None),
                        ..
                    })
                )
            })
            .count();
        let n = args.len().clamp(params.len() - none_defaults, params.len());
        let Some(xs) = self.positional(args, n, m, at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), e(), None));
        };
        let mut out = vec![recv];
        for (x, (take, pt)) in xs.into_iter().zip(&params) {
            let (a, xt) = self.atom(x, Some(pt))?;
            self.expect(&xt, pt, x.span);
            let conv = if *take { Conv::Sink } else { Conv::Let };
            out.push(Arg::Value(conv, a, x.span));
        }
        for _ in n..params.len() {
            out.push(let_arg(Atom::Name("None".into(), at)));
        }
        let rhs = Rhs::Call(call(&path, targs, out, at));
        if writeback.is_none() && (!inout || stores.is_empty()) {
            return Ok((rhs, ret, None));
        }
        let r = self.tmp(&ret, rhs, at);
        if let Some((place, v)) = writeback {
            self.emit(
                StmtKind::Assign {
                    place,
                    rhs: Rhs::Atom(Atom::Name(v, at)),
                    rhs_span: at,
                },
                at,
            );
        }
        if inout {
            self.store_dict_places(mem::take(&mut stores), at);
        }
        Ok((Rhs::Atom(r), ret, None))
    }

    /// A contract expression that is one operation on names and literals, as Yong requires.
    fn pure(&mut self, e: &j::Expr) -> R<Option<y::Expr>> {
        if let Some((a, _)) = self.pure_atom(e) {
            return Ok(Some(y::Expr::Atom(a)));
        }
        let (op, a, b) = match &e.kind {
            j::ExprKind::Compare(op, a, b) | j::ExprKind::Binary(op, a, b) => (*op, a, b),
            _ => return Ok(None),
        };
        let (Some((x, tx)), Some((y_, ty))) = (self.pure_atom(a), self.pure_atom(b)) else {
            return Ok(None);
        };
        if tx != ty {
            return Ok(None);
        }
        let path = match (cmp_name(op), &tx) {
            (Some(c), t) if t.scalar().is_some() => format!("{c}_{}", t.scalar().expect("scalar")),
            (None, Ty::Int) => match int_op(op) {
                Some((p, _)) => p.to_owned(),
                None => return Ok(None),
            },
            (None, Ty::Float) => match float_op(op) {
                Some(p) => p.to_owned(),
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        Ok(Some(y::Expr::Call(call(
            &path,
            vec![],
            vec![let_arg(x), let_arg(y_)],
            e.span,
        ))))
    }

    fn pure_atom(&self, e: &j::Expr) -> Option<(Atom, Ty)> {
        match &e.kind {
            j::ExprKind::Name(n) => self
                .locals
                .get(n)
                .map(|l| (Atom::Name(n.clone(), e.span), l.ty.clone())),
            j::ExprKind::Lit(l) if !matches!(l, j::Lit::None) => {
                Some((Atom::Lit(literal(l).ok()?, e.span), lit_ty(l)))
            }
            j::ExprKind::Unary("-", x) => match &x.kind {
                j::ExprKind::Lit(j::Lit::Int(s)) => Some((
                    Atom::Lit(Literal::Int(format!("-{}", int_text(s).ok()?)), e.span),
                    Ty::Int,
                )),
                _ => None,
            },
            _ => None,
        }
    }
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

fn names_used(stmts: &[j::Stmt], out: &mut HashMap<String, Vec<Span>>) {
    for s in stmts {
        let exprs: Vec<&j::Expr> = match &s.kind {
            j::StmtKind::Assign { target, value, .. } => {
                let mut v: Vec<&j::Expr> = match &target.kind {
                    j::ExprKind::Name(_) => Vec::new(),
                    j::ExprKind::Tuple(ts) => ts
                        .iter()
                        .filter(|t| !matches!(t.kind, j::ExprKind::Name(_)))
                        .collect(),
                    _ => vec![target],
                };
                v.push(value);
                v
            }
            _ => stmt_exprs(s),
        };
        for e in exprs {
            walk(e, &mut |e| {
                if let j::ExprKind::Name(n) = &e.kind {
                    out.entry(n.clone()).or_default().push(e.span);
                }
            });
        }
        for b in sub_blocks(s) {
            names_used(b, out);
        }
    }
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
    fn used_after(&self, n: &str, at: Span) -> bool {
        self.next_use(n, at).is_some()
    }

    /// The first use of `n` after `at`.
    fn next_use(&self, n: &str, at: Span) -> Option<Span> {
        self.uses
            .get(n)?
            .iter()
            .filter(|s| (s.line, s.col) > (at.line, at.col))
            .min_by_key(|s| (s.line, s.col))
            .copied()
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
pub(crate) fn callees(stmts: &[j::Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        for e in stmt_exprs(s) {
            walk(e, &mut |e| {
                if let j::ExprKind::Call(f, _) = &e.kind {
                    match &f.kind {
                        j::ExprKind::Name(n) | j::ExprKind::Attr(_, n, _) => {
                            out.insert(n.clone());
                        }
                        _ => {}
                    }
                }
            });
        }
        for b in sub_blocks(s) {
            callees(b, out);
        }
    }
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
