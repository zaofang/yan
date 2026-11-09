use super::*;

impl Lower<'_, '_, '_> {
    /// `h.f(args)` on an `Actor[S]` calls the handler `def f(var self: S, ...)` (R435) on the
    /// actor's state: `take` it, run the handler, then `put` or `settle` it back with the outcome.
    pub(super) fn handler_call(
        &mut self,
        b: &j::Expr,
        m: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<Option<(Rhs, Ty, Option<Ty>)>> {
        if let j::ExprKind::Call(f, hs) = &b.kind
            && matches!(&f.kind, j::ExprKind::Attr(a, op, _) if op == "send"
                && matches!(&a.kind, j::ExprKind::Name(n) if n == "actor" && !self.locals.contains_key(n)))
            && let [h] = &hs[..]
            && h.name.is_none()
            && let j::ArgKind::Expr(h) = &h.kind
        {
            return self.send_call(h, m, args, at).map(Some);
        }
        let Some(Ty::Coll("Actor", ts)) = self.ty_of_local(b) else {
            return Ok(None);
        };
        let state = ts[0].clone();
        let Some(sig) = self
            .g
            .fns
            .get(m)
            .filter(|s| s.params.first().is_some_and(|p| p.ty == state))
            .cloned()
        else {
            return Ok(None);
        };
        if self.g.reentrant.contains(m) {
            return unsupported("a call to a `@reentrant` actor handler, before RFC 0056 slice 4");
        }
        let (h, _) = self.atom(b, None)?;
        let take = call(
            "std.actor.take",
            vec![state.targ()],
            vec![let_arg(h.clone())],
            at,
        );
        let s = self.fresh("s");
        self.bind(&s, true, &state, Rhs::Call(take), at, at);
        let recv = j::Expr {
            kind: j::ExprKind::Name(s.clone()),
            span: b.span,
            paren: false,
        };
        let (r, t, fail) = self.call_sig(m, &sig, Some(&recv), args, at)?;
        let back = Atom::Name(s, at);
        match (fail, r) {
            (Some(e), Rhs::Call(c)) => {
                let rt = Ty::Result(Box::new(t.clone()), Box::new(e.clone()));
                let tried = self.tmp(&rt, Rhs::Try(c), at);
                let targs = vec![state.targ(), t.targ(), e.targ()];
                let args = vec![let_arg(h), sink_arg(back), sink_arg(tried)];
                let c = call("std.actor.settle", targs, args, at);
                Ok(Some((Rhs::Call(c), t, Some(e))))
            }
            (Some(_), _) => unsupported("a fallible handler call that is not a plain call"),
            (None, r) => {
                let v = self.tmp(&t, r, at);
                let put = call(
                    "std.actor.put",
                    vec![state.targ()],
                    vec![let_arg(h), sink_arg(back)],
                    at,
                );
                self.emit(StmtKind::Rhs(Rhs::Call(put)), at);
                Ok(Some((Rhs::Atom(v), t, None)))
            }
        }
    }
}

