//! Calls of extern functions (12.1, 12.2) in native code: one yan-rt call that resolves the
//! symbol of the block's library, shared with the interpreter.

use super::*;
use crate::ffi::Scalar;

impl Fn<'_, '_> {
    /// Lowers `c` when it names an extern function; false when it does not.
    pub(super) fn extern_call(&mut self, c: &y::Call, dst: Option<Var>) -> R<bool> {
        let name = c.path.join(".");
        let Some(ext) = self.types.externs.get(&name) else {
            return Ok(false);
        };
        let ext = ext.clone()?;
        let args = Self::value_args(c)?;
        if args.len() != ext.params.len() {
            return unsupported(format!("call of `{name}` with these arguments"));
        }
        let mut ints = vec![Operand::Int(0); yan_rt::ffi::INTS];
        let mut floats = vec![Operand::F64(0.0); yan_rt::ffi::FLOATS];
        let (mut i, mut f) = (0, 0);
        for ((_, a), p) in args.into_iter().zip(&ext.params) {
            let v = self.scalar1(a)?;
            match p {
                Scalar::Int(_) => {
                    ints[i] = v;
                    i += 1;
                }
                Scalar::F64 => {
                    floats[f] = v;
                    f += 1;
                }
            }
        }
        let link = self.lit(&ext.link);
        let sym = self.lit(&name);
        let mut ops = vec![link, sym, Operand::Int(ext.kind().into())];
        ops.extend(ints);
        ops.extend(floats);
        let (rt, ty) = match ext.ret {
            Some(Scalar::F64) => (RtFn::FfiCallF64, Ty::F64),
            Some(Scalar::Int(t)) => (RtFn::FfiCall, Ty::Int(t)),
            None => (RtFn::FfiCall, I64),
        };
        let d = dst.and_then(|v| v.locals.first().copied());
        let r = self.local(ty);
        self.emit(Inst::Rt {
            dst: Some(r),
            f: rt,
            args: ops,
        });
        if let (Some(d), Some(_)) = (d, ext.ret) {
            self.emit(Inst::Copy {
                dst: d,
                src: Operand::Local(r),
            });
        }
        Ok(true)
    }
}
