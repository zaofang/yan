use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn rhs(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Rhs, Ty)> {
        self.rhs_inner(e, want).map_err(|u| u.at(e.span))
    }

    pub(super) fn rhs_inner(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Rhs, Ty)> {
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
                self.last_use.mark_read(n);
                let t = self.locals[&m].ty.clone();
                if n == "out" {
                    self.cx.marks.push(crate::marks::Pending {
                        span: at,
                        kind: crate::marks::Kind::Out,
                        call: None,
                        mark: yan_surface::Mark::Out,
                    });
                }
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
                } else if BUILTINS.contains(&n.as_str())
                    && let Some(f @ Ty::Fn(_, ps, _)) = match want {
                        Some(Ty::Option(t)) => Some(&**t),
                        w => w,
                    }
                    && ps.len() == 1
                {
                    // A one-argument builtin as a value, as `key=len` (R659).
                    let x = format!("_{n}_0");
                    let name = |x: &str| j::Expr {
                        kind: K::Name(x.to_owned()),
                        span: at,
                        paren: false,
                    };
                    let arg = j::Arg {
                        name: None,
                        kind: j::ArgKind::Expr(name(&x)),
                        span: at,
                    };
                    let body = j::Expr {
                        kind: K::Call(Box::new(name(n)), vec![arg]),
                        span: at,
                        paren: false,
                    };
                    let f = f.clone();
                    self.lambda(&[x], &body, Some(&f))?
                } else if self.sig.caps.contains(n) || self.handlers.contains(n) {
                    // A capability as a value; the checker rejects returning or storing it (E306).
                    (
                        Rhs::Atom(Atom::Name(n.clone(), at)),
                        Ty::Data(self.sig.cap_trait(n)),
                    )
                } else if let Some(t) = self.gone.get(n) {
                    if zero(t).is_some() {
                        self.hoists
                            .borrow_mut()
                            .entry(n.clone())
                            .or_insert(t.clone());
                    }
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
            // `b"ab"` is `bytes.from_list([97, 98])`.
            K::Lit(j::Lit::Str { prefix, value }) if prefix == "b" => {
                let ex = |kind| j::Expr {
                    kind,
                    span: e.span,
                    paren: false,
                };
                let items = value
                    .bytes()
                    .map(|b| ex(j::ExprKind::Lit(j::Lit::Int(b.to_string()))))
                    .collect();
                let from_list = ex(j::ExprKind::Attr(
                    Box::new(ex(j::ExprKind::Name("bytes".into()))),
                    "from_list".into(),
                    e.span,
                ));
                let list = ex(j::ExprKind::List(items));
                let arg = j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(list),
                    span: e.span,
                };
                let call = ex(j::ExprKind::Call(Box::new(from_list), vec![arg]));
                return self.rhs_inner(&call, want);
            }
            K::Lit(l) => {
                if let j::Lit::Int(s) = l
                    && int_text(s).is_ok_and(|d| d != *s)
                {
                    self.cx.lits.push((at, s.clone()));
                }
                if let j::Lit::Str { prefix, value } = l
                    && prefix == "r"
                {
                    self.cx.lits.push((at, format!("r{value}")));
                }
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
                if let (j::Lit::Str { prefix, value }, Some(Ty::Char)) = (l, want)
                    && (prefix.is_empty() || prefix == "r")
                    && let mut cs = value.chars()
                    && let (Some(c), None) = (cs.next(), cs.next())
                {
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Char(c), at)), Ty::Char));
                }
                if let (j::Lit::Int(s), Some(Ty::BigInt)) = (l, want) {
                    return Ok((self.bigint_lit(&int_text(s)?, at), Ty::BigInt));
                }
                if let (j::Lit::Int(s), Some(w)) = (l, want)
                    && let Some(r) = float_lit(&int_text(s)?, w, at)
                {
                    return Ok((r, w.clone()));
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
            K::List(xs) if let Some(w @ Ty::Array(..)) = unoption(want) => {
                self.array_literal(xs, &w.clone(), at)?
            }
            K::List(xs) => {
                let mut et = match unoption(want) {
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
                    if !matches!(want, Some(Ty::Err)) {
                        self.err("E241", at, "annotate the type of this empty list".into());
                    }
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
                };
                let lt = Ty::List(Box::new(et.clone()));
                let l = self.fresh("t");
                let new = call("std.list.new", vec![et.targ()], vec![], at);
                self.bind(&l, true, &lt, Rhs::Call(new), at, at);
                for a in atoms {
                    let sp = a.span();
                    let push = call(
                        "std.list.append",
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
                let (mut kt, mut vt) = match unoption(want) {
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
                    if !matches!(want, Some(Ty::Err)) {
                        self.err("E241", at, "annotate the type of this empty dict".into());
                    }
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
                let (r, t) = self.rhs(v, want)?;
                let (span, kind) = match &r {
                    Rhs::Atom(Atom::Lit(_, sp)) => (*sp, crate::marks::Kind::Lit),
                    Rhs::Call(c) => (c.span, crate::marks::Kind::Call),
                    _ => (at, crate::marks::Kind::Stmt),
                };
                self.cx.marks.push(crate::marks::Pending {
                    span,
                    kind,
                    call: None,
                    mark: yan_surface::Mark::Const,
                });
                (r, t)
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
            // A lattice value's `.value` is its read (A8).
            K::Attr(b, f, _)
                if f == "value" && matches!(self.ty_of_local(b), Some(Ty::Coll("Max", _))) =>
            {
                let c = j::Expr {
                    kind: K::Call(Box::new(e.clone()), Vec::new()),
                    span: e.span,
                    paren: false,
                };
                self.rhs(&c, want)?
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
                    None if t == Ty::Str && !self.cx.strict => {
                        let item = self.item.clone();
                        self.cx.note(
                            "N12",
                            at,
                            &item,
                            "`s[i]` normalized to `s.char_at(i)`, which costs O(n)".into(),
                        );
                        let s = self.tmp(&t, Rhs::Proj(p), b.span);
                        let (ia, ity) = self.atom(k, Some(&Ty::Int))?;
                        self.expect(&ity, &Ty::Int, k.span);
                        let c = call(
                            "std.text.char_at",
                            vec![],
                            vec![let_arg(s), let_arg(ia)],
                            at,
                        );
                        (Rhs::Call(c), Ty::Char)
                    }
                    _ => {
                        let (p, t) = self.index_place(e, b, k, (p, t))?;
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
                        let injects =
                            matches!(&self.sig.fail, Some(Ty::Union(ms)) if ms.contains(&e));
                        if from.is_some() && injects {
                            self.err(
                                "E313",
                                at,
                                format!(
                                    "`?` could inject the `{e}` failure into the union or convert it \
                                     with `From`; remove the `From` implementation"
                                ),
                            );
                        }
                        let via = match from {
                            Some(f) => Some(vec![f.clone()]),
                            None => into_error
                                .then(|| vec!["std".into(), "error".into(), "wrap".into()]),
                        };
                        (
                            Rhs::Propagate {
                                call: c,
                                via,
                                drops: Vec::new(),
                            },
                            t,
                        )
                    }
                    // The call was reported; what it fails with is unknown.
                    (Some(Ty::Err), r) => (r, t),
                    _ => return unsupported("`?` on an optional or on a call that cannot fail"),
                }
            }
            K::Unary(op, x) => match (*op, &x.kind) {
                ("-", K::Lit(j::Lit::Int(s))) if want == Some(&Ty::BigInt) => (
                    self.bigint_lit(&format!("-{}", int_text(s)?), at),
                    Ty::BigInt,
                ),
                ("-", K::Lit(j::Lit::Int(s)))
                    if let Some(w) = want
                        && let Some(r) = float_lit(&format!("-{}", int_text(s)?), w, at) =>
                {
                    (r, w.clone())
                }
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
                        Ty::Int => {
                            self.mark_call(at, yan_surface::Mark::Negated);
                            call(
                                "sub_checked_i64",
                                vec![],
                                vec![let_arg(Atom::Lit(Literal::Int("0".into()), at)), let_arg(a)],
                                at,
                            )
                        }
                        Ty::Float => {
                            self.mark_call(at, yan_surface::Mark::Negated);
                            call(
                                "mul_f64",
                                vec![],
                                vec![
                                    let_arg(Atom::Lit(Literal::Float("-1.0".into()), at)),
                                    let_arg(a),
                                ],
                                at,
                            )
                        }
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
            K::Binary(op, a, b) => {
                let r = self.arith(op, a, b, at, want)?;
                if matches!(*op, "**" | "+") && matches!(r.0, Rhs::Call(_)) {
                    self.mark_call(at, yan_surface::Mark::Operator);
                }
                r
            }
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
            K::Comp { .. } => self.comprehension(e, want)?,
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
                    Ty::Str => {
                        self.lenient(
                            "N50",
                            at,
                            "`s[a:b]` slices chars, normalized to `std.text.slice_clamped`",
                        );
                        Ty::Str
                    }
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
                let c = if t == Ty::Str {
                    call("std.text.slice_clamped", vec![], args, at)
                } else {
                    call("std.list.slice_clamped", vec![et.targ()], args, at)
                };
                (Rhs::Call(c), t)
            }
            K::Lambda(ps, body) => self.lambda(ps, body, want)?,
            K::Cast(x, j::Type::Named { path, args })
                if args.is_empty()
                    && matches!(path.as_slice(), [n] if n == "i64" || n == "f64") =>
            {
                let conv = if path[0] == "i64" { "int" } else { "float" };
                let arg = j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr((**x).clone()),
                    span: x.span,
                };
                return self.builtin(conv, &[arg], at, want);
            }
            K::Cast(..) => return unsupported("`as` to a fixed-width type"),
            // Yan's `try f(x)`: the call's failure as a `Result` value, as Yan Agent reifies a failing
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
}
