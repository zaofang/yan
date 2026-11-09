use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn stmt_inner(&mut self, s: &j::Stmt) -> R<()> {
        use j::StmtKind as K;
        let at = s.span;
        if !self.open.is_empty() {
            self.learn(s);
        }
        match &s.kind {
            K::Pass | K::Reveal(_) => {}
            K::Expr(e) | K::Return(Some(e)) if matches!(e.kind, j::ExprKind::Hole) => {
                self.hole(e.span);
            }
            K::Ghost { name, .. } => {
                self.ghosts.insert(name.clone());
            }
            K::Expr(j::Expr {
                kind: j::ExprKind::Stage(x),
                ..
            }) if matches!(&x.kind, j::ExprKind::Call(f, _) if matches!(&f.kind, j::ExprKind::Name(n) if n == "compile_error" || n == "compile_warn")) =>
            {
                let j::ExprKind::Call(f, args) = &x.kind else {
                    unreachable!("matched")
                };
                let msg = match args.as_slice() {
                    [
                        j::Arg {
                            kind:
                                j::ArgKind::Expr(j::Expr {
                                    kind: j::ExprKind::Lit(j::Lit::Str { value, .. }),
                                    ..
                                }),
                            ..
                        },
                    ] => value.clone(),
                    _ => return unsupported("`compile_error` with other than one string literal"),
                };
                let item = self.item.clone();
                if matches!(&f.kind, j::ExprKind::Name(n) if n == "compile_error") {
                    // In a type function it is raised when the application reaches it (10.4).
                    if self.g.fns.get(&item).is_some_and(|s| s.ret == Ty::Type) {
                        let arg = Arg::Value(y::Conv::Let, Atom::Lit(Literal::Str(msg), at), at);
                        let c = call("std.meta.compile_error", vec![], vec![arg], at);
                        self.emit(StmtKind::Rhs(Rhs::Call(c)), at);
                    } else {
                        self.cx.err("E270", at, &item, msg);
                    }
                } else {
                    self.cx.warn("W816", at, &item, msg);
                }
            }
            K::Expr(e) => {
                let (rhs, t) = self.rhs(e, None)?;
                if !matches!(t, Ty::Unit | Ty::Never | Ty::Err) && matches!(rhs, Rhs::Call(_)) {
                    self.err(
                        "E312",
                        at,
                        format!("a result of type `{t}` is discarded", t = self.ty_text(&t)),
                    );
                }
                if !matches!(rhs, Rhs::Atom(_)) {
                    self.emit(StmtKind::Rhs(rhs), at);
                }
            }
            K::Assign { target, ty, value } => self.assign(target, ty.as_ref(), value, at)?,
            K::Bind {
                target,
                target_span,
                ty,
                value,
                ..
            } => {
                if self.locals.contains_key(target) {
                    self.err(
                        "E230",
                        *target_span,
                        format!("`{target}` is already bound; pick another name"),
                    );
                    return Ok(());
                }
                let t = j::Expr {
                    kind: j::ExprKind::Name(target.clone()),
                    span: *target_span,
                    paren: false,
                };
                self.assign(&t, ty.as_ref(), value, at)?;
            }
            K::AugAssign { target, op, value } => {
                let op: &'static str = op.trim_end_matches('=');
                if !["+", "-", "*", "/", "//", "%"].contains(&op) {
                    return unsupported(format!("`{op}=`"));
                }
                let bin = j::Expr {
                    kind: j::ExprKind::Binary(
                        op,
                        Box::new(target.clone()),
                        Box::new(value.clone()),
                    ),
                    span: value.span,
                    paren: false,
                };
                if let j::ExprKind::Index(d, k) = &target.kind
                    && let Some(Ty::Dict(kt, vt)) = self.ty_of_local(d)
                {
                    self.insert_default(d, k, &kt, &vt, at)?;
                }
                self.assign(target, None, &bin, at)?;
            }
            K::Return(e) => {
                let want = self.sig.ret.clone();
                let a = match e {
                    None => Atom::Lit(Literal::Unit, at),
                    Some(e) => {
                        self.view_escape(e);
                        let e = &self.become_in_ctor(e, true);
                        let (a, t) = self.atom(e, Some(&want))?;
                        self.expect(&t, &want, e.span);
                        a
                    }
                };
                self.ret(a, at)?;
            }
            K::Fail(e) => {
                let Some(want) = self.sig.fail.clone() else {
                    return unsupported("`fail` in a function without a failure type");
                };
                let (a, t) = self.atom(e, Some(&want))?;
                self.expect(&t, &want, e.span);
                self.emit(StmtKind::Fail(a), at);
            }
            K::Become(e) => {
                self.in_become = true;
                let r = self.rhs(e, None);
                self.in_become = false;
                let (rhs, _) = r?;
                let Rhs::Call(c) = rhs else {
                    return unsupported("`become` of a constructor");
                };
                self.emit(StmtKind::Become(c), at);
            }
            K::Break | K::Continue | K::BreakTo(_) | K::ContinueTo(_) => {
                let label = match &s.kind {
                    K::BreakTo(n) | K::ContinueTo(n) => self
                        .loops
                        .iter()
                        .rev()
                        .find(|(_, name)| name.as_ref() == Some(n))
                        .map(|(l, _)| l.clone()),
                    _ => self.loops.last().map(|(l, _)| l.clone()),
                };
                let Some(label) = label else {
                    return unsupported("`break` outside its loop");
                };
                let kind = if matches!(s.kind, K::Break | K::BreakTo(_)) {
                    StmtKind::Break { label, value: None }
                } else {
                    StmtKind::Continue { label }
                };
                self.emit(kind, at);
            }
            K::Assert(e) => {
                let a = self.cond(e)?;
                self.emit(StmtKind::Assert(y::Expr::Atom(a)), at);
            }
            K::Unreachable => self.emit(StmtKind::Unreachable, at),
            K::If { arms, els } => self.if_(arms, els.as_ref(), at)?,
            K::While {
                label: name,
                cond,
                body,
                ..
            } => {
                let label = self.label();
                self.loops.push((label.clone(), name.clone()));
                let b = self.block_with(|l| {
                    let c = l.cond(cond)?;
                    let stop = y::Block {
                        stmts: vec![y::Stmt {
                            kind: StmtKind::Break {
                                label: label.clone(),
                                value: None,
                            },
                            span: at,
                        }],
                    };
                    l.emit(
                        StmtKind::If {
                            cond: c,
                            then: y::Block { stmts: Vec::new() },
                            els: Some(stop),
                        },
                        at,
                    );
                    for (x, inner) in l.narrowings(cond, true) {
                        l.narrow_to(&x, inner, cond.span);
                    }
                    l.stmts(body)
                });
                self.loops.pop();
                let body = b?;
                self.emit(
                    StmtKind::Loop {
                        label,
                        invariants: Vec::new(),
                        decreases: Vec::new(),
                        body,
                    },
                    at,
                );
            }
            K::For {
                label,
                target,
                iter,
                body,
                ..
            } => self.for_(label.as_deref(), target, iter, at, |l| l.stmts(body))?,
            K::Match { scrutinee, cases } => self.match_(scrutinee, cases, at)?,
            K::Par {
                concurrent,
                limit: None,
                body: j::ParBody::Block(b),
            } => {
                self.par_waits(b, *concurrent);
                let mut blocks = Vec::new();
                let mut called: Vec<String> = Vec::new();
                for s in b {
                    let start = self.fn_calls.len();
                    blocks.push(self.block_scoped(|l| l.stmts(std::slice::from_ref(s)), true)?);
                    let mine: Vec<(String, Span)> = self.fn_calls[start..].to_vec();
                    for (n, sp) in &mine {
                        if called.contains(n) {
                            self.err(
                                "E716",
                                *sp,
                                format!("`{n}` is not `par`; it is called from two branches"),
                            );
                        }
                    }
                    called.extend(mine.into_iter().map(|(n, _)| n));
                }
                self.emit(
                    StmtKind::Par {
                        concurrent: *concurrent,
                        blocks,
                    },
                    at,
                );
            }
            // A streaming `par for` (unit body) runs its items in order: the body may not fail
            // (E712) nor take `inout` access outside its iteration, so no result depends on order.
            K::Par {
                limit,
                body:
                    j::ParBody::For {
                        target,
                        iter,
                        contracts,
                        body,
                    },
                ..
            } => {
                // The limit only bounds concurrency; the in-order run already respects it.
                if let Some(l) = limit {
                    let (_, lt) = self.atom(l, Some(&Ty::Int))?;
                    self.expect(&lt, &Ty::Int, l.span);
                }
                if let Some(sp) = first_failure(body) {
                    self.err(
                        "E712",
                        sp,
                        "the body of a streaming `par for` cannot fail; handle the failure in the item".into(),
                    );
                }
                let s = j::Stmt {
                    kind: K::For {
                        label: None,
                        target: target.clone(),
                        iter: iter.clone(),
                        contracts: contracts.clone(),
                        body: body.clone(),
                    },
                    span: at,
                };
                self.stmt(&s)?;
            }
            K::Par { .. } => return unsupported("`par for` with a limit and `par(limit=...)`"),
            K::With {
                kind: j::WithKind::Bind(name, h),
                body,
            } => {
                // A host handler is the platform's, or an attenuation of the held handle of
                // the same capability (R672).
                if let j::ExprKind::Call(f, _) = &h.kind
                    && let j::ExprKind::Attr(b, op, _) = &f.kind
                    && let j::ExprKind::Name(held) = &b.kind
                    && matches!(op.as_str(), "only" | "sub")
                    && held != name
                    && crate::stdlib::decl(&format!("std.{held}.{op}")).is_some_and(|d| d.cap)
                {
                    self.err(
                        "E326",
                        h.span,
                        format!(
                            "`{held}.{op}` attenuates `{held}`; `with {name} = ...` takes an \
                             attenuation of `{name}` or a handler written in Yan"
                        ),
                    );
                }
                let never = matches!(&h.kind, j::ExprKind::Call(f, args)
                    if args.is_empty() && matches!(&f.kind, j::ExprKind::Name(n) if n == "never"));
                if name == "fault" && self.cx.release && !never {
                    self.err(
                        "E317",
                        h.span,
                        "a release build binds `fault` only to `never`".into(),
                    );
                }
                let (a, ht) = self.atom(h, None)?;
                let want = Ty::Data(self.sig.cap_trait(name));
                let wrong = match &ht {
                    Ty::Data(g) => {
                        *g != self.sig.cap_trait(name) && crate::stdlib::STD.caps.contains(g)
                    }
                    Ty::Err | Ty::Never | Ty::Param(_) => false,
                    _ => true,
                };
                if wrong {
                    self.expect(&ht, &want, h.span);
                }
                self.handlers.push(name.clone());
                let b = self.block(&body.stmts);
                self.handlers.pop();
                self.emit(
                    StmtKind::With {
                        name: name.clone(),
                        dyn_: false,
                        handler: a,
                        body: b?,
                    },
                    at,
                );
            }
            // `timeout(d)` is a deadline `d` from now (9.4).
            K::With {
                kind: kind @ (j::WithKind::Deadline(e) | j::WithKind::Timeout(e)),
                body,
            } => {
                let at_expr = if matches!(kind, j::WithKind::Timeout(_)) {
                    let ex = |kind| j::Expr {
                        kind,
                        span: e.span,
                        paren: false,
                    };
                    let now = ex(j::ExprKind::Call(
                        Box::new(ex(j::ExprKind::Attr(
                            Box::new(ex(j::ExprKind::Name("time".into()))),
                            "monotonic".into(),
                            e.span,
                        ))),
                        Vec::new(),
                    ));
                    ex(j::ExprKind::Binary("+", Box::new(now), Box::new(e.clone())))
                } else {
                    e.clone()
                };
                let timeout = Ty::Data("Timeout".into());
                let lists = match &self.sig.fail {
                    Some(Ty::Union(ms)) => ms.contains(&timeout),
                    f => f.as_ref() == Some(&timeout),
                };
                if !lists {
                    self.err(
                        "E310",
                        at,
                        format!(
                            "`{}` opens a cancel scope, so it can fail with `Timeout`; \
                             add `Timeout` to its `fail` type (R472)",
                            self.item
                        ),
                    );
                }
                let (a, _) = self.atom(&at_expr, None)?;
                let b = self.block(&body.stmts)?;
                self.emit(StmtKind::Deadline { at: a, body: b }, at);
            }
            K::Unsafe(body) => {
                if !self.cx.ffi {
                    self.err(
                        "E901",
                        at,
                        "`unsafe` belongs in an ffi module, under the package's `ffi/` directory"
                            .into(),
                    );
                }
                let outer = mem::replace(&mut self.in_unsafe, true);
                let b = self.block(body);
                self.in_unsafe = outer;
                self.emit(StmtKind::Unsafe(b?), at);
            }
            K::Asm { template, args } => self.asm(template, args, at)?,
            K::StageIf { cond, then, els } => {
                let Some(Sv::Bool(c)) = fold(cond, &self.stage) else {
                    return unsupported("a `#if` condition that is not a stage-0 constant");
                };
                match (c, els) {
                    (true, _) => self.stmts(then)?,
                    (false, Some(b)) => self.stmts(b)?,
                    (false, None) => {}
                }
            }
            K::StageFor { target, iter, body } => {
                let j::ExprKind::List(xs) = &iter.kind else {
                    return unsupported("`#for` over a sequence other than a list literal");
                };
                for x in xs {
                    let Some(v) = fold(x, &self.stage) else {
                        return unsupported("a `#for` element that is not a stage-0 constant");
                    };
                    self.stage.push((target.clone(), v));
                    let r = self.stmts(body);
                    self.stage.pop();
                    r?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn if_(
        &mut self,
        arms: &[(j::Expr, j::Block)],
        els: Option<&j::Block>,
        at: Span,
    ) -> R<()> {
        let (cond, body) = &arms[0];
        let c = self.cond(cond)?;
        let yes = self.narrowings(cond, true);
        let no = self.narrowings(cond, false);
        let then = self.block_with(|l| {
            for (x, inner) in yes.clone() {
                l.narrow_to(&x, inner, cond.span);
            }
            l.stmts(&body.stmts)
        })?;
        let narrow_else = |l: &mut Self| {
            for (x, inner) in no.clone() {
                l.narrow_to(&x, inner, cond.span);
            }
        };
        let (els, els_exits) = if arms.len() > 1 {
            let b = self.block_with(|l| {
                narrow_else(l);
                l.if_(&arms[1..], els, at)
            })?;
            (Some(b), false)
        } else {
            match els {
                Some(e) => {
                    let b = self.block_with(|l| {
                        narrow_else(l);
                        l.stmts(&e.stmts)
                    })?;
                    (Some(b), ends(&e.stmts))
                }
                None => (None, false),
            }
        };
        self.emit(StmtKind::If { cond: c, then, els }, at);
        // A branch that exits narrows the rest of the block by the other branch's test.
        if ends(&body.stmts) && arms.len() == 1 {
            narrow_else(self);
        }
        if els_exits {
            for (x, inner) in yes {
                self.narrow_to(&x, inner, cond.span);
            }
        }
        Ok(())
    }

    /// A hole body never returns, so its `post` lines would go unchecked until the body is
    /// written, after the spec is frozen. Check them against `out` in a block that is dropped.
    pub(super) fn check_posts_of_hole(&mut self, at: Span) -> R<()> {
        if self.posts.is_empty() {
            return Ok(());
        }
        let ty = self.sig.ret.clone();
        let posts = self.posts.clone();
        self.block_with(|l| {
            let out = l.fresh("out");
            l.bind(
                &out,
                false,
                &ty,
                Rhs::Atom(Atom::Lit(Literal::Unit, at)),
                at,
                at,
            );
            let depth = l.scopes.len();
            l.narrow.push(("out".into(), out.clone(), depth));
            for p in &posts {
                l.cond(p)?;
            }
            l.narrow.retain(|m| m.1 != out || m.0 != "out");
            Ok(())
        })?;
        Ok(())
    }

    /// Return `a`, after checking each `post` with `out` naming it.
    pub(super) fn ret(&mut self, a: Atom, at: Span) -> R<()> {
        if self.posts.is_empty() {
            self.emit(StmtKind::Ret(a), at);
            return Ok(());
        }
        let ty = self.sig.ret.clone();
        let out = match &a {
            Atom::Name(n, _) if self.locals.contains_key(n) => n.clone(),
            _ => {
                let n = self.fresh("out");
                self.bind(&n, false, &ty, Rhs::Atom(a), at, at);
                n
            }
        };
        let depth = self.scopes.len();
        self.narrow.push(("out".into(), out.clone(), depth));
        for p in self.posts.clone() {
            let c = self.cond(&p)?;
            self.cx.marks.push(crate::marks::Pending {
                span: p.span,
                kind: crate::marks::Kind::Assert,
                call: None,
                mark: yan_surface::Mark::FromPost,
            });
            self.emit(StmtKind::Assert(y::Expr::Atom(c)), p.span);
        }
        self.narrow.retain(|m| m.1 != out || m.0 != "out");
        self.emit(StmtKind::Ret(Atom::Name(out, at)), at);
        Ok(())
    }
}
