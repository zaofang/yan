//! `iter` adapters over a list's remaining items (17.3, R508 to R515), evaluated at the call:
//! `enumerate`, `take`, `skip`, `windows`, `chunks` and `next`. An `iter` is its list with the
//! cursor in the `limit` slot, as `std.itertools` builds them.

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn iter_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let Some((_, a)) = args.first() else {
            return unsupported(format!("call of `{name}`"));
        };
        let (v, s) = self.atom_any(a)?;
        let Shape::List(e) = s else {
            return unsupported(format!("call of `{name}` on this iterator"));
        };
        let l = v[0].clone();
        // A list that is not an `iter` has no cursor (-1).
        let pos = self.rt(RtFn::DqMaxlen, I64, vec![l.clone()]);
        let pos = self.max_i64(pos, Operand::Int(0));
        let len = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
        let rem = self.scalar_op(ScalarOp::SubChecked, I64, vec![len.clone(), pos.clone()]);
        let advance = |me: &mut Self, to: Operand| {
            me.emit(Inst::Rt {
                dst: None,
                f: RtFn::DqLimit,
                args: vec![l.clone(), to],
            });
        };
        if id == id::STD_ITER_OPS_NEXT {
            let Some(d) = &dst else {
                return Ok(());
            };
            let ds = d.shape.clone();
            let empty = self.scalar_op(ScalarOp::Eq, I64, vec![rem, Operand::Int(0)]);
            let (none, some) = self.branch(empty);
            let join = self.block_new();
            self.cur = none;
            self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "None", vec![]));
            self.goto(join);
            self.cur = some;
            let x = self.element(&l, pos.clone(), &e)?;
            self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "Some", x));
            let next = self.scalar_op(ScalarOp::AddChecked, I64, vec![pos, Operand::Int(1)]);
            advance(self, next);
            self.goto(join);
            return Ok(());
        }
        let at = |me: &mut Self, base: &Operand, i: Operand| {
            me.scalar_op(ScalarOp::AddChecked, I64, vec![base.clone(), i])
        };
        let out = match id {
            id::STD_ITER_OPS_ENUMERATE => {
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(1 + e.width() as i128)],
                );
                self.each(rem, |me, i| {
                    let j = at(me, &pos, i.clone());
                    let mut t = vec![i];
                    t.extend(me.element(&l, j, &e)?);
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), t);
                    Ok(())
                })?;
                advance(self, len);
                out
            }
            id::STD_ITER_OPS_COLLECT => {
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                self.each(rem, |me, i| {
                    let j = at(me, &pos, i);
                    let x = me.element(&l, j, &e)?;
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), x);
                    Ok(())
                })?;
                advance(self, len);
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), vec![out]);
                }
                return Ok(());
            }
            id::STD_ITER_OPS_TAKE | id::STD_ITER_OPS_SKIP => {
                let n = self.scalar1(args[1].1)?;
                let n = self.max_i64(n, Operand::Int(0));
                let k = self.min_i64(rem.clone(), n);
                let (from, count) = if id == id::STD_ITER_OPS_TAKE {
                    (pos.clone(), k.clone())
                } else {
                    let from = at(self, &pos, k.clone());
                    let count = self.scalar_op(ScalarOp::SubChecked, I64, vec![rem, k.clone()]);
                    (from, count)
                };
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                self.each(count, |me, i| {
                    let j = at(me, &from, i);
                    let x = me.element(&l, j, &e)?;
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), x);
                    Ok(())
                })?;
                let to = if id == id::STD_ITER_OPS_TAKE {
                    at(self, &pos, k)
                } else {
                    len
                };
                advance(self, to);
                out
            }
            id::STD_ITER_OPS_WINDOWS | id::STD_ITER_OPS_CHUNKS => {
                let n = self.scalar1(args[1].1)?;
                let big = self.scalar_op(ScalarOp::Ge, I64, vec![n.clone(), Operand::Int(1)]);
                let (ok, bad) = self.branch(big);
                self.cur = bad;
                self.finish(Term::Panic("value"));
                self.cur = ok;
                let windows = id == id::STD_ITER_OPS_WINDOWS;
                let count = if windows {
                    let c = self.scalar_op(ScalarOp::SubChecked, I64, vec![rem.clone(), n.clone()]);
                    let c = self.scalar_op(ScalarOp::AddChecked, I64, vec![c, Operand::Int(1)]);
                    self.max_i64(c, Operand::Int(0))
                } else {
                    let c = self.scalar_op(ScalarOp::AddChecked, I64, vec![rem.clone(), n.clone()]);
                    let c = self.scalar_op(ScalarOp::SubChecked, I64, vec![c, Operand::Int(1)]);
                    self.scalar_op(ScalarOp::FloorDiv, I64, vec![c, n.clone()])
                };
                let out = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(1)]);
                self.each(count, |me, i| {
                    let (start, take) = if windows {
                        (i, n.clone())
                    } else {
                        let start = me.scalar_op(ScalarOp::MulChecked, I64, vec![i, n.clone()]);
                        let left = me.scalar_op(
                            ScalarOp::SubChecked,
                            I64,
                            vec![rem.clone(), start.clone()],
                        );
                        (start, me.min_i64(left, n.clone()))
                    };
                    let from = at(me, &pos, start);
                    let inner = me.rt(
                        RtFn::ListNew,
                        Ty::Ptr,
                        vec![Operand::Int(e.width() as i128)],
                    );
                    me.each(take, |me, j| {
                        let k = at(me, &from, j);
                        let x = me.element(&l, k, &e)?;
                        let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![inner.clone()]);
                        me.write(&At::Mem(slot, 0), x);
                        Ok(())
                    })?;
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), vec![inner]);
                    Ok(())
                })?;
                advance(self, len);
                out
            }
            _ => return unsupported(format!("call of `{name}`")),
        };
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::DqLimit,
            args: vec![out.clone(), Operand::Int(0)],
        });
        if let Some(d) = &dst {
            self.copy_to(&d.locals.clone(), vec![out]);
        }
        Ok(())
    }

    fn min_i64(&mut self, a: Operand, b: Operand) -> Operand {
        let le = self.scalar_op(ScalarOp::Le, I64, vec![a.clone(), b.clone()]);
        self.pick(le, a, b)
    }

    fn max_i64(&mut self, a: Operand, b: Operand) -> Operand {
        let le = self.scalar_op(ScalarOp::Le, I64, vec![a.clone(), b.clone()]);
        self.pick(le, b, a)
    }

    /// `yes` when `cond`, else `no`.
    fn pick(&mut self, cond: Operand, yes: Operand, no: Operand) -> Operand {
        let r = self.local(I64);
        let (t, f) = self.branch(cond);
        let join = self.block_new();
        self.cur = t;
        self.emit(Inst::Copy { dst: r, src: yes });
        self.goto(join);
        self.cur = f;
        self.emit(Inst::Copy { dst: r, src: no });
        self.goto(join);
        Operand::Local(r)
    }
}
