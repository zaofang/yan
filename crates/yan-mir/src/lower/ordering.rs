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
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
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
