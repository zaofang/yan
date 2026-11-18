//! `std.sync.atomic` (6.2) in native code: an `Atomic[T]` of an integer or `bool` is a block of
//! one word holding `T` at its start, and every operation is an `Inst::Atomic` on that block.

use super::*;

pub(super) const ATOMIC_IDS: &[registry::Id] = &[
    id::STD_SYNC_ATOMIC_ATOMIC,
    id::STD_SYNC_ATOMIC_LOAD,
    id::STD_SYNC_ATOMIC_STORE,
    id::STD_SYNC_ATOMIC_SWAP,
    id::STD_SYNC_ATOMIC_COMPARE_EXCHANGE,
    id::STD_SYNC_ATOMIC_COMPARE_EXCHANGE_WEAK,
    id::STD_SYNC_ATOMIC_FETCH_ADD,
    id::STD_SYNC_ATOMIC_FETCH_SUB,
    id::STD_SYNC_ATOMIC_FETCH_AND,
    id::STD_SYNC_ATOMIC_FETCH_OR,
    id::STD_SYNC_ATOMIC_FETCH_XOR,
    id::STD_SYNC_ATOMIC_FETCH_MAX,
    id::STD_SYNC_ATOMIC_FETCH_MIN,
    id::STD_SYNC_ATOMIC_INTO_INNER,
    id::STD_SYNC_ATOMIC_WAIT,
    id::STD_SYNC_ATOMIC_NOTIFY_ONE,
    id::STD_SYNC_ATOMIC_NOTIFY_ALL,
    id::STD_SYNC_ATOMIC_FENCE,
    id::STD_SYNC_ATOMIC_COMPILER_FENCE,
];

