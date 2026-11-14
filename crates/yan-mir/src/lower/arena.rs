//! `Arena` and `Ref[T]` (4.7, 5.5) in native code: an arena is a `yan_rt::arena` block that owns
//! every slot `alloc` hands out; a `Ref` is the address of its slot. Dropping the arena frees all.

use super::*;

pub(super) const ARENA_IDS: &[registry::Id] = &[
    id::STD_ARENA_ARENA,
    id::STD_ARENA_ALLOC,
    id::STD_ARENA_REF_GET,
    id::STD_ARENA_REF_SET,
];

impl<'m> Fn<'_, 'm> {
    pub(super) fn arena_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let elem = |me: &Self| -> R<Shape> {
            match me.targ(c)? {
                Some(e) if !Self::has_heap(&e) => Ok(e),
                Some(e) => unsupported(format!("a `Ref` to a value with heap blocks ({e:?})")),
                None => unsupported("a `Ref` without its type"),
            }
        };
        match (id, args.as_slice()) {
            (id::STD_ARENA_ARENA, []) => {
                let a = self.rt(RtFn::ArenaNew, Ty::Ptr, vec![]);
                set(self, vec![a]);
            }
            (id::STD_ARENA_ALLOC, [(_, a), (_, v)]) => {
                let e = elem(self)?;
                let a = self.scalar1(a)?;
                let v = self.atom(v, &e)?;
                let words = Operand::Int(v.len() as i128);
                let slot = self.rt(RtFn::ArenaAlloc, Ty::Ptr, vec![a, words]);
                self.write(&At::Mem(slot.clone(), 0), v);
                set(self, vec![slot]);
            }
            (id::STD_ARENA_REF_GET, [(_, r)]) => {
                let e = elem(self)?;
                let r = self.scalar1(r)?;
                let v = self.read(&At::Mem(r, 0), &e);
                set(self, v);
            }
            (id::STD_ARENA_REF_SET, [(_, r), (_, v)]) => {
                let e = elem(self)?;
                let r = self.scalar1(r)?;
                let v = self.atom(v, &e)?;
                self.write(&At::Mem(r, 0), v);
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
}
