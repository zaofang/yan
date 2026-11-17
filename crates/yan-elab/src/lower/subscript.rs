//! Subscripts (4.4, R457): `x[k]` on a type with `impl Index[K]` is the inlined body of its `sub at`.
//!
//! A read in an expression becomes the select expression of the yielded places. An assignment
//! target, an augmented assignment or an `&` argument repeats the statement at each `yield`, so the
//! code after the `yield` runs when that statement ends.

use super::*;
use j::{ExprKind as E, StmtKind as K};

const ZERO: u8 = 1;
const ONE: u8 = 2;
const MANY: u8 = 4;

/// E422: the body of `at` must yield exactly once on every path, reported at its name.
pub(crate) fn check_yields(at: &j::FuncDef, cx: &mut Cx) {
    let Some(body) = &at.body else { return };
    if yields(&body.stmts) != ONE {
        cx.err(
            "E422",
            at.name_span,
            &at.name,
            "a subscript body must yield exactly once on every path".into(),
        );
    }
}

fn add(a: u8, b: u8) -> u8 {
    let mut out = 0;
    for x in [ZERO, ONE, MANY] {
        for y in [ZERO, ONE, MANY] {
            if a & x != 0 && b & y != 0 {
                out |= match (x, y) {
                    (ZERO, n) | (n, ZERO) => n,
                    _ => MANY,
                };
            }
        }
    }
    out
}

fn yields(stmts: &[j::Stmt]) -> u8 {
    stmts.iter().fold(ZERO, |acc, s| add(acc, yields_of(s)))
}

fn yields_of(s: &j::Stmt) -> u8 {
    match &s.kind {
        K::Yield(_) => ONE,
        K::If { arms, els } => {
            let mut m = els.as_ref().map_or(ZERO, |b| yields(&b.stmts));
            for (_, b) in arms {
                m |= yields(&b.stmts);
            }
            m
        }
        K::Match { cases, .. } => cases.iter().fold(0, |m, c| m | yields(&c.body.stmts)),
        K::While { body, .. } | K::For { body, .. } => {
            if yields(body) == ZERO {
                ZERO
            } else {
                ZERO | MANY
            }
        }
        _ => ZERO,
    }
}

fn children_mut(e: &mut j::Expr, f: &mut dyn FnMut(&mut j::Expr)) {
    match &mut e.kind {
        E::Tuple(xs) | E::List(xs) | E::Set(xs) => xs.iter_mut().for_each(f),
        E::FStr { pieces, .. } => {
            for p in pieces {
                if let j::FPiece::Expr(x, _) = p {
                    f(x);
                }
            }
        }
        E::Dict(kvs) => kvs.iter_mut().for_each(|(k, v)| {
            f(k);
            f(v);
        }),
        E::Attr(b, ..) | E::Try(b) | E::Unary(_, b) | E::Not(b) | E::Cast(b, _) => f(b),
        E::Call(c, args) => {
            f(c);
            for a in args {
                match &mut a.kind {
                    j::ArgKind::Expr(x) | j::ArgKind::Ref(x) | j::ArgKind::Become(x) => f(x),
                }
            }
        }
        E::Index(a, b) | E::Binary(_, a, b) | E::Compare(_, a, b) => {
            f(a);
            f(b);
        }
        E::Cond { then, cond, els } => {
            f(then);
            f(cond);
            f(els);
        }
        E::Slice(b, parts, _) => {
            f(b);
            parts.iter_mut().flatten().for_each(|p| f(p));
        }
        _ => {}
    }
}

fn replace_first(e: &mut j::Expr, want: &mut dyn FnMut(&j::Expr) -> bool, with: &j::Expr) -> bool {
    if want(e) {
        *e = with.clone();
        return true;
    }
    let mut done = false;
    children_mut(e, &mut |c| {
        if !done {
            done = replace_first(c, &mut *want, with);
        }
    });
    done
}

fn subst(e: &mut j::Expr, map: &HashMap<String, j::Expr>) {
    if let E::Name(n) = &e.kind
        && let Some(by) = map.get(n)
    {
        *e = by.clone();
        return;
    }
    children_mut(e, &mut |c| subst(c, map));
}

