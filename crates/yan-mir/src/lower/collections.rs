use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn deque_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        if id == id::STD_PRELUDE_DEQUE {
            let [y::TArg::Type(t)] = c.targs.as_slice() else {
                return unsupported("`deque` without its element type");
            };
            let e = self.shape(t)?;
            let (src, limit) = match args.as_slice() {
                [] => (None, None),
                [a] => (Some(*a), None),
                [a, m] => (Some(*a), Some(m.1)),
                _ => return unsupported("`deque` with extra arguments"),
            };
            let l = match src {
                Some((conv, a)) => {
                    let (_, s) = self.atom_any(a)?;
                    if !matches!(s, Shape::List(_)) {
                        return unsupported("`deque` of a non-list");
                    }
                    self.owned(&conv, a, &s)?.remove(0)
                }
                None => self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                ),
            };
            let l = match limit {
                Some(m) => {
                    let m = self.scalar1(m)?;
                    let d = self.rt(
                        RtFn::ListNew,
                        Ty::Ptr,
                        vec![Operand::Int(e.width() as i128)],
                    );
                    self.emit(Inst::Rt {
                        dst: None,
                        f: RtFn::DqLimit,
                        args: vec![d.clone(), m],
                    });
                    let out = self.rt(RtFn::DqExtend, Ty::Ptr, vec![d.clone(), l]);
                    self.glue(&Shape::List(Box::new(e)), &[out]);
                    d
                }
                None => l,
            };
            set(self, vec![l]);
            return Ok(());
        }
        let Some((_, h)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (mut v, s) = self.atom_any(h)?;
        let Shape::Deque(e) = s else {
            return unsupported("a `deque` call on a non-deque");
        };
        let l = v.remove(0);
        let w = e.width() as i128;
        match (id, &args[1..]) {
            (id::STD_COLLECTIONS_DEQUE_LEN, []) => {
                let r = self.rt(RtFn::ListLen, I64, vec![l]);
                set(self, vec![r]);
            }
            (
                id::STD_COLLECTIONS_DEQUE_APPEND | id::STD_COLLECTIONS_DEQUE_APPENDLEFT,
                [(xc, x)],
            ) => {
                let front = i128::from(id == id::STD_COLLECTIONS_DEQUE_APPENDLEFT);
                let x = self.owned(xc, x, &e)?;
                let xb = self.key_buf(x);
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let evicted = self.rt(
                    RtFn::DqPush,
                    I64,
                    vec![l, Operand::Int(front), xb.clone(), out.clone()],
                );
                if Self::has_heap(&e) {
                    let (drop_it, join) = (self.block_new(), self.block_new());
                    let none = self.scalar_op(ScalarOp::Eq, I64, vec![evicted, Operand::Int(0)]);
                    let (skip, evict) = self.branch(none);
                    self.cur = skip;
                    self.goto(join);
                    self.cur = evict;
                    self.goto(drop_it);
                    self.cur = drop_it;
                    let old = self.read(&At::Mem(out.clone(), 0), &e);
                    self.glue(&e, &old);
                    self.goto(join);
                }
                self.free(RtFn::EnvFree, xb);
                self.free(RtFn::EnvFree, out);
            }
            (id::STD_COLLECTIONS_DEQUE_POP | id::STD_COLLECTIONS_DEQUE_POPLEFT, []) => {
                let front = i128::from(id == id::STD_COLLECTIONS_DEQUE_POPLEFT);
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let ok = self.rt(RtFn::DqPop, I64, vec![l, Operand::Int(front), out.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![ok, Operand::Int(0)]);
                let (bad, good) = self.branch(empty);
                self.cur = bad;
                self.finish(Term::Panic("bounds"));
                self.cur = good;
                let v = self.read(&At::Mem(out.clone(), 0), &e);
                self.free(RtFn::EnvFree, out);
                set(self, v);
            }
            (id::STD_COLLECTIONS_DEQUE_FIRST | id::STD_COLLECTIONS_DEQUE_LAST, []) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![n.clone(), Operand::Int(0)]);
                let (none, some) = self.branch(empty);
                let join = self.block_new();
                self.cur = none;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = some;
                let i = if id == id::STD_COLLECTIONS_DEQUE_FIRST {
                    Operand::Int(0)
                } else {
                    self.scalar_op(ScalarOp::AddChecked, I64, vec![n, Operand::Int(-1)])
                };
                let p = self.rt(RtFn::ListAt, Ty::Ptr, vec![l, i]);
                let top = self.read(&At::Mem(p, 0), &e);
                let top = self.clone_value(top, &e)?;
                set(self, Self::variant_value(&ds, "Some", top));
                self.goto(join);
            }
            (id::STD_COLLECTIONS_DEQUE_EXTEND, [(xc, x)]) => {
                let (_, xs) = self.atom_any(x)?;
                if !matches!(xs, Shape::List(_)) {
                    return unsupported("`deque.extend` of a non-list");
                }
                let src = self.owned(xc, x, &xs)?.remove(0);
                let out = self.rt(RtFn::DqExtend, Ty::Ptr, vec![l, src]);
                self.glue(&Shape::List(e), &[out]);
            }
            (id::STD_COLLECTIONS_DEQUE_ROTATE, [(_, n)]) => {
                let n = self.scalar1(n)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqRotate,
                    args: vec![l, n],
                });
            }
            (id::STD_COLLECTIONS_DEQUE_CLEAR, []) => {
                let out = self.rt(RtFn::DqTake, Ty::Ptr, vec![l]);
                self.glue(&Shape::List(e), &[out]);
            }
            (id::STD_COLLECTIONS_DEQUE_MAXLEN, []) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let m = self.rt(RtFn::DqMaxlen, I64, vec![l]);
                let none = self.scalar_op(ScalarOp::Le, I64, vec![m.clone(), Operand::Int(-1)]);
                let (n, sm) = self.branch(none);
                let join = self.block_new();
                self.cur = n;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = sm;
                set(self, Self::variant_value(&ds, "Some", vec![m]));
                self.goto(join);
            }
            (id::STD_COLLECTIONS_DEQUE_CONTAINS, [(_, x)]) => {
                let x = self.atom(x, &e)?;
                let xb = self.key_buf(x);
                let mut schema = String::new();
                Self::key_schema(&e, &mut schema)?;
                let schema = self.lit(&schema);
                let r = self.rt(RtFn::DqContains, I64, vec![l, schema, xb.clone()]);
                self.free(RtFn::EnvFree, xb);
                let r = self.scalar_op(ScalarOp::Ne, I64, vec![r, Operand::Int(0)]);
                set(self, vec![r]);
            }
            _ => return unsupported(format!("call of `{}`", c.path.join("."))),
        }
        Ok(())
    }

    /// `std.itertools`, eagerly: the result is the list its `iter` would yield.
    pub(super) fn itertools_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<()> {
        let args = Self::value_args(c)?;
        let Some((_, xs_atom)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (xv, xs) = self.atom_any(xs_atom)?;
        let Shape::List(e) = xs else {
            return unsupported("`std.itertools` on a non-list");
        };
        let xs = xv[0].clone();
        let n = self.rt(RtFn::ListLen, I64, vec![xs.clone()]);
        let out = match id {
            id::STD_ITERTOOLS_PERMUTATIONS
            | id::STD_ITERTOOLS_COMBINATIONS
            | id::STD_ITERTOOLS_COMBINATIONS_WITH_REPLACEMENT => {
                let kind = match id {
                    id::STD_ITERTOOLS_PERMUTATIONS => select_kind::PERMUTATIONS,
                    id::STD_ITERTOOLS_COMBINATIONS => select_kind::COMBINATIONS,
                    _ => select_kind::WITH_REPLACEMENT,
                };
                let Some((_, r)) = args.get(1) else {
                    return unsupported("`std.itertools` without `r`");
                };
                let r = if id == id::STD_ITERTOOLS_PERMUTATIONS {
                    self.int_or(r, n.clone())?
                } else {
                    self.scalar1(r)?
                };
                self.selections(kind, &xs, &e, n, r)?
            }
            id::STD_ITERTOOLS_PRODUCT => {
                let Some((_, b)) = args.get(1) else {
                    return unsupported("`product` without a second list");
                };
                let (bv, bs) = self.atom_any(b)?;
                let Shape::List(f) = bs else {
                    return unsupported("`product` of a non-list");
                };
                let bl = bv[0].clone();
                let m = self.rt(RtFn::ListLen, I64, vec![bl.clone()]);
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int((e.width() + f.width()) as i128)],
                );
                self.each(n, |me, i| {
                    me.each(m.clone(), |me, j| {
                        let mut pair = me.element(&xs, i.clone(), &e)?;
                        pair.extend(me.element(&bl, j, &f)?);
                        let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                        me.write(&At::Mem(slot, 0), pair);
                        Ok(())
                    })
                })?;
                out
            }
            id::STD_ITERTOOLS_PAIRWISE => {
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int((2 * e.width()) as i128)],
                );
                let pairs = self.scalar_op(ScalarOp::SubChecked, I64, vec![n, Operand::Int(1)]);
                self.each(pairs, |me, i| {
                    let mut pair = me.element(&xs, i.clone(), &e)?;
                    let next = me.scalar_op(ScalarOp::AddChecked, I64, vec![i, Operand::Int(1)]);
                    pair.extend(me.element(&xs, next, &e)?);
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), pair);
                    Ok(())
                })?;
                out
            }
            _ => {
                let (Some((_, f)), Some((_, init))) = (args.get(1), args.get(2)) else {
                    return unsupported("`accumulate` without its defaults");
                };
                self.accumulate(&xs, n, &e, f, init)?
            }
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

    /// `accumulate(xs, f, initial)`: the running values, `f` or `+` combining them.
    pub(super) fn accumulate(
        &mut self,
        xs: &Operand,
        n: Operand,
        e: &Shape,
        f: &y::Atom,
        init: &y::Atom,
    ) -> R<Operand> {
        let option = |t: Shape| {
            Shape::Sum(
                "Option".into(),
                vec![
                    ("Some".into(), vec![("value".into(), t)]),
                    ("None".into(), vec![]),
                ],
            )
        };
        let fn_shape = Shape::Fn(vec![e.clone(), e.clone()], Box::new(e.clone()));
        let f = self.atom(f, &option(fn_shape))?;
        let init = self.atom(init, &option(e.clone()))?;
        let add = match e {
            Shape::Scalar(t) if *t == I64 => Some(ScalarOp::AddChecked),
            Shape::Scalar(Ty::F64) => Some(ScalarOp::Add),
            Shape::Scalar(Ty::Int(_)) => return unsupported("`accumulate` of this integer type"),
            _ => None,
        };
        let acc = self.var(e.clone());
        let started = self.local(I64);
        let out = self.rt(
            RtFn::ListNew,
            Ty::Ptr,
            vec![Operand::Int(e.width() as i128)],
        );
        let push = |me: &mut Self, v: Vec<Operand>| -> R<()> {
            let v = me.clone_value(v, e)?;
            let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
            me.write(&At::Mem(slot, 0), v);
            Ok(())
        };
        let has_init = self.scalar_op(ScalarOp::Eq, I64, vec![init[0].clone(), Operand::Int(0)]);
        let (yes, no) = self.branch(has_init);
        let join = self.block_new();
        self.cur = yes;
        let v = self.clone_value(init[1..].to_vec(), e)?;
        self.copy_to(&acc.locals, v);
        self.emit(Inst::Copy {
            dst: started,
            src: Operand::Int(1),
        });
        push(self, Self::ops(&acc))?;
        self.goto(join);
        self.cur = no;
        self.emit(Inst::Copy {
            dst: started,
            src: Operand::Int(0),
        });
        self.goto(join);
        self.each(n, |me, i| {
            let x = me.element(xs, i, e)?;
            let first = me.scalar_op(
                ScalarOp::Eq,
                I64,
                vec![Operand::Local(started), Operand::Int(0)],
            );
            let (first, rest) = me.branch(first);
            let next = me.block_new();
            me.cur = first;
            me.copy_to(&acc.locals, x.clone());
            me.emit(Inst::Copy {
                dst: started,
                src: Operand::Int(1),
            });
            me.goto(next);
            me.cur = rest;
            let cur = Self::ops(&acc);
            let has_f = me.scalar_op(ScalarOp::Eq, I64, vec![f[0].clone(), Operand::Int(0)]);
            let (call, plain) = me.branch(has_f);
            let joined = me.block_new();
            let r = me.var(e.clone());
            me.cur = call;
            let mut ops = vec![f[2].clone()];
            ops.extend(cur.clone());
            ops.extend(x.clone());
            let mut params = vec![Ty::Ptr];
            params.extend(e.tys());
            params.extend(e.tys());
            me.emit(Inst::CallIndirect {
                dst: r.locals.clone(),
                code: f[1].clone(),
                args: ops,
                params,
                ret: e.tys(),
            });
            me.goto(joined);
            me.cur = plain;
            match add {
                Some(op) => {
                    let ty = if op == ScalarOp::Add { Ty::F64 } else { I64 };
                    let sum = me.scalar_op(op, ty, vec![cur[0].clone(), x[0].clone()]);
                    me.copy_to(&r.locals, vec![sum]);
                    me.goto(joined);
                }
                None => {
                    me.finish(Term::Panic("value"));
                    me.cur = joined;
                }
            }
            me.glue(e, &cur);
            me.glue(e, &x);
            me.copy_to(&acc.locals, Self::ops(&r));
            me.goto(next);
            push(me, Self::ops(&acc))
        })?;
        if Self::has_heap(e) {
            let any = self.scalar_op(
                ScalarOp::Eq,
                I64,
                vec![Operand::Local(started), Operand::Int(1)],
            );
            let (yes, no) = self.branch(any);
            let done = self.block_new();
            self.cur = yes;
            self.glue(e, &Self::ops(&acc));
            self.goto(done);
            self.cur = no;
            self.goto(done);
        }
        Ok(out)
    }

    /// Element `i` of the list block `l`, cloned.
    pub(super) fn element(&mut self, l: &Operand, i: Operand, e: &Shape) -> R<Vec<Operand>> {
        let at = At::Mem(self.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i]), 0);
        let v = self.read(&at, e);
        self.clone_value(v, e)
    }

    /// The payload of an `int | None` argument, or `none` for the `None`.
    pub(super) fn int_or(&mut self, a: &y::Atom, none: Operand) -> R<Operand> {
        let want = Shape::Sum(
            "Union".into(),
            vec![
                ("int".into(), vec![("value".into(), Shape::Scalar(I64))]),
                ("None".into(), vec![]),
            ],
        );
        let v = self.atom(a, &want)?;
        let r = self.local(I64);
        let some = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
        let (yes, no) = self.branch(some);
        let join = self.block_new();
        self.cur = yes;
        self.emit(Inst::Copy {
            dst: r,
            src: v[1].clone(),
        });
        self.goto(join);
        self.cur = no;
        self.emit(Inst::Copy { dst: r, src: none });
        self.goto(join);
        Ok(Operand::Local(r))
    }

    /// The list of `r`-element lists of `xs` that `kind` selects.
    pub(super) fn selections(
        &mut self,
        kind: i64,
        xs: &Operand,
        e: &Shape,
        n: Operand,
        r: Operand,
    ) -> R<Operand> {
        let sel = self.rt(
            RtFn::Select,
            Ty::Ptr,
            vec![Operand::Int(kind as i128), n, r.clone()],
        );
        let count = {
            let at = At::Mem(
                self.rt(RtFn::ListAt, Ty::Ptr, vec![sel.clone(), Operand::Int(0)]),
                0,
            );
            self.read(&at, &Shape::Scalar(I64)).remove(0)
        };
        let out = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(1)]);
        self.each(count, |me, k| {
            let row = me.scalar_op(ScalarOp::MulChecked, I64, vec![k, r.clone()]);
            let row = me.scalar_op(ScalarOp::AddChecked, I64, vec![row, Operand::Int(1)]);
            let inner = me.rt(
                RtFn::ListNew,
                Ty::Ptr,
                vec![Operand::Int(e.width() as i128)],
            );
            me.each(r.clone(), |me, j| {
                let at = me.scalar_op(ScalarOp::AddChecked, I64, vec![row.clone(), j]);
                let p = me.rt(RtFn::ListAt, Ty::Ptr, vec![sel.clone(), at]);
                let idx = me.read(&At::Mem(p, 0), &Shape::Scalar(I64)).remove(0);
                let x = me.element(xs, idx, e)?;
                let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![inner.clone()]);
                me.write(&At::Mem(slot, 0), x);
                Ok(())
            })?;
            let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
            me.write(&At::Mem(slot, 0), vec![inner]);
            Ok(())
        })?;
        self.free(RtFn::ListFree, sel);
        Ok(out)
    }

    pub(super) fn heap_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let hrt = |me: &mut Self, f: RtFn, args: Vec<Operand>| {
            me.emit(Inst::Rt { dst: None, f, args });
        };
        if id == id::STD_PRELUDE_HEAP {
            let [y::TArg::Type(t)] = c.targs.as_slice() else {
                return unsupported("`heap` without its element type");
            };
            let e = self.shape(t)?;
            let mut schema = String::new();
            Self::key_schema(&e, &mut schema)?;
            let l = match args.as_slice() {
                [] => self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                ),
                [(conv, a)] => {
                    let (_, s) = self.atom_any(a)?;
                    if !matches!(s, Shape::List(_)) {
                        return unsupported("`heap` of a non-list");
                    }
                    let l = self.owned(conv, a, &s)?.remove(0);
                    let schema = self.lit(&schema);
                    hrt(self, RtFn::HeapFix, vec![l.clone(), schema]);
                    l
                }
                _ => return unsupported("`heap` with extra arguments"),
            };
            set(self, vec![l]);
            return Ok(());
        }
        let Some((_, h)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (mut v, s) = self.atom_any(h)?;
        let Shape::Heap(e) = s else {
            return unsupported("a `heap` call on a non-heap");
        };
        let l = v.remove(0);
        let mut schema = String::new();
        Self::key_schema(&e, &mut schema)?;
        let w = e.width() as i128;
        match (id, &args[1..]) {
            (id::STD_COLLECTIONS_HEAP_LEN, []) => {
                let r = self.rt(RtFn::ListLen, I64, vec![l]);
                set(self, vec![r]);
            }
            (id::STD_COLLECTIONS_HEAP_PUSH, [(xc, x)]) => {
                let x = self.owned(xc, x, &e)?;
                let at = self.rt(RtFn::ListPush, Ty::Ptr, vec![l.clone()]);
                self.write(&At::Mem(at, 0), x);
                let schema = self.lit(&schema);
                hrt(self, RtFn::HeapUp, vec![l, schema]);
            }
            (id::STD_COLLECTIONS_HEAP_POP, []) => {
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let schema = self.lit(&schema);
                let ok = self.rt(RtFn::HeapPop, I64, vec![l, schema, out.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![ok, Operand::Int(0)]);
                let (bad, good) = self.branch(empty);
                self.cur = bad;
                self.finish(Term::Panic("bounds"));
                self.cur = good;
                let v = self.read(&At::Mem(out.clone(), 0), &e);
                self.free(RtFn::EnvFree, out);
                set(self, v);
            }
            (id::STD_COLLECTIONS_HEAP_PEEK, []) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![n, Operand::Int(0)]);
                let (none, some) = self.branch(empty);
                let join = self.block_new();
                self.cur = none;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = some;
                let p = self.rt(RtFn::ListAt, Ty::Ptr, vec![l, Operand::Int(0)]);
                let top = self.read(&At::Mem(p, 0), &e);
                let top = self.clone_value(top, &e)?;
                set(self, Self::variant_value(&ds, "Some", top));
                self.goto(join);
            }
            (id::STD_COLLECTIONS_HEAP_PUSHPOP, [(xc, x)]) => {
                let x = self.owned(xc, x, &e)?;
                let xb = self.key_buf(x);
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let schema = self.lit(&schema);
                hrt(
                    self,
                    RtFn::HeapPushPop,
                    vec![l, schema, xb.clone(), out.clone()],
                );
                let v = self.read(&At::Mem(out.clone(), 0), &e);
                self.free(RtFn::EnvFree, xb);
                self.free(RtFn::EnvFree, out);
                set(self, v);
            }
            (id::STD_COLLECTIONS_HEAP_INTO_SORTED, []) => {
                let (conv, h) = args[0];
                let l = self.owned(&conv, h, &Shape::Heap(e.clone()))?.remove(0);
                let schema = self.lit(&schema);
                hrt(self, RtFn::HeapSort, vec![l.clone(), schema]);
                set(self, vec![l]);
            }
            _ => return unsupported(format!("call of `{}`", c.path.join("."))),
        }
        Ok(())
    }

    /// `set(xs)`: a map block of the distinct elements, keys only, in first-seen order.
    pub(super) fn set_of(&mut self, conv: &y::Conv, a: &y::Atom) -> R<Operand> {
        let (_, s) = self.atom_any(a)?;
        let Shape::List(e) = &s else {
            return unsupported("`set` of a non-list");
        };
        let mut schema = String::new();
        Self::key_schema(e, &mut schema)?;
        let schema = self.lit(&schema);
        let m = self.rt(
            RtFn::MapNew,
            Ty::Ptr,
            vec![Operand::Int(e.width() as i128), Operand::Int(0), schema],
        );
        let xs = self.atom(a, &s)?[0].clone();
        let n = self.rt(RtFn::ListLen, I64, vec![xs.clone()]);
        self.each(n, |me, i| {
            let x = me.element(&xs, i, e)?;
            let buf = me.key_buf(x.clone());
            let p = me.rt(RtFn::MapFind, Ty::Ptr, vec![m.clone(), buf.clone()]);
            let missing = me.scalar_op(ScalarOp::Eq, I64, vec![p, Operand::Int(0)]);
            let (new, seen) = me.branch(missing);
            let join = me.block_new();
            me.cur = new;
            me.rt(RtFn::MapPush, Ty::Ptr, vec![m.clone(), buf.clone()]);
            me.goto(join);
            me.cur = seen;
            me.glue(e, &x);
            me.goto(join);
            me.cur = join;
            me.free(RtFn::EnvFree, buf);
            Ok(())
        })?;
        if matches!(conv, y::Conv::Sink) {
            self.glue(&s, &[xs]);
        }
        Ok(m)
    }

    pub(super) fn dict_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        if id == id::STD_DICT_NEW {
            let [y::TArg::Type(k), y::TArg::Type(e)] = c.targs.as_slice() else {
                return unsupported("`std.dict.new` without its key and value types");
            };
            let (k, e) = (self.shape(k)?, self.shape(e)?);
            let mut schema = String::new();
            Self::key_schema(&k, &mut schema)?;
            let schema = self.lit(&schema);
            let r = self.rt(
                RtFn::MapNew,
                Ty::Ptr,
                vec![
                    Operand::Int(k.width() as i128),
                    Operand::Int(e.width() as i128),
                    schema,
                ],
            );
            set(self, vec![r]);
            return Ok(());
        }
        let Some((_, m)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (m, k, e) = self.map_arg(m)?;
        let kw = k.width() as u32;
        match (id, &args[1..]) {
            (id::STD_DICT_LEN, []) => {
                let r = self.rt(RtFn::MapLen, I64, vec![m]);
                set(self, vec![r]);
            }
            (id::STD_DICT_CONTAINS_KEY, [(_, key)]) => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let r = self.scalar_op(ScalarOp::Ne, I64, vec![p, Operand::Int(0)]);
                set(self, vec![r]);
            }
            (id::STD_DICT_INDEX, [(_, key)]) => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let p = self.rt(RtFn::MapIndex, Ty::Ptr, vec![m, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let v = self.read(&At::Mem(p, 0), &e);
                let v = self.clone_value(v, &e)?;
                set(self, v);
            }
            (id::STD_DICT_GET, [(_, key)]) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (none, some) = self.branch(missing);
                let join = self.block_new();
                self.cur = none;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = some;
                let v = self.read(&At::Mem(p, 0), &e);
                let v = self.clone_value(v, &e)?;
                set(self, Self::variant_value(&ds, "Some", v));
                self.goto(join);
            }
            (id::STD_DICT_INSERT | id::STD_DICT_SETDEFAULT, [(kc, key), (vc, val)]) => {
                let key = self.owned(kc, key, &k)?;
                let val = self.owned(vc, val, &e)?;
                let buf = self.key_buf(key.clone());
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m.clone(), buf.clone()]);
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (new, old) = self.branch(missing);
                let join = self.block_new();
                self.cur = new;
                let q = self.rt(RtFn::MapPush, Ty::Ptr, vec![m, buf.clone()]);
                self.write(&At::Mem(q.clone(), 0), val.clone());
                if id == id::STD_DICT_SETDEFAULT {
                    let v = self.read(&At::Mem(q, 0), &e);
                    let v = self.clone_value(v, &e)?;
                    set(self, v);
                }
                self.goto(join);
                self.cur = old;
                self.glue(&k, &key);
                if id == id::STD_DICT_SETDEFAULT {
                    self.glue(&e, &val);
                    let v = self.read(&At::Mem(p, 0), &e);
                    let v = self.clone_value(v, &e)?;
                    set(self, v);
                } else {
                    let prev = self.read(&At::Mem(p.clone(), 0), &e);
                    self.glue(&e, &prev);
                    self.write(&At::Mem(p, 0), val);
                }
                self.goto(join);
                self.free(RtFn::EnvFree, buf);
            }
            (id::STD_DICT_REMOVE, [(_, key)]) => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let out = self.rt(
                    RtFn::Alloc,
                    Ty::Ptr,
                    vec![Operand::Int(i128::from(kw) + e.width() as i128)],
                );
                let r = self.rt(RtFn::MapRemove, I64, vec![m, buf.clone(), out.clone()]);
                self.free(RtFn::EnvFree, buf);
                let ds = dst.as_ref().map(|d| d.shape.clone());
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![r, Operand::Int(0)]);
                let (none, some) = self.branch(missing);
                let join = self.block_new();
                self.cur = none;
                if let Some(ds) = &ds {
                    set(self, Self::variant_value(ds, "None", vec![]));
                }
                self.goto(join);
                self.cur = some;
                let kv = self.read(&At::Mem(out.clone(), 0), &k);
                self.glue(&k, &kv);
                let v = self.read(&At::Mem(out.clone(), kw), &e);
                match &ds {
                    Some(ds) => set(self, Self::variant_value(ds, "Some", v)),
                    None => self.glue(&e, &v),
                }
                self.goto(join);
                self.free(RtFn::EnvFree, out);
            }
            (id::STD_DICT_ITEMS | id::STD_DICT_KEYS | id::STD_DICT_VALUES, []) => {
                let (take_k, take_v) = match id {
                    id::STD_DICT_ITEMS => (true, true),
                    id::STD_DICT_KEYS => (true, false),
                    _ => (false, true),
                };
                let words = if take_k { k.width() } else { 0 } + if take_v { e.width() } else { 0 };
                let l = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(words as i128)]);
                let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
                self.each(n, |me, i| {
                    let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
                    let mut x = Vec::new();
                    if take_k {
                        let kv = me.read(&At::Mem(at.clone(), 0), &k);
                        x.extend(me.clone_value(kv, &k)?);
                    }
                    if take_v {
                        let ev = me.read(&At::Mem(at, kw), &e);
                        x.extend(me.clone_value(ev, &e)?);
                    }
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![l.clone()]);
                    me.write(&At::Mem(slot, 0), x);
                    Ok(())
                })?;
                set(self, vec![l]);
            }
            _ => return unsupported(format!("call of `{}`", c.path.join("."))),
        }
        Ok(())
    }
}
