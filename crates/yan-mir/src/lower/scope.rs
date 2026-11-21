//! `std.scope` (9.4) in native code: a `Scope` is its id, one word, kept by `yan_rt::scope`.

use super::*;

pub(super) const SCOPE_IDS: &[registry::Id] = &[
    id::STD_SCOPE_ROOT,
    id::STD_SCOPE_CURRENT,
    id::STD_SCOPE_DRAIN,
    id::STD_SCOPE_IS_DRAINING,
];

impl Fn<'_, '_> {
    pub(super) fn scope_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let v = match (id, args.as_slice()) {
            (id::STD_SCOPE_ROOT, []) => Operand::Int(0),
            (id::STD_SCOPE_CURRENT, []) => self.rt(RtFn::ScopeCurrent, I64, Vec::new()),
            (id::STD_SCOPE_DRAIN, [(_, s), (_, grace)]) => {
                let s = self.atom_any(s)?.0[0].clone();
                let (g, _) = self.atom_any(grace)?;
                let [hi, lo] = g.as_slice() else {
                    return unsupported("a drain grace that is not a `Duration`");
                };
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ScopeDrain,
                    args: vec![s, hi.clone(), lo.clone()],
                });
                return Ok(());
            }
            (id::STD_SCOPE_IS_DRAINING, [(_, s)]) => {
                let s = self.atom_any(s)?.0[0].clone();
                let w = self.rt(RtFn::ScopeDraining, I64, vec![s]);
                self.scalar_op(ScalarOp::Ne, I64, vec![w, Operand::Int(0)])
            }
            _ => return unsupported("this `std.scope` call"),
        };
        if let Some(d) = &dst {
            self.copy_to(&d.locals.clone(), vec![v]);
        }
        Ok(())
    }
}
