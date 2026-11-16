//! `==` and order of values `yan_rt::map` has no key schema for (lists, recursive types): part by
//! part, a part with a schema through the runtime, a recurrence through an out-of-line walk.

use super::*;

impl Fn<'_, '_> {
    /// `a == b`, both of shape `s`, borrowed.
    pub(super) fn eq_value(&mut self, s: &Shape, a: &[Operand], b: &[Operand]) -> R<Operand> {
        if Self::key_schema(s, &mut String::new()).is_ok() {
            return self.key_eq(s, a.to_vec(), b.to_vec(), "std.cmp.eq");
        }
        if let Some(r) = self.walk_pair(knot::Walk::Eq, s, a, b) {
            return Ok(r);
        }
        let r = self.local(Ty::Bool);
        let set = |me: &mut Self, v: Operand| me.emit(Inst::Copy { dst: r, src: v });
        set(self, Operand::Bool(true));
        match s {
            Shape::Rec(..) | Shape::Tuple(_) => {
                for (_, at, f) in s.fields().expect("an aggregate") {
                    let w = f.width();
                    self.when(Operand::Local(r), |me| {
                        let e = me.eq_value(f, &a[at..at + w], &b[at..at + w])?;
                        set(me, e);
                        Ok(())
                    })?;
                }
            }
            Shape::Sum(_, vs) => {
                let same = self.scalar_op(ScalarOp::Eq, I64, vec![a[0].clone(), b[0].clone()]);
                set(self, same.clone());
                self.when(same, |me| {
                    me.each_variant(vs, &a[0], |me, at, fs| {
                        let mut at = at;
                        for (_, f) in fs {
                            let w = f.width();
                            me.when(Operand::Local(r), |me| {
                                let e = me.eq_value(f, &a[at..at + w], &b[at..at + w])?;
                                set(me, e);
                                Ok(())
                            })?;
                            at += w;
                        }
                        Ok(())
                    })
                })?;
            }
            Shape::List(e) => {
                let (la, lb) = (a[0].clone(), b[0].clone());
                let na = self.rt(RtFn::ListLen, I64, vec![la.clone()]);
                let nb = self.rt(RtFn::ListLen, I64, vec![lb.clone()]);
                let same = self.scalar_op(ScalarOp::Eq, I64, vec![na.clone(), nb]);
                set(self, same);
                self.each(na, |me, i| {
                    me.when(Operand::Local(r), |me| {
                        let x = me.borrowed_element(&la, i.clone(), e);
                        let y = me.borrowed_element(&lb, i, e);
                        let v = me.eq_value(e, &x, &y)?;
                        set(me, v);
                        Ok(())
                    })
                })?;
            }
            _ => return unsupported("`==` on this type"),
        }
        Ok(Operand::Local(r))
    }

    /// The order of `a` and `b`, both of shape `s`, borrowed: -1, 0 or 1, as `yan_rt::map` orders
    /// keys (fields in order, a sum's variants in declaration order, lists lexicographically).
    pub(super) fn cmp_value(&mut self, s: &Shape, a: &[Operand], b: &[Operand]) -> R<Operand> {
        let mut schema = String::new();
        if Self::key_schema(s, &mut schema).is_ok() {
            let schema = self.lit(&schema);
            let (x, y) = (self.key_buf(a.to_vec()), self.key_buf(b.to_vec()));
            let o = self.rt(RtFn::KeyCmp, I64, vec![schema, x.clone(), y.clone()]);
            self.free(RtFn::EnvFree, x);
            self.free(RtFn::EnvFree, y);
            return Ok(o);
        }
        if let Some(o) = self.walk_pair(knot::Walk::Cmp, s, a, b) {
            return Ok(o);
        }
        let o = self.local(I64);
        let set = |me: &mut Self, v: Operand| me.emit(Inst::Copy { dst: o, src: v });
        set(self, Operand::Int(0));
        let tied = |me: &mut Self| {
            me.scalar_op(ScalarOp::Eq, I64, vec![Operand::Local(o), Operand::Int(0)])
        };
        match s {
            Shape::Rec(..) | Shape::Tuple(_) => {
                for (_, at, f) in s.fields().expect("an aggregate") {
                    let w = f.width();
                    let t = tied(self);
                    self.when(t, |me| {
                        let c = me.cmp_value(f, &a[at..at + w], &b[at..at + w])?;
                        set(me, c);
                        Ok(())
                    })?;
                }
            }
            Shape::Sum(_, vs) => {
                let c = self.cmp_ints(a[0].clone(), b[0].clone());
                set(self, c);
                let t = tied(self);
                self.when(t, |me| {
                    me.each_variant(vs, &a[0], |me, at, fs| {
                        let mut at = at;
                        for (_, f) in fs {
                            let w = f.width();
                            let t = tied(me);
                            me.when(t, |me| {
                                let c = me.cmp_value(f, &a[at..at + w], &b[at..at + w])?;
                                set(me, c);
                                Ok(())
                            })?;
                            at += w;
                        }
                        Ok(())
                    })
                })?;
            }
            Shape::List(e) => {
                let (la, lb) = (a[0].clone(), b[0].clone());
                let na = self.rt(RtFn::ListLen, I64, vec![la.clone()]);
                let nb = self.rt(RtFn::ListLen, I64, vec![lb.clone()]);
                let shorter = self.scalar_op(ScalarOp::Le, I64, vec![na.clone(), nb.clone()]);
                let n = self.local(I64);
                let (ta, tb) = self.branch(shorter);
                let join = self.block_new();
                for (blk, len) in [(ta, na.clone()), (tb, nb.clone())] {
                    self.cur = blk;
                    self.emit(Inst::Copy { dst: n, src: len });
                    self.goto(join);
                }
                self.each(Operand::Local(n), |me, i| {
                    let t = tied(me);
                    me.when(t, |me| {
                        let x = me.borrowed_element(&la, i.clone(), e);
                        let y = me.borrowed_element(&lb, i, e);
                        let c = me.cmp_value(e, &x, &y)?;
                        set(me, c);
                        Ok(())
                    })
                })?;
                let t = tied(self);
                self.when(t, |me| {
                    let c = me.cmp_ints(na, nb);
                    set(me, c);
                    Ok(())
                })?;
            }
            _ => return unsupported("an order on this type"),
        }
        Ok(Operand::Local(o))
    }

