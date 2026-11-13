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
            id::STD_ITER_OPS_ZIP | id::STD_ITER_OPS_CHAIN => {
                let (bv, bs) = self.atom_any(args[1].1)?;
                let Shape::List(f) = bs else {
                    return unsupported(format!("call of `{name}` on this iterator"));
                };
                let b = bv[0].clone();
                let bpos = self.rt(RtFn::DqMaxlen, I64, vec![b.clone()]);
                let bpos = self.max_i64(bpos, Operand::Int(0));
                let blen = self.rt(RtFn::ListLen, I64, vec![b.clone()]);
                let brem =
                    self.scalar_op(ScalarOp::SubChecked, I64, vec![blen.clone(), bpos.clone()]);
                let zip = id == id::STD_ITER_OPS_ZIP;
                if !zip && *f != *e {
                    return unsupported(format!("call of `{name}` on this type"));
                }
                let width = if zip {
                    e.width() + f.width()
                } else {
                    e.width()
                };
                let out = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(width as i128)]);
                let push = |me: &mut Self, v: Vec<Operand>| {
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), v);
                };
                if zip {
                    let n = self.min_i64(rem, brem);
                    self.each(n.clone(), |me, i| {
                        let ja = at(me, &pos, i.clone());
                        let mut t = me.element(&l, ja, &e)?;
                        let jb = at(me, &bpos, i);
                        t.extend(me.element(&b, jb, &f)?);
                        push(me, t);
                        Ok(())
                    })?;
                    let (ta, tb) = (at(self, &pos, n.clone()), at(self, &bpos, n));
                    advance(self, ta);
                    self.emit(Inst::Rt {
                        dst: None,
                        f: RtFn::DqLimit,
                        args: vec![b, tb],
                    });
                } else {
                    self.each(rem, |me, i| {
                        let j = at(me, &pos, i);
                        let x = me.element(&l, j, &e)?;
                        push(me, x);
                        Ok(())
                    })?;
                    self.each(brem, |me, i| {
                        let j = at(me, &bpos, i);
                        let x = me.element(&b, j, &f)?;
                        push(me, x);
                        Ok(())
                    })?;
                    advance(self, len);
                    self.emit(Inst::Rt {
                        dst: None,
                        f: RtFn::DqLimit,
                        args: vec![b, blen],
                    });
                }
                out
            }
            id::STD_ITER_OPS_STEP_BY => {
                let n = self.scalar1(args[1].1)?;
                let big = self.scalar_op(ScalarOp::Ge, I64, vec![n.clone(), Operand::Int(1)]);
                let (ok, bad) = self.branch(big);
                self.cur = bad;
                self.finish(Term::Panic("value"));
                self.cur = ok;
                let c = self.scalar_op(ScalarOp::AddChecked, I64, vec![rem, n.clone()]);
                let c = self.scalar_op(ScalarOp::SubChecked, I64, vec![c, Operand::Int(1)]);
                let count = self.scalar_op(ScalarOp::FloorDiv, I64, vec![c, n.clone()]);
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                self.each(count, |me, i| {
                    let k = me.scalar_op(ScalarOp::MulChecked, I64, vec![i, n.clone()]);
                    let j = at(me, &pos, k);
                    let x = me.element(&l, j, &e)?;
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), x);
                    Ok(())
                })?;
                advance(self, len);
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

