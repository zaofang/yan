//! `std.rand` in native code (RFC 0019): a `Rand` handler is its index in `yan_rt::rand`, and a
//! `Rand` capability parameter is a real parameter that carries it. Other capabilities are
//! ambient.

use super::*;

pub(super) const RAND_IDS: &[registry::Id] = &[
    id::STD_TEST_SEEDED,
    id::STD_RAND_SPLIT,
    id::STD_RAND_NEXT_U64,
    id::STD_RAND_NEXT_F64,
    id::STD_RAND_BOOL,
    id::STD_RAND_BELOW,
    id::STD_RAND_RANGE,
    id::STD_RAND_CHOICE,
    id::STD_RAND_SHUFFLE,
];

impl<'m> Fn<'_, 'm> {
    /// The handler a `std.rand` call draws from: its first capability argument.
    fn rand_handler(&mut self, c: &y::Call) -> R<Operand> {
        let Some(y::Arg::Cap(n, _)) = c.args.iter().find(|a| matches!(a, y::Arg::Cap(..))) else {
            return unsupported("a `std.rand` call without its handler");
        };
        Ok(Self::ops(self.name(n)?)[0].clone())
    }

    /// `n > 0`, else the `value` panic.
    fn positive(&mut self, n: &Operand) {
        let ok = self.scalar_op(ScalarOp::Ge, I64, vec![n.clone(), Operand::Int(1)]);
        let (go, bad) = self.branch(ok);
        self.cur = bad;
        self.finish(Term::Panic("value"));
        self.cur = go;
    }

    fn below(&mut self, h: &Operand, n: Operand) -> Operand {
        self.rt(RtFn::RandBelow, I64, vec![h.clone(), n])
    }

    pub(super) fn rand_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let name = c.path.join(".");
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        match (id, args.as_slice()) {
            (id::STD_TEST_SEEDED, [(_, seed), (_, alg)]) => {
                let seed = self.scalar1(seed)?;
                let tag = match alg {
                    y::Atom::Name(n, _) if n == "ChaCha8" => Operand::Int(0),
                    y::Atom::Name(n, _) if n == "Pcg64Dxsm" => Operand::Int(1),
                    a => self.atom_any(a)?.0[0].clone(),
                };
                let h = self.rt(RtFn::RandSeeded, I64, vec![seed, tag]);
                set(self, vec![h]);
            }
            (id::STD_RAND_SPLIT, [(_, k)]) => {
                let h = self.rand_handler(c)?;
                let k = self.scalar1(k)?;
                let b = self.rt(RtFn::RandSplit, I64, vec![h, k]);
                set(self, vec![b]);
            }
            (id::STD_RAND_NEXT_U64, []) => {
                let h = self.rand_handler(c)?;
                let x = self.rt(RtFn::RandNext, I64, vec![h]);
                set(self, vec![x]);
            }
            (id::STD_RAND_NEXT_F64, []) => {
                let h = self.rand_handler(c)?;
                let x = self.rt(RtFn::RandF64, Ty::F64, vec![h]);
                set(self, vec![x]);
            }
            (id::STD_RAND_BOOL, []) => {
                let h = self.rand_handler(c)?;
                let x = self.rt(RtFn::RandNext, I64, vec![h]);
                // The top bit: negative as a signed word.
                let pos = self.scalar_op(ScalarOp::Ge, I64, vec![x, Operand::Int(0)]);
                let b = self.scalar_op(ScalarOp::Not, Ty::Bool, vec![pos]);
                set(self, vec![b]);
            }
            (id::STD_RAND_BELOW, [(_, n)]) => {
                let h = self.rand_handler(c)?;
                let n = self.scalar1(n)?;
                self.positive(&n);
                let x = self.below(&h, n);
                set(self, vec![x]);
            }
            (id::STD_RAND_RANGE, [(_, lo), (_, hi)]) => {
                let h = self.rand_handler(c)?;
                let (lo, hi) = (self.scalar1(lo)?, self.scalar1(hi)?);
                let n = self.scalar_op(ScalarOp::SubChecked, I64, vec![hi, lo.clone()]);
                self.positive(&n);
                let x = self.below(&h, n);
                let x = self.scalar_op(ScalarOp::AddChecked, I64, vec![lo, x]);
                set(self, vec![x]);
            }
            (id::STD_RAND_CHOICE, [(_, xs)]) => {
                let h = self.rand_handler(c)?;
                let (l, s) = self.atom_any(xs)?;
                let Shape::List(e) = &s else {
                    return unsupported(format!("call of `{name}` on a non-list"));
                };
                let n = self.rt(RtFn::ListLen, I64, vec![l[0].clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![n.clone(), Operand::Int(0)]);
                let (bad, go) = self.branch(empty);
                self.cur = bad;
                self.finish(Term::Panic("bounds"));
                self.cur = go;
                let i = self.below(&h, n);
                let v = self.element(&l[0], i, e)?;
                set(self, v);
            }
            (id::STD_RAND_SHUFFLE, [(_, xs)]) => {
                let h = self.rand_handler(c)?;
                let (l, s) = self.atom_any(xs)?;
                let Shape::List(e) = &s else {
                    return unsupported(format!("call of `{name}` on a non-list"));
                };
                let list = l[0].clone();
                let n = self.rt(RtFn::ListLen, I64, vec![list.clone()]);
                let steps =
                    self.scalar_op(ScalarOp::SubChecked, I64, vec![n.clone(), Operand::Int(1)]);
                self.each(steps, |me, t| {
                    // `i` runs from the last index down to 1.
                    let last =
                        me.scalar_op(ScalarOp::SubChecked, I64, vec![n.clone(), Operand::Int(1)]);
                    let i = me.scalar_op(ScalarOp::SubChecked, I64, vec![last, t]);
                    let bound =
                        me.scalar_op(ScalarOp::AddChecked, I64, vec![i.clone(), Operand::Int(1)]);
                    let j = me.below(&h, bound);
                    let at = |me: &mut Self, k: Operand| {
                        At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![list.clone(), k]), 0)
                    };
                    let (ai, aj) = (at(me, i), at(me, j));
                    let (vi, vj) = (me.read(&ai, e), me.read(&aj, e));
                    me.write(&ai, vj);
                    me.write(&aj, vi);
                    Ok(())
                })?;
            }
            _ => return unsupported(format!("call of `{name}` with these arguments")),
        }
        Ok(())
    }

    /// `with rand = handler: body`: `name` is the handler in `body`.
    pub(super) fn with_rand(&mut self, name: &str, handler: &y::Atom, body: &'m y::Block) -> R<()> {
        let h = self.scalar1(handler)?;
        let var = self.var(Shape::Scalar(I64));
        self.copy_to(&var.locals.clone(), vec![h]);
        let outer = self.names.insert(name.to_owned(), var);
        if name == "rand" && !self.rands.iter().any(|n| n == name) {
            self.rands.push(name.to_owned());
        }
        self.block(body)?;
        match outer {
            Some(v) => self.names.insert(name.to_owned(), v),
            None => self.names.remove(name),
        };
        Ok(())
    }

    /// Bind each `Rand` handler in scope to the handler of branch `k` of a `par`; the result
    /// restores them.
    pub(super) fn split_rands(&mut self, k: usize) -> Vec<(String, Var)> {
        let mut saved = Vec::new();
        for n in self.rands.clone() {
            let Some(old) = self.names.get(&n).cloned() else {
                continue;
            };
            let b = self.rt(
                RtFn::RandSplit,
                I64,
                vec![Self::ops(&old)[0].clone(), Operand::Int(k as i128)],
            );
            let var = self.var(Shape::Scalar(I64));
            self.copy_to(&var.locals.clone(), vec![b]);
            self.names.insert(n.clone(), var);
            saved.push((n, old));
        }
        saved
    }

    pub(super) fn restore_rands(&mut self, saved: Vec<(String, Var)>) {
        for (n, v) in saved {
            self.names.insert(n, v);
        }
    }
}
