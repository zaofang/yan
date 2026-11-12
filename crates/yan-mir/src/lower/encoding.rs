//! `std.encoding` operations that fail with an `EncodingError` (R579 to R585).

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    /// `ok` computes the success value once `bad` (-1, else the fault index) says it is valid.
    fn encoding_result(
        &mut self,
        dst: &Var,
        bad: Operand,
        reason: &str,
        ok: impl FnOnce(&mut Self) -> Operand,
    ) -> R<()> {
        let ds = dst.shape.clone();
        let Some((_, _, fs)) = ds.variant("Err") else {
            return unsupported("a fallible `std.encoding` call without a `Result`");
        };
        let Some(Shape::Rec(_, fields)) = fs.first().map(|(_, s)| s) else {
            return unsupported("an `EncodingError` of this shape");
        };
        let Some((_, reason_shape)) = fields.iter().find(|(n, _)| n == "reason") else {
            return unsupported("an `EncodingError` without a reason");
        };
        let faulty = self.scalar_op(ScalarOp::Ge, I64, vec![bad.clone(), Operand::Int(0)]);
        let (err, good) = self.branch(faulty);
        let join = self.block_new();
        let locals = dst.locals.clone();
        self.cur = err;
        let mut payload = vec![bad];
        payload.extend(Self::variant_value(reason_shape, reason, vec![]));
        self.copy_to(&locals, Self::variant_value(&ds, "Err", payload));
        self.goto(join);
        self.cur = good;
        let v = ok(self);
        self.copy_to(&locals, Self::variant_value(&ds, "Ok", vec![v]));
        self.goto(join);
        Ok(())
    }

    /// The `Result` shape of a fallible call that returns `ok`.
    pub(super) fn encoding_shape(&mut self, ok: Shape) -> R<Shape> {
        let e = self.shape(&y::Type::Named(y::Named {
            path: vec!["EncodingError".into()],
            args: Vec::new(),
        }))?;
        Ok(result(ok, e))
    }

    pub(super) fn encoding_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let (Some(dst), [(_, a), rest @ ..]) = (dst, args.as_slice()) else {
            return unsupported(format!("call of `{name}`"));
        };
        let x = self.scalar1(a)?;
        match id {
            id::STD_ENCODING_BASE64_DECODE => {
                let [(_, url)] = rest else {
                    return unsupported(format!("call of `{name}`"));
                };
                let url = self.scalar1(url)?;
                let bad = self.rt(RtFn::Base64Check, I64, vec![x.clone(), url.clone()]);
                self.encoding_result(&dst, bad, "Base64", |me| {
                    me.rt(RtFn::Base64Decode, Ty::Ptr, vec![x, url])
                })
            }
            id::STD_ENCODING_DECODE_UTF8 => {
                let bad = self.rt(RtFn::Utf8Check, I64, vec![x.clone()]);
                self.encoding_result(&dst, bad, "Utf8", |me| {
                    me.rt(RtFn::BytesLossy, Ty::Ptr, vec![x])
                })
            }
            _ => {
                let bad = self.rt(RtFn::HexCheck, I64, vec![x.clone()]);
                self.encoding_result(&dst, bad, "Hex", |me| {
                    me.rt(RtFn::HexDecode, Ty::Ptr, vec![x])
                })
            }
        }
    }
}