impl<'m> Fn<'_, 'm> {
    /// The ordering `a` holds when it was built by the variant constructor just above its use;
    /// `SeqCst`, which every backend may use for any ordering, when it cannot be told.
    fn mem_order(&mut self, a: &y::Atom) -> R<MemOrder> {
        if let y::Atom::Name(n, _) = a
            && !self.names.contains_key(n)
        {
            return Ok(Self::order_named(n));
        }
        let (ops, shape) = self.atom_any(a)?;
        let (Shape::Sum(_, vs), Some(Operand::Local(l))) = (&shape, ops.first()) else {
            return Ok(MemOrder::SeqCst);
        };
        let tag = self.blocks[self.cur as usize]
            .insts
            .iter()
            .rev()
            .find_map(|i| match i {
                Inst::Copy {
                    dst,
                    src: Operand::Int(k),
                } if dst == l => Some(*k),
                _ => None,
            });
        let name = tag
            .and_then(|k| vs.get(usize::try_from(k).ok()?))
            .map(|v| v.0.as_str());
        Ok(name.map_or(MemOrder::SeqCst, Self::order_named))
    }

    fn order_named(n: &str) -> MemOrder {
        match n {
            "Relaxed" => MemOrder::Relaxed,
            "Acquire" => MemOrder::Acquire,
            "Release" => MemOrder::Release,
            "AcqRel" => MemOrder::AcqRel,
            _ => MemOrder::SeqCst,
        }
    }

    pub(super) fn atomic_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        match (id, args.as_slice()) {
            (id::STD_SYNC_ATOMIC_FENCE, [(_, o)]) => {
                let order = self.mem_order(o)?;
                self.emit(Inst::Fence { order });
                return Ok(());
            }
            // The cooperative scheduler and the backends order everything; compiler fences
            // constrain only the optimizer, which has no atomics to move.
            (id::STD_SYNC_ATOMIC_COMPILER_FENCE, [_]) => return Ok(()),
            _ => {}
        }
        let Some(Shape::Scalar(ty @ (Ty::Int(_) | Ty::Bool))) = self.targ(c)? else {
            return unsupported("an `Atomic` of a type that is not an integer or `bool`");
        };
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let rmw = |id| match id {
            id::STD_SYNC_ATOMIC_FETCH_ADD => Some(AtomicOp::Add),
            id::STD_SYNC_ATOMIC_FETCH_SUB => Some(AtomicOp::Sub),
            id::STD_SYNC_ATOMIC_FETCH_AND => Some(AtomicOp::And),
            id::STD_SYNC_ATOMIC_FETCH_OR => Some(AtomicOp::Or),
            id::STD_SYNC_ATOMIC_FETCH_XOR => Some(AtomicOp::Xor),
            id::STD_SYNC_ATOMIC_FETCH_MAX => Some(AtomicOp::Max),
            id::STD_SYNC_ATOMIC_FETCH_MIN => Some(AtomicOp::Min),
            _ => None,
        };
        let val = Shape::Scalar(ty);
        if matches!(
            id,
            id::STD_SYNC_ATOMIC_WAIT
                | id::STD_SYNC_ATOMIC_NOTIFY_ONE
                | id::STD_SYNC_ATOMIC_NOTIFY_ALL
        ) && !matches!(ty, Ty::Int(IntTy { bits: 32, .. }))
        {
            return unsupported(format!(
                "`{}` on a word that is not 32 bits",
                c.path.join(".")
            ));
        }
        if ty == Ty::Bool
            && matches!(
                rmw(id),
                Some(AtomicOp::Add | AtomicOp::Sub | AtomicOp::Max | AtomicOp::Min)
            )
        {
            return unsupported(format!("`{}` on an `Atomic[bool]`", c.path.join(".")));
        }
        match (id, args.as_slice()) {
            (id::STD_SYNC_ATOMIC_ATOMIC, [(_, v)]) => {
                let v = self.scalar1(v)?;
                let p = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(1)]);
                self.emit(Inst::Atomic {
                    dst: None,
                    op: AtomicOp::Store,
                    ty,
                    order: MemOrder::SeqCst,
                    failure: MemOrder::SeqCst,
                    ptr: p.clone(),
                    args: vec![v],
                });
                set(self, vec![p]);
            }
            (id::STD_SYNC_ATOMIC_LOAD, [(_, a), (_, o)]) => {
                let (p, order) = (self.scalar1(a)?, self.mem_order(o)?);
                let d = self.local(ty);
                self.atomic(Some(d), AtomicOp::Load, ty, order, p, vec![]);
                set(self, vec![Operand::Local(d)]);
            }
            (id::STD_SYNC_ATOMIC_INTO_INNER, [(_, a)]) => {
                let p = self.scalar1(a)?;
                let d = self.local(ty);
                self.atomic(
                    Some(d),
                    AtomicOp::Load,
                    ty,
                    MemOrder::SeqCst,
                    p.clone(),
                    vec![],
                );
                self.free(RtFn::EnvFree, p);
                set(self, vec![Operand::Local(d)]);
            }
            (id::STD_SYNC_ATOMIC_STORE, [(_, a), (_, v), (_, o)]) => {
                let (p, order) = (self.scalar1(a)?, self.mem_order(o)?);
                let v = self.atom(v, &val)?;
                self.atomic(None, AtomicOp::Store, ty, order, p, v);
            }
            (id::STD_SYNC_ATOMIC_SWAP, [(_, a), (_, v), (_, o)]) => {
                let (p, order) = (self.scalar1(a)?, self.mem_order(o)?);
                let v = self.atom(v, &val)?;
                let d = self.local(ty);
                self.atomic(Some(d), AtomicOp::Swap, ty, order, p, v);
                set(self, vec![Operand::Local(d)]);
            }
            (
                id::STD_SYNC_ATOMIC_COMPARE_EXCHANGE | id::STD_SYNC_ATOMIC_COMPARE_EXCHANGE_WEAK,
                [(_, a), (_, cur), (_, new), (_, so), (_, fo)],
            ) => {
                let p = self.scalar1(a)?;
                let (order, failure) = (self.mem_order(so)?, self.mem_order(fo)?);
                let cur = self.atom(cur, &val)?;
                let new = self.atom(new, &val)?;
                let old = self.local(ty);
                self.emit(Inst::Atomic {
                    dst: Some(old),
                    op: AtomicOp::Cas,
                    ty,
                    order,
                    failure,
                    ptr: p,
                    args: vec![cur[0].clone(), new[0].clone()],
                });
                let Some(d) = &dst else { return Ok(()) };
                let (Some((ok_tag, ..)), Some((err_tag, ..))) =
                    (d.shape.variant("Ok"), d.shape.variant("Err"))
                else {
                    return unsupported("a compare-exchange result that is not a `Result`");
                };
                let hit =
                    self.scalar_op(ScalarOp::Eq, ty, vec![Operand::Local(old), cur[0].clone()]);
                let tag = self.local(I64);
                let (then, els) = self.branch(hit);
                let join = self.block_new();
                for (b, k) in [(then, ok_tag), (els, err_tag)] {
                    self.cur = b;
                    self.emit(Inst::Copy {
                        dst: tag,
                        src: Operand::Int(k),
                    });
                    self.goto(join);
                }
                let mut out: Vec<Operand> = d.shape.tys().into_iter().map(zero).collect();
                out[0] = Operand::Local(tag);
                for (_, slot, _) in [d.shape.variant("Ok"), d.shape.variant("Err")]
                    .into_iter()
                    .flatten()
                {
                    out[slot] = Operand::Local(old);
                }
                set(self, out);
            }
            (id::STD_SYNC_ATOMIC_WAIT, [(_, a), (_, e), (_, o)]) => {
                let Some(dst) = dst else {
                    return unsupported("`wait` without its result");
                };
                let (p, order) = (self.scalar1(a)?, self.mem_order(o)?);
                let e = self.atom(e, &val)?;
                let st = self.blocking(
                    RtFn::FutexWait,
                    vec![p, e[0].clone(), Operand::Int(order.code())],
                );
                self.chan_result(&dst, st, |_| Vec::new())?;
            }
            (id::STD_SYNC_ATOMIC_NOTIFY_ONE | id::STD_SYNC_ATOMIC_NOTIFY_ALL, [(_, a)]) => {
                let p = self.scalar1(a)?;
                let count = if id == id::STD_SYNC_ATOMIC_NOTIFY_ONE {
                    1
                } else {
                    -1
                };
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::FutexNotify,
                    args: vec![p, Operand::Int(count)],
                });
            }
            (id, [(_, a), (_, v), (_, o)]) if rmw(id).is_some() => {
                let (p, order) = (self.scalar1(a)?, self.mem_order(o)?);
                let v = self.atom(v, &val)?;
                let d = self.local(ty);
                self.atomic(Some(d), rmw(id).expect("checked"), ty, order, p, v);
                set(self, vec![Operand::Local(d)]);
            }
            _ => {
                return unsupported(format!(
                    "call of `{}` with these arguments",
                    c.path.join(".")
                ));
            }
        }
        Ok(())
    }

    fn atomic(
        &mut self,
        dst: Option<Local>,
        op: AtomicOp,
        ty: Ty,
        order: MemOrder,
        ptr: Operand,
        args: Vec<Operand>,
    ) {
        self.emit(Inst::Atomic {
            dst,
            op,
            ty,
            order,
            failure: MemOrder::SeqCst,
            ptr,
            args,
        });
    }
}
