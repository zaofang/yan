use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn assign(
        &mut self,
        target: &j::Expr,
        ann: Option<&j::Type>,
        value: &j::Expr,
        at: Span,
    ) -> R<()> {
        let ann = ann.map(|t| self.g.ty(t, self.cx, at)).transpose()?;
        if value.paren && matches!(&value.kind, j::ExprKind::Tuple(vs) if vs.len() > 1) {
            self.cx.marks.push(crate::marks::Pending {
                span: value.span,
                kind: crate::marks::Kind::Stmt,
                call: None,
                mark: yan_surface::Mark::Parenthesized,
            });
        }
        match &target.kind {
            j::ExprKind::Tuple(ts) => {
                let mut vals = Vec::new();
                let first = self.out.len();
                match &value.kind {
                    j::ExprKind::Tuple(vs) if vs.len() == ts.len() => {
                        // R216: snapshot names too, so `a, b = b, a` swaps.
                        for v in vs {
                            let (a, t) = self.atom(v, None)?;
                            let a = match a {
                                Atom::Name(ref x, sp) => {
                                    let n = self.fresh("t");
                                    // A name this statement assigns is overwritten, so it moves.
                                    let overwritten = !t.is_copy()
                                        && ts.iter().any(
                                            |e| matches!(&e.kind, j::ExprKind::Name(m) if m == x),
                                        );
                                    let r = if overwritten {
                                        Rhs::Move(Place {
                                            root: x.clone(),
                                            root_span: sp,
                                            projs: Vec::new(),
                                        })
                                    } else {
                                        self.consume(Rhs::Atom(a), &t, v.span)
                                    };
                                    self.bind(&n, false, &t, r, v.span, v.span);
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
                let snapshots = self.out.len() > first;
                if ts.len() > 1 && matches!(value.kind, j::ExprKind::Tuple(_)) {
                    // The first statement and the last store: a group's ends (R860).
                    let head = if snapshots {
                        self.out[first].span
                    } else {
                        ts[0].span
                    };
                    for span in [head, ts[ts.len() - 1].span] {
                        self.cx.marks.push(crate::marks::Pending {
                            span,
                            kind: crate::marks::Kind::Stmt,
                            call: None,
                            mark: yan_surface::Mark::Parallel,
                        });
                    }
                }
                for (t, (a, ty, sp)) in ts.iter().zip(vals) {
                    self.store(t, None, Rhs::Atom(a), ty, sp)?;
                }
                Ok(())
            }
            j::ExprKind::Name(n) => {
                let mut want = ann
                    .clone()
                    .or_else(|| self.locals.get(n).map(|l| l.ty.clone()));
                if want.is_none() && is_empty_literal(value) {
                    let known = self.hints.borrow().get(&value.span).cloned();
                    if known.is_none() {
                        self.open.insert(n.clone(), value.span);
                    }
                    want = known;
                }
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

    pub(super) fn store(
        &mut self,
        target: &j::Expr,
        ann: Option<Ty>,
        r: Rhs,
        t: Ty,
        at: Span,
    ) -> R<()> {
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
        let r = match &target.kind {
            j::ExprKind::Name(n) if n != "_" => self.consume(r, &t, at),
            _ => r,
        };
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
                let (r, t) = self.widen(r, t, Some(&want), at)?;
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
                if ann.is_some() {
                    self.cx.marks.push(crate::marks::Pending {
                        span: target.span,
                        kind: crate::marks::Kind::Stmt,
                        call: None,
                        mark: yan_surface::Mark::Annotated,
                    });
                }
                let (r, ty) = match ann {
                    Some(a) => {
                        let (r, t) = self.widen(r, t, Some(&a), at)?;
                        let (r, t) = self.inject(r, t, Some(&a), at);
                        self.expect(&t, &a, at);
                        (r, a)
                    }
                    None if matches!(t, Ty::Coll("iter", _))
                        && !self.cx.strict
                        && self.can_collect()
                        && !matches!(r, Rhs::Atom(Atom::Name(..)) | Rhs::Proj(_))
                        && self.indexed.contains(n) =>
                    {
                        self.lenient(
                            "N37",
                            at,
                            "an `iter` bound to a name that is indexed or measured normalized to `.collect()`",
                        );
                        self.collect(r, &t, at)?
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
            j::ExprKind::Wild => {
                // A discarded copy is reported at what it copies (E416).
                let sp = match &r {
                    Rhs::Copy(p) => p.root_span,
                    _ => target.span,
                };
                self.emit(StmtKind::Rhs(r), sp);
            }
            _ => return unsupported("this assignment target"),
        }
        Ok(())
    }

    /// `d[k] op= v` inserts the value type's default first when `k` is missing (R438).
    pub(super) fn insert_default(
        &mut self,
        d: &j::Expr,
        k: &j::Expr,
        kt: &Ty,
        vt: &Ty,
        at: Span,
    ) -> R<()> {
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

    pub(super) fn ty_of_local(&self, e: &j::Expr) -> Option<Ty> {
        match &e.kind {
            j::ExprKind::Name(n) => {
                let n = self.narrowed(n).unwrap_or(n);
                self.locals.get(n).map(|l| l.ty.clone())
            }
            _ => None,
        }
    }

    pub(super) fn place(&mut self, e: &j::Expr) -> R<(Place, Ty)> {
        self.place_inner(e).map_err(|u| u.at(e.span))
    }

    pub(super) fn place_inner(&mut self, e: &j::Expr) -> R<(Place, Ty)> {
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
                let pt = self.place(b)?;
                self.index_place(e, b, i, pt)
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

    /// `b[i]` as a place, once `b` is lowered to `p` of type `t`.
    pub(super) fn index_place(
        &mut self,
        e: &j::Expr,
        b: &j::Expr,
        i: &j::Expr,
        (p, t): (Place, Ty),
    ) -> R<(Place, Ty)> {
        // N37: an `iter` that no name holds is collected before it is indexed.
        let (mut p, t) = match t {
            Ty::Coll("iter", _)
                if !self.cx.strict
                    && self.can_collect()
                    && !matches!(b.kind, j::ExprKind::Name(_)) =>
            {
                self.lenient(
                    "N37",
                    b.span,
                    "an indexed `iter` normalized to `.collect()`",
                );
                let (r, lt) = self.collect(Rhs::Proj(p), &t, b.span)?;
                let Atom::Name(root, root_span) = self.tmp(&lt, r, b.span) else {
                    unreachable!("a call result is bound")
                };
                let p = Place {
                    root,
                    root_span,
                    projs: Vec::new(),
                };
                (p, lt)
            }
            t => (p, t),
        };
        match &t {
            Ty::Array(..) => {
                if let Some(dims) = t.dims() {
                    self.sites.insert(e.span, Site::Array(dims));
                }
                self.array_place(i, p, &t)
            }
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
                                kind: j::ArgKind::Expr(b.clone()),
                                span: b.span,
                            }],
                        ));
                        from_end = x(j::ExprKind::Binary("+", Box::new(len), Box::new(i.clone())));
                        &from_end
                    }
                    _ => i,
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
            Ty::Coll("iter", _) => {
                self.err(
                    "E201",
                    e.span,
                    "an `iter` is single-pass and cannot be indexed; \
                     `.collect()` it to a list first"
                        .into(),
                );
                Ok((p, Ty::Err))
            }
            t => unsupported(format!("indexing a `{t}` as a place", t = self.ty_text(t))),
        }
    }

    /// Stores the dict elements `place` copied out back into their dicts.
    pub(super) fn store_dict_places(&mut self, places: Vec<DictPlace>, at: Span) {
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
}
