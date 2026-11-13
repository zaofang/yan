//! `sort`, `sort_unstable`, `reverse` and `sorted` without a key function (17.3, R494, R496):
//! the list block ordered by its element schema, as `heap` is.

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn ordering_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let keyed = |a: &y::Atom| !matches!(a, y::Atom::Name(n, _) if n == "None");
        match (id, args.as_slice()) {
            (id::STD_LIST_REVERSE, [(_, y::Atom::Name(l, _))]) => {
                let list = self.name(l)?.clone();
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ListReverse,
                    args: Self::ops(&list),
                });
            }
            // Equal elements are identical without a key, so stability is not observable.
            (
                id::STD_LIST_SORT,
                [
                    (_, y::Atom::Name(l, _)),
                    (_, key),
                    (_, y::Atom::Lit(y::Literal::Bool(rev), _)),
                ],
            ) if !keyed(key) => {
                let list = self.name(l)?.clone();
                self.sort_list(&list.shape, Self::ops(&list)[0].clone(), *rev)?;
            }
            (id::STD_LIST_SORT_UNSTABLE, [(_, y::Atom::Name(l, _)), (_, key)]) if !keyed(key) => {
                let list = self.name(l)?.clone();
                self.sort_list(&list.shape, Self::ops(&list)[0].clone(), false)?;
            }
            (
                id::STD_PRELUDE_SORTED,
                [
                    (_, xs),
                    (_, key),
                    (_, y::Atom::Lit(y::Literal::Bool(rev), _)),
                ],
            ) if !keyed(key) => {
                let (_, s) = self.atom_any(xs)?;
                let v = self.atom(xs, &s)?;
                let copy = self.rt(RtFn::ListCopy, Ty::Ptr, v);
                if let Shape::List(e) = &s
                    && Self::has_heap(e)
                {
                    let n = self.rt(RtFn::ListLen, I64, vec![copy.clone()]);
                    self.each(n, |me, i| {
                        let at = At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![copy.clone(), i]), 0);
                        let ev = me.read(&at, e);
                        let c = me.clone_value(ev, e)?;
                        me.write(&at, c);
                        Ok(())
                    })?;
                }
                self.sort_list(&s, copy.clone(), *rev)?;
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), vec![copy]);
                }
            }
            (id::STD_LIST_INDEX, [(_, a), (_, x)]) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let (ds, locals) = (d.shape.clone(), d.locals.clone());
                let (l, s) = self.atom_any(a)?;
                let Shape::List(e) = &s else {
                    return unsupported(format!("call of `{name}`"));
                };
                let x = self.atom(x, e)?;
                let list = l[0].clone();
                let n = self.rt(RtFn::ListLen, I64, vec![list.clone()]);
                let found = self.local(I64);
                self.emit(Inst::Copy {
                    dst: found,
                    src: Operand::Int(-1),
                });
                self.each(n, |me, i| {
                    let open = me.scalar_op(
                        ScalarOp::Eq,
                        I64,
                        vec![Operand::Local(found), Operand::Int(-1)],
                    );
                    let (look, skip) = me.branch(open);
                    let join = me.block_new();
                    me.cur = look;
                    let at = At::Mem(
                        me.rt(RtFn::ListAt, Ty::Ptr, vec![list.clone(), i.clone()]),
                        0,
                    );
                    let ev = me.read(&at, e);
                    let hit = me.key_eq(e, ev, x.clone(), "index")?;
                    let (yes, no) = me.branch(hit);
                    me.cur = yes;
                    me.emit(Inst::Copy { dst: found, src: i });
                    me.goto(join);
                    me.cur = no;
                    me.goto(join);
                    me.cur = skip;
                    me.goto(join);
                    me.cur = join;
                    Ok(())
                })?;
                let miss = self.scalar_op(
                    ScalarOp::Eq,
                    I64,
                    vec![Operand::Local(found), Operand::Int(-1)],
                );
                let (none, some) = self.branch(miss);
                let join = self.block_new();
                self.cur = none;
                self.copy_to(&locals, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = some;
                self.copy_to(
                    &locals,
                    Self::variant_value(&ds, "Some", vec![Operand::Local(found)]),
                );
                self.goto(join);
            }
            (id::STD_LIST_BISECT_LEFT | id::STD_LIST_BISECT_RIGHT, [(_, a), (_, x), (_, key)])
                if !keyed(key) =>
            {
                let (l, s) = self.atom_any(a)?;
                let Shape::List(e) = &s else {
                    return unsupported(format!("call of `{name}`"));
                };
                let x = self.atom(x, e)?;
                let right = id == id::STD_LIST_BISECT_RIGHT;
                let at = self.bisect(&name, l[0].clone(), e, x, right, None)?;
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), vec![at]);
                }
            }
            (id::STD_LIST_INSORT, [(_, y::Atom::Name(l, _)), (_, x), (_, key)]) if !keyed(key) => {
                let list = self.name(l)?.clone();
                let Shape::List(e) = &list.shape else {
                    return unsupported(format!("call of `{name}`"));
                };
                let v = self.atom(x, e)?;
                let ops = Self::ops(&list);
                let at = self.bisect(&name, ops[0].clone(), e, v.clone(), true, None)?;
                let mut args = ops;
                args.push(at);
                let slot = self.rt(RtFn::ListInsert, Ty::Ptr, args);
                self.write(&At::Mem(slot, 0), v);
            }
            (id::STD_LIST_BISECT_LEFT | id::STD_LIST_BISECT_RIGHT, [(_, a), (_, x), (_, key)]) => {
                let (l, s) = self.atom_any(a)?;
                let Shape::List(e) = &s else {
                    return unsupported(format!("call of `{name}`"));
                };
                let kf = self.key_fn(key)?;
                let x = self.atom(x, &kf.1)?;
                let right = id == id::STD_LIST_BISECT_RIGHT;
                let at = self.bisect(&name, l[0].clone(), e, x, right, Some(&kf))?;
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), vec![at]);
                }
            }
            (id::STD_LIST_INSORT, [(_, y::Atom::Name(l, _)), (_, x), (_, key)]) => {
                let list = self.name(l)?.clone();
                let Shape::List(e) = &list.shape else {
                    return unsupported(format!("call of `{name}`"));
                };
                let kf = self.key_fn(key)?;
                let v = self.atom(x, e)?;
                let k = self.call_closure(&kf.0, &[(e, &v)], &kf.1);
                let ops = Self::ops(&list);
                let at = self.bisect(&name, ops[0].clone(), e, k.clone(), true, Some(&kf))?;
                self.glue(&kf.1, &k);
                let mut args = ops;
                args.push(at);
                let slot = self.rt(RtFn::ListInsert, Ty::Ptr, args);
                self.write(&At::Mem(slot, 0), v);
            }
            (
                id::STD_LIST_SORT,
                [
                    (_, y::Atom::Name(l, _)),
                    (_, key),
                    (_, y::Atom::Lit(y::Literal::Bool(rev), _)),
                ],
            ) => {
                let list = self.name(l)?.clone();
                let Shape::List(e) = &list.shape else {
                    return unsupported(format!("call of `{name}`"));
                };
                let kf = self.key_fn(key)?;
                self.sort_keyed(&name, Self::ops(&list)[0].clone(), e, &kf, *rev)?;
            }
            (id::STD_LIST_SORT_UNSTABLE, [(_, y::Atom::Name(l, _)), (_, key)]) => {
                let list = self.name(l)?.clone();
                let Shape::List(e) = &list.shape else {
                    return unsupported(format!("call of `{name}`"));
                };
                let kf = self.key_fn(key)?;
                self.sort_keyed(&name, Self::ops(&list)[0].clone(), e, &kf, false)?;
            }
            (
                id::STD_PRELUDE_SORTED,
                [
                    (_, xs),
                    (_, key),
                    (_, y::Atom::Lit(y::Literal::Bool(rev), _)),
                ],
            ) => {
                let (_, s) = self.atom_any(xs)?;
                let Shape::List(e) = &s else {
                    return unsupported(format!("call of `{name}` on this iterable"));
                };
                let kf = self.key_fn(key)?;
                let v = self.atom(xs, &s)?;
                let copy = self.rt(RtFn::ListCopy, Ty::Ptr, v);
                if Self::has_heap(e) {
                    let n = self.rt(RtFn::ListLen, I64, vec![copy.clone()]);
                    self.each(n, |me, i| {
                        let at = At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![copy.clone(), i]), 0);
                        let ev = me.read(&at, e);
                        let c = me.clone_value(ev, e)?;
                        me.write(&at, c);
                        Ok(())
                    })?;
                }
                self.sort_keyed(&name, copy.clone(), e, &kf, *rev)?;
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), vec![copy]);
                }
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }

    /// A stable insertion sort of list block `l` by `key(element)`, comparing keys by schema
    /// order; with `reverse`, equal keys still keep their order. The key closure runs on each
    /// comparison, and each key value is freed after it.
    fn sort_keyed(
        &mut self,
        name: &str,
        l: Operand,
        e: &Shape,
        key: &(Vec<Operand>, Shape),
        reverse: bool,
    ) -> R<()> {
        let ks = &key.1;
        let mut schema = String::new();
        if Self::key_schema(ks, &mut schema).is_err() {
            return unsupported(format!("call of `{name}` with this key type"));
        }
        let schema = self.lit(&schema);
        let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
        let j = self.local(I64);
        self.each(n, |me, i| {
            let at_i = At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i.clone()]), 0);
            let x = me.read(&at_i, e);
            let kx = me.call_closure(&key.0, &[(e, &x)], ks);
            me.emit(Inst::Copy {
                dst: j,
                src: i.clone(),
            });
            let head = me.block_new();
            me.goto(head);
            let more = me.scalar_op(ScalarOp::Ge, I64, vec![Operand::Local(j), Operand::Int(1)]);
            let (check, place) = me.branch(more);
            me.cur = check;
            let prev = me.scalar_op(
                ScalarOp::SubChecked,
                I64,
                vec![Operand::Local(j), Operand::Int(1)],
            );
            let at_p = At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), prev]), 0);
            let p = me.read(&at_p, e);
            let kp = me.call_closure(&key.0, &[(e, &p)], ks);
            let (pb, xb) = (me.key_buf(kp.clone()), me.key_buf(kx.clone()));
            let o = me.rt(
                RtFn::KeyCmp,
                I64,
                vec![schema.clone(), pb.clone(), xb.clone()],
            );
            me.free(RtFn::EnvFree, pb);
            me.free(RtFn::EnvFree, xb);
            me.glue(ks, &kp);
            // Shift while the previous key sorts after `x` (before it, when reversed).
            let shift = if reverse {
                me.scalar_op(ScalarOp::Le, I64, vec![o, Operand::Int(-1)])
            } else {
                me.scalar_op(ScalarOp::Ge, I64, vec![o, Operand::Int(1)])
            };
            let (go, stop) = me.branch(shift);
            me.cur = go;
            let at_j = At::Mem(
                me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), Operand::Local(j)]),
                0,
            );
            let p2 = me.read(&at_p, e);
            me.write(&at_j, p2);
            let down = me.scalar_op(
                ScalarOp::SubChecked,
                I64,
                vec![Operand::Local(j), Operand::Int(1)],
            );
            me.emit(Inst::Copy { dst: j, src: down });
            me.goto(head);
            me.cur = stop;
            me.goto(place);
            let at_j = At::Mem(
                me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), Operand::Local(j)]),
                0,
            );
            me.write(&at_j, x);
            me.glue(ks, &kx);
            Ok(())
        })
    }

    /// The first index of list block `l` whose element is not before `x` (`right`: not at or
    /// before), by binary search over the elements' schema order.
    pub(super) fn bisect(
        &mut self,
        name: &str,
        l: Operand,
        e: &Shape,
        x: Vec<Operand>,
        right: bool,
        key: Option<&(Vec<Operand>, Shape)>,
    ) -> R<Operand> {
        // `x` and the compared values have the key's shape when there is a key function.
        let ke = key.map_or(e, |(_, ks)| ks);
        let mut schema = String::new();
        if Self::key_schema(ke, &mut schema).is_err() {
            return unsupported(format!("call of `{name}` on this type"));
        }
        let schema = self.lit(&schema);
        let (lo, hi) = (self.local(I64), self.local(I64));
        self.emit(Inst::Copy {
            dst: lo,
            src: Operand::Int(0),
        });
        let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
        self.emit(Inst::Copy { dst: hi, src: n });
        let head = self.block_new();
        self.goto(head);
        let done = self.scalar_op(
            ScalarOp::Ge,
            I64,
            vec![Operand::Local(lo), Operand::Local(hi)],
        );
        let (exit, body) = self.branch(done);
        self.cur = body;
        let sum = self.scalar_op(
            ScalarOp::AddChecked,
            I64,
            vec![Operand::Local(lo), Operand::Local(hi)],
        );
        let mid = self.scalar_op(ScalarOp::FloorDiv, I64, vec![sum, Operand::Int(2)]);
        let at = At::Mem(
            self.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), mid.clone()]),
            0,
        );
        let el = self.read(&at, e);
        let m = match key {
            None => el,
            Some((f, ks)) => self.call_closure(f, &[(e, &el)], ks),
        };
        let (mb, xb) = (self.key_buf(m.clone()), self.key_buf(x.clone()));
        let o = self.rt(RtFn::KeyCmp, I64, vec![schema, mb.clone(), xb.clone()]);
        if key.is_some() {
            self.glue(ke, &m);
        }
        self.free(RtFn::EnvFree, mb);
        self.free(RtFn::EnvFree, xb);
        let bound = Operand::Int(if right { 0 } else { -1 });
        let before = self.scalar_op(ScalarOp::Le, I64, vec![o, bound]);
        let (lt, ge) = self.branch(before);
        self.cur = lt;
        let up = self.scalar_op(
            ScalarOp::AddChecked,
            I64,
            vec![mid.clone(), Operand::Int(1)],
        );
        self.emit(Inst::Copy { dst: lo, src: up });
        self.finish(Term::Jump(head));
        self.cur = ge;
        self.emit(Inst::Copy { dst: hi, src: mid });
        self.finish(Term::Jump(head));
        self.cur = exit;
        Ok(Operand::Local(lo))
    }

    /// `a == b` of a record, tuple or sum of scalars and strs: the runtime compares the two
    /// elements' slots by their schema, as `heap` orders them.
    pub(super) fn key_eq(
        &mut self,
        shape: &Shape,
        a: Vec<Operand>,
        b: Vec<Operand>,
        name: &str,
    ) -> R<Operand> {
        let mut schema = String::new();
        if Self::key_schema(shape, &mut schema).is_err() {
            return unsupported(format!("call of `{name}` on this type"));
        }
        let schema = self.lit(&schema);
        let (a, b) = (self.key_buf(a), self.key_buf(b));
        let r = self.rt(RtFn::KeyEq, Ty::Bool, vec![schema, a.clone(), b.clone()]);
        self.free(RtFn::EnvFree, a);
        self.free(RtFn::EnvFree, b);
        Ok(r)
    }

    fn sort_list(&mut self, shape: &Shape, list: Operand, reverse: bool) -> R<()> {
        let Shape::List(e) = shape else {
            return unsupported("a sort of a non-list");
        };
        let mut schema = String::new();
        Self::key_schema(e, &mut schema)?;
        let schema = self.lit(&schema);
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::HeapSort,
            args: vec![list.clone(), schema],
        });
        if reverse {
            self.emit(Inst::Rt {
                dst: None,
                f: RtFn::ListReverse,
                args: vec![list],
            });
        }
        Ok(())
    }
}
