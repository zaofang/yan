use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn user_call(
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
        if sig.ret == Ty::Type && sig.params.iter().all(|p| p.ty != Ty::Type) {
            crate::reify::stage0_only(self.g, &self.item, name, at, self.cx);
        }
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
    pub(super) fn pattern_check(&mut self, ty: &str, fmt: Option<&j::Arg>) {
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
    pub(super) fn civil_literal(&mut self, name: &str, args: &[j::Arg], at: Span) {
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
    pub(super) fn instance(&mut self, n: &str, v: Sv, at: Span) -> R<(String, Sig)> {
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
            if d.severity == yan_diag::Severity::Error && d.code != "E270" {
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
    pub(super) fn variadic_call(
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
    pub(super) fn asm(&mut self, template: &str, args: &[j::AsmArg], at: Span) -> R<()> {
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

    /// The caller's binding of capability trait `t`, which the callee binds as `callee`.
    pub(super) fn cap_for(&self, t: &str, callee: &str) -> String {
        if self.handlers.iter().any(|h| h == callee)
            || self
                .sig
                .caps
                .iter()
                .any(|c| c == callee && self.sig.cap_trait(c) == t)
        {
            return callee.to_owned();
        }
        self.sig
            .caps
            .iter()
            .find(|c| self.sig.cap_trait(c) == t)
            .cloned()
            .unwrap_or_else(|| callee.to_owned())
    }

    /// A call of `path` with signature `sig`. A generic callee's type arguments are inferred
    /// from the argument types, and each must implement its bounds (E221).
    pub(super) fn call_sig(
        &mut self,
        path: &str,
        sig: &Sig,
        recv: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let mut out: Vec<Arg> = sig
            .caps
            .iter()
            .map(|c| Arg::Cap(self.cap_for(&sig.cap_trait(c), c), at))
            .collect();
        let want = self.want_ret.take().filter(|(_, sp)| *sp == at);
        let outer = mem::take(&mut self.arg_tys);
        // Arguments for ghost parameters are erased with them.
        let kept: Vec<j::Arg>;
        let args = if sig.ghosts.is_empty() {
            args
        } else {
            let first = usize::from(recv.is_some());
            kept = args
                .iter()
                .enumerate()
                .filter(|(k, a)| match &a.name {
                    Some(n) => !sig.ghosts.iter().any(|(_, g)| g == n),
                    None => !sig.ghosts.iter().any(|(i, _)| *i == k + first),
                })
                .map(|(_, a)| a.clone())
                .collect();
            &kept[..]
        };
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
        // The interpreter cannot see element types, so a float `sum` is its own function: an
        // empty one is `-0.0` (R382), not an int `0`.
        let float_sum = (path == "std.iter.ops.sum"
            || path == "sum" && self.g.std_fns.contains(path))
            && matches!(map.first(), Some((_, Ty::Float)));
        let (path, targs) = if float_sum {
            let float = if path == "sum" {
                "std.prelude.sum_float".to_owned()
            } else {
                format!("{path}_float")
            };
            (float, vec![])
        } else {
            (path.to_owned(), targs)
        };
        Ok((
            Rhs::Call(call(&path, targs, out, at)),
            sig.ret.subst(&map),
            sig.fail.as_ref().map(|f| f.subst(&map)),
        ))
    }

    /// The signature of method `m` on `t` from an `impl`, or from a bound of a type parameter.
    pub(super) fn trait_method(&self, t: &Ty, m: &str) -> Option<Sig> {
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
    pub(super) fn implements(&self, t: &Ty, b: &str) -> bool {
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

    pub(super) fn ctor(
        &mut self,
        name: &str,
        ps: &[crate::P],
        args: &[j::Arg],
        at: Span,
    ) -> R<Rhs> {
        let before = self.cx.marks.len();
        let Some(vals) = self.args(ps, None, args, at)? else {
            return Ok(Rhs::Atom(Atom::Lit(Literal::Unit, at)));
        };
        // A constructor has no argument nodes: the marks go on its fields, joined by atom span.
        self.cx.marks.truncate(before);
        let mut next = 0;
        for a in args {
            let k = match &a.name {
                None => {
                    next += 1;
                    next - 1
                }
                Some(n) => ps.iter().position(|p| p.name == *n).unwrap_or(0),
            };
            let keyword = a.name.is_some() || self.keyworded_from.is_some_and(|f| k >= f);
            if let Some(Arg::Value(_, Atom::Name(_, sp) | Atom::Lit(_, sp), _)) = vals.get(k)
                && (keyword || !ps[k].positional)
            {
                self.cx.marks.push(crate::marks::Pending {
                    span: *sp,
                    kind: crate::marks::Kind::Field(k),
                    call: None,
                    mark: if keyword {
                        yan_surface::Mark::Keyword
                    } else {
                        yan_surface::Mark::Positional
                    },
                });
            }
        }
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

    /// A returned closure may not capture a view (E406), and a returned view must borrow from a
    /// parameter, not a local (E414, 4.5).
    pub(super) fn view_escape(&mut self, e: &j::Expr) {
        if let j::ExprKind::Lambda(ps, body) = &e.kind {
            let mut hit = None;
            walk(body, &mut |x| {
                if let j::ExprKind::Name(n) = &x.kind
                    && hit.is_none()
                    && self.views.contains(n)
                    && !ps.contains(n)
                {
                    hit = Some((n.clone(), x.span));
                }
            });
            if let Some((n, sp)) = hit {
                self.err(
                    "E406",
                    sp,
                    format!("this closure escapes but captures the view `{n}`"),
                );
            }
            return;
        }
        if !self.returns_view {
            return;
        }
        let j::ExprKind::Call(f, args) = &e.kind else {
            return;
        };
        let j::ExprKind::Name(callee) = &f.kind else {
            return;
        };
        let Some(s) = self.g.fns.get(callee) else {
            return;
        };
        for (k, a) in args.iter().enumerate() {
            let param = match &a.name {
                Some(n) => Some(n.as_str()),
                None => s.params.get(k).map(|p| p.name.as_str()),
            };
            if let (
                Some(p),
                j::ArgKind::Expr(j::Expr {
                    kind: j::ExprKind::Name(x),
                    ..
                }),
            ) = (param, &a.kind)
                && s.from.iter().any(|f| f == p)
                && !self.sig.params.iter().any(|q| q.name == *x)
            {
                self.err(
                    "E414",
                    e.span,
                    format!("this view borrows from the local `{x}`, which ends with the function"),
                );
            }
        }
    }

    /// `return C(..., become f(x))` (R456): `become` is legal as the last argument of a constructor
    /// in return position, nesting; anywhere else in a constructor it is E143. The call lowers as
    /// an ordinary call that fills the field, so it does not yet run in constant stack.
    pub(super) fn become_in_ctor(&mut self, e: &j::Expr, last: bool) -> j::Expr {
        let j::ExprKind::Call(f, args) = &e.kind else {
            return e.clone();
        };
        let is_ctor = matches!(&f.kind, j::ExprKind::Name(n)
            if self.g.records.contains_key(n) || self.g.variant_of.contains_key(n));
        if !is_ctor {
            return e.clone();
        }
        let n = args.len();
        // The arguments of a constructor holding a `become` are matched by position (E210 exempt).
        let fields: Vec<String> = match &f.kind {
            j::ExprKind::Name(c)
                if args.iter().any(|a| matches!(a.kind, j::ArgKind::Become(_))) =>
            {
                let fs = self.g.records.get(c).or_else(|| {
                    let s = self.g.sums.get(self.g.variant_of.get(c)?)?;
                    s.iter().find(|(v, _)| v == c).map(|(_, fs)| fs)
                });
                fs.map(|fs| fs.iter().map(|f| f.name.clone()).collect())
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let args = args
            .iter()
            .enumerate()
            .map(|(k, a)| {
                let name = a.name.clone().or_else(|| fields.get(k).cloned());
                let tail = last && k + 1 == n;
                let kind = match &a.kind {
                    j::ArgKind::Become(x) => {
                        if !tail {
                            self.err(
                                "E143",
                                a.span,
                                "`become` in a constructor must be its last-evaluated argument"
                                    .into(),
                            );
                        }
                        j::ArgKind::Expr(x.clone())
                    }
                    j::ArgKind::Expr(x) => j::ArgKind::Expr(self.become_in_ctor(x, tail)),
                    k => k.clone(),
                };
                j::Arg {
                    name,
                    kind,
                    span: a.span,
                }
            })
            .collect();
        j::Expr {
            kind: j::ExprKind::Call(f.clone(), args),
            ..e.clone()
        }
    }
}
