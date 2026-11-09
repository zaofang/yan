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
        let fail = actor_fail(sig.fail.as_ref());
        let (h, _) = self.atom(b, None)?;
        let took = Ty::Result(Box::new(state.clone()), Box::new(fail.clone()));
        let take = call(
            "std.actor.take",
            vec![state.targ(), fail.targ()],
            vec![let_arg(h.clone())],
            at,
        );
        let took = self.tmp(&took, Rhs::Call(take), at);
        let (v, rt) = self.join(at, |l, k| {
            let (o, s) = (l.fresh("o"), l.fresh("s"));
            let mut ret = Ty::Err;
            let ok = l.block_with(|l| {
                l.declare(&o, state.clone());
                l.bind(
                    &s,
                    true,
                    &state,
                    Rhs::Atom(Atom::Name(o.clone(), at)),
                    at,
                    at,
                );
                let recv = j::Expr {
                    kind: j::ExprKind::Name(s.clone()),
                    span: b.span,
                    paren: false,
                };
                let (r, t, e) = l.call_sig(m, &sig, Some(&recv), args, at)?;
                let rt = Ty::Result(Box::new(t.clone()), Box::new(fail.clone()));
                let back = Atom::Name(s.clone(), at);
                let out = match (e, r) {
                    (Some(e), Rhs::Call(c)) => {
                        let tried = Ty::Result(Box::new(t.clone()), Box::new(e.clone()));
                        let tried = l.tmp(&tried, Rhs::Try(c), at);
                        let targs = vec![state.targ(), t.targ(), e.targ(), fail.targ()];
                        let args = vec![let_arg(h.clone()), sink_arg(back), sink_arg(tried)];
                        let c = call("std.actor.settle", targs, args, at);
                        l.tmp(&rt, Rhs::Call(c), at)
                    }
                    (Some(_), _) => {
                        return unsupported("a fallible handler call that is not a plain call");
                    }
                    (None, r) => {
                        let v = l.tmp(&t, r, at);
                        let put = call(
                            "std.actor.put",
                            vec![state.targ()],
                            vec![let_arg(h.clone()), sink_arg(back)],
                            at,
                        );
                        l.emit(StmtKind::Rhs(Rhs::Call(put)), at);
                        l.tmp(&rt, ctor("Ok", "value", v), at)
                    }
                };
                l.jump(k, out, at);
                ret = rt;
                Ok(())
            })?;
            let e = l.fresh("e");
            let err = l.block_with(|l| {
                l.declare(&e, fail.clone());
                let out = l.tmp(&ret, ctor("Err", "error", Atom::Name(e.clone(), at)), at);
                l.jump(k, out, at);
                Ok(())
            })?;
            let arm = |c: &str, f: &str, n: &str| y::Pat::Ctor {
                path: vec![c.into()],
                fields: Some((vec![(f.into(), y::Pat::Bind(n.into()))], false)),
            };
            let m = y::Stmt {
                kind: StmtKind::Match {
                    scrutinee: took,
                    arms: vec![(arm("Ok", "value", &o), ok), (arm("Err", "error", &e), err)],
                },
                span: at,
            };
            Ok((m, ret.clone()))
        })?;
        let Ty::Result(t, _) = &rt else {
            return Ok(Some((Rhs::Atom(v), Ty::Err, None)));
        };
        let t = (**t).clone();
        let c = call(
            "std.actor.reply",
            vec![t.targ(), fail.targ()],
            vec![sink_arg(v)],
            at,
        );
        Ok(Some((Rhs::Call(c), t, Some(fail))))
    }
}

/// A handler call fails with the handler's own failures plus `ActorError` (RFC 0056 D1).
fn actor_fail(own: Option<&Ty>) -> Ty {
    let mut ms = Vec::new();
    let actor = ["Stopped", "Timeout", "Cancelled", "Deadlock"].map(|n| Ty::Data(n.into()));
    for t in own.into_iter().cloned().chain(actor) {
        crate::union_add(&mut ms, t);
    }
    Ty::Union(ms)
}

fn ctor(name: &str, field: &str, a: Atom) -> Rhs {
    Rhs::Ctor(y::Ctor::Named {
        path: vec![name.into()],
        targs: Vec::new(),
        fields: vec![(field.into(), a)],
    })
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
                let what = match (&s.ret, &s.fail) {
                    (Ty::Unit, Some(e)) => format!("fails `{}`", self.ty_text(e)),
                    (t, _) => format!("returns `{}`", self.ty_text(t)),
                };
                self.err(
                    "E729",
                    at,
                    format!(
                        "`{m}` {what}; only a handler with a unit result and no fail type can be \
                         sent (R813); call it as `h.{m}(...)` to get its outcome"
                    ),
                );
                return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
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
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let handler = ex(j::ExprKind::Attr(Box::new(name(hn, h.span)), m.into(), at));
        let msg = ex(j::ExprKind::Call(Box::new(handler), moved));
        // A sent handler has no failure of its own, and an admitted message runs on an idle
        // actor, so the call's `ActorError` cannot occur; a stopped actor drops it (R813).
        let none = j::Fallback::Value(ex(j::ExprKind::Lit(j::Lit::None)));
        let body = ex(j::ExprKind::Fallback(Box::new(msg), Box::new(none)));
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