impl Lower<'_, '_, '_> {
    /// `actor.spawn(state, capacity=1024)` moves `state` into a new actor (R956).
    pub(super) fn spawn_call(&mut self, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        let expr = |a: &j::Arg| match &a.kind {
            j::ArgKind::Expr(e) => Some(e.clone()),
            _ => None,
        };
        let (state, cap) = match args {
            [s] if s.name.is_none() => (expr(s), None),
            [s, c] if s.name.is_none() && c.name.as_deref().is_none_or(|n| n == "capacity") => {
                (expr(s), expr(c).map(Some))
            }
            _ => (None, None),
        };
        let (Some(state), Some(cap)) = (state, cap.or(Some(None))) else {
            self.err(
                "E201",
                at,
                "`actor.spawn` takes a state and an optional `capacity`".into(),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let (state, st) = self.atom(&state, None)?;
        let cap = match cap {
            Some(e) => {
                let (c, ct) = self.atom(&e, Some(&Ty::Int))?;
                self.expect(&ct, &Ty::Int, e.span);
                c
            }
            None => Atom::Lit(Literal::Int("1024".into()), at),
        };
        let c = call(
            "std.actor.spawn",
            vec![st.targ()],
            vec![sink_arg(state), let_arg(cap)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Coll("Actor", vec![st]), None))
    }
}

impl Lower<'_, '_, '_> {
    /// `actor.send(h).f(args)` (R813): move the arguments now and enqueue `lambda: h.f(args)` in
    /// the actor's mailbox; the actor runs it when it is next scheduled.
    fn send_call(
        &mut self,
        h: &j::Expr,
        m: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let (ha, ht) = self.atom(h, None)?;
        let Ty::Coll("Actor", ts) = &ht else {
            self.err(
                "E201",
                h.span,
                "`actor.send` takes an `Actor` handle".into(),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let sig = self
            .g
            .fns
            .get(m)
            .filter(|s| s.params.first().is_some_and(|p| p.ty == ts[0]))
            .cloned();
        match sig {
            None => {
                return unsupported("`actor.send` of a name that is not a handler of the state");
            }
            Some(s) if s.ret != Ty::Unit || s.fail.is_some() => {
                return unsupported("`actor.send` of a handler with a result or a fail type");
            }
            // R397 runs a message under its supervisor's capabilities, which no actor holds yet;
            // the sender's would escape in the message (E306).
            Some(s) if !s.caps.is_empty() => {
                return unsupported(
                    "`actor.send` of a handler with `uses`, before RFC 0056 slice 3",
                );
            }
            Some(_) => {}
        }
        let name = |n: String, span: Span| j::Expr {
            kind: j::ExprKind::Name(n),
            span,
            paren: false,
        };
        let hn = self.fresh("h");
        self.bind(&hn, false, &ht, Rhs::Atom(ha), at, at);
        let hv = Atom::Name(hn.clone(), at);
        let mut moved = Vec::with_capacity(args.len());
        for a in args {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported("a spread argument to `actor.send`");
            };
            let (v, t) = self.atom(e, None)?;
            let n = self.fresh("m");
            self.bind(&n, false, &t, Rhs::Atom(v), e.span, e.span);
            moved.push(j::Arg {
                name: a.name.clone(),
                kind: j::ArgKind::Expr(name(n, e.span)),
                span: a.span,
            });
        }
        let body = j::Expr {
            kind: j::ExprKind::Call(
                Box::new(j::Expr {
                    kind: j::ExprKind::Attr(Box::new(name(hn, h.span)), m.into(), at),
                    span: at,
                    paren: false,
                }),
                moved,
            ),
            span: at,
            paren: false,
        };
        let want = Ty::Fn(false, Vec::new(), Box::new(Ty::Unit));
        let (clo, ct) = self.lambda(&[], &body, Some(&want))?;
        let clo = self.tmp(&ct, clo, at);
        let c = call(
            "std.actor.send",
            vec![ts[0].targ()],
            vec![let_arg(hv), sink_arg(clo)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Unit, None))
    }
}

impl Lower<'_, '_, '_> {
    /// `with supervisor(spec): body`: the actors started in `body` are children of a supervisor
    /// with `spec`, stopped in shutdown order when `body` ends (9.6).
    pub(super) fn with_supervisor(&mut self, spec: &j::Expr, body: &j::Block, at: Span) -> R<()> {
        let want = Ty::Data("Supervisor".into());
        let (a, t) = self.atom(spec, Some(&want))?;
        self.expect(&t, &want, spec.span);
        let b = self.block(&body.stmts)?;
        self.emit(
            StmtKind::With {
                name: "supervisor".into(),
                dyn_: false,
                handler: a,
                body: b,
            },
            at,
        );
        Ok(())
    }
}

