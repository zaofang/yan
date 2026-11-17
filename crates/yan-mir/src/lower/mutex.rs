//! `std.sync.mutex` (6.3) in native code: a `Mutex[T]` is a block whose word 0 is the lock flag
//! and whose words from 1 hold `T`; a `MutexGuard[T]` is the address of that block. The flag is
//! taken and released with atomic accesses, so the machine's race detector sees the
//! release/acquire edge between critical sections. A cooperative task never waits on a lock
//! (R395), so a lock found held is a deadlock (R396).

use super::*;

pub(super) const MUTEX_IDS: &[registry::Id] = &[
    id::STD_SYNC_MUTEX_MUTEX,
    id::STD_SYNC_MUTEX_LOCK,
    id::STD_SYNC_MUTEX_TRY_LOCK,
    id::STD_SYNC_MUTEX_GUARD_GET,
    id::STD_SYNC_MUTEX_GUARD_SET,
    id::STD_SYNC_MUTEX_GUARD_REPLACE,
];

/// The words of the block the value starts at.
pub(super) const VALUE: u32 = 1;

impl<'m> Fn<'_, 'm> {
    pub(super) fn mutex_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let Some(e) = self.targ(c)? else {
            return unsupported("a `Mutex` without its type");
        };
        match (id, args.as_slice()) {
            (id::STD_SYNC_MUTEX_MUTEX, [(_, v)]) => {
                let v = self.atom(v, &e)?;
                let words = 1 + v.len() as i128;
                let p = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(words)]);
                self.emit(Inst::Store {
                    ptr: p.clone(),
                    word: 0,
                    src: Operand::Int(0),
                });
                self.write(&At::Mem(p.clone(), VALUE), v);
                set(self, vec![p]);
            }
            (id::STD_SYNC_MUTEX_LOCK, [(_, m)]) => {
                let p = self.scalar1(m)?;
                let old = self.local(I64);
                self.emit(Inst::Atomic {
                    dst: Some(old),
                    op: AtomicOp::Swap,
                    ty: I64,
                    order: MemOrder::Acquire,
                    failure: MemOrder::SeqCst,
                    ptr: p.clone(),
                    args: vec![Operand::Int(1)],
                });
                let free = self.scalar_op(
                    ScalarOp::Eq,
                    I64,
                    vec![Operand::Local(old), Operand::Int(0)],
                );
                let (ok, held) = self.branch(free);
                self.cur = held;
                self.finish(Term::Panic("deadlock"));
                self.cur = ok;
                set(self, vec![p]);
            }
            // `Option[MutexGuard[T]]`: tag 0 is `Some` with the block, tag 1 is `None`. A lock
            // found held is `None` whether another task or this one holds it, and the failed
            // swap leaves the flag as it was.
            (id::STD_SYNC_MUTEX_TRY_LOCK, [(_, m)]) => {
                let p = self.scalar1(m)?;
                let old = self.local(I64);
                self.emit(Inst::Atomic {
                    dst: Some(old),
                    op: AtomicOp::Swap,
                    ty: I64,
                    order: MemOrder::Acquire,
                    failure: MemOrder::SeqCst,
                    ptr: p.clone(),
                    args: vec![Operand::Int(1)],
                });
                let free = self.scalar_op(
                    ScalarOp::Eq,
                    I64,
                    vec![Operand::Local(old), Operand::Int(0)],
                );
                let (some, none) = self.branch(free);
                let join = self.block_new();
                self.cur = some;
                set(self, vec![Operand::Int(0), p]);
                self.goto(join);
                self.cur = none;
                set(self, vec![Operand::Int(1), Operand::Int(0)]);
                self.goto(join);
                self.cur = join;
            }
            (id::STD_SYNC_MUTEX_GUARD_GET, [(_, g)]) => {
                if Self::has_heap(&e) {
                    return unsupported(format!(
                        "`MutexGuard.get` of a value with heap blocks ({e:?})"
                    ));
                }
                let p = self.scalar1(g)?;
                let v = self.read(&At::Mem(p, VALUE), &e);
                set(self, v);
            }
            (id::STD_SYNC_MUTEX_GUARD_SET, [(_, g), (_, v)]) => {
                let p = self.scalar1(g)?;
                let v = self.atom(v, &e)?;
                let old = self.read(&At::Mem(p.clone(), VALUE), &e);
                self.glue(&e, &old);
                self.write(&At::Mem(p, VALUE), v);
            }
            (id::STD_SYNC_MUTEX_GUARD_REPLACE, [(_, g), (_, v)]) => {
                let p = self.scalar1(g)?;
                let v = self.atom(v, &e)?;
                let old = self.read(&At::Mem(p.clone(), VALUE), &e);
                self.write(&At::Mem(p, VALUE), v);
                set(self, old);
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

    /// Drop glue of a guard: release the lock.
    pub(super) fn unlock(&mut self, block: Operand) {
        self.emit(Inst::Atomic {
            dst: None,
            op: AtomicOp::Store,
            ty: I64,
            order: MemOrder::Release,
            failure: MemOrder::SeqCst,
            ptr: block,
            args: vec![Operand::Int(0)],
        });
    }

    /// Drop glue of a mutex: the value's blocks, then the mutex's own.
    pub(super) fn mutex_glue(&mut self, inner: &Shape, block: Operand) {
        if Self::has_heap(inner) {
            let v = self.read(&At::Mem(block.clone(), VALUE), inner);
            self.glue(inner, &v);
        }
        self.free(RtFn::EnvFree, block);
    }
}
