//! `core.ptr` (6.6) in native code: a `Ptr` is two words, its address and the allocation it
//! derives from (0 for none). Exposed provenance is kept in `yan_rt::ptr`, as the interpreter does.

use super::*;

pub(super) const PTR_IDS: &[registry::Id] = &[
    id::STD_PTR_ADDR,
    id::STD_PTR_WITH_ADDR,
    id::STD_PTR_MAP_ADDR,
    id::STD_PTR_WITHOUT_PROVENANCE,
    id::STD_PTR_DANGLING,
    id::STD_PTR_EXPOSE_PROVENANCE,
    id::STD_PTR_WITH_EXPOSED_PROVENANCE,
];

impl<'m> Fn<'_, 'm> {
    pub(super) fn ptr_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        match (id, args.as_slice()) {
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
