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
            (id::STD_LIST_BISECT_LEFT | id::STD_LIST_BISECT_RIGHT, [(_, a), (_, x), (_, key)])
                if !keyed(key) =>
            {
                let (l, s) = self.atom_any(a)?;
                let Shape::List(e) = &s else {
                    return unsupported(format!("call of `{name}`"));
                };
                let x = self.atom(x, e)?;
                let right = id == id::STD_LIST_BISECT_RIGHT;
                let at = self.bisect(&name, l[0].clone(), e, x, right)?;
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
                let at = self.bisect(&name, ops[0].clone(), e, v.clone(), true)?;
                let mut args = ops;
                args.push(at);
                let slot = self.rt(RtFn::ListInsert, Ty::Ptr, args);
                self.write(&At::Mem(slot, 0), v);
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }

    /// The first index of list block `l` whose element is not before `x` (`right`: not at or
    /// before), by binary search over the elements' schema order.
    fn bisect(
        &mut self,
        name: &str,
        l: Operand,
        e: &Shape,
        x: Vec<Operand>,
        right: bool,
    ) -> R<Operand> {
        let mut schema = String::new();
        if Self::key_schema(e, &mut schema).is_err() {
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
        let m = self.read(&at, e);
        let (mb, xb) = (self.key_buf(m), self.key_buf(x.clone()));
        let o = self.rt(RtFn::KeyCmp, I64, vec![schema, mb.clone(), xb.clone()]);
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
