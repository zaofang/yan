use super::*;

impl Lower<'_, '_, '_> {
    /// `wrap_as[T](x)`, `sat_as[T](x)` and `checked_as[T](x)` (3.4, assumption A6).
    pub(super) fn conv_call(
        &mut self,
        f: &str,
        to: &j::Expr,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        if f == "trunc_as" {
            return unsupported("`trunc_as`");
        }
        let Some(xs) = self.positional(args, 1, f, at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let j::ExprKind::Name(to) = &to.kind else {
            return unsupported("a conversion target that is not an integer type name");
        };
        self.int_conv(f, to, xs[0], at)
    }

    /// Whether `x` is a local of a fixed-width integer type, which `as` converts by range.
    pub(super) fn fixed_local(&self, x: &j::Expr) -> bool {
        matches!(self.ty_of_local(x), Some(Ty::Fixed(t)) if !t.starts_with('f'))
    }

    /// `x` converted to the integer type named `to` by `kind`: `wrap_as`, `sat_as` or `checked_as`.
    pub(super) fn int_conv(
        &mut self,
        kind: &str,
        to: &str,
        x: &j::Expr,
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let target = crate::ty::builtin(&core_type_name(to), &[]);
        let Some(target) = target.filter(|t| is_integer(t) && *t != Ty::BigInt) else {
            return unsupported("a conversion to anything but a fixed-width integer");
        };
        let (a, from) = self.atom(x, None)?;
        if from == Ty::Err {
            return Ok(err);
        }
        if !is_integer(&from) || from == Ty::BigInt {
            self.err(
                "E201",
                x.span,
                format!("`{kind}` converts an integer, not a `{from}`"),
            );
            return Ok(err);
        }
        let (f, t) = (
            from.scalar().expect("an integer"),
            target.scalar().expect("an integer"),
        );
        if f == "i128" || t == "i128" {
            return unsupported("a conversion to or from `i128`");
        }
        let p = format!("{kind}_{f}_{t}");
        Ok((
            Rhs::Call(call(&p, vec![], vec![let_arg(a)], at)),
            target,
            None,
        ))
    }
}
