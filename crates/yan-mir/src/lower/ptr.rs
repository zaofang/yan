//! `core.ptr` (6.6) in native code: a `Ptr` is two words, its address and the allocation it
//! derives from (0 for none). Exposed provenance is kept in `yan_rt::ptr`, as the interpreter does.
//! Native code does not check provenance; `read`, `write` and `offset` act on the slot layout
//! (one 8-byte word per slot, as `Arena` does). A local whose place `Ptr.of` takes lives in a
//! stack slot for the whole function (`Func::pinned`).

use super::*;

pub(super) const PTR_IDS: &[registry::Id] = &[
    id::STD_PTR_ADDR,
    id::STD_PTR_WITH_ADDR,
    id::STD_PTR_MAP_ADDR,
    id::STD_PTR_WITHOUT_PROVENANCE,
    id::STD_PTR_DANGLING,
    id::STD_PTR_EXPOSE_PROVENANCE,
    id::STD_PTR_WITH_EXPOSED_PROVENANCE,
    id::STD_PTR_OF,
    id::STD_PTR_AS_PTR,
    id::STD_PTR_OFFSET,
    id::STD_PTR_CAST,
    id::STD_PTR_READ,
    id::STD_PTR_WRITE,
];

impl<'m> Fn<'_, 'm> {
    pub(super) fn ptr_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let elem = |me: &Self| -> R<Shape> {
            match me.targ(c)? {
                Some(e) => Ok(e),
                None => unsupported("a `Ptr` without its type"),
            }
        };
        // `write` of a heap value would leak the slot's old blocks, which `Ptr` cannot know.
        let heap_free = |me: &Self| -> R<Shape> {
            match elem(me)? {
                e if !Self::has_heap(&e) => Ok(e),
                e => unsupported(format!("a `Ptr` write of a value with heap blocks ({e:?})")),
            }
        };
        match (id, args.as_slice()) {
            (id::STD_PTR_OF, [(_, y::Atom::Name(n, _))]) => {
                let locals = self.name(n)?.locals.clone();
                let Some(first) = locals.first().copied() else {
                    return unsupported("`Ptr.of` of a value with no slots");
                };
                if !self.pinned.iter().flatten().any(|l| *l == first) {
                    self.pinned.push(locals);
                }
                let a = self.local(I64);
                self.emit(Inst::AddrOf {
                    dst: a,
                    local: first,
                });
                set(self, vec![Operand::Local(a), Operand::Int(0)]);
            }
            (id::STD_PTR_AS_PTR, [(_, l)]) => {
                let l = self.scalar1(l)?;
                let a = self.rt(RtFn::ListData, I64, vec![l]);
                set(self, vec![a, Operand::Int(0)]);
            }
            (id::STD_PTR_OFFSET, [(_, p), (_, n)]) => {
                let stride = elem(self)?.tys().len() as i128 * 8;
                let p = self.atom_any(p)?.0;
                let n = self.scalar1(n)?;
                let by = self.scalar_op(ScalarOp::MulChecked, I64, vec![n, Operand::Int(stride)]);
                let a = self.scalar_op(ScalarOp::AddChecked, I64, vec![p[0].clone(), by]);
                set(self, vec![a, p[1].clone()]);
            }
            (id::STD_PTR_CAST, [(_, p)]) => {
                let p = self.atom_any(p)?.0;
                set(self, p);
            }
            (id::STD_PTR_READ, [(_, p)]) => {
                if matches!(c.targs.as_slice(), [y::TArg::Type(t)] if self.types.is_capability(t)) {
                    self.end(Term::Panic("forged"));
                }
                let e = elem(self)?;
                let p = self.atom_any(p)?.0;
                let v = self.read(&At::Mem(p[0].clone(), 0), &e);
                set(self, v);
            }
            (id::STD_PTR_WRITE, [(_, p), (_, v)]) => {
                let e = heap_free(self)?;
                let p = self.atom_any(p)?.0;
                let v = self.atom(v, &e)?;
                self.write(&At::Mem(p[0].clone(), 0), v);
            }
            (id::STD_PTR_ADDR, [(_, p)]) => {
                let p = self.atom_any(p)?.0;
                set(self, vec![p[0].clone()]);
            }
            (id::STD_PTR_WITH_ADDR, [(_, p), (_, a)]) => {
                let p = self.atom_any(p)?.0;
                let a = self.scalar1(a)?;
                set(self, vec![a, p[1].clone()]);
            }
            (id::STD_PTR_MAP_ADDR, [(_, p), (_, f)]) => {
                let p = self.atom_any(p)?.0;
                let (f, _) = self.atom_any(f)?;
                let r = self.var(Shape::Scalar(I64));
                self.emit(Inst::CallIndirect {
                    dst: r.locals.clone(),
                    code: f[0].clone(),
                    args: vec![f[1].clone(), p[0].clone()],
                    params: vec![Ty::Ptr, I64],
                    ret: vec![I64],
                });
                set(self, vec![Operand::Local(r.locals[0]), p[1].clone()]);
            }
            (id::STD_PTR_WITHOUT_PROVENANCE, [(_, a)]) => {
                let a = self.scalar1(a)?;
                set(self, vec![a, Operand::Int(0)]);
            }
            (id::STD_PTR_DANGLING, []) => set(self, vec![Operand::Int(1), Operand::Int(0)]),
            (id::STD_PTR_EXPOSE_PROVENANCE, [(_, p)]) => {
                let p = self.atom_any(p)?.0;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::PtrExpose,
                    args: p.clone(),
                });
                set(self, vec![p[0].clone()]);
            }
            (id::STD_PTR_WITH_EXPOSED_PROVENANCE, [(_, a)]) => {
                let a = self.scalar1(a)?;
                let prov = self.rt(RtFn::PtrExposed, I64, vec![a.clone()]);
                set(self, vec![a, prov]);
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
