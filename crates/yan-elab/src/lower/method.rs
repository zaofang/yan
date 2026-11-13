use super::*;

impl Lower<'_, '_, '_> {
    /// `module.v` names variant `v` of a std sum type declared in `module`, as `math.Up`.
    /// Whether `e` writes a std variant with its module: `math.Up()` or `math.Up`.
    pub(super) fn qualified_variant(&self, e: &j::Expr) -> bool {
        let callee = match &e.kind {
            j::ExprKind::Call(f, args) if args.is_empty() => &f.kind,
            k => k,
        };
        matches!(callee, j::ExprKind::Attr(b, m, _)
            if matches!(&b.kind, j::ExprKind::Name(module)
                if !self.locals.contains_key(module) && self.std_variant(module, m)))
    }

    pub(super) fn std_variant(&self, module: &str, v: &str) -> bool {
        self.g
            .variant_of
            .get(v)
            .is_some_and(|s| crate::stdlib::sum_module(s) == Some(module))
    }

    /// The arguments of a std call matched to `names`: positional, then by keyword, then the
    /// declared defaults. `None` after reporting a mismatch.
    pub(super) fn bind_std<'e>(
        &mut self,
        args: &'e [j::Arg],
        names: &[(&str, Option<&'e j::Expr>)],
        positional: usize,
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
                    Some(i) if i < positional => {
                        self.err("E202", a.span, format!("`{what}` takes `{n}` by position"));
                        return Ok(None);
                    }
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
            if self.qualified_variant(e) {
                self.cx.marks.push(crate::marks::Pending {
                    span: e.span,
                    kind: crate::marks::Kind::Arg,
                    call: Some(at),
                    mark: yan_surface::Mark::Qualified,
                });
            }
            if a.name.is_some() || i >= positional {
                self.cx.marks.push(crate::marks::Pending {
                    span: e.span,
                    kind: crate::marks::Kind::Arg,
                    call: Some(at),
                    mark: if a.name.is_some() {
                        yan_surface::Mark::Keyword
                    } else {
                        yan_surface::Mark::Positional
                    },
                });
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
    pub(super) fn std_call(
        &mut self,
        path: &str,
        p: crate::stdlib::Plain,
        mut lead: Vec<Arg>,
        args: &[j::Arg],
        what: &str,
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let Some(xs) = self.bind_std(args, &p.names, p.positional, what, at)? else {
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

    pub(super) fn method(
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
        if let Some(r) = self.handler_call(b, m, args, at)? {
            return Ok(r);
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
        let canon = match (&t, m) {
            (Ty::Char, "isdigit") => "is_digit",
            (Ty::Char, "isalpha") => "is_alpha",
            (Ty::Char, "isalnum") => "is_alnum",
            (Ty::Char, "isupper") => "is_upper",
            (Ty::Char, "islower") => "is_lower",
            (Ty::Char, "isspace") => "is_space",
            (Ty::Str, "is_digit") => "isdigit",
            (Ty::Str, "is_alpha") => "isalpha",
            (Ty::Str, "is_alnum") => "isalnum",
            (Ty::Str, "is_upper") => "isupper",
            (Ty::Str, "is_lower") => "islower",
            (Ty::Str, "is_space") => "isspace",
            (Ty::Data(n), "findall" | "finditer") if n == "Regex" => "find_all",
            _ => m,
        };
        if canon != m && !self.cx.strict {
            let item = self.item.clone();
            self.cx.note(
                "N34",
                at,
                &item,
                format!(
                    "`{m}` on a `{t}` normalized to `{canon}`",
                    t = self.ty_text(&t)
                ),
            );
        }
        let m = if self.cx.strict { m } else { canon };
        let zero = [j::Arg {
            name: None,
            kind: j::ArgKind::Expr(j::Expr {
                kind: j::ExprKind::Lit(j::Lit::Int("0".into())),
                span: at,
                paren: false,
            }),
            span: at,
        }];
        let args = match &t {
            Ty::Data(n) if n == "Match" && m == "group" && args.is_empty() && !self.cx.strict => {
                let item = self.item.clone();
                self.cx.note(
                    "N34",
                    at,
                    &item,
                    "`group()` normalized to `group(0)`".into(),
                );
                &zero[..]
            }
            _ => args,
        };
        // `std.text` declares these on `str` only; a `char` goes through its text.
        if t == Ty::Char
            && matches!(m, "upper" | "lower" | "parse_int" | "parse_float")
            && args.is_empty()
        {
            let c = if p.projs.is_empty() {
                Atom::Name(p.root, p.root_span)
            } else {
                self.tmp(&t, Rhs::Proj(p), b.span)
            };
            let text = call("std.fmt.show", vec![t.targ()], vec![let_arg(c)], b.span);
            let s = self.tmp(&Ty::Str, Rhs::Call(text), b.span);
            let c = call(&format!("std.text.{m}"), vec![], vec![let_arg(s)], at);
            let ret = match m {
                "parse_int" => Ty::Option(Box::new(Ty::Int)),
                "parse_float" => Ty::Option(Box::new(Ty::Float)),
                _ => Ty::Str,
            };
            return Ok((Rhs::Call(c), ret, None));
        }
        let std_class = match &t {
            Ty::Data(n) if self.g.std_records.contains(n) => crate::stdlib::class_module(n),
            Ty::Data(n) if self.g.std_sums.contains(n) => crate::stdlib::sum_module(n),
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
            if matches!(t, Ty::Coll("iter", _)) && m == "len" {
                self.err(
                    "E203",
                    name_at,
                    "an `iter` has no `len`; it is single-pass, so `.count()` consumes it \
                     (or `.collect()` it to a list first)"
                        .into(),
                );
                return Ok((Rhs::Proj(p), e(), None));
            }
            self.err_near(
                "E203",
                name_at,
                format!("`{t}` has no method `{m}`", t = self.ty_text(&t)),
                m,
                names,
            );
            return Ok((Rhs::Proj(p), e(), None));
        };
        if m == "len"
            || matches!(
                path.as_str(),
                "std.dict.index" | "std.collections.sorted_dict.index" | "std.list.concat"
            )
        {
            self.mark_call(at, yan_surface::Mark::Method);
        }
        if !d.intrinsic || d.typed_at_call() {
            // A receiver that is not a place was lowered into `p.root` above; naming it keeps
            // `user_call` from lowering it a second time.
            if p.projs.is_empty()
                && !matches!(
                    b.kind,
                    j::ExprKind::Name(_) | j::ExprKind::Attr(..) | j::ExprKind::Index(..)
                )
            {
                let recv = j::Expr {
                    kind: j::ExprKind::Name(p.root),
                    span: b.span,
                    paren: false,
                };
                return self.user_call(&path, Some(&recv), args, at);
            }
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
            fail,
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
            if *take {
                self.sink_of_element(&a, x.span);
            }
            out.push(Arg::Value(conv, a, x.span));
        }
        for _ in n..params.len() {
            out.push(let_arg(Atom::Name("None".into(), at)));
        }
        let rhs = Rhs::Call(call(&path, targs, out, at));
        if writeback.is_none() && (!inout || stores.is_empty()) {
            return Ok((rhs, ret, fail));
        }
        if fail.is_some() {
            return unsupported(format!("a fallible `{m}` on a field or element"));
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

    /// A contract expression that is one operation on names and literals, as Yan Core requires.
    pub(super) fn pure(&mut self, e: &j::Expr) -> R<Option<y::Expr>> {
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

    /// A `cost` bound (R610) as one Yan Core expression: itself when it is one operation, else a
    /// call of a generated `__cost_<f>_<which>` over `f`'s value parameters that returns it.
    pub(super) fn cost_bound(&mut self, f: &j::FuncDef, which: &str, e: &j::Expr) -> R<y::Expr> {
        if let Some(x) = self.pure(e)? {
            return Ok(x);
        }
        let params: Vec<j::Param> = f
            .params
            .iter()
            .filter_map(|p| match p {
                j::Param::Value { name, span, ty, .. } => Some(j::Param::Value {
                    conv: None,
                    name: name.clone(),
                    span: *span,
                    ty: ty.clone(),
                    default: None,
                }),
                _ => None,
            })
            .collect();
        let args = params
            .iter()
            .filter_map(|p| match p {
                j::Param::Value { name, span, .. } => {
                    Some(let_arg(Atom::Name(name.clone(), *span)))
                }
                _ => None,
            })
            .collect();
        let name = format!("__cost_{}_{which}", f.name);
        let h = j::FuncDef {
            name: name.clone(),
            name_span: e.span,
            tparams: f.tparams.clone(),
            params,
            ret: Some(j::RetType {
                ty: Some(j::Type::Named {
                    path: vec!["i64".into()],
                    args: Vec::new(),
                }),
                fail: Vec::new(),
                from: Vec::new(),
            }),
            uses: None,
            body: Some(j::Body {
                doc: None,
                contracts: Vec::new(),
                stmts: vec![j::Stmt {
                    kind: j::StmtKind::Return(Some(e.clone())),
                    span: e.span,
                }],
            }),
        };
        // The helper has no source of its own, so it leaves no spelling marks.
        let kept = self.cx.spelling();
        let sig = crate::sig(self.g, &h, self.cx)?;
        let fd = function(self.g, &h, &sig, false, self.cx);
        self.cx.restore_spelling(kept);
        let fd = fd?;
        let mut generated = self.g.generated.borrow_mut();
        if !generated
            .iter()
            .any(|it| matches!(&it.kind, y::ItemKind::Fn(d) if d.path == [name.as_str()]))
        {
            generated.push(y::Item {
                attrs: Vec::new(),
                kind: y::ItemKind::Fn(fd),
                span: e.span,
            });
        }
        Ok(y::Expr::Call(call(&name, Vec::new(), args, e.span)))
    }

    pub(super) fn pure_atom(&self, e: &j::Expr) -> Option<(Atom, Ty)> {
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
