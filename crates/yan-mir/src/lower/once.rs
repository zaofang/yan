//! `std.sync.once` and `std.sync.lazy` (6.3) in native code. A `Once[T]` is a block whose word 0
//! is the state (0 empty, 2 set) and whose words from 1 hold `T`. A `Lazy[T]` is a block whose word
//! 0 is the state (0 pending, 1 running, 2 set), words 1 and 2 are the initializer's code and
//! environment, and words from 3 hold `T`. A cooperative task never runs two initializers at once,
//! so a `Lazy` found running is forced inside its own initializer: a deadlock (R396).

use super::*;

pub(super) const ONCE_IDS: &[registry::Id] = &[
    id::STD_SYNC_ONCE_ONCE,
    id::STD_SYNC_ONCE_GET_OR_INIT,
    id::STD_SYNC_ONCE_GET,
    id::STD_SYNC_LAZY_LAZY,
    id::STD_SYNC_LAZY_FORCE,
];

const SET: i128 = 2;
const ONCE_VALUE: u32 = 1;
const LAZY_INIT: u32 = 1;
const LAZY_VALUE: u32 = 3;

fn init_fn(e: &Shape) -> Shape {
    Shape::Fn(Vec::new(), Box::new(e.clone()))
}

impl<'m> Fn<'_, 'm> {
    fn state_is(&mut self, p: &Operand, state: i128) -> Operand {
        let st = self.read(&At::Mem(p.clone(), 0), &Shape::Scalar(I64));
        self.scalar_op(ScalarOp::Eq, I64, vec![st[0].clone(), Operand::Int(state)])
    }

    fn set_state(&mut self, p: &Operand, state: i128) {
        self.emit(Inst::Store {
            ptr: p.clone(),
            word: 0,
            src: Operand::Int(state),
        });
    }

    pub(super) fn once_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let Some(e) = self.targ(c)? else {
            return unsupported("a `Once` or `Lazy` without its type");
        };
        let reads = !matches!(id, id::STD_SYNC_ONCE_ONCE | id::STD_SYNC_LAZY_LAZY);
        if reads && Self::has_heap(&e) {
            return unsupported(format!(
                "`{}` of a value with heap blocks ({e:?})",
                c.path.join(".")
            ));
        }
        match (id, args.as_slice()) {
            (id::STD_SYNC_ONCE_ONCE, []) => {
                let words = 1 + e.width() as i128;
                let p = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(words)]);
                self.set_state(&p, 0);
                set(self, vec![p]);
            }
            (id::STD_SYNC_LAZY_LAZY, [(_, f)]) => {
                let f = self.atom(f, &init_fn(&e))?;
                let words = LAZY_VALUE as i128 + e.width() as i128;
                let p = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(words)]);
                self.set_state(&p, 0);
                self.write(&At::Mem(p.clone(), LAZY_INIT), f);
                set(self, vec![p]);
            }
            (id::STD_SYNC_ONCE_GET, [(_, o)]) => {
                let p = self.scalar1(o)?;
                let ok = self.state_is(&p, SET);
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                self.some_if(ok, &dst, &ds, |me| {
                    Ok(me.read(&At::Mem(p.clone(), ONCE_VALUE), &e))
                })?;
            }
            (id::STD_SYNC_ONCE_GET_OR_INIT, [(_, o), (_, f)]) => {
                let p = self.scalar1(o)?;
                let f = self.atom(f, &init_fn(&e))?;
                let done = self.state_is(&p, SET);
                let (set_b, empty) = self.branch(done);
                let join = self.block_new();
                self.cur = set_b;
                let v = self.read(&At::Mem(p.clone(), ONCE_VALUE), &e);
                set(self, v);
                self.goto(join);
                self.cur = empty;
                let r = self.call_closure(&f, &[], &e);
                // A re-entrant call may have set the cell first; the first value stays.
                let raced = self.state_is(&p, SET);
                let (won, first) = self.branch(raced);
                self.cur = won;
                let v = self.read(&At::Mem(p.clone(), ONCE_VALUE), &e);
                set(self, v);
                self.goto(join);
                self.cur = first;
                self.write(&At::Mem(p.clone(), ONCE_VALUE), r.clone());
                self.set_state(&p, SET);
                set(self, r);
                self.goto(join);
                self.cur = join;
            }
            (id::STD_SYNC_LAZY_FORCE, [(_, l)]) => {
                let p = self.scalar1(l)?;
                let done = self.state_is(&p, SET);
                let (set_b, rest) = self.branch(done);
                let join = self.block_new();
                self.cur = set_b;
                let v = self.read(&At::Mem(p.clone(), LAZY_VALUE), &e);
                set(self, v);
                self.goto(join);
                self.cur = rest;
                let pending = self.state_is(&p, 0);
                let (go, running) = self.branch(pending);
                self.cur = running;
                self.finish(Term::Panic("deadlock"));
                self.cur = go;
                self.set_state(&p, 1);
                let f = self.read(&At::Mem(p.clone(), LAZY_INIT), &init_fn(&e));
                let r = self.call_closure(&f, &[], &e);
                self.glue(&init_fn(&e), &f);
                self.write(&At::Mem(p.clone(), LAZY_VALUE), r.clone());
                self.set_state(&p, SET);
                set(self, r);
                self.goto(join);
                self.cur = join;
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

    /// Drop glue of a `Once` (`lazy` false) or `Lazy`: the value if set, the initializer's
    /// environment if never run, then the block.
    pub(super) fn once_glue(&mut self, inner: &Shape, lazy: bool, block: Operand) {
        if lazy {
            let pending = self.state_is(&block, 0);
            let (yes, no) = self.branch(pending);
            let join = self.block_new();
            self.cur = yes;
            let f = self.read(&At::Mem(block.clone(), LAZY_INIT), &init_fn(inner));
            self.glue(&init_fn(inner), &f);
            self.goto(join);
            self.cur = no;
            self.goto(join);
            self.cur = join;
        }
        if Self::has_heap(inner) {
            let at = if lazy { LAZY_VALUE } else { ONCE_VALUE };
            let done = self.state_is(&block, SET);
            let (yes, no) = self.branch(done);
            let join = self.block_new();
            self.cur = yes;
            let v = self.read(&At::Mem(block.clone(), at), inner);
            self.glue(inner, &v);
            self.goto(join);
            self.cur = no;
            self.goto(join);
            self.cur = join;
        }
        self.free(RtFn::EnvFree, block);
    }
}
