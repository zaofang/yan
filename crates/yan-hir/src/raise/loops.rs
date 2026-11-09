use super::*;

impl<'a> Fold<'a> {
    pub(super) fn loop_body(&mut self, label: &str, body: &[y::Stmt]) -> R<Vec<Stmt>> {
        self.loops.push(label.to_owned());
        let r = self.stmts(body);
        self.loops.pop();
        r
    }

    /// A `for` over a list or a `range`, as elaboration expands it (`for_` in yan-elab): a
    /// counter `i` starting at `var` `n`, an optional length temporary, then a loop that
    /// breaks when `i >= stop`, binds the target, and steps `i`.
    pub(super) fn for_loop(
        &mut self,
        head: &y::Stmt,
        i: &str,
        start: &y::Rhs,
        rest: &[y::Stmt],
        out: &mut Vec<Stmt>,
    ) -> R<Option<usize>> {
        use y::StmtKind as K;
        let y::Rhs::Atom(start) = start else {
            return Ok(None);
        };
        // The list case binds its length first.
        let (len, k) = match rest.first().map(|s| &s.kind) {
            Some(K::Let {
                pat: y::Pat::Bind(n),
                rhs: y::Rhs::Call(c),
                ..
            }) if generated(n) && c.path.last().is_some_and(|f| f == "len") => {
                (Some((n.clone(), c.clone())), 1)
            }
            _ => (None, 0),
        };
        let Some(y::Stmt {
            kind: K::Loop { label, body, .. },
            ..
        }) = rest.get(k)
        else {
            return Ok(None);
        };
        let [check, exit, bind, tail @ ..] = body.stmts.as_slice() else {
            return Ok(None);
        };
        let K::Let {
            pat: y::Pat::Bind(done),
            rhs: y::Rhs::Call(ge),
            ..
        } = &check.kind
        else {
            return Ok(None);
        };
        let [
            y::Arg::Value(_, y::Atom::Name(ci, _), _),
            y::Arg::Value(_, stop, _),
        ] = ge.args.as_slice()
        else {
            return Ok(None);
        };
        if ge.path != ["ge_i64"] || ci != i {
            return Ok(None);
        }
        match &exit.kind {
            K::If {
                cond: y::Atom::Name(c, _),
                then,
                els: None,
            } if c == done
                && matches!(then.stmts.as_slice(),
                    [y::Stmt { kind: K::Break { label: l, value: None }, .. }] if l == label) => {}
            _ => return Ok(None),
        }
        // The target: one name, or a tuple through a temporary and its fields.
        let K::Let {
            pat: y::Pat::Bind(t),
            ty: tt,
            rhs: cur,
            ..
        } = &bind.kind
        else {
            return Ok(None);
        };
        let mut targets = vec![t.clone()];
        let mut tail = tail;
        let fields: Vec<(String, y::Type)> = tail
            .iter()
            .map_while(|s| match &s.kind {
                K::Let {
                    pat: y::Pat::Bind(n),
                    ty,
                    rhs: y::Rhs::Proj(p),
                    ..
                } if p.root == *t
                    && matches!(p.projs.as_slice(), [y::Proj::Field(f, _)] if f.starts_with('_')) =>
                {
                    Some((n.clone(), ty.clone()))
                }
                _ => None,
            })
            .collect();
        if generated(t) && !fields.is_empty() {
            targets = fields.iter().map(|(n, _)| n.clone()).collect();
            for (n, ty) in &fields {
                self.types.insert(n.clone(), ty.clone());
            }
            tail = &tail[fields.len()..];
        } else {
            self.types.insert(t.clone(), tt.clone());
        }
        let [step_stmt, body @ ..] = tail else {
            return Ok(None);
        };
        let K::Assign {
            place,
            rhs: y::Rhs::Call(add),
            ..
        } = &step_stmt.kind
        else {
            return Ok(None);
        };
        let [
            y::Arg::Value(_, y::Atom::Name(ai, _), _),
            y::Arg::Value(_, step, _),
        ] = add.args.as_slice()
        else {
            return Ok(None);
        };
        if place.root != i || !place.projs.is_empty() || add.path != ["add_checked_i64"] || ai != i
        {
            return Ok(None);
        }
        let iter = match (cur, &len) {
            (y::Rhs::Proj(p), Some((n, c)))
                if matches!(stop, y::Atom::Name(s, _) if s == n)
                    && matches!(start, y::Atom::Lit(y::Literal::Int(z), _) if z == "0")
                    && matches!(p.projs.as_slice(), [y::Proj::Index(y::Atom::Name(x, _))] if x == i) =>
            {
                let [y::Arg::Value(_, y::Atom::Name(xs, _), _)] = c.args.as_slice() else {
                    return Ok(None);
                };
                if *xs != p.root {
                    return Ok(None);
                }
                // The list was bound to a temporary used only by the loop.
                match out.last() {
                    Some(Stmt {
                        kind:
                            StmtKind::Assign {
                                target:
                                    Expr {
                                        kind: ExprKind::Name(n),
                                        ..
                                    },
                                ..
                            },
                        ..
                    }) if n == xs && generated(xs) && self.uses(xs) == 2 => {
                        let Some(Stmt {
                            kind: StmtKind::Assign { value, .. },
                            ..
                        }) = out.pop()
                        else {
                            unreachable!("matched")
                        };
                        keys_iterated(value)
                    }
                    _ => self.atom(&y::Atom::Name(xs.clone(), AT))?,
                }
            }
            (y::Rhs::Atom(y::Atom::Name(x, _)), None) if x == i => {
                let mut args = Vec::new();
                let zero = matches!(start, y::Atom::Lit(y::Literal::Int(z), _) if z == "0");
                let one = matches!(step, y::Atom::Lit(y::Literal::Int(o), _) if o == "1");
                let written = self.marks.stmt_mark(head, &Mark::Explicit);
                if let Some(p) = written.clone() {
                    if zero {
                        self.marks.accept(p)
                    } else {
                        self.marks.refuse(p)
                    }
                }
                if !zero || !one || written.is_some() {
                    args.push(arg(self.atom(start)?));
                }
                args.push(arg(self.atom(stop)?));
                if !one {
                    args.push(arg(self.atom(step)?));
                }
                call(name("range"), args)
            }
            _ => return Ok(None),
        };
        let body = self.loop_body(label, body)?;
        out.push(stmt(StmtKind::For {
            label: None,
            target: targets,
            iter,
            contracts: Vec::new(),
            body,
        }));
        Ok(Some(k + 1))
    }

