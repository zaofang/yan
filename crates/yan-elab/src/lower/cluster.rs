use super::*;

impl Lower<'_, '_, '_> {
    /// `cluster.spawn(ChildSpec(...), placement=Local(), capacity=1024)` starts the child on the
    /// node `placement` names and returns it as `Remote` (RFC 0063 D2): `place`, the
    /// `actor.start` lowering (R958), `leave`, then `share`.
    pub(super) fn cluster_spawn(
        &mut self,
        c: &j::Expr,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let placement = args
            .iter()
            .find(|a| a.name.as_deref() == Some("placement"))
            .and_then(|a| match &a.kind {
                j::ArgKind::Expr(e) => Some(e),
                _ => None,
            });
        let rest: Vec<j::Arg> = args
            .iter()
            .filter(|a| a.name.as_deref() != Some("placement"))
            .cloned()
            .collect();
        let node = match placement.map(|p| &p.kind) {
            None => None,
            Some(j::ExprKind::Call(f, xs)) => match (&f.kind, &xs[..]) {
                (j::ExprKind::Name(n), []) if n == "Local" => None,
                (j::ExprKind::Name(n), [x]) if n == "On" => match &x.kind {
                    j::ArgKind::Expr(e) => Some(e),
                    _ => return unsupported("this `On` placement"),
                },
                (j::ExprKind::Name(n), _) => {
                    return unsupported(format!(
                        "placement `{n}` in this release; use `Local()` or `On(node)` (#929)"
                    ));
                }
                _ => return unsupported("a computed placement"),
            },
            Some(_) => return unsupported("a computed placement"),
        };
        if node.is_some() {
            self.off_node_start(&rest);
        }
        if let Some(n) = node {
            let (cl, _) = self.atom(c, None)?;
            let want = Ty::Data("Node".into());
            let (n, t) = self.atom(n, Some(&want))?;
            self.expect(&t, &want, at);
            let place = call(
                "std.cluster.place",
                Vec::new(),
                vec![let_arg(cl), let_arg(n)],
                at,
            );
            self.emit(StmtKind::Rhs(Rhs::Call(place)), at);
        }
        let (r, t, _) = self.start_call(&rest, at, None)?;
        let h = self.tmp(&t, r, at);
        if node.is_some() {
            let leave = call("std.cluster.leave", Vec::new(), Vec::new(), at);
            self.emit(StmtKind::Rhs(Rhs::Call(leave)), at);
        }
        let Ty::Coll("Actor", ts) = &t else {
            return Ok((Rhs::Atom(h), Ty::Err, None));
        };
        self.wire_handlers(&ts[0], at);
        let share = call(
            "std.actor.share",
            ts.iter().map(|t| t.targ()).collect(),
            vec![let_arg(h)],
            at,
        );
        Ok((Rhs::Call(share), Ty::Coll("Remote", vec![t.clone()]), None))
    }
}

impl Lower<'_, '_, '_> {
    /// An off-node start names a `@remote` function (E721) taking Wire arguments (E720), since
    /// it runs by signature hash on the chosen node (R407, R958).
    fn off_node_start(&mut self, args: &[j::Arg]) {
        let Some(j::ArgKind::Expr(j::Expr {
            kind: j::ExprKind::Call(_, cargs),
            ..
        })) = args.first().map(|a| &a.kind)
        else {
            return;
        };
        let Some(((f, span), _, _)) = super::actor::child_spec(cargs) else {
            return;
        };
        let Some(sig) = self.g.fns.get(f.as_str()) else {
            return;
        };
        if !self.g.remote.contains(&f) {
            self.err(
                "E721",
                span,
                format!("an off-node start names a `@remote` function; mark `{f}` `@remote`"),
            );
        } else if let Some(p) = sig.params.iter().find(|p| !wire(&p.ty)) {
            let msg = format!(
                "an off-node start takes Wire arguments, but `{f}` takes `{}` that is not",
                p.name
            );
            self.err("E720", span, msg);
        }
    }

    /// E720 unless every handler of an actor with state `state` takes Wire values, since a
    /// `Remote` handle may call it from another node (R401).
    pub(super) fn wire_handlers(&mut self, state: &Ty, at: Span) {
        let mut bad: Vec<&String> = self
            .g
            .fns
            .iter()
            .filter(|(_, s)| {
                s.params.first().is_some_and(|p| p.ty == *state)
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

    /// `x` of type `t` after the encode and decode every call through `Remote` makes (R402).
    pub(super) fn remote_wire(&mut self, t: &Ty, x: Atom, at: Span) -> R<Atom> {
        let sc = match crate::wire::schemas(self.g, t) {
            Ok(sc) => sc,
            Err(why) => {
                self.err(
                    "E720",
                    at,
                    format!("a call through `Remote` carries Wire values: {why}"),
                );
                return Ok(x);
            }
        };
        let text = serde_json::to_string(&sc).expect("schemas serialize");
        let c = call(
            "std.cluster.wire",
            vec![t.targ()],
            vec![let_arg(x), let_arg(Atom::Lit(Literal::Str(text), at))],
            at,
        );
        Ok(self.tmp(t, Rhs::Call(c), at))
    }

    /// The arguments of a call through `Remote` after the round trip; the receiver is first.
    pub(super) fn remote_args(&mut self, c: &mut y::Call, params: &[Ty], at: Span) -> R<()> {
        let mut k = 0;
        for a in &mut c.args {
            let y::Arg::Value(conv, x, sp) = a else {
                continue;
            };
            k += 1;
            if k == 1 {
                continue;
            }
            let Some(t) = params.get(k - 1) else { continue };
            if *conv == y::Conv::Inout {
                return unsupported("an `inout` argument in a call through `Remote`");
            }
            *x = self.remote_wire(t, x.clone(), *sp)?;
            let _ = at;
        }
        Ok(())
    }
}
