//! Function bodies: Jian statements and expressions to Yong statements in A-normal form.
//!
//! Every compound expression becomes a typed `let` of a fresh `_tN`. A value that joins
//! control flow (`a if c else b`, `and`, `or`, `x else y`) becomes a join point whose body is
//! the rest of the enclosing block: the block is split where the join is made and closed
//! when the block ends.

use std::collections::{HashMap, HashSet};
use std::mem;

use yong_core::ast::{self as y, Arg, Atom, Conv, Literal, Place, Proj, Rhs, StmtKind};
use yong_diag::Span;
use yong_hir as j;

use crate::bounds::{self, Site};
use crate::{Cx, Globals, R, Sig, Ty, cap_type, unsupported};

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
    loops: Vec<String>,
    n: usize,
    /// Lowering a `become` call, whose arguments are exempt from E210 (SPEC_QUESTIONS).
    in_become: bool,
    /// Every read of each name, for last-use moves (E413).
    uses: HashMap<String, Vec<Span>>,
    ghosts: HashSet<String>,
    /// The parameter N05 took as the measure.
    inferred: Option<String>,
    /// User bindings and where each is made, for W812.
    bound: Vec<(String, Span)>,
    /// What each index expression indexes, for the bounds solver.
    sites: HashMap<Span, Site>,
}

