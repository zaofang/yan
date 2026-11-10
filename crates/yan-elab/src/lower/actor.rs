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
        let (kind, ts, remote) = match self.ty_of_local(b) {
            Some(Ty::Coll(k @ ("Actor" | "SendOnly"), ts)) => (k, ts, false),
            Some(Ty::Coll("Remote", r)) => match r.into_iter().next() {
                Some(Ty::Coll("Actor", ts)) => ("Actor", ts, true),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let (state, policy) = (ts[0].clone(), ts[1].clone());
        let Some(sig) = self
            .g
            .fns
            .get(m)
            .filter(|s| s.params.first().is_some_and(|p| p.ty == state))
            .cloned()
        else {
            return Ok(None);
        };
        if kind == "SendOnly"
            && !matches!((&b.kind, &self.sent_through), (j::ExprKind::Name(n), Some(s)) if n == s)
        {
            self.err(
                "E201",
                at,
                format!(
                    "a `SendOnly` handle can send but not call `{m}` (R975); send it with \
                     `actor.send(h).{m}(...)`"
                ),
            );
            return Ok(Some((
                Rhs::Atom(Atom::Lit(Literal::Unit, at)),
                Ty::Err,
                None,
            )));
        }
        let reentrant = self.g.reentrant.contains(m);
        let mut fail = actor_fail(sig.fail.as_ref());
        let (mut h, _) = self.atom(b, None)?;
        // A call through `Remote` fails `RemoteError` too (R408, RFC 0063 D4) and runs on the
        // actor's home node (D1).
        if remote {
            if !self
                .sig
                .caps
                .iter()
                .chain(&self.handlers)
                .any(|c| c == "cluster")
            {
                self.err(
                    "E301",
                    at,
                    format!("`{m}` is a call through `Remote`; declare `uses cluster` (R400)"),
                );
            }
            if let Ty::Union(ms) = &mut fail {
                for n in ["Unreachable", "Rejected", "SchemaMismatch"] {
                    crate::union_add(ms, Ty::Data(n.into()));
                }
            }
            let actor = Ty::Coll("Actor", vec![state.clone(), policy.clone()]);
            let c = call(
                "std.cluster.reach",
                vec![state.targ(), policy.targ()],
                vec![let_arg(h)],
                at,
            );
            h = self.tmp(&actor, Rhs::Call(c), at);
        }
        let took = Ty::Result(Box::new(state.clone()), Box::new(fail.clone()));
        let take = call(
            "std.actor.take",
            vec![state.targ(), policy.targ(), fail.targ()],
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
                let (mut r, t, e) = l.call_sig(m, &sig, Some(&recv), args, at)?;
                if remote && let Rhs::Call(c) = &mut r {
                    let ps: Vec<Ty> = sig.params.iter().map(|p| p.ty.clone()).collect();
                    l.remote_args(c, &ps, at)?;
                }
                let rt = Ty::Result(Box::new(t.clone()), Box::new(fail.clone()));
                ret = rt.clone();
                let back = Atom::Name(s.clone(), at);
                let rest = |l: &mut Self, k: &str| -> R<()> {
                    let (e, r) = match r {
                        // It lends the state back at each suspension (9.6).
                        Rhs::Call(mut c) if reentrant => {
                            c.path = vec![super::reentrant::lent_name(m)];
                            c.targs.push(policy.targ());
                            c.args.push(let_arg(h.clone()));
                            (Some(fail.clone()), Rhs::Call(c))
                        }
                        r => (e, r),
                    };
                    let out = match (e, r) {
                        (Some(e), Rhs::Call(c)) => {
                            let tried = Ty::Result(Box::new(t.clone()), Box::new(e.clone()));
                            let mut tried_a = l.tmp(&tried, Rhs::Try(c), at);
                            if remote {
                                tried_a = l.remote_wire(&tried, tried_a, true, at)?;
                            }
                            let tried = tried_a;
                            let targs =
                                vec![state.targ(), policy.targ(), t.targ(), e.targ(), fail.targ()];
                            let args = vec![let_arg(h.clone()), sink_arg(back), sink_arg(tried)];
                            let c = call("std.actor.settle", targs, args, at);
                            let settled = l.tmp(&rt, Rhs::Call(c), at);
                            l.arrive(&rt, settled, remote, at)
                        }
                        (Some(_), _) => {
                            return unsupported("a fallible handler call that is not a plain call");
                        }
                        (None, r) => {
                            let mut v = l.tmp(&t, r, at);
                            if remote {
                                v = l.remote_wire(&t, v, true, at)?;
                            }
                            let put = call(
                                "std.actor.put",
                                vec![state.targ(), policy.targ()],
                                vec![let_arg(h.clone()), sink_arg(back)],
                                at,
                            );
                            l.emit(StmtKind::Rhs(Rhs::Call(put)), at);
                            let ok = l.tmp(&rt, ctor("Ok", "value", v), at);
                            l.arrive(&rt, ok, remote, at)
                        }
                    };
                    l.jump(k, out, at);
                    Ok(())
                };
                // An argument that cannot be encoded fails the call before the handler runs (R408).
                if remote {
                    let none = Ty::Result(Box::new(Ty::Unit), Box::new(fail.clone()));
                    let adm = call("std.cluster.admit", vec![fail.targ()], Vec::new(), at);
                    let adm = l.tmp(&none, Rhs::Call(adm), at);
                    let (u, e2) = (l.fresh("u"), l.fresh("e"));
                    let (v2, _) = l.join(at, |l, k2| {
                        let ok2 = l.block_with(|l| {
                            l.declare(&u, Ty::Unit);
                            rest(l, k2)
                        })?;
                        let bad = l.block_with(|l| {
                            l.declare(&e2, fail.clone());
                            let put = call(
                                "std.actor.put",
                                vec![state.targ(), policy.targ()],
                                vec![let_arg(h.clone()), sink_arg(Atom::Name(s.clone(), at))],
                                at,
                            );
                            l.emit(StmtKind::Rhs(Rhs::Call(put)), at);
                            let out =
                                l.tmp(&rt, ctor("Err", "error", Atom::Name(e2.clone(), at)), at);
                            l.jump(k2, out, at);
                            Ok(())
                        })?;
                        let arm = |c: &str, f: &str, n: &str| y::Pat::Ctor {
                            path: vec![c.into()],
                            fields: Some((vec![(f.into(), y::Pat::Bind(n.into()))], false)),
                        };
                        let m = y::Stmt {
                            kind: StmtKind::Match {
                                scrutinee: adm,
                                arms: vec![
                                    (arm("Ok", "value", &u), ok2),
                                    (arm("Err", "error", &e2), bad),
                                ],
                            },
                            span: at,
                        };
                        Ok((m, rt.clone()))
                    })?;
                    l.jump(k, v2, at);
                } else {
                    rest(l, k)?;
                }
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
pub(super) fn actor_fail(own: Option<&Ty>) -> Ty {
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
    /// `actor.spawn(state, capacity=1024)` moves `state` into a new actor (R956) whose overflow
    /// policy is the one the expected type names (R974).
    pub(super) fn spawn_call(
        &mut self,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let (state, cap) = match args {
            [s] if s.name.is_none() => (arg_expr(s), None),
            [s, c] if s.name.is_none() && c.name.as_deref().is_none_or(|n| n == "capacity") => {
                (arg_expr(s), arg_expr(c))
            }
            _ => (None, None),
        };
        let Some(state) = state else {
            self.err(
                "E201",
                at,
                "`actor.spawn` takes a state and an optional `capacity`".into(),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let (state, st) = self.atom(state, None)?;
        let cap = self.capacity(cap, at)?;
        let (p, code) = self.policy(want, at);
        let c = call(
            "std.actor.spawn_as",
            vec![st.targ(), p.targ()],
            vec![sink_arg(state), let_arg(cap), let_arg(code)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Coll("Actor", vec![st, p]), None))
    }

    /// `actor.send_only(h)`: the same handle as a `SendOnly[S, P]`, which sends but cannot call
    /// (R975).
    pub(super) fn send_only_call(&mut self, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        let h = match args {
            [h] if h.name.is_none() => arg_expr(h),
            _ => None,
        };
        let typed = match h {
            Some(h) => {
                let (a, t) = self.atom(h, None)?;
                match t {
                    Ty::Coll("Actor", ts) => Some((a, ts)),
                    _ => None,
                }
            }
            None => None,
        };
        let Some((h, ts)) = typed else {
            self.err(
                "E201",
                at,
                "`actor.send_only` takes one `Actor` handle".into(),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let c = call(
            "std.actor.send_only",
            ts.iter().map(Ty::targ).collect(),
            vec![let_arg(h)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Coll("SendOnly", ts), None))
    }

    fn capacity(&mut self, cap: Option<&j::Expr>, at: Span) -> R<Atom> {
        Ok(match cap {
            Some(e) => {
                let (c, ct) = self.atom(e, Some(&Ty::Int))?;
                self.expect(&ct, &Ty::Int, e.span);
                c
            }
            None => Atom::Lit(Literal::Int("1024".into()), at),
        })
    }

    /// The overflow policy `P` of the `Actor[S, P]` a spawn is expected to make, `Block` when
    /// none is expected, and its runtime code. Any but `Block` needs `uses nondet` (R974, E709).
    fn policy(&mut self, want: Option<&Ty>, at: Span) -> (Ty, Atom) {
        let p = match want {
            Some(Ty::Coll("Actor", ts)) => ts[1].clone(),
            _ => Ty::Data("Block".into()),
        };
        let code = match &p {
            Ty::Data(n) => POLICIES.iter().position(|q| q == n),
            Ty::Err => Some(0),
            _ => None,
        };
        let Some(code) = code else {
            let t = self.ty_text(&p);
            self.err(
                "E201",
                at,
                format!(
                    "`{t}` is not an overflow policy; `P` in `Actor[S, P]` is `Block`, \
                     `DropOldest`, `DropNew` or `Reject` (R974)"
                ),
            );
            return (Ty::Err, Atom::Lit(Literal::Int("0".into()), at));
        };
        if code != 0 && !self.sig.caps.iter().any(|c| c == "nondet") {
            self.err(
                "E709",
                at,
                format!(
                    "an actor with overflow policy `{}` drops or refuses messages by how fast it \
                     drains; declare `uses nondet`",
                    POLICIES[code]
                ),
            );
        }
        (p, Atom::Lit(Literal::Int(code.to_string()), at))
    }
}

/// The overflow policies, by their runtime code (R974).
const POLICIES: [&str; 4] = ["Block", "DropOldest", "DropNew", "Reject"];

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
        let Ty::Coll("Actor" | "SendOnly", ts) = &ht else {
            self.err(
                "E201",
                h.span,
                "`actor.send` takes an `Actor` or `SendOnly` handle".into(),
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
                    "`actor.send` of a handler with `uses`, before actors hold their \
                     supervisor's capabilities (R397)",
                );
            }
            Some(_) => {}
        }
        let name = |n: String, span: Span| j::Expr {
            kind: j::ExprKind::Name(n),
            span,
            paren: false,
        };
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        // The message borrows the handle, which does not own the actor (9.6), so the sender
        // keeps it.
        let hn = match ha {
            Atom::Name(n, _) => n,
            a => {
                let n = self.fresh("h");
                self.bind(&n, false, &ht, Rhs::Atom(a), at, at);
                n
            }
        };
        let hv = Atom::Name(hn.clone(), at);
        // The arguments move into a tuple that the message takes, or that a failed send returns
        // in `Undelivered.args` (R976).
        let mut moved = Vec::with_capacity(args.len());
        let mut keys = Vec::with_capacity(args.len());
        for a in args {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported("a reference argument to `actor.send`");
            };
            let (v, t) = self.atom(e, None)?;
            let n = self.fresh("m");
            self.bind(&n, false, &t, Rhs::Atom(v), e.span, e.span);
            moved.push(name(n, e.span));
            keys.push(a.name.clone());
        }
        let (tup, at_ty) = self.atom(&ex(j::ExprKind::Tuple(moved)), None)?;
        let tup = self.tmp(&at_ty, Rhs::Atom(tup), at);
        let pn = self.fresh("a");
        let unpacked = keys
            .into_iter()
            .enumerate()
            .map(|(i, key)| j::Arg {
                name: key,
                kind: j::ArgKind::Expr(ex(j::ExprKind::Index(
                    Box::new(name(pn.clone(), at)),
                    Box::new(ex(j::ExprKind::Lit(j::Lit::Int(i.to_string())))),
                ))),
                span: at,
            })
            .collect();
        let handler = ex(j::ExprKind::Attr(
            Box::new(name(hn.clone(), h.span)),
            m.into(),
            at,
        ));
        let msg = ex(j::ExprKind::Call(Box::new(handler), unpacked));
        // A sent handler has no failure of its own, and an admitted message runs on an idle
        // actor, so the call's `ActorError` cannot occur; a stopped actor drops it (R813).
        let none = j::Fallback::Bind(self.fresh("e"), ex(j::ExprKind::Lit(j::Lit::None)));
        let body = ex(j::ExprKind::Fallback(Box::new(msg), Box::new(none)));
        let want = Ty::Fn(false, vec![at_ty.clone()], Box::new(Ty::Unit));
        let was = self.sent_through.replace(hn);
        let clo = self.lambda(&[pn], &body, Some(&want));
        self.sent_through = was;
        let (clo, ct) = clo?;
        let clo = self.tmp(&ct, clo, at);
        let reason = send_reasons(&ts[1]);
        let fail = Ty::Coll("Undelivered", vec![at_ty.clone(), reason.clone()]);
        let c = call(
            "std.actor.send",
            vec![ts[0].targ(), ts[1].targ(), at_ty.targ(), fail.targ()],
            vec![let_arg(hv), sink_arg(tup), sink_arg(clo)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Unit, Some(fail)))
    }
}

/// Why a send under policy `p` can fail (R976).
fn send_reasons(p: &Ty) -> Ty {
    let data = |n: &str| Ty::Data(n.into());
    match p {
        Ty::Data(n) if n == "Reject" => Ty::Union(vec![data("Stopped"), data("Full")]),
        Ty::Data(n) if n == "DropOldest" || n == "DropNew" => data("Stopped"),
        _ => Ty::Union(vec![data("Stopped"), data("Timeout"), data("Cancelled")]),
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
    pub(super) fn start_call(
        &mut self,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
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
        let starts = Ty::Fn(false, Vec::new(), Box::new(state.clone()));
        let (clo, ct) = self.lambda(&[], &body, Some(&starts))?;
        let clo = self.tmp(&ct, clo, at);
        let cap = self.capacity(cap, at)?;
        let (p, code) = self.policy(want, at);
        let restart = Atom::Lit(Literal::Int(restart.to_string()), at);
        let c = call(
            "std.actor.start",
            vec![state.targ(), p.targ()],
            vec![sink_arg(clo), let_arg(restart), let_arg(cap), let_arg(code)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Coll("Actor", vec![state, p]), None))
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

pub(super) fn child_spec(args: &[j::Arg]) -> Option<((String, Span), &[j::Expr], u8)> {
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
