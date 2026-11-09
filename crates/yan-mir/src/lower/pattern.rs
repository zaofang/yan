use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    /// Test `v` against `p`, binding names, and continue in `fail` when it does not match.
    pub(super) fn pat(&mut self, p: &y::Pat, v: &[Operand], s: &Shape, fail: BlockId) -> R<()> {
        let test = |me: &mut Self, ok: Operand| {
            let next = me.block_new();
            me.finish(Term::Branch {
                cond: ok,
                then: next,
                els: fail,
            });
            me.cur = next;
        };
        match (p, s) {
            (y::Pat::Wild, _) => {}
            (y::Pat::Bind(n), _) => {
                let var = self.var(s.clone());
                self.copy_to(&var.locals.clone(), v.to_vec());
                self.names.insert(n.clone(), var);
            }
            (y::Pat::Lit(l), _) => {
                let (o, _) = self.literal(l)?;
                let ok = match s {
                    Shape::Str => self.rt(RtFn::StrEq, Ty::Bool, vec![v[0].clone(), o]),
                    Shape::Scalar(t @ (Ty::Int(_) | Ty::Bool)) => {
                        let d = self.local(Ty::Bool);
                        self.emit(Inst::Scalar {
                            dst: Some(d),
                            id: registry::id("eq_i64").expect("registered"),
                            op: ScalarOp::Eq,
                            ty: *t,
                            args: vec![v[0].clone(), o],
                        });
                        Operand::Local(d)
                    }
                    _ => return unsupported("this literal pattern"),
                };
                test(self, ok);
            }
            (y::Pat::Range(lo, hi), Shape::Scalar(t @ Ty::Int(_))) => {
                let (lo, _) = self.literal(lo)?;
                let (hi, _) = self.literal(hi)?;
                for (op, bound) in [(ScalarOp::Ge, lo), (ScalarOp::Le, hi)] {
                    let ok = self.scalar_op(op, *t, vec![v[0].clone(), bound]);
                    test(self, ok);
                }
            }
            (y::Pat::Tuple(ps), Shape::Tuple(ss)) if ps.len() == ss.len() => {
                let mut at = 0;
                for (p, e) in ps.iter().zip(ss) {
                    let w = e.width();
                    self.pat(p, &v[at..at + w], e, fail)?;
                    at += w;
                }
            }
            (y::Pat::Ctor { path, fields }, Shape::Rec(..) | Shape::Sum(..)) => {
                let name = path.last().expect("named");
                let (start, fs) = match s {
                    Shape::Sum(..) => {
                        let Some((tag, at, fs)) = s.variant(name) else {
                            return unsupported(format!("pattern `{name}`"));
                        };
                        let ok = self.scalar_op(
                            ScalarOp::Eq,
                            I64,
                            vec![v[0].clone(), Operand::Int(tag)],
                        );
                        test(self, ok);
                        (at, fs.to_vec())
                    }
                    Shape::Rec(_, fs) => (0, fs.clone()),
                    _ => unreachable!("matched"),
                };
                for (n, sub) in fields.iter().flat_map(|(fs, _)| fs) {
                    let mut at = start;
                    let Some((_, fsh)) = fs.iter().find(|(f, sh)| {
                        if f == n {
                            return true;
                        }
                        at += sh.width();
                        false
                    }) else {
                        return unsupported(format!("field `{n}`"));
                    };
                    let w = fsh.width();
                    self.pat(sub, &v[at..at + w], fsh, fail)?;
                }
            }
            (y::Pat::Or(ps), _) => {
                let ok = self.block_new();
                for p in ps {
                    let next = self.block_new();
                    let before = self.names.len();
                    self.pat(p, v, s, next)?;
                    if self.names.len() != before {
                        return unsupported("an or-pattern that binds");
                    }
                    self.finish(Term::Jump(ok));
                    self.cur = next;
                }
                self.finish(Term::Jump(fail));
                self.cur = ok;
            }
            _ => return unsupported("this pattern"),
        }
        Ok(())
    }

    /// Lower the clause matrix `rows` over `cols` to a decision tree (Maranget, "Compiling
    /// pattern matching to good decision trees", ML 2008), which tests each part of the
    /// scrutinee at most once on any path. A leaf copies its row's bindings into the arm's
    /// variables, and every arm body is lowered once.
    pub(super) fn tree(
        &mut self,
        v: &[Operand],
        mut cols: Vec<Occ>,
        rows: Vec<Row<'m>>,
        leaves: &mut [Vec<Leaf>],
    ) -> R<()> {
        let mut rows = normalize(rows, &cols);
        let Some(first) = rows.first() else {
            self.finish(Term::Panic("unreachable"));
            return Ok(());
        };
        let refutable = |p: &y::Pat| !matches!(p, y::Pat::Wild);
        // The needed column of the first row with the longest refutable prefix (heuristic p).
        let Some(col) = (0..cols.len())
            .filter(|&c| refutable(first.pats[c]))
            .max_by_key(|&c| {
                let n = rows.iter().take_while(|r| refutable(r.pats[c])).count();
                (n, std::cmp::Reverse(c))
            })
        else {
            let first = rows.swap_remove(0);
            return self.leaf(v, &first, leaves);
        };
        let occ = cols[col].clone();
        match &occ.shape {
            Shape::Tuple(_) | Shape::Rec(..) => {
                let fs = occ.shape.fields().expect("a record or tuple");
                let names: Vec<&str> = fs.iter().map(|(n, ..)| n.as_str()).collect();
                cols.remove(col);
                cols.extend(fs.iter().map(|(_, at, s)| Occ {
                    at: occ.at + at,
                    shape: (*s).clone(),
                }));
                for r in &mut rows {
                    let h = r.pats.remove(col);
                    r.pats.extend(sub_pats(h, &names)?);
                }
                self.tree(v, cols, rows, leaves)
            }
            Shape::Sum(_, vs) => {
                cols.remove(col);
                let heads: Vec<&y::Pat> = rows.iter_mut().map(|r| r.pats.remove(col)).collect();
                let mut named = Vec::new();
                for h in &heads {
                    match ctor_name(h, &occ.shape) {
                        Some(n) => named.push(n),
                        None if matches!(h, y::Pat::Wild) => {}
                        None => return unsupported("this pattern"),
                    }
                }
                let on = match &v[occ.at] {
                    Operand::Local(l) => *l,
                    o => {
                        let l = self.local(I64);
                        self.emit(Inst::Copy {
                            dst: l,
                            src: o.clone(),
                        });
                        l
                    }
                };
                let mut subs = Vec::new();
                let mut default = None;
                let mut targets = Vec::new();
                for (name, fs) in vs {
                    if !named.contains(&name.as_str()) {
                        let b = *default.get_or_insert_with(|| {
                            let b = self.block_new();
                            let rows = rows
                                .iter()
                                .zip(&heads)
                                .filter(|(_, h)| matches!(h, y::Pat::Wild))
                                .map(|(r, _)| r.clone())
                                .collect();
                            subs.push((b, cols.clone(), rows));
                            b
                        });
                        targets.push(b);
                        continue;
                    }
                    let (_, mut at, _) = occ.shape.variant(name).expect("a variant");
                    let mut c = cols.clone();
                    for (_, s) in fs {
                        c.push(Occ {
                            at: occ.at + at,
                            shape: s.clone(),
                        });
                        at += s.width();
                    }
                    let names: Vec<&str> = fs.iter().map(|(n, _)| n.as_str()).collect();
                    let mut sub = Vec::new();
                    for (r, h) in rows.iter().zip(&heads) {
                        if matches!(h, y::Pat::Wild) || ctor_name(h, &occ.shape) == Some(name) {
                            let mut r = r.clone();
                            r.pats.extend(sub_pats(h, &names)?);
                            sub.push(r);
                        }
                    }
                    let b = self.block_new();
                    subs.push((b, c, sub));
                    targets.push(b);
                }
                self.finish(Term::Switch { on, targets });
                for (b, c, sub) in subs {
                    self.cur = b;
                    self.tree(v, c, sub, leaves)?;
                }
                Ok(())
            }
            s => {
                let h = first.pats[col];
                let w = s.width();
                let no = self.block_new();
                self.pat(h, &v[occ.at..occ.at + w], s, no)?;
                let is_bool = matches!(s, Shape::Scalar(Ty::Bool));
                let (mut yes_rows, mut no_rows) = (Vec::new(), Vec::new());
                for r in &rows {
                    let same = same_test(h, r.pats[col]);
                    if same != Some(false) {
                        let mut r = r.clone();
                        if same == Some(true) {
                            r.pats[col] = &WILD;
                        }
                        yes_rows.push(r);
                    }
                    if same != Some(true) {
                        let mut r = r.clone();
                        if is_bool && same == Some(false) {
                            r.pats[col] = &WILD;
                        }
                        no_rows.push(r);
                    }
                }
                self.tree(v, cols.clone(), yes_rows, leaves)?;
                self.cur = no;
                self.tree(v, cols, no_rows, leaves)
            }
        }
    }

    /// Leave the current block open for `row`'s arm, with the slots each name binds.
    pub(super) fn leaf(&mut self, v: &[Operand], row: &Row, leaves: &mut [Vec<Leaf>]) -> R<()> {
        let binds = row
            .binds
            .iter()
            .map(|(n, o)| {
                let src = v[o.at..o.at + o.shape.width()].to_vec();
                ((*n).to_owned(), src, o.shape.clone())
            })
            .collect();
        leaves[row.arm].push((self.cur, binds));
        Ok(())
    }

    /// Lower `body` after its leaves: in the leaf's own block when there is one, else in a
    /// block they all jump to.
    pub(super) fn arm(&mut self, leaves: Vec<Leaf>, body: &'m y::Block, join: BlockId) -> R<()> {
        let outer = self.names.clone();
        let Some((first, names)) = leaves.first() else {
            return Ok(());
        };
        let vars: Vec<(String, Var)> = names
            .iter()
            .map(|(n, _, s)| (n.clone(), self.var(s.clone())))
            .collect();
        let entry = if leaves.len() == 1 {
            *first
        } else {
            self.block_new()
        };
        for (b, binds) in &leaves {
            self.cur = *b;
            for (n, src, _) in binds {
                let Some((_, var)) = vars.iter().find(|(m, _)| m == n) else {
                    return unsupported("an or-pattern whose alternatives bind different names");
                };
                let dst = var.locals.clone();
                self.copy_to(&dst, src.clone());
            }
            if *b != entry {
                self.finish(Term::Jump(entry));
            }
        }
        self.cur = entry;
        self.names.extend(vars);
        self.block(body)?;
        self.finish(Term::Jump(join));
        self.names = outer;
        Ok(())
    }
}
