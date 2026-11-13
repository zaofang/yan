use super::*;

impl Lower<'_, '_, '_> {
    /// Match arguments to parameters (14.1 Calls) and lower them in parameter order.
    /// `None` when an arity error was reported.
    pub(super) fn args(
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
        // E210: parameters that share a type take keyword arguments, in a callee with three or
        // more parameters (R800). The lenient reader takes them by position and reads them as
        // keywords (N49).
        let mut unkeyed: Vec<usize> = Vec::new();
        for (k, slot) in slots.iter().enumerate().filter(|_| ps.len() > 2) {
            let Some(Given::Expr(e, _, _, keyword)) = slot else {
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
                unkeyed.push(k);
            }
        }
        self.keyworded_from = None;
        if let Some(&first) = unkeyed.first() {
            let sp = match &slots[first] {
                Some(Given::Expr(_, _, sp, _)) => *sp,
                _ => at,
            };
            if self.cx.strict || self.cx.yn {
                self.err(
                    "E210",
                    sp,
                    format!(
                        "pass `{}` by keyword: another parameter has type `{}`",
                        ps[first].name, ps[first].ty
                    ),
                );
            } else {
                self.lenient(
                    "N49",
                    sp,
                    "positional arguments for parameters that share a type normalized to keywords",
                );
                // A positional argument cannot follow a keyword one, so every later one is
                // named too.
                for (k, slot) in slots.iter_mut().enumerate().skip(first) {
                    if let Some(Given::Expr(.., keyword)) = slot
                        && !ps[k].positional
                    {
                        *keyword = true;
                    }
                }
                self.keyworded_from = Some(first);
            }
        }
        let mut elems: Vec<(String, Atom)> = Vec::new();
        let mut inouts: Vec<(String, Vec<String>, bool)> = Vec::new();
        // Lambdas go last, so their parameter types can come from the other arguments.
        let lambda = |s: &Option<Given>| matches!(s, Some(Given::Expr(e, ..)) if self.fn_value(e));
        let mut order: Vec<usize> = (0..ps.len()).filter(|&k| !lambda(&slots[k])).collect();
        order.extend((0..ps.len()).filter(|&k| lambda(&slots[k])));
        let mut done: Vec<Option<Arg>> = (0..ps.len()).map(|_| None).collect();
        for k in order {
            let (p, slot) = (&ps[k], slots[k].take());
            let out = &mut done[k];
            let (e, is_ref, sp) = match slot {
                Some(Given::Expr(e, r, sp, keyword)) => {
                    if self.qualified_variant(e) {
                        self.cx.marks.push(crate::marks::Pending {
                            span: sp,
                            kind: crate::marks::Kind::Arg,
                            call: Some(at),
                            mark: yan_surface::Mark::Qualified,
                        });
                    }
                    if keyword || !p.positional {
                        self.cx.marks.push(crate::marks::Pending {
                            span: sp,
                            kind: crate::marks::Kind::Arg,
                            call: Some(at),
                            mark: if keyword {
                                yan_surface::Mark::Keyword
                            } else {
                                yan_surface::Mark::Positional
                            },
                        });
                    }
                    (e, r, sp)
                }
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
                    // E402: `&q` and `&q.a` overlap, since one place is a prefix of the other.
                    let fields: Vec<&str> = pl
                        .projs
                        .iter()
                        .map_while(|p| match p {
                            Proj::Field(f, _) => Some(f.as_str()),
                            _ => None,
                        })
                        .collect();
                    let key: Vec<String> = fields.iter().map(|f| f.to_string()).collect();
                    let whole = fields.len() == pl.projs.len();
                    let overlaps = inouts.iter().any(|(r, fs, w)| {
                        *r == pl.root
                            && ((*w && key.starts_with(fs)) || (whole && fs.starts_with(&key)))
                    });
                    if overlaps {
                        self.err(
                            "E402",
                            sp,
                            format!("this place overlaps another part of `{}` passed for mutation in this call", pl.root),
                        );
                    }
                    inouts.push((pl.root.clone(), key, whole));
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
                    let lambda = self.fn_value(e);
                    let w = (!matches!(w, Ty::Coll("iter", _))
                        && (lambda || !self.unbound_param(&w)))
                    .then_some(w);
                    let (a, t) = self.atom(e, w.as_ref())?;
                    let (a, t) = self.as_iter(&p.ty, a, t, sp)?;
                    let (r, t) = self.widen(Rhs::Atom(a), t, Some(&p.ty), sp)?;
                    let a = self.tmp(&t, r, sp);
                    self.expect(&t, &p.ty, sp);
                    self.arg_tys.push((p.ty.clone(), t.clone(), sp));
                    if conv == Conv::Sink && self.sink_of_element(&a, sp) {
                    } else if let (Conv::Sink, Atom::Name(n, _)) = (conv, &a)
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

    /// E413 when `a`, passed by `sink`, names an element of a list: the list still owns it.
    pub(super) fn sink_of_element(&mut self, a: &Atom, sp: Span) -> bool {
        let Atom::Name(n, _) = a else {
            return false;
        };
        if !self.index_views.contains(n) {
            return false;
        }
        self.err(
            "E413",
            sp,
            "an element of a list cannot be moved out; pass `.copy()`".into(),
        );
        let copied = format!("{}.copy()", self.cx.text(sp));
        self.cx.fix(Applicability::Auto, "copy", sp, &copied);
        true
    }

    /// Whether `t` mentions a type parameter of the callee that is not yet bound; the caller's
    /// own type parameters are fixed types here.
    pub(super) fn unbound_param(&self, t: &Ty) -> bool {
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
    pub(super) fn as_iter(&mut self, want: &Ty, a: Atom, t: Ty, sp: Span) -> R<(Atom, Ty)> {
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
        // The call's span is one column past the argument's, so the origin joins this call and not
        // the argument's own construction, which shares the argument's span.
        let at = Span {
            end_col: sp.end_col + 1,
            ..sp
        };
        let call = j::Expr {
            kind: j::ExprKind::Call(Box::new(f), vec![]),
            span: at,
            paren: false,
        };
        self.cx
            .origins
            .push((at, yan_surface::Inserted::Conversion));
        self.atom(&call, None)
    }

    /// A lambda, or a builtin named as a value (`key=len`): its type comes from the parameter.
    pub(super) fn fn_value(&self, e: &j::Expr) -> bool {
        match &e.kind {
            j::ExprKind::Lambda(..) => true,
            j::ExprKind::Name(n) => {
                BUILTINS.contains(&n.as_str())
                    && !self.locals.contains_key(n)
                    && !self.g.fns.contains_key(n)
            }
            _ => false,
        }
    }

    /// `max(xs, key=None, default=...)` over one iterable (R658): the iterator's `max`, with
    /// `default` when it is empty, and a `value` panic when it is empty with no `default`.
    pub(super) fn extreme(
        &mut self,
        n: &str,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let (mut xs, mut key, mut default) = (None, None, None);
        for a in args {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported(format!("a reference argument to `{n}`"));
            };
            match a.name.as_deref() {
                None => xs = Some(e),
                Some("key") => key = Some(a.clone()),
                Some("default") => default = Some(e.clone()),
                Some(k) => {
                    self.err("E202", a.span, format!("`{n}` has no parameter `{k}`"));
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
                }
            }
        }
        let xs = xs.expect("one positional argument");
        let (a, t) = self.atom(xs, None)?;
        let Atom::Name(v, _) = a else {
            return unsupported(format!("`{n}` of a literal"));
        };
        let recv = j::Expr {
            kind: j::ExprKind::Name(v),
            span: xs.span,
            paren: false,
        };
        let it = match t {
            Ty::Err => return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None)),
            Ty::Coll("iter", _) => recv,
            _ => ex(j::ExprKind::Call(
                Box::new(ex(j::ExprKind::Attr(Box::new(recv), "iter".into(), at))),
                Vec::new(),
            )),
        };
        let m = if default.is_some() {
            n.to_owned()
        } else {
            format!("{n}_nonempty")
        };
        let c = ex(j::ExprKind::Call(
            Box::new(ex(j::ExprKind::Attr(Box::new(it), m, at))),
            key.into_iter().collect(),
        ));
        let e = match default {
            Some(d) => ex(j::ExprKind::Fallback(
                Box::new(c),
                Box::new(j::Fallback::Value(d)),
            )),
            None => c,
        };
        let (r, t) = self.rhs(&e, want)?;
        Ok((r, t, None))
    }

    pub(super) fn positional<'e>(
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
    pub(super) fn collection(
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

    /// `e` as an expression that can be lowered twice: a name or a literal as written, anything
    /// else bound to a temporary first.
    pub(super) fn reusable(&mut self, e: &j::Expr) -> R<j::Expr> {
        if matches!(&e.kind, j::ExprKind::Name(_) | j::ExprKind::Lit(_))
            || matches!(&e.kind, j::ExprKind::Unary("-", x) if matches!(x.kind, j::ExprKind::Lit(_)))
        {
            return Ok(e.clone());
        }
        Ok(match self.atom(e, None)?.0 {
            Atom::Name(n, span) => j::Expr {
                kind: j::ExprKind::Name(n),
                span,
                paren: false,
            },
            Atom::Lit(..) => e.clone(),
        })
    }

    /// Records how the source spelled the call at `at`: `x.f()` or `f(x)` (RFC 0034 step 4).
    pub(super) fn mark_call(&mut self, at: Span, mark: yan_surface::Mark) {
        self.cx.marks.push(crate::marks::Pending {
            span: at,
            kind: crate::marks::Kind::Call,
            call: None,
            mark,
        });
    }
}