fn exprs_mut(s: &mut j::Stmt) -> Vec<&mut j::Expr> {
    match &mut s.kind {
        K::Expr(e) | K::Become(e) | K::Fail(e) | K::Assert(e) | K::Yield(e) => vec![e],
        K::Return(e) => e.iter_mut().collect(),
        K::Assign { target, value, .. } | K::AugAssign { target, value, .. } => vec![target, value],
        K::Bind { value, .. } => vec![value],
        K::If { arms, .. } => arms.iter_mut().map(|(c, _)| c).collect(),
        K::While { cond, .. } => vec![cond],
        K::Match { scrutinee, .. } => vec![scrutinee],
        _ => Vec::new(),
    }
}

fn blocks_mut(s: &mut j::Stmt) -> Vec<&mut Vec<j::Stmt>> {
    match &mut s.kind {
        K::If { arms, els } => arms
            .iter_mut()
            .map(|(_, b)| &mut b.stmts)
            .chain(els.iter_mut().map(|b| &mut b.stmts))
            .collect(),
        K::Match { cases, .. } => cases.iter_mut().map(|c| &mut c.body.stmts).collect(),
        K::While { body, .. } | K::For { body, .. } => vec![body],
        _ => Vec::new(),
    }
}

fn binds(stmts: &[j::Stmt], out: &mut Vec<String>) {
    for s in stmts {
        if let K::Bind { target, .. } = &s.kind {
            out.push(target.clone());
        }
        let mut s = s.clone();
        for b in blocks_mut(&mut s) {
            binds(b, out);
        }
    }
}

fn rename(stmts: &mut [j::Stmt], map: &HashMap<String, j::Expr>) {
    for s in stmts {
        if let K::Bind { target, .. } = &mut s.kind
            && let Some(E::Name(n)) = map.get(target).map(|e| &e.kind)
        {
            *target = n.clone();
        }
        exprs_mut(s).into_iter().for_each(|e| subst(e, map));
        blocks_mut(s).into_iter().for_each(|b| rename(b, map));
    }
}

/// Each `yield p` replaced by the statements `k(p)`.
fn weave(stmts: Vec<j::Stmt>, k: &mut dyn FnMut(j::Expr) -> Vec<j::Stmt>) -> Vec<j::Stmt> {
    let mut out = Vec::new();
    for mut s in stmts {
        if let K::Yield(p) = s.kind {
            out.extend(k(p));
            continue;
        }
        for b in blocks_mut(&mut s) {
            *b = weave(mem::take(b), k);
        }
        out.push(s);
    }
    out
}

fn root(e: &j::Expr) -> Option<&str> {
    match &e.kind {
        E::Name(n) => Some(n),
        E::Attr(b, ..) | E::Index(b, _) => root(b),
        _ => None,
    }
}

fn yielded(stmts: &[j::Stmt], out: &mut Vec<j::Expr>) {
    for s in stmts {
        if let K::Yield(e) = &s.kind {
            out.push(e.clone());
        }
        for b in blocks_mut(&mut s.clone()) {
            yielded(b, out);
        }
    }
}

fn is_place(e: &j::Expr) -> bool {
    match &e.kind {
        E::Name(_) => true,
        E::Attr(b, ..) | E::Index(b, _) => is_place(b),
        _ => false,
    }
}

/// The body as one expression when it only selects among yields.
fn select(stmts: &[j::Stmt]) -> Option<j::Expr> {
    match stmts {
        [
            j::Stmt {
                kind: K::Yield(e), ..
            },
        ] => Some(e.clone()),
        [
            j::Stmt {
                kind:
                    K::If {
                        arms,
                        els: Some(els),
                    },
                ..
            },
        ] => {
            let mut acc = select(&els.stmts)?;
            for (c, b) in arms.iter().rev() {
                let then = select(&b.stmts)?;
                acc = j::Expr {
                    span: then.span,
                    paren: false,
                    kind: E::Cond {
                        then: Box::new(then),
                        cond: Box::new(c.clone()),
                        els: Box::new(acc),
                    },
                };
            }
            Some(acc)
        }
        _ => None,
    }
}

