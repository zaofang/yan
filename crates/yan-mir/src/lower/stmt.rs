use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    /// Names bound in `b` go out of scope at its end.
    pub(super) fn block(&mut self, b: &'m y::Block) -> R<()> {
        let outer = self.names.clone();
        let r = self.block_in(b);
        self.names = outer;
        r
    }

    pub(super) fn block_in(&mut self, b: &'m y::Block) -> R<()> {
        let decls: Vec<_> = b
            .stmts
            .iter()
            .filter_map(|s| match &s.kind {
                y::StmtKind::Jp { name, params, body } => Some((name, params, body)),
                _ => None,
            })
            .collect();
        if decls.is_empty() {
            return b.stmts.iter().try_for_each(|s| self.stmt(s));
        }
        let cont = self.local(I64);
        let mut jps = Vec::new();
        let mut jp_vars = Vec::new();
        for (name, params, _) in &decls {
            let mut ls = Vec::new();
            let mut vs = Vec::new();
            for p in params.iter() {
                let y::Param::Value { name, ty, .. } = p else {
                    return unsupported("a capability join-point parameter");
                };
                let v = self.var(self.shape(ty)?);
                ls.extend(&v.locals);
                vs.push((name.clone(), v));
            }
            jps.push(((*name).clone(), self.block_new(), ls));
            jp_vars.push(vs);
        }
        self.scopes.push(Scope::Jps {
            jps: jps.clone(),
            cont,
            at: None,
        });
        let mut resume = Vec::new();
        // A join point's body sees the names declared before it, and its parameters.
        let mut seen = Vec::new();
        for s in &b.stmts {
            if matches!(s.kind, y::StmtKind::Jp { .. }) {
                seen.push(self.names.clone());
                continue;
            }
            if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
                *at = Some(resume.len() as i128);
            }
            self.stmt(s)?;
            let next = self.block_new();
            self.goto(next);
            resume.push(next);
        }
        let after = self.cur;
        if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
            *at = None;
        }
        let single = decls.len() == 1;
        let jps_in = decls.iter().zip(&jps).zip(jp_vars).zip(seen);
        for ((((_, _, body), (_, start, _)), vs), names) in jps_in {
            self.cur = *start;
            self.names = names;
            self.names.extend(vs);
            if single {
                // The one continuation is where the block's code resumes, so names its body binds
                // stay visible to a `par` branch's results.
                self.block_in(body)?;
            } else {
                self.block(body)?;
            }
            self.finish(Term::Switch {
                on: cont,
                targets: resume.clone(),
            });
        }
        self.scopes.pop();
        self.cur = after;
        Ok(())
    }

    pub(super) fn jump(&mut self, name: &str, args: &[y::Atom]) -> R<()> {
        let found = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Jps { jps, cont, at } => jps
                .iter()
                .find(|(n, ..)| n == name)
                .map(|(_, b, ps)| (*b, ps.clone(), *cont, *at)),
            Scope::Loop { .. } => None,
        });
        let Some((target, params, cont, at)) = found else {
            return unsupported(format!("jump to `{name}`"));
        };
        let mut vals = Vec::new();
        for a in args {
            vals.extend(self.atom_any(a)?.0);
        }
        // Through temporaries, so `jump k(b, a)` reads before it writes.
        let temps: Vec<Local> = vals
            .into_iter()
            .zip(&params)
            .map(|(src, p)| {
                let t = self.local(self.locals[*p as usize]);
                self.emit(Inst::Copy { dst: t, src });
                t
            })
            .collect();
        for (p, t) in params.iter().zip(temps) {
            self.emit(Inst::Copy {
                dst: *p,
                src: Operand::Local(t),
            });
        }
        if let Some(at) = at {
            self.emit(Inst::Copy {
                dst: cont,
                src: Operand::Int(at),
            });
        }
        self.end(Term::Jump(target));
        Ok(())
    }

    pub(super) fn leave(&mut self, label: &str, brk: bool) -> R<()> {
        let to = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Loop {
                label: l,
                head,
                exit,
            } if l == label => Some(if brk { *exit } else { *head }),
            _ => None,
        });
        match to {
            Some(b) => {
                if !brk {
                    self.back_edge();
                }
                self.end(Term::Jump(b));
                Ok(())
            }
            None => unsupported(format!("loop label `{label}`")),
        }
    }

    pub(super) fn stmt(&mut self, s: &'m y::Stmt) -> R<()> {
        use y::StmtKind as K;
        self.line = s.span.line;
        if !matches!(s.kind, K::Drop(_) | K::Ret(_) | K::Fail(_)) {
            self.flush_deferred();
        }
        match &s.kind {
            K::Let { pat, ty, rhs, .. } => {
                let shape = self.shape(ty)?;
                let v = self.value(rhs, &shape)?;
                match pat {
                    y::Pat::Bind(n) => {
                        match comparison(rhs) {
                            Some(sides) => self.cmps.insert(n.clone(), sides),
                            None => self.cmps.remove(n),
                        };
                        let var = self.var(shape);
                        self.copy_to(&var.locals.clone(), v);
                        self.names.insert(n.clone(), var);
                    }
                    y::Pat::Wild => {}
                    p => {
                        let fail = self.block_new();
                        self.pat(p, &v, &shape, fail)?;
                        let ok = self.cur;
                        self.cur = fail;
                        self.finish(Term::Panic("unreachable"));
                        self.cur = ok;
                    }
                }
                Ok(())
            }
            K::Assign { place, rhs, .. } => {
                let (at, shape) = self.place(place)?;
                let v = self.value(rhs, &shape)?;
                self.write(&at, v);
                Ok(())
            }
            K::If { cond, then, els } => {
                let cond = self.scalar1(cond)?;
                let (t, e) = self.branch(cond);
                let join = self.block_new();
                self.cur = t;
                self.block(then)?;
                self.goto(join);
                self.cur = e;
                if let Some(els) = els {
                    self.block(els)?;
                }
                self.goto(join);
                Ok(())
            }
            K::Match { scrutinee, arms } => {
                let (v, shape) = self.atom_any(scrutinee)?;
                let rows = arms
                    .iter()
                    .enumerate()
                    .map(|(arm, (p, _))| Row {
                        pats: vec![p],
                        arm,
                        binds: Vec::new(),
                    })
                    .collect();
                let mut leaves = vec![Vec::new(); arms.len()];
                self.tree(&v, vec![Occ { at: 0, shape }], rows, &mut leaves)?;
                let join = self.block_new();
                for ((_, body), l) in arms.iter().zip(leaves) {
                    self.arm(l, body, join)?;
                }
                self.cur = join;
                Ok(())
            }
            K::Loop { label, body, .. } => {
                let (head, exit) = (self.block_new(), self.block_new());
                self.goto(head);
                self.scopes.push(Scope::Loop {
                    label: label.clone(),
                    head,
                    exit,
                });
                self.block(body)?;
                self.scopes.pop();
                self.back_edge();
                self.goto(head);
                self.cur = exit;
                Ok(())
            }
            K::Break { value: Some(_), .. } => unsupported("`break` with a value"),
            K::Break { label, value: None } => self.leave(label, true),
            K::Continue { label } => self.leave(label, false),
            K::Jp { .. } => unreachable!("handled by `block`"),
            K::Jump { name, args } => self.jump(name, args),
            K::Ret(a) => {
                let sig = &self.sig(self.me as FuncId);
                let (ok, ret, fails) = (sig.ok.clone(), sig.ret.clone(), sig.fails);
                let v = self.atom(a, &ok)?;
                self.check_ensures(&v, &ok)?;
                let v = if fails {
                    Self::variant_value(&ret, "Ok", v)
                } else {
                    v
                };
                self.unwind(&[], None);
                self.ret(v);
                Ok(())
            }
            K::Fail(a) => {
                let ret = self.sig(self.me as FuncId).ret.clone();
                let Some((_, _, fs)) = ret.variant("Err") else {
                    return unsupported("`fail` outside a failing function");
                };
                let e = fs[0].1.clone();
                let v = self.atom(a, &e)?;
                self.unwind(&[], Some((&e, &v)));
                self.ret(Self::variant_value(&ret, "Err", v));
                Ok(())
            }
            K::Become(c) => {
                if !self.ensures.is_empty() {
                    return unsupported("`become` in a function with `ensures`");
                }
                let name = c.path.join(".");
                let Some(func) = self.callee(c)? else {
                    return unsupported(format!("`become` of `{name}`"));
                };
                if !self.inout.is_empty() || !self.sig(func).inout().is_empty() {
                    return unsupported("`become` with `inout` parameters");
                }
                if self.sig(func).ret != self.sig(self.me as FuncId).ret {
                    return unsupported("`become` across failure types");
                }
                let (args, _) = self.user_args(func, c)?;
                if !self.cost_tokens.is_empty() {
                    // The callee's work counts against this frame's bounds, so the frame stays.
                    let dst: Vec<Local> = self
                        .sig(func)
                        .ret
                        .tys()
                        .into_iter()
                        .map(|t| self.local(t))
                        .collect();
                    self.emit(Inst::Call {
                        dst: dst.clone(),
                        func,
                        args,
                    });
                    self.ret(dst.into_iter().map(Operand::Local).collect());
                    return Ok(());
                }
                if self.costing {
                    self.count(0);
                }
                self.end(Term::TailCall { func, args });
                Ok(())
            }
            // Names bound in a `par` child are read after the block.
            // Branches as tasks that switch at blocking channel operations (RFC 0043).
            K::Par {
                concurrent: true,
                blocks,
            } if blocks.len() > 1 => {
                let tasks: Vec<BlockId> = blocks.iter().map(|_| self.block_new()).collect();
                let join = self.block_new();
                self.finish(Term::Par {
                    tasks: tasks.clone(),
                    join,
                });
                for (b, t) in blocks.iter().zip(tasks) {
                    self.cur = t;
                    self.task_starts.push(self.locals.len() as Local);
                    self.block_in(b)?;
                    self.flush_deferred();
                    self.task_starts.pop();
                    self.finish(Term::TaskEnd);
                }
                self.cur = join;
                Ok(())
            }
            K::Par { blocks, .. } => blocks.iter().try_for_each(|b| self.block_in(b)),
            K::Assert(e) => {
                let sides = match e {
                    y::Expr::Atom(y::Atom::Name(n, _)) => self.cmps.get(n).cloned(),
                    _ => None,
                };
                let t = self.expr(e)?;
                let mut fields = Vec::new();
                for (name, a) in [("left", 0), ("right", 1)]
                    .into_iter()
                    .filter_map(|(n, i)| sides.as_ref().map(|s| (n, s[i].clone())))
                {
                    if let Some(v) = self.scalar_field(&a) {
                        fields.push((name.to_owned(), v));
                    }
                }
                let mut seen = Vec::new();
                for a in sides.iter().flatten() {
                    if let y::Atom::Name(n, _) = a
                        && !seen.contains(n)
                    {
                        seen.push(n.clone());
                        if let Some(v) = self.scalar_field(a) {
                            fields.push((n.clone(), v));
                        }
                    }
                }
                let fields = fields.into_iter().map(|(n, v)| (self.lit(&n), v)).collect();
                self.check_fields(t, "assert", fields);
                Ok(())
            }
            K::Unreachable => {
                self.end(Term::Panic("unreachable"));
                Ok(())
            }
            K::Unsafe(b) => self.block(b),
            K::Drop(n) => {
                self.drop_stmt(n);
                Ok(())
            }
            K::Rhs(y::Rhs::Call(c)) => self.call(c, None),
            K::Rhs(r @ y::Rhs::Propagate { .. }) => {
                self.value(r, &Shape::Scalar(Ty::Unit))?;
                Ok(())
            }
            K::Rhs(_) => Ok(()),
            K::Yield(_) => unsupported("generators"),
            K::With { .. } => unsupported("`with`"),
            K::Deadline { .. } => unsupported("`deadline`"),
            K::ComptimeFor { .. } | K::ComptimeIf { .. } => unsupported("a comptime statement"),
            K::Asm { .. } => unsupported("`asm`"),
            K::Reuse { .. } => unsupported("`reuse`"),
        }
    }
}