    /// A `for` over an iterator, as elaboration expands it (`for_iter` in yan-elab): a loop
    /// that breaks when `done(it)`, then binds the target from `current(it)`.
    pub(super) fn iter_loop(
        &mut self,
        label: &str,
        body: &[y::Stmt],
        out: &mut Vec<Stmt>,
    ) -> R<Option<usize>> {
        use y::StmtKind as K;
        let [check, exit, bind, tail @ ..] = body else {
            return Ok(None);
        };
        let (
            K::Let {
                pat: y::Pat::Bind(done),
                rhs: y::Rhs::Call(d),
                ..
            },
            K::Let {
                pat: y::Pat::Bind(t),
                ty: tt,
                rhs: y::Rhs::Call(cur),
                ..
            },
        ) = (&check.kind, &bind.kind)
        else {
            return Ok(None);
        };
        let it = match (
            d.path.join(".").as_str(),
            d.args.as_slice(),
            cur.path.join(".").as_str(),
            cur.args.as_slice(),
        ) {
            (
                "std.iter.ops.done",
                [y::Arg::Value(_, a @ y::Atom::Name(i, _), _)],
                "std.iter.ops.current",
                [y::Arg::Value(_, y::Atom::Name(j, _), _)],
            ) if i == j => a.clone(),
            _ => return Ok(None),
        };
        if !matches!(&exit.kind, K::If { cond: y::Atom::Name(c, _), then, els: None }
            if c == done && matches!(then.stmts.as_slice(),
                [y::Stmt { kind: K::Break { label: l, value: None }, .. }] if l == label))
        {
            return Ok(None);
        }
        let (names, k) = if generated(t) {
            let (ns, k) = destructured_from(t, tail);
            (ns, k)
        } else {
            (Vec::new(), 0)
        };
        let targets = if names.is_empty() {
            self.types.insert(t.clone(), tt.clone());
            vec![t.clone()]
        } else {
            for s in &tail[..k] {
                if let K::Let {
                    pat: y::Pat::Bind(n),
                    ty,
                    ..
                } = &s.kind
                {
                    self.types.insert(n.clone(), ty.clone());
                }
            }
            names
        };
        // The iterator was bound to a temporary used only by the loop.
        let iter = match (&it, out.last()) {
            (
                y::Atom::Name(n, _),
                Some(Stmt {
                    kind:
                        StmtKind::Assign {
                            target:
                                Expr {
                                    kind: ExprKind::Name(m),
                                    ..
                                },
                            ..
                        },
                    ..
                }),
            ) if n == m && generated(n) && self.uses(n) == 2 => {
                let Some(Stmt {
                    kind: StmtKind::Assign { value, .. },
                    ..
                }) = out.pop()
                else {
                    unreachable!("matched")
                };
                value
            }
            _ => self.atom(&it)?,
        };
        let body = self.loop_body(label, &tail[k..])?;
        out.push(stmt(StmtKind::For {
            label: None,
            target: targets,
            iter,
            contracts: Vec::new(),
            body,
        }));
        Ok(Some(1))
    }