/// A subscript use: the definition, its `self` operand and its remaining arguments.
struct Use {
    def: j::FuncDef,
    base: j::Expr,
    args: Vec<j::Arg>,
}

impl Lower<'_, '_, '_> {
    /// `x[k]` on a type with `impl Index[K]` (R457), or `x.name(args)` naming a free `sub` (R1006).
    fn sub_use(&self, e: &j::Expr) -> Option<Use> {
        match &e.kind {
            E::Index(b, k) => {
                let t = self.ty_of_local(b)?;
                let def = self.g.subs.get(&t.to_string())?.clone();
                Some(Use {
                    def,
                    base: (**b).clone(),
                    args: vec![j::Arg {
                        name: None,
                        kind: j::ArgKind::Expr((**k).clone()),
                        span: k.span,
                    }],
                })
            }
            E::Call(f, args) => {
                let E::Attr(b, name, _) = &f.kind else {
                    return None;
                };
                let def = self.g.free_subs.get(name)?.clone();
                Some(Use {
                    def,
                    base: (**b).clone(),
                    args: args.clone(),
                })
            }
            _ => None,
        }
    }

    /// The first subscript use on the spine of an assignment target.
    fn spine_use(&self, e: &j::Expr) -> Option<j::Expr> {
        match &e.kind {
            E::Index(b, _) => self
                .sub_use(e)
                .map(|_| e.clone())
                .or_else(|| self.spine_use(b)),
            E::Attr(b, ..) => self.spine_use(b),
            E::Call(..) => self.sub_use(e).map(|_| e.clone()),
            _ => None,
        }
    }

    /// `s` with its first subscript use inlined, or `None` when it has none.
    pub(super) fn inline_sub(&mut self, s: &j::Stmt) -> Option<R<Vec<j::Stmt>>> {
        if self.g.subs.is_empty() && self.g.free_subs.is_empty() {
            return None;
        }
        let mut s2 = s.clone();
        let mut site = None;
        if let K::Assign { target, .. } | K::AugAssign { target, .. } = &s2.kind {
            site = self.spine_use(target).map(|u| (true, u));
        }
        if site.is_none() {
            for e in stmt_exprs(s) {
                walk(e, &mut |x| {
                    if site.is_some() {
                        return;
                    }
                    if let E::Call(_, args) = &x.kind {
                        for a in args {
                            if let j::ArgKind::Ref(r) = &a.kind
                                && let Some(u) = self.spine_use(r)
                            {
                                site = Some((true, u));
                                break;
                            }
                        }
                    }
                });
            }
        }
        if site.is_none() {
            for e in stmt_exprs(s) {
                walk(e, &mut |x| {
                    if site.is_none() && self.sub_use(x).is_some() {
                        site = Some((false, x.clone()));
                    }
                });
            }
        }
        let (write, used) = site?;
        Some(self.inline(&mut s2, write, &used))
    }

    fn inline(&mut self, s: &mut j::Stmt, write: bool, used: &j::Expr) -> R<Vec<j::Stmt>> {
        let Use {
            def: at,
            base,
            args,
        } = self.sub_use(used).expect("a subscript use");
        let n = self.g.sub_n.get() + 1;
        if n > 4096 {
            return unsupported("a recursive subscript");
        }
        self.g.sub_n.set(n);
        let Some(body) = &at.body else {
            return unsupported("a subscript without a body");
        };
        let params: Vec<(&str, bool)> = at
            .params
            .iter()
            .filter_map(|p| match p {
                j::Param::Value { name, conv, .. } => {
                    Some((name.as_str(), matches!(conv, Some(j::Conv::Var))))
                }
                _ => None,
            })
            .collect();
        let Some((&(selfp, _), keyps)) = params.split_first() else {
            return unsupported("a subscript without `self`");
        };
        // Positional arguments fill the parameters after `self` in order; keywords name one.
        let mut slots: Vec<Option<(&j::Expr, bool)>> = vec![None; keyps.len()];
        let mut next = 0;
        let mut bad = false;
        for a in &args {
            let (x, by_ref) = match &a.kind {
                j::ArgKind::Expr(x) => (x, false),
                j::ArgKind::Ref(x) => (x, true),
                _ => return unsupported("a `become` argument to a subscript"),
            };
            let i = match &a.name {
                Some(n) => keyps.iter().position(|(k, _)| k == n),
                None => {
                    next += 1;
                    (next <= keyps.len()).then(|| next - 1)
                }
            };
            match i {
                Some(i) if slots[i].is_none() => slots[i] = Some((x, by_ref)),
                _ => bad = true,
            }
        }
        if bad || slots.iter().any(Option::is_none) {
            self.err(
                "E202",
                used.span,
                format!("expected {} arguments", keyps.len()),
            );
            return Ok(Vec::new());
        }
        let mut keys = Vec::new();
        for ((name, inout), slot) in keyps.iter().zip(slots) {
            let (x, by_ref) = slot.expect("checked above");
            match (inout, by_ref) {
                (true, false) => self.err(
                    "E404",
                    x.span,
                    format!("`{name}` is a `var` parameter; pass `&place`"),
                ),
                (false, true) => self.err(
                    "E405",
                    x.span,
                    format!("`{name}` is not a `var` parameter; remove `&`"),
                ),
                _ if by_ref && !is_place(x) => self.err("E405", x.span, "`&` needs a place".into()),
                _ => {}
            }
            keys.push((x, by_ref));
        }
        if self
            .cx
            .diags
            .iter()
            .any(|d| d.code == "E404" || d.code == "E405")
        {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        let mut map = HashMap::from([(selfp.to_string(), base)]);
        for ((keyp, _), (key, by_ref)) in keyps.iter().zip(keys) {
            let key = if by_ref || matches!(key.kind, E::Name(_) | E::Lit(_)) {
                key.clone()
            } else {
                let name = format!("__sub{n}_{keyp}");
                out.push(j::Stmt {
                    kind: K::Bind {
                        mutable: false,
                        target: name.clone(),
                        target_span: key.span,
                        ty: None,
                        value: key.clone(),
                    },
                    span: s.span,
                });
                j::Expr {
                    kind: E::Name(name),
                    span: key.span,
                    paren: false,
                }
            };
            map.insert(keyp.to_string(), key);
        }
        let mut locals = Vec::new();
        binds(&body.stmts, &mut locals);
        for l in locals {
            let name = format!("__sub{n}_{l}");
            map.insert(
                l,
                j::Expr {
                    kind: E::Name(name),
                    span: used.span,
                    paren: false,
                },
            );
        }
        if write {
            let mut ys = Vec::new();
            yielded(&body.stmts, &mut ys);
            let mut failed = false;
            for y in ys.iter().filter(|y| is_place(y) && root(y) != Some(selfp)) {
                failed = true;
                self.cx.err(
                    "E423",
                    y.span,
                    &self.item.clone(),
                    "the yielded place is not reachable from `self`, so the inout mode cannot write it"
                        .into(),
                );
            }
            if failed {
                return Ok(out);
            }
        }
        let mut stmts = body.stmts.clone();
        rename(&mut stmts, &map);

        let split = if write {
            0
        } else {
            stmts.iter().take_while(|x| yields_of(x) == ZERO).count()
        };
        let read = if write { None } else { select(&stmts[split..]) };
        if let Some(sel) = read {
            out.extend(stmts.drain(..split));
            for e in exprs_mut(s) {
                if replace_first(e, &mut |x| x == used, &sel) {
                    break;
                }
            }
            out.push(s.clone());
            return Ok(out);
        }
        if matches!(s.kind, K::Bind { .. }) && !stmts.iter().any(|x| matches!(x.kind, K::Yield(_)))
        {
            return unsupported("a subscript yielding in a branch, read into a new name");
        }
        let proto = s.clone();
        let item = self.item.clone();
        out.extend(weave(stmts, &mut |p| {
            if !is_place(&p) {
                self.cx.err(
                    "E423",
                    p.span,
                    &item,
                    "the yielded value is not a place, so the inout mode cannot write it".into(),
                );
                return Vec::new();
            }
            let mut copy = proto.clone();
            for e in exprs_mut(&mut copy) {
                if replace_first(e, &mut |x| x == used, &p) {
                    break;
                }
            }
            vec![copy]
        }));
        Ok(out)
    }
}