    /// Walk `w` over `a` and `b` out of line, for a recurrence.
    fn walk_pair(
        &mut self,
        w: knot::Walk,
        s: &Shape,
        a: &[Operand],
        b: &[Operand],
    ) -> Option<Operand> {
        let (k, mut v) = match s {
            Shape::Named(k) => (k, a.to_vec()),
            Shape::Boxed(k) => (k, self.unbox(k, &a[0])),
            _ => return None,
        };
        match s {
            Shape::Boxed(_) => v.extend(self.unbox(k, &b[0])),
            _ => v.extend_from_slice(b),
        }
        Some(self.walk_call(w, k, &v)[0].clone())
    }

    /// Run `f` when `cond` holds, then continue after it.
    fn when(&mut self, cond: Operand, f: impl FnOnce(&mut Self) -> R<()>) -> R<()> {
        let (then, join) = self.branch(cond);
        self.cur = then;
        f(self)?;
        self.goto(join);
        Ok(())
    }

    /// Run `f` in the arm of the variant of the sum `vs` whose tag is `tag`, with its first slot
    /// and fields, then continue after them.
    fn each_variant(
        &mut self,
        vs: &[(String, Fields)],
        tag: &Operand,
        mut f: impl FnMut(&mut Self, usize, &Fields) -> R<()>,
    ) -> R<()> {
        let on = match tag {
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
        let join = self.block_new();
        let arms: Vec<BlockId> = vs.iter().map(|_| self.block_new()).collect();
        self.finish(Term::Switch {
            on,
            targets: arms.clone(),
        });
        let mut at = 1;
        for (arm, (_, fs)) in arms.into_iter().zip(vs) {
            self.cur = arm;
            f(self, at, fs)?;
            self.finish(Term::Jump(join));
            at += fs.iter().map(|(_, s)| s.width()).sum::<usize>();
        }
        self.cur = join;
        Ok(())
    }

    /// -1, 0 or 1 as the `I64` `a` is below, equal to or above `b`.
    fn cmp_ints(&mut self, a: Operand, b: Operand) -> Operand {
        let o = self.local(I64);
        let lt = self.scalar_op(ScalarOp::Lt, I64, vec![a.clone(), b.clone()]);
        let gt = self.scalar_op(ScalarOp::Gt, I64, vec![a, b]);
        let join = self.block_new();
        let (yes, no) = self.branch(lt);
        self.cur = yes;
        self.emit(Inst::Copy {
            dst: o,
            src: Operand::Int(-1),
        });
        self.goto(join);
        self.cur = no;
        let (yes, no) = self.branch(gt);
        for (blk, v) in [(yes, 1), (no, 0)] {
            self.cur = blk;
            self.emit(Inst::Copy {
                dst: o,
                src: Operand::Int(v),
            });
            self.goto(join);
        }
        Operand::Local(o)
    }

    /// Element `i` of the list block `l`, which keeps it.
    fn borrowed_element(&mut self, l: &Operand, i: Operand, e: &Shape) -> Vec<Operand> {
        let at = self.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i]);
        self.read(&At::Mem(at, 0), e)
    }
}