    /// The value an arm passes to its join point, with the temporaries computing it folded in.
    pub(super) fn arm_value(&mut self, pre: &[y::Stmt], v: &y::Atom) -> R<Expr> {
        if !self.stmts(pre)?.is_empty() {
            return no("a fallback value with statements");
        }
        self.atom(v)
    }

    /// `x else v` as elaboration expands it: a join point `k` followed by a match on an
    /// optional or a result whose arms each jump to `k`.
    pub(super) fn fallback(&mut self, k: &str, next: &y::StmtKind) -> R<Option<Expr>> {
        let y::StmtKind::Match {
            scrutinee: y::Atom::Name(s, _),
            arms,
        } = next
        else {
            return Ok(None);
        };
        // An arm that jumps to `k`, after temporaries for the argument.
        fn jump<'b>(k: &str, b: &'b y::Block) -> Option<(&'b [y::Stmt], y::Atom)> {
            let (last, pre) = b.stmts.split_last()?;
            match &last.kind {
                y::StmtKind::Jump { name, args }
                    if name == k
                        && args.len() == 1
                        && pre.iter().all(|s| {
                            matches!(&s.kind,
                            y::StmtKind::Let { pat: y::Pat::Bind(t), .. } if generated(t))
                        }) =>
                {
                    Some((pre, args[0].clone()))
                }
                _ => None,
            }
        }
        let mut other = None;
        let mut bound = None;
        let mut some = false;
        let mut test = None;
        for (p, b) in arms {
            match p {
                y::Pat::Ctor { path, fields } if path == &["Some"] || path == &["Ok"] => {
                    if !jump(k, b).is_some_and(|(pre, _)| pre.is_empty()) {
                        return Ok(None);
                    }
                    some = true;
                    if path == &["Some"]
                        && matches!(fields, Some((fs, _)) if matches!(fs.as_slice(), [(_, y::Pat::Wild)]))
                    {
                        test = Some(b);
                    }
                }
                y::Pat::Ctor { path, fields: None } if path == &["None"] => other = Some(b),
                y::Pat::Bind(n) if n == "None" => other = Some(b),
                y::Pat::Ctor {
                    path,
                    fields: Some((fs, _)),
                } if path == &["Err"] => {
                    other = Some(b);
                    if let [(_, y::Pat::Bind(n))] = fs.as_slice() {
                        bound = Some(n.clone());
                    }
                }
                _ => return Ok(None),
            }
        }
        let (true, Some(b)) = (some, other) else {
            return Ok(None);
        };
        // `Some(_)` and `None` jumping to `true` and `false` is how `x is not None` elaborates.
        let flag = |b: &y::Block| match jump(k, b) {
            Some((_, y::Atom::Lit(y::Literal::Bool(v), _))) => Some(v),
            _ => None,
        };
        if let Some(t) = test
            && bound.is_none()
            && (flag(t), flag(b)) == (Some(true), Some(false))
        {
            let x = self.atom(&y::Atom::Name(s.clone(), AT))?;
            return Ok(Some(e(ExprKind::Compare(
                "is not",
                Box::new(x),
                Box::new(e(ExprKind::Lit(Lit::None))),
            ))));
        }
        if let Some(n) = &bound
            && let Some((pre, v)) = jump(k, b)
        {
            let x = self.atom(&y::Atom::Name(s.clone(), AT))?;
            let v = self.arm_value(pre, &v)?;
            return Ok(Some(e(ExprKind::Fallback(
                Box::new(x),
                Box::new(Fallback::Bind(n.clone(), v)),
            ))));
        }
        let x = self.atom(&y::Atom::Name(s.clone(), AT))?;
        let fb = match jump(k, b) {
            Some((pre, v)) => Fallback::Value(self.arm_value(pre, &v)?),
            None => {
                let mut ss = self.stmts(&b.stmts)?;
                let (Some(last), true) = (ss.pop(), ss.is_empty()) else {
                    return no("a fallback block");
                };
                match last.kind {
                    StmtKind::Fail(v) => Fallback::Fail(v),
                    StmtKind::Return(v) => Fallback::Return(v),
                    StmtKind::Break => Fallback::Break,
                    StmtKind::Continue => Fallback::Continue,
                    _ => return no("a fallback block"),
                }
            }
        };
        Ok(Some(e(ExprKind::Fallback(Box::new(x), Box::new(fb)))))
    }
}