impl<'a, 'm> Fn<'a, 'm> {
    /// `map`, `filter`, `take_while`, `drop_while`, `fold`, `any`, `all`, `find`, `count`, `sum`,
    /// `min` and `max` over the remaining items, evaluated at the call; a closure runs only for
    /// the items its adapter pulls.
    pub(super) fn iter_closure_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let Some((_, a)) = args.first() else {
            return unsupported(format!("call of `{name}`"));
        };
        let (v, s) = self.atom_any(a)?;
        let Shape::List(e) = s else {
            return unsupported(format!("call of `{name}` on this iterator"));
        };
        let e = *e;
        let l = v[0].clone();
        let pos = self.rt(RtFn::DqMaxlen, I64, vec![l.clone()]);
        let pos = self.max_i64(pos, Operand::Int(0));
        let len = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
        let rem = self.scalar_op(ScalarOp::SubChecked, I64, vec![len.clone(), pos.clone()]);
        let advance = |me: &mut Self| {
            me.emit(Inst::Rt {
                dst: None,
                f: RtFn::DqLimit,
                args: vec![l.clone(), len.clone()],
            });
        };
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let item = |me: &mut Self, i: Operand| -> R<Vec<Operand>> {
            let j = me.scalar_op(ScalarOp::AddChecked, I64, vec![pos.clone(), i]);
            me.element(&l, j, &e)
        };
        let boolean = Shape::Scalar(Ty::Bool);
        match id {
            id::STD_ITER_OPS_COUNT => {
                advance(self);
                set(self, vec![rem]);
            }
            id::STD_ITER_OPS_SUM | id::STD_ITER_OPS_SUM_FLOAT => {
                let (zero, op, ty) = match &e {
                    Shape::Scalar(t) if *t == I64 => (Operand::Int(0), ScalarOp::AddChecked, I64),
                    Shape::Scalar(Ty::F64) => (Operand::F64(0.0), ScalarOp::Add, Ty::F64),
                    _ => return unsupported(format!("call of `{name}` on this type")),
                };
                let acc = self.local(ty);
                self.emit(Inst::Copy {
                    dst: acc,
                    src: zero,
                });
                self.each(rem, |me, i| {
                    let x = item(me, i)?;
                    let sum = me.scalar_op(op, ty, vec![Operand::Local(acc), x[0].clone()]);
                    me.emit(Inst::Copy { dst: acc, src: sum });
                    Ok(())
                })?;
                advance(self);
                set(self, vec![Operand::Local(acc)]);
            }
            id::STD_ITER_OPS_MIN | id::STD_ITER_OPS_MAX => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                if !matches!(args.get(1), Some((_, y::Atom::Name(n, _))) if n == "None") {
                    let f = self.key_closure(args[1].1)?;
                    let ds = d.shape.clone();
                    let best = self.var(e.clone());
                    let (best_key, have) = (self.local(I64), self.local(I64));
                    self.emit(Inst::Copy {
                        dst: have,
                        src: Operand::Int(0),
                    });
                    let max = id == id::STD_ITER_OPS_MAX;
                    self.each(rem, |me, i| {
                        let x = item(me, i)?;
                        let k = me.call_closure(&f, &[(&e, &x)], &Shape::Scalar(I64))[0].clone();
                        let first = me.scalar_op(
                            ScalarOp::Eq,
                            I64,
                            vec![Operand::Local(have), Operand::Int(0)],
                        );
                        let (init, rest) = me.branch(first);
                        let next = me.block_new();
                        me.cur = init;
                        me.copy_to(&best.locals.clone(), x.clone());
                        me.emit(Inst::Copy {
                            dst: best_key,
                            src: k.clone(),
                        });
                        me.emit(Inst::Copy {
                            dst: have,
                            src: Operand::Int(1),
                        });
                        me.goto(next);
                        me.cur = rest;
                        // The first of equal keys wins.
                        let cmp = if max { ScalarOp::Le } else { ScalarOp::Ge };
                        let keep =
                            me.scalar_op(cmp, I64, vec![k.clone(), Operand::Local(best_key)]);
                        let (skip, take) = me.branch(keep);
                        me.cur = take;
                        let old = Self::ops(&best);
                        me.glue(&e, &old);
                        me.copy_to(&best.locals.clone(), x.clone());
                        me.emit(Inst::Copy {
                            dst: best_key,
                            src: k,
                        });
                        me.goto(next);
                        me.cur = skip;
                        me.glue(&e, &x);
                        me.goto(next);
                        me.cur = next;
                        Ok(())
                    })?;
                    advance(self);
                    let some = self.scalar_op(
                        ScalarOp::Eq,
                        I64,
                        vec![Operand::Local(have), Operand::Int(1)],
                    );
                    let (yes, no) = self.branch(some);
                    let join = self.block_new();
                    self.cur = yes;
                    set(self, Self::variant_value(&ds, "Some", Self::ops(&best)));
                    self.goto(join);
                    self.cur = no;
                    set(self, Self::variant_value(&ds, "None", vec![]));
                    self.goto(join);
                    self.cur = join;
                    return Ok(());
                }
                if !matches!(&e, Shape::Scalar(t) if *t == I64) {
                    return unsupported(format!("call of `{name}` on this type"));
                }
                let ds = d.shape.clone();
                let best = self.local(I64);
                let have = self.local(I64);
                self.emit(Inst::Copy {
                    dst: have,
                    src: Operand::Int(0),
                });
                let max = id == id::STD_ITER_OPS_MAX;
                self.each(rem, |me, i| {
                    let x = item(me, i)?;
                    let first = me.scalar_op(
                        ScalarOp::Eq,
                        I64,
                        vec![Operand::Local(have), Operand::Int(0)],
                    );
                    let (init, rest) = me.branch(first);
                    let next = me.block_new();
                    me.cur = init;
                    me.emit(Inst::Copy {
                        dst: best,
                        src: x[0].clone(),
                    });
                    me.emit(Inst::Copy {
                        dst: have,
                        src: Operand::Int(1),
                    });
                    me.goto(next);
                    me.cur = rest;
                    // Replace when `x` is strictly better.
                    let keep = if max {
                        me.scalar_op(ScalarOp::Le, I64, vec![x[0].clone(), Operand::Local(best)])
                    } else {
                        me.scalar_op(ScalarOp::Ge, I64, vec![x[0].clone(), Operand::Local(best)])
                    };
                    let (skip, take) = me.branch(keep);
                    me.cur = take;
                    me.emit(Inst::Copy {
                        dst: best,
                        src: x[0].clone(),
                    });
                    me.goto(next);
                    me.cur = skip;
                    me.goto(next);
                    Ok(())
                })?;
                advance(self);
                let some = self.scalar_op(
                    ScalarOp::Eq,
                    I64,
                    vec![Operand::Local(have), Operand::Int(1)],
                );
                let (yes, no) = self.branch(some);
                let join = self.block_new();
                self.cur = yes;
                set(
                    self,
                    Self::variant_value(&ds, "Some", vec![Operand::Local(best)]),
                );
                self.goto(join);
                self.cur = no;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
            }
            id::STD_ITER_OPS_MAP => {
                let Some(d) = &dst else {
                    self.drop_taken_source(id, &e, &v);
                    return Ok(());
                };
                let Shape::List(u) = d.shape.clone() else {
                    return unsupported(format!("call of `{name}`"));
                };
                let f = self.atom(args[1].1, &Shape::Fn(vec![e.clone()], u.clone()))?;
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(u.width() as i128)],
                );
                self.each(rem, |me, i| {
                    let x = item(me, i)?;
                    let r = me.call_closure(&f, &[(&e, &x)], &u);
                    me.glue(&e, &x);
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), r);
                    Ok(())
                })?;
                advance(self);
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqLimit,
                    args: vec![out.clone(), Operand::Int(0)],
                });
                set(self, vec![out]);
            }
            id::STD_ITER_OPS_FILTER | id::STD_ITER_OPS_TAKE_WHILE | id::STD_ITER_OPS_DROP_WHILE => {
                let f = self.atom(
                    args[1].1,
                    &Shape::Fn(vec![e.clone()], Box::new(boolean.clone())),
                )?;
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                // `on`: the predicate still runs (`take_while` until it fails, `drop_while`
                // while it holds); `filter` always runs it.
                let on = self.local(I64);
                self.emit(Inst::Copy {
                    dst: on,
                    src: Operand::Int(1),
                });
                self.each(rem, |me, i| {
                    let x = item(me, i)?;
                    let join = me.block_new();
                    let live =
                        me.scalar_op(ScalarOp::Eq, I64, vec![Operand::Local(on), Operand::Int(1)]);
                    let (run, idle) = me.branch(live);
                    me.cur = idle;
                    // The predicate is off: `take_while` is done, `drop_while` keeps everything.
                    if id == id::STD_ITER_OPS_DROP_WHILE {
                        let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                        me.write(&At::Mem(slot, 0), x.clone());
                    } else {
                        me.glue(&e, &x);
                    }
                    me.goto(join);
                    me.cur = run;
                    let r = me.call_closure(&f, &[(&e, &x)], &boolean);
                    let (pass, fail) = me.branch(r[0].clone());
                    me.cur = pass;
                    if id == id::STD_ITER_OPS_DROP_WHILE {
                        me.glue(&e, &x);
                    } else {
                        let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                        me.write(&At::Mem(slot, 0), x.clone());
                    }
                    me.goto(join);
                    me.cur = fail;
                    match id {
                        id::STD_ITER_OPS_FILTER => me.glue(&e, &x),
                        id::STD_ITER_OPS_TAKE_WHILE => {
                            me.emit(Inst::Copy {
                                dst: on,
                                src: Operand::Int(0),
                            });
                            me.glue(&e, &x);
                        }
                        _ => {
                            me.emit(Inst::Copy {
                                dst: on,
                                src: Operand::Int(0),
                            });
                            let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                            me.write(&At::Mem(slot, 0), x.clone());
                        }
                    }
                    me.goto(join);
                    Ok(())
                })?;
                advance(self);
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqLimit,
                    args: vec![out.clone(), Operand::Int(0)],
                });
                set(self, vec![out]);
            }
            id::STD_ITER_OPS_FOLD => {
                let (init, acc_shape) = self.atom_any(args[1].1)?;
                if Self::has_heap(&acc_shape) || Self::has_heap(&e) {
                    return unsupported(format!("call of `{name}` on this type"));
                }
                let f = self.atom(
                    args[2].1,
                    &Shape::Fn(
                        vec![acc_shape.clone(), e.clone()],
                        Box::new(acc_shape.clone()),
                    ),
                )?;
                let acc = self.var(acc_shape.clone());
                self.copy_to(&acc.locals, init);
                self.each(rem, |me, i| {
                    let x = item(me, i)?;
                    let cur = Self::ops(&acc);
                    let r = me.call_closure(&f, &[(&acc_shape, &cur), (&e, &x)], &acc_shape);
                    me.copy_to(&acc.locals, r);
                    Ok(())
                })?;
                advance(self);
                set(self, Self::ops(&acc));
            }
            id::STD_ITER_OPS_ANY | id::STD_ITER_OPS_ALL | id::STD_ITER_OPS_FIND => {
                let f = self.atom(
                    args[1].1,
                    &Shape::Fn(vec![e.clone()], Box::new(boolean.clone())),
                )?;
                let find = id == id::STD_ITER_OPS_FIND;
                let all = id == id::STD_ITER_OPS_ALL;
                let found = self.local(I64);
                self.emit(Inst::Copy {
                    dst: found,
                    src: Operand::Int(0),
                });
                let hit = self.var(e.clone());
                self.each(rem, |me, i| {
                    let x = item(me, i)?;
                    let join = me.block_new();
                    let open = me.scalar_op(
                        ScalarOp::Eq,
                        I64,
                        vec![Operand::Local(found), Operand::Int(0)],
                    );
                    let (run, idle) = me.branch(open);
                    me.cur = idle;
                    me.glue(&e, &x);
                    me.goto(join);
                    me.cur = run;
                    let r = me.call_closure(&f, &[(&e, &x)], &boolean);
                    // `all` stops on a false, `any` and `find` on a true.
                    let (yes, no) = me.branch(r[0].clone());
                    let (stop, go) = if all { (no, yes) } else { (yes, no) };
                    me.cur = stop;
                    me.emit(Inst::Copy {
                        dst: found,
                        src: Operand::Int(1),
                    });
                    if find {
                        me.copy_to(&hit.locals, x.clone());
                    } else {
                        me.glue(&e, &x);
                    }
                    me.goto(join);
                    me.cur = go;
                    me.glue(&e, &x);
                    me.goto(join);
                    Ok(())
                })?;
                advance(self);
                let hit_any = self.scalar_op(
                    ScalarOp::Eq,
                    I64,
                    vec![Operand::Local(found), Operand::Int(1)],
                );
                if find {
                    let Some(d) = &dst else {
                        return Ok(());
                    };
                    let ds = d.shape.clone();
                    let (yes, no) = self.branch(hit_any);
                    let join = self.block_new();
                    self.cur = yes;
                    set(self, Self::variant_value(&ds, "Some", Self::ops(&hit)));
                    self.goto(join);
                    self.cur = no;
                    set(self, Self::variant_value(&ds, "None", vec![]));
                    self.goto(join);
                } else if all {
                    let r = self.scalar_op(ScalarOp::Not, Ty::Bool, vec![hit_any]);
                    set(self, vec![r]);
                } else {
                    set(self, vec![hit_any]);
                }
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        self.drop_taken_source(id, &e, &v);
        Ok(())
    }

    /// The adapters whose source is `take self` own it: free it once the result is built.
    fn drop_taken_source(&mut self, id: registry::Id, e: &Shape, v: &[Operand]) {
        if [
            id::STD_ITER_OPS_MAP,
            id::STD_ITER_OPS_FILTER,
            id::STD_ITER_OPS_TAKE_WHILE,
            id::STD_ITER_OPS_DROP_WHILE,
        ]
        .contains(&id)
        {
            self.glue(&Shape::List(Box::new(e.clone())), v);
        }
    }

    /// `f(args)` for the closure value `f` (code, environment); the arguments stay the caller's.
    /// `min(a, b)` and `max(a, b)`, with an optional `key` returning an `int`; `b` wins only when
    /// strictly better.
    pub(super) fn prelude_extreme(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let (Some(dst), [(_, a), (_, b), (_, key)]) = (dst, args.as_slice()) else {
            return unsupported(format!("call of `{name}`"));
        };
        let s = dst.shape.clone();
        if !matches!(s, Shape::Scalar(_)) {
            return unsupported(format!("call of `{name}` on this type"));
        }
        let (xa, xb) = (self.atom(a, &s)?, self.atom(b, &s)?);
        let (ka, kb) = if matches!(key, y::Atom::Name(n, _) if n == "None") {
            if !matches!(&s, Shape::Scalar(t) if *t == I64) {
                return unsupported(format!("call of `{name}` on this type"));
            }
            (xa[0].clone(), xb[0].clone())
        } else {
            let f = self.key_closure(key)?;
            let i = Shape::Scalar(I64);
            (
                self.call_closure(&f, &[(&s, &xa)], &i)[0].clone(),
                self.call_closure(&f, &[(&s, &xb)], &i)[0].clone(),
            )
        };
        let cmp = if id == id::STD_PRELUDE_MIN {
            ScalarOp::Ge
        } else {
            ScalarOp::Le
        };
        let keep_a = self.scalar_op(cmp, I64, vec![kb, ka]);
        let (a_wins, b_wins) = self.branch(keep_a);
        let join = self.block_new();
        let locals = dst.locals.clone();
        self.cur = a_wins;
        self.copy_to(&locals, xa);
        self.goto(join);
        self.cur = b_wins;
        self.copy_to(&locals, xb);
        self.goto(join);
        self.cur = join;
        Ok(())
    }

    /// The closure inside a `key: Callable | None` argument; a run-time `None` panics `value`.
    fn key_closure(&mut self, key: &y::Atom) -> R<Vec<Operand>> {
        Ok(self.key_fn(key)?.0)
    }

    /// As `key_closure`, with the shape of the keys it returns.
    pub(super) fn key_fn(&mut self, key: &y::Atom) -> R<(Vec<Operand>, Shape)> {
        let (v, shape) = self.atom_any(key)?;
        let Some((tag, at, fs)) = shape.variant("Some") else {
            return unsupported("a `key` argument of this type");
        };
        let Some((_, Shape::Fn(_, ret))) = fs.first() else {
            return unsupported("a `key` argument of this type");
        };
        let ret = (**ret).clone();
        let some = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(tag)]);
        let (go, bad) = self.branch(some);
        self.cur = bad;
        self.finish(Term::Panic("value"));
        self.cur = go;
        Ok((v[at..at + 2].to_vec(), ret))
    }

    pub(super) fn call_closure(
        &mut self,
        f: &[Operand],
        args: &[(&Shape, &Vec<Operand>)],
        ret: &Shape,
    ) -> Vec<Operand> {
        let mut ops = vec![f[1].clone()];
        let mut params = vec![Ty::Ptr];
        for (s, v) in args {
            ops.extend((*v).iter().cloned());
            params.extend(s.tys());
        }
        let d = self.var(ret.clone());
        self.emit(Inst::CallIndirect {
            dst: d.locals.clone(),
            code: f[0].clone(),
            args: ops,
            params,
            ret: ret.tys(),
        });
        Self::ops(&d)
    }
}
