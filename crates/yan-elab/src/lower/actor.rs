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
