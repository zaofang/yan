use super::*;

impl<'a> Fold<'a> {
    pub(super) fn bind_pending(&mut self, n: &str, x: Pending) {
        self.pending.insert(n.to_owned(), x);
    }

    pub(super) fn stmts(&mut self, ss: &[y::Stmt]) -> R<Vec<Stmt>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < ss.len() {
            i += self.stmt(&ss[i..], &mut out)?;
        }
        Ok(out)
    }

    pub(super) fn block(&mut self, b: &y::Block) -> R<Block> {
        Ok(Block {
            inline: false,
            stmts: self.stmts(&b.stmts)?,
        })
    }

    /// `var t = copy p` and stores to its fields as `evolve(p, f=v, ...)` (14), pending at
    /// `t`'s one use after them; returns how many statements after the copy it consumed.
    pub(super) fn evolve(
        &mut self,
        t: &str,
        from: &y::Place,
        rest: &[y::Stmt],
        out: &mut Vec<Stmt>,
    ) -> R<Option<usize>> {
        use y::StmtKind as K;
        let mut stores = 0;
        let mut end = 0;
        for (k, s) in rest.iter().enumerate() {
            match &s.kind {
                K::Let {
                    pat: y::Pat::Bind(x),
                    ..
                } if generated(x) => {}
                K::Assign { place, .. }
                    if place.root == t
                        && matches!(place.projs.as_slice(), [y::Proj::Field(..)]) =>
                {
                    stores += 1;
                    end = k + 1;
                }
                _ => break,
            }
        }
        if stores == 0 || self.uses(t) != stores + 1 {
            return Ok(None);
        }
        let mut args = Vec::new();
        let mut k = 0;
        while k < end {
            match &rest[k].kind {
                K::Assign { place, rhs, .. } => {
                    let [y::Proj::Field(f, _)] = place.projs.as_slice() else {
                        unreachable!("matched")
                    };
                    args.push(Arg {
                        name: Some(f.clone()),
                        kind: ArgKind::Expr(self.rhs(rhs)?),
                        span: AT,
                    });
                    k += 1;
                }
                _ => k += self.stmt(&rest[k..], out)?,
            }
        }
        let mut all = vec![arg(self.place(from)?)];
        all.extend(args);
        self.bind_pending(t, Pending::Expr(call(name("evolve"), all)));
        Ok(Some(end))
    }

    /// Raises the statement at the head of `ss` into `out`; returns how many it consumed.
    pub(super) fn stmt(&mut self, ss: &[y::Stmt], out: &mut Vec<Stmt>) -> R<usize> {
        if let Some(m) = self.marks.stmt_mark(&ss[0], &Mark::Hoisted)
            && matches!(
                ss[0].kind,
                y::StmtKind::Let {
                    pat: y::Pat::Bind(_),
                    ..
                }
            )
        {
            // The declaration elaboration inserted before an `if` or `with` (R863): dropped only
            // when that statement follows.
            let mut decl = Vec::new();
            let k = self.stmt_inner(ss, &mut decl)?;
            let mut rest = Vec::new();
            let mut more = 0;
            while rest.is_empty() && k + more < ss.len() {
                more += self.stmt(&ss[k + more..], &mut rest)?;
            }
            let follows = matches!(
                rest.first().map(|s| &s.kind),
                Some(StmtKind::If { els: Some(_), .. } | StmtKind::With { .. })
            );
            if follows {
                self.marks.accept(m);
            } else {
                self.marks.refuse(m);
                out.extend(decl);
            }
            out.extend(rest);
            return Ok(k + more);
        }
        if let Some(m) = self.marks.stmt_mark(&ss[0], &Mark::Parenthesized) {
            // A tuple written in parentheses (R868): the next tuple raised keeps them, and the
            // mark fails if the statement does not bind one.
            if matches!(
                &ss[0].kind,
                y::StmtKind::Let { rhs: y::Rhs::Ctor(y::Ctor::Tuple(xs)), .. } if xs.len() > 1
            ) {
                self.marks.accept(m);
                self.paren_tuple = true;
                let k = self.stmt_inner(ss, out);
                self.paren_tuple = false;
                return k;
            }
            self.marks.refuse(m);
        }
        if let y::StmtKind::Let {
            pat: y::Pat::Bind(n),
            rhs: y::Rhs::Call(c),
            ..
        } = &ss[0].kind
            && generated(n)
            && n.starts_with("_c")
            && self
                .marks
                .stmt_mark(&ss[0], &Mark::Lenient("N36".into()))
                .is_none()
            && matches!(
                c.path.join(".").as_str(),
                "std.list.new" | "std.set.new" | "std.dict.new"
            )
        {
            // A comprehension lowered to a binding and an appending loop (R866): printed as the
            // comprehension only when the loop has the lowered shape and the binding is read once.
            let mut raised = Vec::new();
            let mut k = self.stmt_inner(ss, &mut raised)?;
            while !raised
                .iter()
                .any(|s| matches!(s.kind, StmtKind::For { .. }))
                && k < ss.len()
            {
                k += self.stmt(&ss[k..], &mut raised)?;
            }
            // The accumulator's name is one a program cannot write, so the shape alone says the
            // loop was a comprehension (the bare check of RFC 0034 D7 needs it); the mark, when
            // present, is consumed.
            let mark = self.marks.stmt_mark(&ss[0], &Mark::Comprehension);
            match comprehension(n, &raised) {
                Some(x) if self.uses(n) == 2 => {
                    mark.into_iter().for_each(|p| self.marks.accept(p));
                    self.bind_pending(n, Pending::Expr(x));
                }
                _ => {
                    mark.into_iter().for_each(|p| self.marks.refuse(p));
                    out.extend(raised);
                }
            }
            return Ok(k);
        }
        if let Some(p) = self.marks.stmt_mark(&ss[0], &Mark::Lenient("N36".into()))
            && let y::StmtKind::Let {
                pat: y::Pat::Bind(n),
                rhs: y::Rhs::Call(c),
                ..
            } = &ss[0].kind
            && c.path.join(".") == "std.list.new"
        {
            // N36's `list[char]` rewrite: the new list and the loop that shows each element.
            let mut raised = Vec::new();
            let mut k = self.stmt_inner(ss, &mut raised)?;
            while raised.len() < 2 && k < ss.len() {
                k += self.stmt_inner(&ss[k..], &mut raised)?;
            }
            if let [
                Stmt {
                    kind: StmtKind::Assign { target, value, .. },
                    ..
                },
                Stmt {
                    kind:
                        StmtKind::For {
                            target: v,
                            iter,
                            body,
                            ..
                        },
                    ..
                },
            ] = raised.as_slice()
                && matches!(&target.kind, ExprKind::Name(t) if t == n)
                && matches!(&value.kind, ExprKind::List(xs) if xs.is_empty())
                && let [v] = v.as_slice()
                && let [
                    Stmt {
                        kind:
                            StmtKind::Expr(Expr {
                                kind: ExprKind::Call(f, args),
                                ..
                            }),
                        ..
                    },
                ] = body.as_slice()
                && let ExprKind::Attr(recv, m, _) = &f.kind
                && m == "append"
                && matches!(&recv.kind, ExprKind::Name(t) if t == n)
                && let [
                    Arg {
                        kind: ArgKind::Expr(a),
                        ..
                    },
                ] = args.as_slice()
                && match &a.kind {
                    ExprKind::Name(x) => x == v,
                    ExprKind::FStr { pieces, .. } => matches!(pieces.as_slice(),
                        [FPiece::Expr(Expr { kind: ExprKind::Name(x), .. }, None)] if x == v),
                    _ => false,
                }
            {
                self.marks.accept(p);
                self.bind_pending(n, Pending::Expr(iter.clone()));
                return Ok(k);
            }
            self.marks.refuse(p);
            out.extend(raised);
            return Ok(k);
        }
        // An operand temporary of a marked `divmod` is read by both halves, and folds into the call.
        if let y::StmtKind::Let {
            pat: y::Pat::Bind(n),
            rhs,
            ..
        } = &ss[0].kind
            && generated(n)
            && self.uses(n) == 2
            && ss
                .iter()
                .take(3)
                .skip(1)
                .any(|s| self.marks.stmt_mark(s, &Mark::Prelude).is_some())
        {
            let x = self.rhs(rhs)?;
            self.bind_pending(n, Pending::Expr(x));
            return Ok(1);
        }
        if let Some(m) = self.marks.stmt_mark(&ss[0], &Mark::Prelude) {
            match self.divmod(ss) {
                Some((t, x, k)) => {
                    self.marks.accept(m);
                    let (names, more) = destructured(&t, &ss[k..]);
                    if !names.is_empty() && self.uses(&t) == names.len() {
                        for (i, b) in names.iter().enumerate() {
                            if let y::StmtKind::Let { ty, .. } = &ss[k + names.len() + i].kind {
                                self.types.insert(b.clone(), ty.clone());
                            }
                        }
                        out.push(stmt(StmtKind::Assign {
                            target: e(ExprKind::Tuple(names.iter().map(|b| name(b)).collect())),
                            ty: None,
                            value: x,
                        }));
                        return Ok(k + more);
                    }
                    self.bind_pending(&t, Pending::Expr(x));
                    return Ok(k);
                }
                None => self.marks.refuse(m),
            }
        }
        let head = match &ss[0].kind {
            y::StmtKind::Let {
                pat: y::Pat::Bind(n),
                ..
            } => generated(n),
            _ => false,
        } || self.marks.stmt_mark(&ss[0], &Mark::Parallel).is_some();
        if head && let Some(k) = self.parallel(ss, out)? {
            return Ok(k);
        }
        self.stmt_inner(ss, out)
    }

    /// The prelude `divmod(a, b)` (R801): `a // b`, `a % b` and their pair, with the operands
    /// the same atoms. Returns the pair's temporary, the call, and the statements consumed.
    pub(super) fn divmod(&mut self, ss: &[y::Stmt]) -> Option<(String, Expr, usize)> {
        use y::StmtKind as K;
        let op = |s: &y::Stmt, f: &str| match &s.kind {
            K::Let {
                pat: y::Pat::Bind(t),
                rhs: y::Rhs::Call(c),
                ..
            } if c.path.len() == 1 && c.path[0].starts_with(f) => match self.values(c).as_slice() {
                [a, b] => Some((
                    t.clone(),
                    (*a).clone(),
                    (*b).clone(),
                    c.path[0][f.len()..].to_owned(),
                )),
                _ => None,
            },
            _ => None,
        };
        let (q, a, b, ty) = op(ss.first()?, "floordiv_")?;
        let (r, a2, b2, ty2) = op(ss.get(1)?, "mod_")?;
        let K::Let {
            pat: y::Pat::Bind(t),
            rhs: y::Rhs::Ctor(ctor),
            ..
        } = &ss.get(2)?.kind
        else {
            return None;
        };
        let atoms = |x: &y::Atom, y_: &y::Atom| match (x, y_) {
            (y::Atom::Name(m, _), y::Atom::Name(n, _)) => m == n,
            (y::Atom::Lit(l, _), y::Atom::Lit(k, _)) => l == k,
            _ => false,
        };
        let y::Ctor::Tuple(items) = ctor else {
            return None;
        };
        let named = |x: &y::Atom, n: &String| matches!(x, y::Atom::Name(m, _) if m == n);
        if ty != ty2
            || !atoms(&a, &a2)
            || !atoms(&b, &b2)
            || !matches!(items.as_slice(), [x, y_] if named(x, &q) && named(y_, &r))
        {
            return None;
        }
        let (a, b) = (self.atom(&a).ok()?, self.atom(&b).ok()?);
        Some((t.clone(), call(name("divmod"), vec![arg(a), arg(b)]), 3))
    }

    /// A tuple assignment (R216): temporaries holding every right-hand value, then one store
    /// per target. The `Parallel` mark sits on its first statement and on its last store; with
    /// it the group prints as one statement. Without it the snapshots stay, since inlining
    /// them would make the stores sequential.
    pub(super) fn parallel(&mut self, ss: &[y::Stmt], out: &mut Vec<Stmt>) -> R<Option<usize>> {
        use y::StmtKind as K;
        let mut temps = HashSet::new();
        let mut p = 0;
        while let Some(K::Let {
            pat: y::Pat::Bind(n),
            ..
        }) = ss.get(p).map(|s| &s.kind)
            && generated(n)
        {
            temps.insert(n.as_str());
            p += 1;
        }
        // A tuple unpacked into names binds projections of one temporary: not a snapshot.
        if ss[..p]
            .iter()
            .any(|s| matches!(&s.kind, K::Let { rhs: y::Rhs::Proj(pl), .. } if generated(&pl.root)))
        {
            return Ok(None);
        }
        let mark = |i: usize| {
            ss.get(i)
                .and_then(|s| self.marks.stmt_mark(s, &Mark::Parallel))
        };
        // A store of a temporary, or of a literal when the group is marked.
        let store = |s: &y::Stmt, literals: bool| match &s.kind {
            K::Let {
                pat: y::Pat::Bind(_),
                rhs: y::Rhs::Atom(a),
                ..
            }
            | K::Assign {
                rhs: y::Rhs::Atom(a),
                ..
            } => match a {
                y::Atom::Name(t, _) if temps.contains(t.as_str()) => Some(Some(t.clone())),
                y::Atom::Lit(..) if literals => Some(None),
                _ => None,
            },
            _ => None,
        };
        let head = (0..=p).find(|&i| mark(i).is_some());
        let last = head.and_then(|h| (h + 1..ss.len()).find(|&i| mark(i).is_some()));
        let (end, marks) = match (head, last) {
            (Some(h), Some(l)) => (l + 1, vec![mark(h), mark(l)]),
            _ => {
                let stores: Vec<_> = ss[p..].iter().map_while(|s| store(s, false)).collect();
                (p + stores.len(), Vec::new())
            }
        };
        let stores: Vec<_> = ss[p.min(end)..end]
            .iter()
            .map_while(|s| store(s, !marks.is_empty()))
            .collect();
        let k = end.saturating_sub(p);
        let temp_stores: Vec<_> = stores.iter().flatten().collect();
        if k < 2
            || stores.len() != k
            || temp_stores.iter().collect::<HashSet<_>>().len() != temp_stores.len()
        {
            marks
                .into_iter()
                .flatten()
                .for_each(|m| self.marks.refuse(m));
            return Ok(None);
        }
        if marks.is_empty() {
            for s in &ss[..p] {
                let K::Let {
                    pat: y::Pat::Bind(n),
                    rhs,
                    ty,
                    ..
                } = &s.kind
                else {
                    unreachable!("matched above")
                };
                self.types.insert(n.clone(), ty.clone());
                let value = self.rhs(rhs)?;
                out.push(stmt(StmtKind::Assign {
                    target: name(n),
                    ty: None,
                    value,
                }));
            }
            let mut i = p;
            while i < end {
                i += self.stmt_inner(&ss[i..], out)?;
            }
            return Ok(Some(end));
        }
        let mut raised = Vec::new();
        let mut i = 0;
        while i < end {
            i += self.stmt_inner(&ss[i..], &mut raised)?;
        }
        let mut targets = Vec::new();
        let mut values = Vec::new();
        for r in &raised {
            if let StmtKind::Assign {
                target,
                ty: None,
                value,
            } = &r.kind
            {
                targets.push(target.clone());
                values.push(value.clone());
            }
        }
        if i == end && targets.len() == k && raised.len() == k {
            marks
                .into_iter()
                .flatten()
                .for_each(|m| self.marks.accept(m));
            out.push(stmt(StmtKind::Assign {
                target: e(ExprKind::Tuple(targets)),
                ty: None,
                value: e(ExprKind::Tuple(values)),
            }));
            return Ok(Some(i));
        }
        marks
            .into_iter()
            .flatten()
            .for_each(|m| self.marks.refuse(m));
        out.extend(raised);
        Ok(Some(i))
    }
}
