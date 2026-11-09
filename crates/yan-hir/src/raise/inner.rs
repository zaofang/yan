use super::*;

impl<'a> Fold<'a> {
    pub(super) fn stmt_inner(&mut self, ss: &[y::Stmt], out: &mut Vec<Stmt>) -> R<usize> {
        use y::StmtKind as K;
        let s = &ss[0];
        if let K::Let { rhs, .. } | K::Assign { rhs, .. } | K::Rhs(rhs) = &s.kind
            && let y::Rhs::Call(c) | y::Rhs::Try(c) | y::Rhs::Propagate { call: c, .. } = rhs
        {
            self.marks.consume(&self.marks.args(c));
        }
        match &s.kind {
            K::Let {
                pat: y::Pat::Bind(n),
                ty: t,
                rhs,
                mutable,
                ..
            } => {
                self.types.insert(n.clone(), t.clone());
                if *mutable
                    && generated(n)
                    && let Some(k) = self.for_loop(s, n, rhs, &ss[1..], out)?
                {
                    return Ok(1 + k);
                }
                if *mutable
                    && generated(n)
                    && let y::Rhs::Copy(from) = rhs
                    && let Some(k) = self.evolve(n, from, &ss[1..], out)?
                {
                    return Ok(1 + k);
                }
                if generated(n)
                    && let y::Rhs::Ctor(_) = rhs
                    && let Some((list, k)) = tuple_iterated(n, &ss[1..])
                {
                    let value = self.rhs(rhs)?;
                    self.bind_pending(&list, Pending::Expr(value));
                    return Ok(1 + k);
                }
                if generated(n) {
                    let (names, k) = destructured(n, &ss[1..]);
                    if !names.is_empty() && self.uses(n) == names.len() {
                        let mut value = self.rhs(rhs)?;
                        // `a, b = x, y` elaborates as a parallel assignment, not through a tuple.
                        if matches!(rhs, y::Rhs::Ctor(y::Ctor::Tuple(_))) {
                            out.push(stmt(StmtKind::Assign {
                                target: name(n),
                                ty: None,
                                value,
                            }));
                            value = name(n);
                        }
                        for (i, b) in names.iter().enumerate() {
                            if let K::Let { ty, .. } = &ss[1 + names.len() + i].kind {
                                self.types.insert(b.clone(), ty.clone());
                            }
                        }
                        out.push(stmt(StmtKind::Assign {
                            target: e(ExprKind::Tuple(names.iter().map(|b| name(b)).collect())),
                            ty: None,
                            value,
                        }));
                        return Ok(1 + k);
                    }
                    let builder = match rhs {
                        y::Rhs::Call(c) if c.args.is_empty() => match c.path.join(".").as_str() {
                            "std.list.new" => Some(Pending::List(Vec::new())),
                            "std.set.new" => Some(Pending::Set(Vec::new())),
                            "std.dict.new" => Some(Pending::Dict(Vec::new())),
                            _ => None,
                        },
                        _ => None,
                    };
                    let builds = count_builds(n, &ss[1..]);
                    // A `for` over the list also takes its length and indexes it.
                    let loops = usize::from(for_over(n, &ss[1..]));
                    if let Some(b) = builder
                        && self.uses(n) == builds + 1 + loops
                    {
                        self.bind_pending(n, b);
                        return Ok(1);
                    }
                    // yan-elab leaves unused temporaries where it lowers an expression twice;
                    // a std call without capabilities or `inout` arguments has no effect.
                    let pure = matches!(rhs, y::Rhs::Closure(_) | y::Rhs::Atom(y::Atom::Lit(..)))
                        || matches!(rhs, y::Rhs::Call(c)
                            if c.path.first().is_some_and(|p| p == "std")
                                && c.args.iter().all(|a| matches!(a, y::Arg::Value(y::Conv::Let, ..))));
                    if self.uses(n) == 0 && pure {
                        return Ok(1);
                    }
                    if pure {
                        self.pure.insert(n.clone());
                    }
                    if self.uses(n) == 1 {
                        let x = self.rhs(rhs)?;
                        self.bind_pending(n, Pending::Expr(x));
                        return Ok(1);
                    }
                    if self.uses(n) == 0 {
                        let x = self.rhs(rhs)?;
                        out.push(stmt(StmtKind::Expr(x)));
                        return Ok(1);
                    }
                }
                let value = self.rhs(rhs)?;
                let ty = match self.marks.annotated(s) {
                    Some(true) => match ty(t) {
                        Ok(t) => Some(t),
                        Err(_) => {
                            self.marks.reject_stmt(s);
                            annotation(&value, t)?
                        }
                    },
                    Some(false) => None,
                    None => annotation(&value, t)?,
                };
                out.push(stmt(StmtKind::Assign {
                    target: name(n),
                    ty,
                    value,
                }));
            }
            K::Let {
                pat: y::Pat::Tuple(ps),
                rhs,
                ..
            } => {
                let mut names = Vec::new();
                for p in ps {
                    let y::Pat::Bind(n) = p else {
                        return no("a nested `let` pattern");
                    };
                    names.push(name(n));
                }
                let value = self.rhs(rhs)?;
                out.push(stmt(StmtKind::Assign {
                    target: e(ExprKind::Tuple(names)),
                    ty: None,
                    value,
                }));
            }
            K::Let { .. } => return no("this `let` pattern"),
            K::Assign { place, rhs, .. } => {
                let target = self.place(place)?;
                let concat = |c: &y::Call| (c.path.join(".") == "std.text.concat").then_some("+");
                if let y::Rhs::Call(c) = rhs
                    && let Some(op) = binary(&c.path.join(".")).or_else(|| concat(c))
                    && !COMPARE.contains(&op)
                    && let [
                        y::Arg::Value(_, a @ y::Atom::Name(an, _), _),
                        y::Arg::Value(_, b, _),
                    ] = c.args.as_slice()
                    && if place.projs.is_empty() {
                        *an == place.root
                    } else {
                        self.peek(a).is_some_and(|x| same(x, &target))
                    }
                {
                    if !place.projs.is_empty() {
                        self.pending.remove(an);
                    }
                    self.marks
                        .call_mark(c, &Mark::Operator)
                        .into_iter()
                        .for_each(|m| self.marks.accept(m));
                    let value = self.atom(b)?;
                    out.push(stmt(StmtKind::AugAssign {
                        target,
                        op: aug(op),
                        value,
                    }));
                } else {
                    let value = self.rhs(rhs)?;
                    out.push(stmt(StmtKind::Assign {
                        target,
                        ty: None,
                        value,
                    }));
                }
            }
            K::Rhs(y::Rhs::Call(c))
                if BUILDERS.contains(&c.path.join(".").as_str())
                    && let Some(y::Arg::Value(y::Conv::Inout, y::Atom::Name(t, _), _)) =
                        c.args.first()
                    && self.pending.contains_key(t) =>
            {
                let vals = self.values(c);
                let xs = vals[1..]
                    .iter()
                    .map(|a| self.atom(a))
                    .collect::<R<Vec<_>>>()?;
                match (self.pending.get_mut(t), xs.as_slice()) {
                    (Some(Pending::List(v) | Pending::Set(v)), [x]) => v.push(x.clone()),
                    (Some(Pending::Dict(v)), [k, x]) => v.push((k.clone(), x.clone())),
                    _ => return no("a collection built in pieces"),
                }
            }
            K::Rhs(y::Rhs::Call(c))
                if matches!(
                    c.path.join(".").as_str(),
                    "std.dict.insert" | "std.collections.sorted_dict.insert" // `m[k] = v` passes the key by `let`; the method takes it by `sink`.
                ) && let [
                    y::Arg::Value(y::Conv::Inout, d, _),
                    y::Arg::Value(y::Conv::Let, k, _),
                    y::Arg::Value(_, v, _),
                ] = c.args.as_slice() =>
            {
                let d = self.atom(d)?;
                let k = self.atom(k)?;
                let value = self.atom(v)?;
                out.push(stmt(StmtKind::Assign {
                    target: e(ExprKind::Index(Box::new(d), Box::new(k))),
                    ty: None,
                    value,
                }));
            }
            K::Rhs(r) => {
                let discards =
                    matches!(r, y::Rhs::Call(c) if self.std.returns_value(&c.path.join(".")));
                let x = self.rhs(r)?;
                out.push(stmt(if discards {
                    StmtKind::Assign {
                        target: name("_"),
                        ty: None,
                        value: x,
                    }
                } else {
                    StmtKind::Expr(x)
                }));
            }
            K::If { cond, then, els } => {
                let c = self.atom(cond)?;
                let then = self.block(then)?;
                let els = els.as_ref().map(|b| self.block(b)).transpose()?;
                let mut arms = vec![(c, then)];
                let els = match els {
                    Some(b)
                        if b.stmts.len() == 1 && matches!(b.stmts[0].kind, StmtKind::If { .. }) =>
                    {
                        let StmtKind::If { arms: more, els } =
                            b.stmts.into_iter().next().expect("one").kind
                        else {
                            unreachable!("matched")
                        };
                        arms.extend(more);
                        els
                    }
                    els => els,
                };
                out.push(stmt(StmtKind::If { arms, els }));
            }
            K::Ret(a) => {
                let x = self.atom(a)?;
                let r = match x.kind {
                    ExprKind::Lit(Lit::None) if matches!(a, y::Atom::Lit(y::Literal::Unit, _)) => {
                        None
                    }
                    _ => Some(x),
                };
                out.push(stmt(StmtKind::Return(r)));
            }
            K::Fail(a) => {
                let x = self.atom(a)?;
                out.push(stmt(StmtKind::Fail(x)));
            }
            K::Loop { label, body, .. }
                if let Some(k) = self.iter_loop(label, &body.stmts, out)? =>
            {
                return Ok(k);
            }
            K::Loop { label, body, .. } => {
                let ss = &body.stmts;
                // `while c` is a loop whose first statement after `c`'s temporaries is
                // `if c {} else { break }`.
                let j = ss.iter().position(|s| {
                    !matches!(&s.kind,
                    K::Let { pat: y::Pat::Bind(n), .. } if generated(n))
                });
                let test = j.and_then(|j| match &ss[j].kind {
                    K::If { cond, then, els: Some(b) }
                        if then.stmts.is_empty()
                            && matches!(b.stmts.as_slice(),
                                [y::Stmt { kind: K::Break { label: l, value: None }, .. }] if l == label) =>
                    {
                        Some((j, cond.clone()))
                    }
                    _ => None,
                });
                let (cond, rest) = match test {
                    Some((j, c)) => {
                        let pre = self.stmts(&ss[..j])?;
                        if !pre.is_empty() {
                            return no("a loop condition with statements");
                        }
                        (self.atom(&c)?, &ss[j + 1..])
                    }
                    None => (e(ExprKind::Lit(Lit::Bool(true))), &ss[..]),
                };
                let b = self.loop_body(label, rest)?;
                out.push(stmt(StmtKind::While {
                    label: None,
                    cond,
                    contracts: Vec::new(),
                    body: b,
                }));
            }
            K::Break { label, value: None } if self.loops.last() == Some(label) => {
                out.push(stmt(StmtKind::Break))
            }
            K::Continue { label } if self.loops.last() == Some(label) => {
                out.push(stmt(StmtKind::Continue))
            }
            K::Break { .. } | K::Continue { .. } => {
                return no("a `break` or `continue` to an outer loop");
            }
            K::Jp {
                name: k,
                params,
                body,
            } => {
                if let Some(next) = ss.get(1)
                    && let Some(x) = self.fallback(k, &next.kind)?
                {
                    let [y::Param::Value { name: v, ty: t, .. }] = params.as_slice() else {
                        return no("a join point with other than one parameter");
                    };
                    self.types.insert(v.clone(), t.clone());
                    let (names, k) = destructured(v, &body.stmts);
                    if names.is_empty() {
                        self.bind_pending(v, Pending::Expr(x));
                    } else {
                        out.push(stmt(StmtKind::Assign {
                            target: e(ExprKind::Tuple(names.iter().map(|n| name(n)).collect())),
                            ty: None,
                            value: x,
                        }));
                    }
                    out.extend(self.stmts(&body.stmts[k..])?);
                    return Ok(2);
                }
                return no("join points");
            }
            K::Match { scrutinee, arms } => {
                let scrutinee = self.atom(scrutinee)?;
                let mut cases = Vec::new();
                for (p, b) in arms {
                    cases.push(Case {
                        pat: pat(p)?,
                        span: AT,
                        guard: None,
                        body: self.block(b)?,
                    });
                }
                out.push(stmt(StmtKind::Match { scrutinee, cases }));
            }
            K::With {
                name: n,
                dyn_: false,
                handler,
                body,
            } => {
                let h = self.atom(handler)?;
                let body = self.block(body)?;
                out.push(stmt(StmtKind::With {
                    kind: WithKind::Bind(n.clone(), h),
                    body,
                }));
            }
            K::Deadline { at, body } => {
                let at = self.atom(at)?;
                let body = self.block(body)?;
                out.push(stmt(StmtKind::With {
                    kind: WithKind::Deadline(at),
                    body,
                }));
            }
            K::Assert(x) => {
                let x = self.pure(x)?;
                if out.is_empty() && self.marks.marked_pre(s) {
                    self.pre.push(x);
                } else if self.marks.marked_post(s) {
                    if !self.post.contains(&x) {
                        self.post.push(x);
                    }
                } else {
                    out.push(stmt(StmtKind::Assert(x)));
                }
            }
            K::Unreachable => out.push(stmt(StmtKind::Unreachable)),
            K::Drop(_) => {}
            other => return no(format!("this statement: {}", kind_name(other))),
        }
        Ok(1)
    }
}
