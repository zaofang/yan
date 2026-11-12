//! `std.error.context` (8.1): a failure becomes an `Error` carrying its shown text and the notes.

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    /// `Result[T, Error]`, what `context[T, E]` makes of a `Result[T, E]`.
    pub(super) fn context_shape(&mut self, c: &y::Call) -> R<Option<Shape>> {
        let Some(y::TArg::Type(t)) = c.targs.first() else {
            return Ok(None);
        };
        let ok = self.shape(t)?;
        let err = self.shape(&y::Type::Named(y::Named {
            path: vec!["Error".into()],
            args: Vec::new(),
        }))?;
        Ok(Some(result(ok, err)))
    }

    pub(super) fn error_call(&mut self, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let [(_, r), (_, note)] = args.as_slice() else {
            return unsupported("call of `std.error.context` with these arguments");
        };
        let (rv, rs) = self.atom_any(r)?;
        let note = self.atom_any(note)?.0[0].clone();
        let (Some((_, ok_at, ofs)), Some((_, err_at, efs))) = (rs.variant("Ok"), rs.variant("Err"))
        else {
            return unsupported("`std.error.context` of a non-result");
        };
        let (ok_shape, e) = (ofs[0].1.clone(), efs[0].1.clone());
        if matches!(&e, Shape::Rec(n, _) if n == "Error") {
            return unsupported("`std.error.context` of an `Error`");
        }
        let Some(d) = dst else {
            return Ok(());
        };
        let ds = d.shape.clone();
        let is_ok = self.scalar_op(ScalarOp::Eq, I64, vec![rv[0].clone(), Operand::Int(0)]);
        let (good, bad) = self.branch(is_ok);
        let join = self.block_new();
        self.cur = good;
        let value = rv[ok_at..ok_at + ok_shape.width()].to_vec();
        self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "Ok", value));
        self.goto(join);
        self.cur = bad;
        let ev = rv[err_at..err_at + e.width()].to_vec();
        let message = self.show(&ev, &e, false)?;
        self.glue(&e, &ev);
        let empty = self.lit("");
        let note = self.rt(RtFn::Concat, Ty::Ptr, vec![note, empty]);
        let notes = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(1)]);
        let slot = self.rt(RtFn::ListPush, Ty::Ptr, vec![notes.clone()]);
        self.write(&At::Mem(slot, 0), vec![note]);
        self.copy_to(
            &d.locals.clone(),
            Self::variant_value(&ds, "Err", vec![message, notes]),
        );
        self.goto(join);
        self.cur = join;
        Ok(())
    }
}
