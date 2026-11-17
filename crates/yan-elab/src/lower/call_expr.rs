use super::*;
use yan_core::auto_traits::auto_traits;

pub(crate) const NOT_SEND: &str = "this value is not Send (it holds an `Rc` or a `MutexGuard`), so it cannot cross to another task";

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
        let session = |e: &j::Expr| matches!(&e.kind, j::ExprKind::Name(s) if s == "session" && !self.value_bound(s));
        match &f.kind {
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Name(c) = &b.kind
                    && matches!(c.as_str(), "wrap_as" | "sat_as" | "trunc_as")
                    && !self.value_bound(c)
                    && !self.g.fns.contains_key(c) =>
            {
                self.conv_call(c, k, args, at)
            }
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
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cli" && !self.value_bound(c))
                    && matches!(op.as_str(), "parse" | "parse_args") =>
            {
                self.cli_call(op, k, args, at, None)
            }
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "json" && !self.value_bound(c))
                    && matches!(op.as_str(), "parse" | "dump") =>
            {
                self.json_call(op, Some(k), args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "dump"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "json" && !self.value_bound(c)) =>
            {
                self.json_call(op, None, args, at)
            }
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "csv" && !self.value_bound(c))
                    && matches!(op.as_str(), "parse" | "dump") =>
            {
                self.csv_call(op, Some(k), args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "dump"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "csv" && !self.value_bound(c)) =>
            {
                self.csv_call(op, None, args, at)
            }
            // The schema written out: `cli.parse_args(xs, "{..}")`, what raise prints with no marks.
            j::ExprKind::Attr(m, op, _)
                if op == "parse_args"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cli" && !self.value_bound(c))
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
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "gpu" && !self.value_bound(c)) =>
            {
                self.launch(args, at)
            }
            j::ExprKind::Name(n)
                if !self.in_launch
                    && (n == "TileOrigin" || self.g.kernels.contains_key(n))
                    && !self.value_bound(n) =>
            {
                crate::refuse(
                    crate::kernel::MISUSE,
                    if n == "TileOrigin" {
                        "only `gpu.launch` builds a `TileOrigin`"
                    } else {
                        "a `@kernel` function is not called directly: launch it with `gpu.launch`"
                    },
                )
            }
            j::ExprKind::Attr(m, op, _)
                if op == "spawn"
                    && matches!(&m.kind, j::ExprKind::Name(a) if a == "actor" && !self.value_bound(a)) =>
            {
                self.spawn_call(args, at, want)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "monitor"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cluster")
                    && self.sig.caps.iter().any(|c| c == "cluster") =>
            {
                self.cluster_monitor(args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "group"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cluster")
                    && self.sig.caps.iter().any(|c| c == "cluster") =>
            {
                self.cluster_group(args, at, want)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "send_only"
                    && matches!(&m.kind, j::ExprKind::Name(a) if a == "actor" && !self.value_bound(a)) =>
            {
                self.send_only_call(args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "spawn"
                    && matches!(&m.kind, j::ExprKind::Name(c) if c == "cluster")
                    && self.sig.caps.iter().any(|c| c == "cluster") =>
            {
                self.cluster_spawn(m, args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "start"
                    && matches!(&m.kind, j::ExprKind::Name(a) if a == "actor" && !self.value_bound(a)) =>
            {
                self.start_call(args, at, want)
            }
            j::ExprKind::Attr(m, op, _)
                if op == "of"
                    && matches!(&m.kind, j::ExprKind::Name(p) if p == "Ptr" && !self.value_bound(p)) =>
            {
                self.ptr_of(args, at)
            }
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && op == "cast" =>
            {
                self.ptr_cast(m, k, args, at)
            }
            j::ExprKind::Attr(m, op, _)
                if (self.cx.ffi || ptr::exposes(op))
                    && matches!(&m.kind, j::ExprKind::Name(p) if p == "ptr" && !self.value_bound(p)) =>
            {
                self.ptr_call(op, None, args, at, want)
            }
            j::ExprKind::Index(b, k)
                if let j::ExprKind::Attr(m, op, _) = &b.kind
                    && (self.cx.ffi || ptr::exposes(op))
                    && matches!(&m.kind, j::ExprKind::Name(p) if p == "ptr" && !self.value_bound(p)) =>
            {
                self.ptr_call(op, Some(k), args, at, want)
            }
            j::ExprKind::Index(b, k) if matches!(&b.kind, j::ExprKind::Name(n) if n == "Channel" && !self.value_bound(n)) =>
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
            j::ExprKind::Index(b, k) if matches!(&b.kind, j::ExprKind::Name(n) if n == "Atomic" && !self.value_bound(n)) =>
            {
                let j::ExprKind::Name(tn) = &k.kind else {
                    return unsupported("`Atomic` of a type that is not a name");
                };
                let tn = yan_names::type_name(tn, yan_names::Surface::Yag, yan_names::Surface::Yir)
                    .unwrap_or(tn);
                let named = j::Type::Named {
                    path: vec![tn.to_owned()],
                    args: Vec::new(),
                };
                let t = self.g.ty(&named, self.cx, k.span)?;
                let Some(xs) = self.positional(args, 1, "Atomic", at)? else {
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
                };
                let (v, vt) = self.atom(xs[0], Some(&t))?;
                self.expect(&vt, &t, xs[0].span);
                let c = call(
                    "std.sync.atomic.Atomic",
                    vec![t.targ()],
                    vec![sink_arg(v)],
                    at,
                );
                Ok((Rhs::Call(c), Ty::Coll("Atomic", vec![t]), None))
            }
            j::ExprKind::Index(b, k) if matches!(&b.kind, j::ExprKind::Name(n) if n == "Mutex" && !self.value_bound(n)) =>
            {
                let Some(named) = self.type_expr(k) else {
                    return unsupported("`Mutex` of a type that is not a type expression");
                };
                let t = self.g.ty(&named, self.cx, k.span)?;
                let Some(xs) = self.positional(args, 1, "Mutex", at)? else {
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
                };
                let (v, vt) = self.atom(xs[0], Some(&t))?;
                self.expect(&vt, &t, xs[0].span);
                let c = call(
                    "std.sync.mutex.Mutex",
                    vec![t.targ()],
                    vec![sink_arg(v)],
                    at,
                );
                Ok((Rhs::Call(c), Ty::Coll("Mutex", vec![t]), None))
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
            j::ExprKind::Name(n) if !self.value_bound(n) && self.pending.contains_key(n) => {
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
            j::ExprKind::Name(n) if !self.value_bound(n) => {
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
                if self.g.records.contains_key(n) && self.g.type_params.contains_key(n) {
                    let (r, t) = self.generic_ctor(n, n, args, at, want)?;
                    return Ok((r, t, None));
                }
                if let Some(fs) = self.g.records.get(n) {
                    let pos = self.g.record_positional.get(n).copied().unwrap_or(0);
                    let ps = field_params(fs, pos, at);
                    let r = self.ctor(None, n, &ps, args, at)?;
                    return Ok((r, Ty::Data(n.clone()), None));
                }
                if let Some(s) = self.g.variant_of.get(n).cloned() {
                    if self.g.shared_variants.contains(n) {
                        self.err(
                            "E200",
                            f.span,
                            format!("is a variant of several sums; write `Sum.{n}`"),
                        );
                        return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
                    }
                    return self.variant_call(n, &s, args, at, want);
                }
                let path = format!("std.prelude.{n}");
                if let Some(p) = crate::stdlib::decl(&path).and_then(|d| d.plain())
                    && p.fail.is_some()
                {
                    return self.std_call(&path, p, Vec::new(), args, n, at);
                }
                if matches!(n.as_str(), "fence" | "compiler_fence") {
                    let path = format!("std.sync.atomic.{n}");
                    if let Some(p) = crate::stdlib::decl(&path).and_then(|d| d.plain()) {
                        return self.std_call(&path, p, Vec::new(), args, n, at);
                    }
                }
                let (r, t) = self.builtin(n, args, at, want)?;
                Ok((r, t, None))
            }
            j::ExprKind::Attr(b, m, name_at) => {
                if let j::ExprKind::Name(c) = &b.kind
                    && !self.value_bound(c)
                    && (self.sig.caps.contains(c) || self.handlers.contains(c) || c == "io")
                {
                    return self.cap_call(c, m, args, at);
                }
                if let j::ExprKind::Name(t) = &b.kind
                    && !self.value_bound(t)
                    && let Some(s) = self.g.qualified_variant(t, m).map(str::to_owned)
                {
                    return self.variant_call(m, &s, args, at, want);
                }
                if let j::ExprKind::Name(module) = &b.kind
                    && let Some((r, t)) =
                        self.lenient_module_call(module, m, args, at, *name_at, want)?
                {
                    return Ok((r, t, None));
                }
                if let j::ExprKind::Name(module) = &b.kind
                    && !self.value_bound(module)
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
                    && !self.value_bound(module)
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
                    && !self.value_bound(module)
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
                    && !self.value_bound(module)
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
                    && !self.value_bound(module)
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
                    && !self.value_bound(module)
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
                    self.wire_handlers(&ts[0], at);
                }
                if let (Rhs::Call(c), _, _) = &r
                    && matches!(
                        c.path.join(".").as_str(),
                        "std.chan.sender.send" | "std.chan.sender.try_send"
                    )
                    && c.targs.iter().any(|t| {
                        matches!(t, y::TArg::Type(t) if !auto_traits(&|n| self.g.components(n), t).send)
                    })
                    && let Some(a) = args.first()
                {
                    self.err(
                        "E415",
                        a.span,
                        NOT_SEND.into(),
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
