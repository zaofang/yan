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
                    return unsupported(&format!(
                        "placement `{n}` in this release; use `Local()` or `On(node)` (#929)"
                    ));
                }
                _ => return unsupported("a computed placement"),
            },
            Some(_) => return unsupported("a computed placement"),
        };
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
        let share = call(
            "std.actor.share",
            ts.iter().map(|t| t.targ()).collect(),
            vec![let_arg(h)],
            at,
        );
        Ok((Rhs::Call(share), Ty::Coll("Remote", vec![t.clone()]), None))
    }
}
