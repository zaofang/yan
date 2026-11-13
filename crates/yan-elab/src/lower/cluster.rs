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
        let rest = self.encode_start_args(rest)?;
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
    /// The arguments of a `ChildSpec` start round-trip through the codec before the start runs,
    /// as every value that crosses to a node does (R402); a value that cannot be encoded
    /// panics `wire`.
    fn encode_start_args(&mut self, mut rest: Vec<j::Arg>) -> R<Vec<j::Arg>> {
        let Some(j::ArgKind::Expr(spec)) = rest.first_mut().map(|a| &mut a.kind) else {
            return Ok(rest);
        };
        let j::ExprKind::Call(_, cargs) = &mut spec.kind else {
            return Ok(rest);
        };
        let Some(((f, _), _, _)) = super::actor::child_spec(cargs) else {
            return Ok(rest);
        };
        let Some(sig) = self.g.fns.get(f.as_str()).cloned() else {
            return Ok(rest);
        };
        let Some(j::ArgKind::Expr(tuple)) = cargs.get_mut(1).map(|a| &mut a.kind) else {
            return Ok(rest);
        };
        let j::ExprKind::Tuple(xs) = &mut tuple.kind else {
            return Ok(rest);
        };
        for (x, p) in xs.iter_mut().zip(&sig.params) {
            let sp = x.span;
            let (v, t) = self.atom(x, Some(&p.ty))?;
            self.expect(&t, &p.ty, sp);
            let sc = match crate::wire::schemas(self.g, &p.ty) {
                Ok(sc) => sc,
                Err(why) => {
                    self.err(
                        "E720",
                        sp,
                        format!("a start argument carries Wire values: {why}"),
                    );
                    continue;
                }
            };
            let text = serde_json::to_string(&sc).expect("schemas serialize");
            let c = call(
                "std.cluster.wire_start",
                vec![p.ty.targ()],
                vec![let_arg(v), let_arg(Atom::Lit(Literal::Str(text), sp))],
                sp,
            );
            let n = self.fresh("s");
            self.bind(&n, false, &p.ty, Rhs::Call(c), sp, sp);
            x.kind = j::ExprKind::Name(n);
        }
        Ok(rest)
    }

    /// `cluster.group(name)` is the named group of `A` handles (R414); `A` comes from the
    /// annotation of the binding.
    pub(super) fn cluster_group(
        &mut self,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let [
            j::Arg {
                name: None,
                kind: j::ArgKind::Expr(e),
                ..
            },
        ] = args
        else {
            return unsupported("`group` takes the one name of the group");
        };
        let Some(t @ Ty::Coll("Group", a)) = want else {
            return unsupported(
                "`cluster.group` needs its type written, as in `g: Group[A] = ...`",
            );
        };
        let (x, st) = self.atom(e, Some(&Ty::Str))?;
        self.expect(&st, &Ty::Str, e.span);
        let c = call(
            "std.cluster.group",
            vec![a[0].targ()],
            vec![Arg::Cap("cluster".into(), at), let_arg(x)],
            at,
        );
        Ok((Rhs::Call(c), t.clone(), None))
    }

    /// `cluster.monitor(remote)` is a receiver that gets one `Stopped` when the actor stops (R411).
    pub(super) fn cluster_monitor(
        &mut self,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let [
            j::Arg {
                name: None,
                kind: j::ArgKind::Expr(e),
                ..
            },
        ] = args
        else {
            return unsupported("`monitor` takes the one `Remote` handle to watch");
        };
        let (x, t) = self.atom(e, None)?;
        let Ty::Coll("Remote", r) = &t else {
            if t != Ty::Err {
                self.err(
                    "E201",
                    e.span,
                    "`monitor` watches a `Remote` actor handle".to_string(),
                );
            }
            return Ok((Rhs::Atom(x), Ty::Err, None));
        };
        let Some(Ty::Coll("Actor", ts)) = r.first() else {
            return unsupported("`monitor` of a `Remote` that is not an actor handle");
        };
        let c = call(
            "std.cluster.monitor",
            ts.iter()
                .map(|t| t.targ())
                .chain([Ty::Data("Stopped".into()).targ()])
                .collect(),
            vec![Arg::Cap("cluster".into(), at), let_arg(x)],
            at,
        );
        let down = Ty::Coll("Receiver", vec![Ty::Data("Stopped".into())]);
        Ok((Rhs::Call(c), down, None))
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
        let g = self.g;
        let ok = |t: &Ty| wire(t) && crate::wire::schemas(g, t).is_ok();
        let mut bad: Vec<&String> = g
            .fns
            .iter()
            .filter(|(_, s)| {
                s.params.first().is_some_and(|p| p.ty == *state)
                    && (s.params[1..].iter().any(|p| !ok(&p.ty))
                        || !ok(&s.ret)
                        || s.fail.as_ref().is_some_and(|f| !ok(f)))
            })
            .map(|(n, _)| n)
            .collect();
        bad.sort();
        if let Some(h) = bad.first() {
            let msg = format!(
                "a remote actor's handlers take and return Wire values, but `{h}` does not"
            );
            self.err("E720", at, msg);
        }
    }

    /// `x` of type `t` after the encode and decode every call through `Remote` makes (R402).
    pub(super) fn remote_wire(&mut self, t: &Ty, x: Atom, reply: bool, at: Span) -> R<Atom> {
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
            if reply {
                "std.cluster.wire_reply"
            } else {
                "std.cluster.wire"
            },
            vec![t.targ()],
            vec![let_arg(x), let_arg(Atom::Lit(Literal::Str(text), at))],
            at,
        );
        Ok(self.tmp(t, Rhs::Call(c), at))
    }

    /// The argument `x` of a send through `Remote` after the encode and decode; one the codec
    /// rejects fails the send `SchemaMismatch` (R408).
    pub(super) fn send_wire(&mut self, t: &Ty, x: j::Expr, at: Span) -> R<Atom> {
        let (x, _) = self.atom(&x, Some(t))?;
        let sc = match crate::wire::schemas(self.g, t) {
            Ok(sc) => sc,
            Err(why) => {
                self.err(
                    "E720",
                    at,
                    format!("a send through `Remote` carries Wire values: {why}"),
                );
                return Ok(x);
            }
        };
        let text = serde_json::to_string(&sc).expect("schemas serialize");
        let c = call(
            "std.cluster.wire_send",
            vec![t.targ()],
            vec![let_arg(x), let_arg(Atom::Lit(Literal::Str(text), at))],
            at,
        );
        Ok(self.tmp(t, Rhs::Call(c), at))
    }

    /// The reply of a call through `Remote` arrives, or is lost on the way back (RFC 0063 D5).
    pub(super) fn arrive(&mut self, rt: &Ty, outcome: Atom, remote: bool, at: Span) -> Atom {
        if !remote {
            return outcome;
        }
        let Ty::Result(t, e) = rt else { return outcome };
        let c = call(
            "std.cluster.arrive",
            vec![t.targ(), e.targ()],
            vec![let_arg(outcome)],
            at,
        );
        self.tmp(rt, Rhs::Call(c), at)
    }

    /// The arguments of a call through `Remote` after the round trip; the receiver is first. An
    /// `inout` argument is passed as an encoded copy, returned as `(place, copy, type, span)` so
    /// the caller commits the copy back once the reply arrives (R408).
    pub(super) fn remote_args(
        &mut self,
        c: &mut y::Call,
        params: &[Ty],
    ) -> R<Vec<(String, String, Ty, Span)>> {
        let mut k = 0;
        let mut copies = Vec::new();
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
                let Atom::Name(n, _) = x.clone() else {
                    return unsupported("an `inout` argument in a call through `Remote`");
                };
                let v = self.remote_wire(t, x.clone(), false, *sp)?;
                let copy = self.fresh("i");
                self.bind(&copy, true, t, Rhs::Atom(v), *sp, *sp);
                *x = Atom::Name(copy.clone(), *sp);
                copies.push((n, copy, t.clone(), *sp));
                continue;
            }
            *x = self.remote_wire(t, x.clone(), false, *sp)?;
        }
        Ok(copies)
    }

    /// The reply carries each `inout` copy back: it is encoded with the result, so a copy that
    /// cannot be encoded fails the call `SchemaMismatch`.
    pub(super) fn inout_replies(
        &mut self,
        copies: &[(String, String, Ty, Span)],
    ) -> R<Vec<(String, Atom, Ty, Span)>> {
        let mut out = Vec::new();
        for (n, copy, t, sp) in copies {
            let v = self.remote_wire(t, Atom::Name(copy.clone(), *sp), true, *sp)?;
            out.push((n.clone(), v, t.clone(), *sp));
        }
        Ok(out)
    }

    /// Once the reply has arrived, each `inout` place takes its copy unless the reply was lost.
    pub(super) fn commit_inouts(&mut self, back: Vec<(String, Atom, Ty, Span)>) {
        for (n, v, t, sp) in back {
            let c = call(
                "std.cluster.commit",
                vec![t.targ()],
                vec![
                    Arg::Value(y::Conv::Inout, Atom::Name(n, sp), sp),
                    sink_arg(v),
                ],
                sp,
            );
            self.emit(StmtKind::Rhs(Rhs::Call(c)), sp);
        }
    }
}