pub(crate) fn literal(l: &j::Lit) -> R<Literal> {
    Ok(match l {
        j::Lit::Int(s) => Literal::Int(int_text(s)?),
        j::Lit::Float(s) => Literal::Float(s.replace('_', "")),
        j::Lit::Str { prefix, value } if prefix.is_empty() || prefix == "r" => {
            Literal::Str(value.clone())
        }
        j::Lit::Str { .. } => return unsupported("byte and C strings"),
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
        K::Attr(b, ..) | K::Try(b) | K::Unary(_, b) | K::Not(b) | K::Lambda(_, b) => walk(b, f),
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
                j::Fallback::Break | j::Fallback::Continue => {}
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

fn root(e: &j::Expr) -> Option<&str> {
    match &e.kind {
        j::ExprKind::Name(n) => Some(n),
        j::ExprKind::Attr(b, ..) | j::ExprKind::Index(b, _) => root(b),
        _ => None,
    }
}

/// Exprs directly in a statement, for the mutability scan.
fn stmt_exprs(s: &j::Stmt) -> Vec<&j::Expr> {
    use j::StmtKind as K;
    match &s.kind {
        K::Expr(e) | K::Become(e) | K::Fail(e) | K::Assert(e) => vec![e],
        K::Assign { target, value, .. } | K::AugAssign { target, value, .. } => vec![target, value],
        K::Ghost { value, .. } => vec![value],
        K::Return(e) => e.iter().collect(),
        K::If { arms, .. } => arms.iter().map(|(c, _)| c).collect(),
        K::While { cond, .. } => vec![cond],
        K::For { iter, .. } => vec![iter],
        K::Match { scrutinee, .. } => vec![scrutinee],
        K::Par {
            body: j::ParBody::For { iter, .. },
            ..
        } => vec![iter],
        _ => Vec::new(),
    }
}

fn sub_blocks(s: &j::Stmt) -> Vec<&[j::Stmt]> {
    use j::StmtKind as K;
    match &s.kind {
        K::If { arms, els } => arms
            .iter()
            .map(|(_, b)| b.stmts.as_slice())
            .chain(els.iter().map(|b| b.stmts.as_slice()))
            .collect(),
        K::While { body, .. } | K::For { body, .. } => vec![body],
        K::Match { cases, .. } => cases.iter().map(|c| c.body.stmts.as_slice()).collect(),
        K::With { body, .. } => vec![&body.stmts],
        K::Par { body, .. } => match body {
            j::ParBody::Block(b) => vec![b],
            j::ParBody::For { body, .. } => vec![body],
        },
        _ => Vec::new(),
    }
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
                        if var_first || MUTATING.contains(&m.as_str()) {
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

fn ends(stmts: &[j::Stmt]) -> bool {
    matches!(
        stmts.last().map(|s| &s.kind),
        Some(
            j::StmtKind::Return(_)
                | j::StmtKind::Fail(_)
                | j::StmtKind::Become(_)
                | j::StmtKind::Unreachable
        )
    )
}

pub(crate) fn function(g: &Globals, f: &j::FuncDef, is_pub: bool, cx: &mut Cx) -> R<y::FnDef> {
    let sig = &g.fns[&f.name];
    let Some(body) = &f.body else {
        return unsupported("functions without bodies");
    };
    let mut assigned = HashMap::new();
    let mut mutable = HashSet::new();
    mutables(g, &body.stmts, &mut assigned, &mut mutable);
    mutable.extend(assigned.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k));
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
        uses: HashMap::new(),
        ghosts: HashSet::new(),
        inferred: None,
        bound: Vec::new(),
        sites: HashMap::new(),
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
            j::Contract::Post(_) => return unsupported("`post`"),
            j::Contract::Ex { .. } => {}
        }
    }
    if let Some(p) = l.inferred.clone() {
        contracts.push(y::Contract::Decreases(vec![y::Expr::Atom(Atom::Name(
            p,
            f.name_span,
        ))]));
    }
    let block = l.block_with(|l| {
        for e in pre {
            let (a, t) = l.atom(e, Some(&Ty::Bool))?;
            l.expect(&t, &Ty::Bool, e.span);
            l.emit(StmtKind::Assert(y::Expr::Atom(a)), e.span);
        }
        l.stmts(&body.stmts)?;
        if !ends(&body.stmts) && matches!(sig.ret, Ty::Unit) {
            let at = body.stmts.last().map_or(f.name_span, |s| s.span);
            l.emit(StmtKind::Ret(Atom::Lit(Literal::Unit, at)), at);
        }
        Ok(())
    })?;
    let unused: Vec<(String, Span)> = sig
        .params
        .iter()
        .map(|p| (p.name.clone(), p.span))
        .chain(mem::take(&mut l.bound))
        .filter(|(n, _)| !n.starts_with('_') && l.uses.get(n).is_none_or(Vec::is_empty))
        .collect();
    let item = l.item.clone();
    for b in bounds::check(&body.stmts, &l.sites, l.cx.release) {
        match b.code {
            "E501" => l.cx.err(b.code, b.span, &item, b.message),
            _ => l.cx.warn(b.code, b.span, &item, b.message),
        }
    }
    for (n, sp) in unused {
        let item = l.item.clone();
        l.cx.warn(
            "W812",
            sp,
            &item,
            format!("`{n}` is never read; name it `_{n}` or remove it"),
        );
    }
    Ok(y::FnDef {
        is_pub,
        is_unsafe: false,
        path: vec![f.name.clone()],
        name_span: f.name_span,
        tparams: Vec::new(),
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

fn bigint_op(op: &str) -> Option<&'static str> {
    Some(match op {
        "+" => "add_bigint",
        "-" => "sub_bigint",
        "*" => "mul_bigint",
        "//" => "floordiv_bigint",
        "%" => "mod_bigint",
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

    fn err(&mut self, code: &str, span: Span, msg: String) {
        let item = self.item.clone();
        self.cx.err(code, span, &item, msg);
    }

    fn expect(&mut self, got: &Ty, want: &Ty, at: Span) {
        if !got.fits(want) {
            self.err("E201", at, format!("expected `{want}`, found `{got}`"));
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
        for (name, prev) in self.scopes.pop().expect("pushed").into_iter().rev() {
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
        stmts.iter().try_for_each(|s| self.stmt(s))
    }

    fn label(&mut self) -> String {
        self.fresh("l")
    }

    fn stmt(&mut self, s: &j::Stmt) -> R<()> {
        use j::StmtKind as K;
        let at = s.span;
        match &s.kind {
            K::Pass | K::Reveal(_) => {}
            K::Ghost { name, .. } => {
                self.ghosts.insert(name.clone());
            }
            K::Expr(e) => {
                let (rhs, t) = self.rhs(e, None)?;
                if !matches!(t, Ty::Unit | Ty::Never | Ty::Err) && matches!(rhs, Rhs::Call(_)) {
                    self.err("E312", at, format!("a result of type `{t}` is discarded"));
                }
                if !matches!(rhs, Rhs::Atom(_)) {
                    self.emit(StmtKind::Rhs(rhs), at);
                }
            }
            K::Assign { target, ty, value } => self.assign(target, ty.as_ref(), value, at)?,
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
                self.emit(StmtKind::Ret(a), at);
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
            K::Break | K::Continue => {
                let Some(label) = self.loops.last().cloned() else {
                    return unsupported("`break` outside a loop");
                };
                let kind = if matches!(s.kind, K::Break) {
                    StmtKind::Break { label, value: None }
                } else {
                    StmtKind::Continue { label }
                };
                self.emit(kind, at);
            }
            K::Assert(e) => {
                let (a, t) = self.atom(e, Some(&Ty::Bool))?;
                self.expect(&t, &Ty::Bool, e.span);
                self.emit(StmtKind::Assert(y::Expr::Atom(a)), at);
            }
            K::Unreachable => self.emit(StmtKind::Unreachable, at),
            K::If { arms, els } => self.if_(arms, els.as_ref(), at)?,
            K::While { cond, body, .. } => {
                let label = self.label();
                self.loops.push(label.clone());
                let b = self.block_with(|l| {
                    let (c, t) = l.atom(cond, Some(&Ty::Bool))?;
                    l.expect(&t, &Ty::Bool, cond.span);
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
                    l.stmts(body)
                });
                self.loops.pop();
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
                target, iter, body, ..
            } => self.for_(target, iter, body, at)?,
            K::Match { scrutinee, cases } => self.match_(scrutinee, cases, at)?,
            K::Par {
                concurrent,
                limit: None,
                body: j::ParBody::Block(b),
            } => {
                let mut blocks = Vec::new();
                for s in b {
                    blocks.push(self.block(std::slice::from_ref(s))?);
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
            K::With { .. } => return unsupported("`with`"),
        }
        Ok(())
    }

    fn if_(&mut self, arms: &[(j::Expr, j::Block)], els: Option<&j::Block>, at: Span) -> R<()> {
        let (cond, body) = &arms[0];
        let (c, t) = self.atom(cond, Some(&Ty::Bool))?;
        self.expect(&t, &Ty::Bool, cond.span);
        let then = self.block(&body.stmts)?;
        let els = if arms.len() > 1 {
            Some(self.block_with(|l| l.if_(&arms[1..], els, at))?)
        } else {
            els.map(|b| self.block(&b.stmts)).transpose()?
        };
        self.emit(StmtKind::If { cond: c, then, els }, at);
        Ok(())
    }

    fn for_(&mut self, target: &[String], iter: &j::Expr, body: &[j::Stmt], at: Span) -> R<()> {
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
            let (xs, t) = self.atom(iter, None)?;
            let e = match &t {
                Ty::List(e) => (**e).clone(),
                Ty::Str => {
                    self.err(
                        "E504",
                        iter.span,
                        "a `str` is not iterable; use `s.chars()`".into(),
                    );
                    return Ok(());
                }
                Ty::Err => return Ok(()),
                t => return unsupported(format!("iterating a `{t}`")),
            };
            let zero = Atom::Lit(Literal::Int("0".into()), at);
            self.bind(&i, true, &Ty::Int, Rhs::Atom(zero), at, at);
            let len = call(
                "std.list.len",
                vec![e.targ()],
                vec![let_arg(xs.clone())],
                at,
            );
            let n = self.tmp(&Ty::Int, Rhs::Call(len), at);
            (Some((xs, e)), n, one)
        };
        self.loops.push(label.clone());
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
            match (target, &ty) {
                ([n], _) => {
                    let m = l.mutable.contains(n);
                    l.bind(n, m, &ty, cur, at, iter.span);
                }
                (ns, Ty::Tuple(ts)) if ns.len() == ts.len() => {
                    let e = l.tmp(&ty, cur, at);
                    let Atom::Name(e, _) = e else {
                        unreachable!("a projection is bound")
                    };
                    for (k, (n, t)) in ns.iter().zip(ts).enumerate() {
                        let p = Rhs::Proj(Place {
                            root: e.clone(),
                            root_span: at,
                            projs: vec![Proj::Field(format!("_{k}"), at)],
                        });
                        let m = l.mutable.contains(n);
                        l.bind(n, m, t, p, at, at);
                    }
                }
                _ => {
                    l.err(
                        "E201",
                        at,
                        format!("cannot unpack `{ty}` into {} names", target.len()),
                    );
                }
            }
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
            l.stmts(body)
        });
        self.loops.pop();
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
            && let Some(c) = cases.iter().find(|c| matches!(c.pat, j::Pat::Wild))
        {
            self.err(
                "E502",
                c.span,
                format!("`case _` on the sum `{d}`; name each variant"),
            );
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
            self.err("E503", at, format!("this `match` on `{t}` needs `case _`"));
        }
        let mut arms = Vec::new();
        for c in cases {
            if c.guard.is_some() {
                return unsupported("case guards");
            }
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
                        self.err("E201", at, format!("pattern `{n}` cannot match a `{ty}`"));
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
                    self.err("E201", at, format!("a tuple pattern cannot match a `{ty}`"));
                    return Ok((y::Pat::Wild, Vec::new()));
                };
                if ts.len() != ps.len() {
                    self.err(
                        "E201",
                        at,
                        format!("a {}-tuple pattern cannot match a `{ty}`", ps.len()),
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
                                    format!("cannot unpack `{t}` into {} targets", ts.len()),
                                );
                            }
                            return Ok(());
                        };
                        if tys.len() != ts.len() {
                            self.err(
                                "E201",
                                value.span,
                                format!("cannot unpack `{t}` into {} targets", ts.len()),
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
                let (r, t) = self.rhs(value, want.as_ref())?;
                self.store(target, ann, r, t, value.span)
            }
            _ => {
                let (r, t) = self.rhs(value, None)?;
                self.store(target, ann, r, t, value.span)
            }
        }
    }

    fn store(&mut self, target: &j::Expr, ann: Option<Ty>, r: Rhs, t: Ty, at: Span) -> R<()> {
        if let Rhs::Atom(Atom::Name(x, _)) = &r
            && !t.is_copy()
            && self.locals.contains_key(x)
            && !x.starts_with('_')
            && self.used_after(x, at)
        {
            self.err(
                "E413",
                at,
                format!("`{x}` is used again later; bind `{x}.copy()`"),
            );
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
                let want = self.locals[n].ty.clone();
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
                let ty = match ann {
                    Some(a) => {
                        self.expect(&t, &a, at);
                        a
                    }
                    None => t,
                };
                let m = self.mutable.contains(n);
                self.bind(n, m, &ty, r, target.span, at);
                self.bound.push((n.clone(), target.span));
            }
            j::ExprKind::Index(d, k) if matches!(self.ty_of_local(d), Some(Ty::Dict(..))) => {
                let Some(Ty::Dict(kt, vt)) = self.ty_of_local(d) else {
                    unreachable!("matched")
                };
                let j::ExprKind::Name(dn) = &d.kind else {
                    unreachable!("a local")
                };
                let (ka, kty) = self.atom(k, Some(&kt))?;
                self.expect(&kty, &kt, k.span);
                self.expect(&t, &vt, at);
                let v = self.tmp(&t, r, at);
                let c = call(
                    "std.dict.insert",
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
                let (p, pt) = self.place(target)?;
                self.expect(&t, &pt, at);
                self.emit(
                    StmtKind::Assign {
                        place: p,
                        rhs: r,
                        rhs_span: at,
                    },
                    target.span,
                );
            }
            j::ExprKind::Wild => self.emit(StmtKind::Rhs(r), target.span),
            _ => return unsupported("this assignment target"),
        }
        Ok(())
    }

    fn ty_of_local(&self, e: &j::Expr) -> Option<Ty> {
        match &e.kind {
            j::ExprKind::Name(n) => self.locals.get(n).map(|l| l.ty.clone()),
            _ => None,
        }
    }

    fn place(&mut self, e: &j::Expr) -> R<(Place, Ty)> {
        match &e.kind {
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
                                self.err("E203", *fsp, format!("`{r}` has no field `{f}`"));
                                Ty::Err
                            }
                        }
                    }
                    t => {
                        self.err("E203", *fsp, format!("`{t}` has no field `{f}`"));
                        Ty::Err
                    }
                };
                p.projs.push(Proj::Field(f.clone(), *fsp));
                Ok((p, ft))
            }
            j::ExprKind::Index(b, i) => {
                let (mut p, t) = self.place(b)?;
                match &t {
                    Ty::List(et) => {
                        let et = (**et).clone();
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
                    Ty::Str => {
                        self.err(
                            "E504",
                            e.span,
                            "`str` has no indexing; use `s.chars()`".into(),
                        );
                        Ok((p, Ty::Err))
                    }
                    Ty::Err => Ok((p, Ty::Err)),
                    t => unsupported(format!("indexing a `{t}` as a place")),
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
                    Atom::Lit(..) => unsupported("a projection of a literal"),
                }
            }
        }
    }

    fn atom(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Atom, Ty)> {
        let (r, t) = self.rhs(e, want)?;
        Ok((self.tmp(&t, r, e.span), t))
    }

    fn rhs(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Rhs, Ty)> {
        use j::ExprKind as K;
        let at = e.span;
        Ok(match &e.kind {
            K::Name(n) => {
                if let Some(l) = self.locals.get(n) {
                    (Rhs::Atom(Atom::Name(n.clone(), at)), l.ty.clone())
                } else if let Some(t) = self.g.consts.get(n) {
                    (Rhs::Atom(Atom::Name(n.clone(), at)), t.clone())
                } else if let Some(s) = self.g.variant_of.get(n) {
                    (Rhs::Atom(Atom::Name(n.clone(), at)), Ty::Data(s.clone()))
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
                    self.err("E200", at, format!("cannot find `{n}`"));
                    (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)
                }
            }
            K::Lit(j::Lit::None) => match want {
                Some(t @ Ty::Option(_)) => (Rhs::Atom(Atom::Name("None".into(), at)), t.clone()),
                _ => (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Unit),
            },
            K::Lit(l) => {
                let t = match (l, want) {
                    (j::Lit::Int(_), Some(w @ Ty::Fixed(n))) if !n.starts_with('f') => w.clone(),
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
            K::Attr(..) => {
                let (p, t) = self.place(e)?;
                (Rhs::Proj(p), t)
            }
            K::Index(b, k) => {
                let (p, t) = self.place(b)?;
                match &t {
                    Ty::Dict(kt, vt) => {
                        self.sites.insert(e.span, Site::Dict);
                        let (kt, vt) = ((**kt).clone(), (**vt).clone());
                        let d = self.tmp(&t, Rhs::Proj(p), b.span);
                        let (ka, kty) = self.atom(k, Some(&kt))?;
                        self.expect(&kty, &kt, k.span);
                        let c = call(
                            "std.dict.index",
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
                    (Some(_), Rhs::Call(c)) => (Rhs::Propagate { call: c, via: None }, t),
                    _ => return unsupported("`?` on an optional or on a call that cannot fail"),
                }
            }
            K::Unary(op, x) => match (*op, &x.kind) {
                ("-", K::Lit(j::Lit::Int(s))) => (
                    Rhs::Atom(Atom::Lit(Literal::Int(format!("-{}", int_text(s)?)), at)),
                    Ty::Int,
                ),
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
                        Ty::Err => return Ok((Rhs::Atom(a), Ty::Err)),
                        t => {
                            self.err("E201", x.span, format!("cannot negate a `{t}`"));
                            return Ok((Rhs::Atom(a), Ty::Err));
                        }
                    };
                    (Rhs::Call(c), t)
                }
                _ => return unsupported(format!("unary `{op}`")),
            },
            K::Not(x) => {
                let (a, t) = self.atom(x, Some(&Ty::Bool))?;
                self.expect(&t, &Ty::Bool, x.span);
                (
                    Rhs::Call(call("not_bool", vec![], vec![let_arg(a)], at)),
                    Ty::Bool,
                )
            }
            K::Binary(op @ ("and" | "or"), a, b) => {
                let (x, t) = self.atom(a, Some(&Ty::Bool))?;
                self.expect(&t, &Ty::Bool, a.span);
                let op = *op;
                let (v, t) = self.join(at, |l, k| {
                    let (rest, _) = l.arm(k, b, Some(&Ty::Bool))?;
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
                let (c, t) = self.atom(cond, Some(&Ty::Bool))?;
                self.expect(&t, &Ty::Bool, cond.span);
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
            K::Comp { .. } => return unsupported("comprehensions"),
            K::Slice(..) => return unsupported("slices"),
            K::Lambda(..) => return unsupported("lambdas"),
            K::Set(_) => return unsupported("set literals"),
            K::Wild => return unsupported("`_` as a value"),
        })
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
                                format!("`else` needs an optional or a fallible call, found `{t}`"),
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
                            format!("`else` needs an optional or a fallible call, found `{t}`"),
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
                (j::Fallback::Bind(n, _), Some(_)) => (
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
        let (y_, ty) = self.atom(b, Some(&tx))?;
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
            (Ty::Str, Ty::Str) if op == "+" => ("std.text.concat".to_owned(), Ty::Str, vec![]),
            (Ty::List(e), Ty::List(_)) if op == "+" => {
                ("std.list.concat".to_owned(), tx.clone(), vec![e.targ()])
            }
            _ => {
                self.err("E201", at, format!("`{op}` between `{tx}` and `{ty}`"));
                return Ok((Rhs::Atom(x), Ty::Err));
            }
        };
        Ok((Rhs::Call(call(&path, targs, args, at)), rt))
    }

    fn compare(&mut self, op: &str, a: &j::Expr, b: &j::Expr, at: Span) -> R<(Rhs, Ty)> {
        if matches!(op, "is" | "is not") {
            return unsupported("`is None` narrowing");
        }
        if matches!(op, "in" | "not in") {
            let (x, _) = self.atom(a, None)?;
            let (c, ct) = self.atom(b, None)?;
            let (path, targs) = match &ct {
                Ty::List(e) => ("std.list.contains", vec![e.targ()]),
                Ty::Set(e) => ("std.set.contains", vec![e.targ()]),
                Ty::Dict(k, v) => ("std.dict.contains_key", vec![k.targ(), v.targ()]),
                Ty::Str => ("std.text.contains", vec![]),
                Ty::Err => return Ok((Rhs::Atom(x), Ty::Err)),
                t => {
                    self.err("E201", b.span, format!("`in` on a `{t}`"));
                    return Ok((Rhs::Atom(x), Ty::Err));
                }
            };
            let r = Rhs::Call(call(path, targs, vec![let_arg(c), let_arg(x)], at));
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
        let (x, tx) = self.atom(a, None)?;
        let (y_, ty) = self.atom(b, Some(&tx))?;
        if matches!(tx, Ty::Err) || matches!(ty, Ty::Err) {
            return Ok((Rhs::Atom(x), Ty::Bool));
        }
        if !ty.fits(&tx) && !tx.fits(&ty) {
            self.err("E201", b.span, format!("cannot compare `{tx}` with `{ty}`"));
            return Ok((Rhs::Atom(x), Ty::Bool));
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

fn lit_ty(l: &j::Lit) -> Ty {
    match l {
        j::Lit::Int(_) => Ty::Int,
        j::Lit::Float(_) => Ty::Float,
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
        match &f.kind {
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
            j::ExprKind::Attr(b, m, _) => {
                if let j::ExprKind::Name(c) = &b.kind
                    && !self.locals.contains_key(c)
                    && (self.sig.caps.contains(c) || c == "io")
                {
                    let (r, t) = self.cap_call(c, m, args, at)?;
                    return Ok((r, t, None));
                }
                self.method(b, m, args, at)
            }
            _ => unsupported("calling a value"),
        }
    }

    fn user_call(
        &mut self,
        name: &str,
        recv: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let sig = &self.g.fns[name];
        let mut out: Vec<Arg> = sig.caps.iter().map(|c| Arg::Cap(c.clone(), at)).collect();
        let Some(vals) = self.args(&sig.params, recv, args, at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        out.extend(vals);
        Ok((
            Rhs::Call(call(name, vec![], out, at)),
            sig.ret.clone(),
            sig.fail.clone(),
        ))
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
        let mut out = Vec::new();
        for (p, slot) in ps.iter().zip(slots) {
            let (e, is_ref, sp) = match slot {
                Some(Given::Expr(e, r, sp, _)) => (e, r, sp),
                Some(Given::Recv(e)) => (e, p.conv == Conv::Inout, e.span),
                None => match &p.default {
                    Some(d) => {
                        let (a, t) = self.atom(&d.clone(), Some(&p.ty))?;
                        self.expect(&t, &p.ty, at);
                        out.push(Arg::Value(p.conv, a, at));
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
                    let (pl, t) = self.place(e)?;
                    self.expect(&t, &p.ty, sp);
                    if !pl.projs.is_empty() {
                        return unsupported("`&` of a field or element");
                    }
                    out.push(Arg::Value(
                        Conv::Inout,
                        Atom::Name(pl.root, pl.root_span),
                        sp,
                    ));
                }
                (Conv::Inout, false) => {
                    self.err(
                        "E404",
                        sp,
                        format!("`{}` is a `var` parameter; pass `&place`", p.name),
                    );
                }
                (_, true) => {
                    self.err(
                        "E405",
                        sp,
                        format!("`{}` is not a `var` parameter; remove `&`", p.name),
                    );
                }
                (conv, false) => {
                    let (a, t) = self.atom(e, Some(&p.ty))?;
                    self.expect(&t, &p.ty, sp);
                    if let (Conv::Sink, Atom::Name(n, _)) = (conv, &a)
                        && !t.is_copy()
                        && self.locals.contains_key(n)
                        && self.used_after(n, e.span)
                    {
                        self.err(
                            "E413",
                            sp,
                            format!("`{n}` is used again later; pass `{n}.copy()`"),
                        );
                    }
                    out.push(Arg::Value(conv, a, sp));
                }
            }
        }
        Ok(Some(out))
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
                    ("len", Ty::Str) if self.cx.strict => {
                        self.err(
                            "E504",
                            at,
                            "`len` of a `str`; use `s.char_count()` or `s.byte_len()`".into(),
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
                        self.err("E201", xs[0].span, format!("`{n}` does not take a `{t}`"));
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
            "print" => self.cap_call("io", "print", args, at),
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
                        format!("`evolve` takes a record, found `{t}`"),
                    );
                    return Ok(err);
                };
                let Some(fs) = self.g.records.get(r) else {
                    self.err(
                        "E201",
                        c.span,
                        format!("`evolve` takes a record, found `{t}`"),
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
                        self.err("E203", a.span, format!("`{r}` has no field `{name}`"));
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
            "dict" | "set" | "enumerate" | "zip" | "sum" | "any" | "all" | "reversed"
            | "isinstance" | "hash" | "repr" | "divmod" | "pow" | "chr" | "ord" | "atan2"
            | "hypot" => unsupported(format!("`{n}`")),
            _ => {
                self.err("E200", at, format!("cannot find function `{n}`"));
                for a in args {
                    if let j::ArgKind::Expr(e) | j::ArgKind::Ref(e) = &a.kind {
                        self.atom(e, None)?;
                    }
                }
                Ok(err)
            }
        }
    }

    /// `io.print(s)`; other capability operations come with std.
    fn cap_call(&mut self, cap: &str, op: &str, args: &[j::Arg], at: Span) -> R<(Rhs, Ty)> {
        if (cap, op) != ("io", "print") {
            return unsupported(format!("capability operation `{cap}.{op}`"));
        }
        let Some(xs) = self.positional(args, 1, "print", at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
        };
        let (a, t) = self.atom(xs[0], Some(&Ty::Str))?;
        self.expect(&t, &Ty::Str, xs[0].span);
        let c = call(
            "std.io.print",
            vec![],
            vec![Arg::Cap(cap.to_owned(), at), let_arg(a)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Unit))
    }

    fn method(
        &mut self,
        b: &j::Expr,
        m: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        if m == "copy" && args.is_empty() {
            let (p, t) = self.place(b)?;
            return Ok((Rhs::Copy(p), t, None));
        }
        if self.g.fns.contains_key(m) {
            return self.user_call(m, Some(b), args, at);
        }
        let (p, t) = self.place(b)?;
        let e = || Ty::Err;
        // (path, receiver is var, argument types, result)
        let (path, inout, params, ret): (String, bool, Vec<Ty>, Ty) = match (&t, m) {
            (Ty::Err, _) => return Ok((Rhs::Proj(p), e(), None)),
            (Ty::List(x), "append") => ("append".into(), true, vec![(**x).clone()], Ty::Unit),
            (Ty::List(x), "pop") => ("pop".into(), true, vec![], (**x).clone()),
            (Ty::Str, "byte_len" | "char_count" | "grapheme_count") => {
                (m.into(), false, vec![], Ty::Int)
            }
            (Ty::Str, "upper" | "lower" | "strip") => (m.into(), false, vec![], Ty::Str),
            (Ty::Str, "startswith" | "endswith") => (m.into(), false, vec![Ty::Str], Ty::Bool),
            (Ty::Str, "splitlines") => (m.into(), false, vec![], Ty::List(Box::new(Ty::Str))),
            (Ty::Str, "partition") => (
                m.into(),
                false,
                vec![Ty::Str],
                Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Str]),
            ),
            (Ty::Str, "parse_int") => (m.into(), false, vec![], Ty::Option(Box::new(Ty::Int))),
            (Ty::Str, "parse_float") => (m.into(), false, vec![], Ty::Option(Box::new(Ty::Float))),
            (Ty::List(x), "index") => (
                m.into(),
                false,
                vec![(**x).clone()],
                Ty::Option(Box::new(Ty::Int)),
            ),
            (Ty::List(x), "get") => (m.into(), false, vec![Ty::Int], Ty::Option(x.clone())),
            (Ty::Set(x), "add" | "discard") => (m.into(), true, vec![(**x).clone()], Ty::Unit),
            (Ty::Set(a), "union" | "intersection") => {
                (m.into(), false, vec![t.clone()], Ty::Set(a.clone()))
            }
            (Ty::Dict(_, v), "remove") => {
                (m.into(), true, vec![dict_key(&t)], Ty::Option(v.clone()))
            }
            (Ty::Dict(_, v), "setdefault") => (
                m.into(),
                true,
                vec![dict_key(&t), (**v).clone()],
                (**v).clone(),
            ),
            (Ty::Str, "split") => (m.into(), false, vec![Ty::Str], Ty::List(Box::new(Ty::Str))),
            (Ty::Str, "join") => (m.into(), false, vec![Ty::List(Box::new(Ty::Str))], Ty::Str),
            (Ty::Str, "replace") => (m.into(), false, vec![Ty::Str, Ty::Str], Ty::Str),
            (Ty::Dict(_, v), "get") => (m.into(), false, vec![dict_key(&t)], Ty::Option(v.clone())),
            (Ty::Dict(k, v), "items") => (
                m.into(),
                false,
                vec![],
                Ty::List(Box::new(Ty::Tuple(vec![(**k).clone(), (**v).clone()]))),
            ),
            (Ty::Dict(k, _), "keys") => (m.into(), false, vec![], Ty::List(k.clone())),
            (Ty::Dict(_, v), "values") => (m.into(), false, vec![], Ty::List(v.clone())),
            _ => {
                if t.module().is_some() {
                    return unsupported(format!("method `{m}` on `{t}`"));
                }
                self.err("E200", at, format!("no function `{m}` takes a `{t}` first"));
                return Ok((Rhs::Proj(p), e(), None));
            }
        };
        let module = t.module().expect("std types have a module");
        let targs = match &t {
            Ty::List(x) | Ty::Set(x) => vec![x.targ()],
            Ty::Dict(k, v) => vec![k.targ(), v.targ()],
            _ => vec![],
        };
        let recv = if inout {
            if !p.projs.is_empty() {
                return unsupported("a `var` method on a field or element");
            }
            Arg::Value(Conv::Inout, Atom::Name(p.root, p.root_span), b.span)
        } else {
            let a = if p.projs.is_empty() {
                Atom::Name(p.root, p.root_span)
            } else {
                self.tmp(&t, Rhs::Proj(p), b.span)
            };
            let_arg(a)
        };
        let Some(xs) = self.positional(args, params.len(), m, at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), e(), None));
        };
        let mut out = vec![recv];
        for (x, pt) in xs.into_iter().zip(&params) {
            let (a, xt) = self.atom(x, Some(pt))?;
            self.expect(&xt, pt, x.span);
            let conv = if inout && !matches!(m, "discard" | "remove") {
                Conv::Sink
            } else {
                Conv::Let
            };
            out.push(Arg::Value(conv, a, x.span));
        }
        let path = format!("std.{module}.{path}");
        Ok((Rhs::Call(call(&path, targs, out, at)), ret, None))
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

fn dict_key(t: &Ty) -> Ty {
    match t {
        Ty::Dict(k, _) => (**k).clone(),
        _ => Ty::Err,
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
        self.uses
            .get(n)
            .is_some_and(|v| v.iter().any(|s| (s.line, s.col) > (at.line, at.col)))
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
