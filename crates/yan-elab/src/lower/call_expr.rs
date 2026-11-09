use super::*;

impl Lower<'_, '_, '_> {
    /// Lower a call. Returns the right-hand side, its success type and its failure type.
    pub(super) fn call_expr(
        &mut self,
        f: &j::Expr,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        self.want_ret = want.map(|w| (w.clone(), at));
        let session = |e: &j::Expr| matches!(&e.kind, j::ExprKind::Name(s) if s == "session" && !self.locals.contains_key(s));
        match &f.kind {
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && op == "connect"
                    && session(m) =>
            {
                self.session_connect(k, args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if matches!(op.as_str(), "send" | "recv" | "close") && session(m) =>
            {
                self.session_op(op, args, at)
            }
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cli" && !self.locals.contains_key(c))
                    && matches!(op.as_str(), "parse" | "parse_args") =>
            {
                self.cli_call(op, k, args, at, None)
            }
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "json" && !self.locals.contains_key(c))
                    && matches!(op.as_str(), "parse" | "dump") =>
            {
                self.json_call(op, Some(k), args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "dump"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "json" && !self.locals.contains_key(c)) =>
            {
                self.json_call(op, None, args, at)
            }
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "csv" && !self.locals.contains_key(c))
                    && matches!(op.as_str(), "parse" | "dump") =>
            {
                self.csv_call(op, Some(k), args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "dump"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "csv" && !self.locals.contains_key(c)) =>
            {
                self.csv_call(op, None, args, at)
            }
            // The schema written out: `cli.parse_args(xs, "{..}")`, what raise prints with no marks.
            j::ExprKind::Attr(m, op, _)
                if op == "parse_args"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cli" && !self.locals.contains_key(c))
                    && let [xs, schema] = args
                    && let j::ArgKind::Expr(j::Expr {
                        kind: j::ExprKind::Lit(j::Lit::Str { value, .. }),
                        ..
                    }) = &schema.kind
                    && let Some(i) = value.rfind("\"name\":\"")
                    && let Some(n) = value[i + 8..].split('"').next()
                    && self.g.records.contains_key(n) =>
            {
                let t = j::Expr {
                    kind: j::ExprKind::Name(n.to_owned()),
                    span: f.span,
                    paren: false,
                };
                self.cli_call("parse_args", &t, std::slice::from_ref(xs), at, Some(value))
            }
            j::ExprKind::Attr(m, op, _)
                if op == "launch"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "gpu" && !self.locals.contains_key(c)) =>
            {
                self.launch(args, at)
            }
            j::ExprKind::Name(n)
                if !self.in_launch
                    && (n == "TileOrigin" || self.g.kernels.contains_key(n))
                    && !self.locals.contains_key(n) =>
            {
                unsupported(if n == "TileOrigin" {
                    "building a `TileOrigin`: only `gpu.launch` supplies one (R953)"
                } else {
                    "calling a `@kernel` function: launch it with `gpu.launch` (RFC 0058)"
                })
            }
            j::ExprKind::Attr(m, op, _)
                if op == "spawn"
                    && matches!(&m.kind, j::ExprKind::Name(a) if a == "actor" && !self.locals.contains_key(a)) =>
            {
                self.spawn_call(args, at, want)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "send_only"
                    && matches!(&m.kind, j::ExprKind::Name(a) if a == "actor" && !self.locals.contains_key(a)) =>
            {
                self.send_only_call(args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "start"
                    && matches!(&m.kind, j::ExprKind::Name(a) if a == "actor" && !self.locals.contains_key(a)) =>
            {
                self.start_call(args, at, want)
            }
            j::ExprKind::Index(b, k) if matches!(&b.kind, j::ExprKind::Name(n) if n == "Channel" && !self.locals.contains_key(n)) =>
            {
                let j::ExprKind::Name(tn) = &k.kind else {
                    return unsupported("`Channel` of a type that is not a name");
                };
                let tn = yan_names::type_name(tn, yan_names::Surface::Yag, yan_names::Surface::Yir)
                    .unwrap_or(tn);
                let named = j::Type::Named {
                    path: vec![tn.to_owned()],
                    args: Vec::new(),
                };
                let t = self.g.ty(&named, self.cx, k.span)?;
                let Some(xs) = self.positional(args, 1, "Channel", at)? else {
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
                };
                let (cap, ct) = self.atom(xs[0], Some(&Ty::Int))?;
                self.expect(&ct, &Ty::Int, xs[0].span);
                let ret = Ty::Tuple(vec![
                    Ty::Coll("Sender", vec![t.clone()]),
                    Ty::Coll("Receiver", vec![t.clone()]),
                ]);
                let c = call("std.chan.Channel", vec![t.targ()], vec![let_arg(cap)], at);
                Ok((Rhs::Call(c), ret, None))
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
                let prelude = !self.g.fns.contains_key(n) || self.g.std_fns.contains(n);
                if prelude
                    && matches!(n.as_str(), "min" | "max")
                    && args.iter().filter(|a| a.name.is_none()).count() == 1
                {
                    return self.extreme(n, args, at, want);
                }
                if prelude
                    && !self.cx.strict
                    && matches!(n.as_str(), "map" | "filter")
                    && let [func, xs] = args
                    && func.name.is_none()
                    && xs.name.is_none()
                    && let j::ArgKind::Expr(recv) = &xs.kind
                {
                    let item = self.item.clone();
                    self.cx.note(
                        "N33",
                        at,
                        &item,
                        format!("`{n}(f, xs)` normalized to `xs.{n}(f)`"),
                    );
                    return self.method(recv, n, f.span, &args[..1], at);
                }
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
                let path = format!("std.prelude.{n}");
                if let Some(p) = crate::stdlib::decl(&path).and_then(|d| d.plain())
                    && p.fail.is_some()
                {
                    return self.std_call(&path, p, Vec::new(), args, n, at);
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
                if let j::ExprKind::Name(module) = &b.kind
                    && module == "math"
                    && !self.locals.contains_key(module)
                    && !self.cx.strict
                    && matches!(m.as_str(), "floor" | "ceil" | "trunc")
                    && want == Some(&Ty::Int)
                {
                    // Python's `floor`, `ceil` and `trunc` return an `int`.
                    let (r, t, fail) = self.call_expr(f, args, at, Some(&Ty::Float))?;
                    if t != Ty::Float || fail.is_some() {
                        return Ok((r, t, fail));
                    }
                    self.lenient(
                        "N39",
                        at,
                        &format!("`math.{m}` normalized to `int(math.{m}(...))`"),
                    );
                    let a = self.tmp(&Ty::Float, r, at);
                    // The conversion has its own span so the mark joins it, not the inner call.
                    let c = call("std.prelude.int", vec![], vec![let_arg(a)], *name_at);
                    self.mark_call(*name_at, yan_surface::Mark::Lenient("N39".into()));
                    return Ok((Rhs::Call(c), Ty::Int, None));
                }
                if let j::ExprKind::Name(module) = &b.kind
                    && module == "math"
                    && !self.locals.contains_key(module)
                    && !self.cx.strict
                    && let Some((e, to)) = math_alias(m, b, *name_at, args, at)
                {
                    self.lenient("N39", at, &format!("`math.{m}` normalized to `{to}`"));
                    self.mark_call(at, yan_surface::Mark::Lenient("N39".into()));
                    return match &e.kind {
                        j::ExprKind::Call(f, args) => self.call_expr(f, args, at, want),
                        _ => {
                            let (r, t) = self.rhs(&e, want)?;
                            Ok((r, t, None))
                        }
                    };
                }
                let flagged;
                let mut flag_mark = None;
                let args = if let j::ExprKind::Name(module) = &b.kind
                    && module == "re"
                    && !self.locals.contains_key(module)
                    && !self.cx.strict
                    && let Some((a, keyword)) = self.re_flags(m, args)
                {
                    flag_mark = Some(if keyword {
                        yan_surface::Mark::FlagsKeyword
                    } else {
                        yan_surface::Mark::FlagsPositional
                    });
                    flagged = a;
                    &flagged[..]
                } else {
                    args
                };
                // Python's `re.search(pattern, s)`: compile the pattern, then call the method.
                if let j::ExprKind::Name(module) = &b.kind
                    && module == "re"
                    && !self.locals.contains_key(module)
                    && !self.cx.strict
                    && let Some(meth) = match m.as_str() {
                        "match" | "search" | "fullmatch" | "split" | "sub" => Some(m.as_str()),
                        "findall" | "finditer" | "find_all" => Some("find_all"),
                        _ => None,
                    }
                    && let [pat, rest @ ..] = args
                    && pat.name.is_none()
                    && !rest.is_empty()
                {
                    if let j::ArgKind::Expr(p) = &pat.kind
                        && let Some(Ty::Data(t)) = self.ty_of_local(p)
                        && t == "Regex"
                    {
                        self.lenient(
                            "N40",
                            at,
                            &format!(
                                "`re.{m}(compiled, ...)` normalized to `compiled.{meth}(...)`"
                            ),
                        );
                        self.mark_call(at, yan_surface::Mark::Lenient("N40".into()));
                        return self.method(p, meth, *name_at, rest, at);
                    }
                    let item = self.item.clone();
                    self.cx.note(
                        "N35",
                        at,
                        &item,
                        format!("`re.{m}(pattern, ...)` normalized to `regex.compile(pattern).{meth}(...)`"),
                    );
                    let name = |n: &str| j::Expr {
                        kind: j::ExprKind::Name(n.to_owned()),
                        span: b.span,
                        paren: false,
                    };
                    let compile = j::Expr {
                        kind: j::ExprKind::Attr(
                            Box::new(name("regex")),
                            "compile".to_owned(),
                            *name_at,
                        ),
                        span: b.span,
                        paren: false,
                    };
                    let compiled = j::Expr {
                        kind: j::ExprKind::Call(Box::new(compile), vec![pat.clone()]),
                        span: b.span,
                        paren: false,
                    };
                    self.mark_call(at, yan_surface::Mark::Lenient("N35".into()));
                    if let Some(m) = flag_mark {
                        self.mark_call(at, m);
                    }
                    return self.method(&compiled, meth, *name_at, rest, at);
                }
                // `bytes.from_list` and `bytearray.from_bytes` name their type's declaring module.
                if let j::ExprKind::Name(module) = &b.kind
                    && !self.locals.contains_key(module)
                    && let module = match module.as_str() {
                        "re" => "regex",
                        m => yan_hir::raise::TYPE_STATICS
                            .iter()
                            .find(|(t, _)| *t == m)
                            .map_or(m, |(_, m)| *m),
                    }
                    && let Some(d) = crate::stdlib::decl(&format!("std.{module}.{m}"))
                    && !d.cap
                {
                    return self.module_call(module, m, args, at);
                }
                if let (j::ExprKind::Call(f, fargs), "context") = (&b.kind, m.as_str()) {
                    return self.context(f, fargs, b.span, args, at);
                }
                let r = self.method(b, m, *name_at, args, at)?;
                if let (Rhs::Call(c), _, _) = &r
                    && NONDET_OPS.contains(&c.path.join(".").as_str())
                    && !self.sig.caps.iter().any(|c| c == "nondet")
                {
                    self.err(
                        "E709",
                        at,
                        format!("`{m}` depends on other tasks; declare `uses nondet`"),
                    );
                }
                if let (Rhs::Call(c), _, _) = &r
                    && c.path.join(".") == "std.actor.share"
                    && let Some(Ty::Coll("Actor", ts)) = self.ty_of_local(b)
                {
                    let state = ts[0].clone();
                    let mut bad: Vec<&String> = self
                        .g
                        .fns
                        .iter()
                        .filter(|(_, s)| {
                            s.params.first().is_some_and(|p| p.ty == state)
                                && s.params[1..].iter().any(|p| !wire(&p.ty))
                        })
                        .map(|(n, _)| n)
                        .collect();
                    bad.sort();
                    if let Some(h) = bad.first() {
                        let msg = format!(
                            "a remote actor's handlers take Wire values, but `{h}` takes one that is not"
                        );
                        self.err("E720", at, msg);
                    }
                }
                if let (Rhs::Call(c), _, _) = &r
                    && c.path.join(".") == "std.chan.sender.send"
                    && c.targs
                        .iter()
                        .any(|t| matches!(t, y::TArg::Type(t) if not_send(t)))
                    && let Some(a) = args.first()
                {
                    self.err(
                        "E415",
                        a.span,
                        "this value is not Send (it holds an `Rc`), so it cannot cross to another task".into(),
                    );
                }
                Ok(r)
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
}