impl Lower<'_, '_, '_> {
    /// `actor.start(ChildSpec(f, (args...), restart=permanent), capacity=1024)`: a restartable
    /// child whose every start calls the top-level `f(args)`, the arguments evaluated once (R958).
    pub(super) fn start_call(&mut self, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        let (spec, cap) = match args {
            [s] if s.name.is_none() => (arg_expr(s), None),
            [s, c] if s.name.is_none() && c.name.as_deref().is_none_or(|n| n == "capacity") => {
                (arg_expr(s), arg_expr(c))
            }
            _ => (None, None),
        };
        let parts = spec.and_then(|e| match &e.kind {
            j::ExprKind::Call(f, cargs)
                if matches!(&f.kind, j::ExprKind::Name(n) if n == "ChildSpec") =>
            {
                child_spec(cargs)
            }
            _ => None,
        });
        let Some((f, fargs, restart)) = parts else {
            self.err(
                "E201",
                at,
                "`actor.start` takes `ChildSpec(start, (args...), restart=permanent)` and an \
                 optional `capacity`; `restart` is `permanent`, `transient` or `temporary`"
                    .into(),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let Some(sig) = self.g.fns.get(f.0.as_str()).cloned() else {
            self.err(
                "E201",
                f.1,
                format!(
                    "the start of a `ChildSpec` is a top-level function; `{}` is not one",
                    f.0
                ),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        if sig.fail.is_some() {
            return unsupported("a `ChildSpec` start function with a fail type");
        }
        // R958 gives a start the capabilities of the scope that binds the supervisor, which no
        // actor holds yet; the starter's would escape in the child.
        if !sig.caps.is_empty() {
            return unsupported("a `ChildSpec` start function with `uses`");
        }
        let name = |n: String, span: Span| j::Expr {
            kind: j::ExprKind::Name(n),
            span,
            paren: false,
        };
        let mut moved = Vec::with_capacity(fargs.len());
        for e in fargs {
            let (v, t) = self.atom(e, None)?;
            let n = self.fresh("a");
            self.bind(&n, false, &t, Rhs::Atom(v), e.span, e.span);
            moved.push(j::Arg {
                name: None,
                kind: j::ArgKind::Expr(name(n, e.span)),
                span: e.span,
            });
        }
        let body = j::Expr {
            kind: j::ExprKind::Call(Box::new(name(f.0.clone(), f.1)), moved),
            span: at,
            paren: false,
        };
        let state = sig.ret.clone();
        let want = Ty::Fn(false, Vec::new(), Box::new(state.clone()));
        let (clo, ct) = self.lambda(&[], &body, Some(&want))?;
        let clo = self.tmp(&ct, clo, at);
        let cap = match cap {
            Some(e) => {
                let (c, ct) = self.atom(e, Some(&Ty::Int))?;
                self.expect(&ct, &Ty::Int, e.span);
                c
            }
            None => Atom::Lit(Literal::Int("1024".into()), at),
        };
        let restart = Atom::Lit(Literal::Int(restart.to_string()), at);
        let c = call(
            "std.actor.start",
            vec![state.targ()],
            vec![sink_arg(clo), let_arg(restart), let_arg(cap)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Coll("Actor", vec![state]), None))
    }
}

/// The start function, its arguments and the restart type (0 `permanent`, 1 `transient`,
/// 2 `temporary`) of `ChildSpec(start, (args...), restart=...)`.
fn arg_expr(a: &j::Arg) -> Option<&j::Expr> {
    match &a.kind {
        j::ArgKind::Expr(e) => Some(e),
        _ => None,
    }
}

fn child_spec(args: &[j::Arg]) -> Option<((String, Span), &[j::Expr], u8)> {
    let expr = arg_expr;
    let (f, a, r) = match args {
        [f, a] => (f, a, None),
        [f, a, r] if r.name.as_deref() == Some("restart") => (f, a, Some(r)),
        _ => return None,
    };
    if f.name.is_some() || a.name.is_some() {
        return None;
    }
    let j::ExprKind::Name(n) = &expr(f)?.kind else {
        return None;
    };
    let j::ExprKind::Tuple(xs) = &expr(a)?.kind else {
        return None;
    };
    let restart = match r {
        None => 0,
        Some(r) => match &expr(r)?.kind {
            j::ExprKind::Name(r) if r == "permanent" => 0,
            j::ExprKind::Name(r) if r == "transient" => 1,
            j::ExprKind::Name(r) if r == "temporary" => 2,
            _ => return None,
        },
    };
    Some(((n.clone(), expr(f)?.span), xs, restart))
}
