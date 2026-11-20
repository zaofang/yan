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
        if f == "trunc_as" || f == "try_as" {
            return self.conv_composed(f, to, args, at);
        }
        let Some(xs) = self.positional(args, 1, f, at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let j::ExprKind::Name(to) = &to.kind else {
            return unsupported("a conversion target that is not an integer type name");
        };
        self.int_conv(f, to, xs[0], at)
    }

    /// `trunc_as[T](f)` is `checked_as[T](int(f))` and `try_as[T](x)` is `Some(checked_as[T](x))`
    /// when `x` fits `T` and `None` otherwise, both built from the checked conversion so
    /// there is one range rule (RFC 0098).
    fn conv_composed(
        &mut self,
        f: &str,
        to: &j::Expr,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let Some(xs) = self.positional(args, 1, f, at)? else {
            return Ok(err);
        };
        let j::ExprKind::Name(tn) = &to.kind else {
            return unsupported("a conversion target that is not an integer type name");
        };
        let target = crate::ty::builtin(&core_type_name(tn), &[]);
        let Some(target) = target.filter(|t| is_integer(t) && *t != Ty::BigInt) else {
            return unsupported("a conversion to anything but a fixed-width integer");
        };
        let tn = target.scalar().expect("an integer");
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let name = |n: &str| ex(j::ExprKind::Name(n.to_owned()));
        let checked = |x: j::Expr| {
            let head = ex(j::ExprKind::Index(
                Box::new(name("checked_as")),
                Box::new(name(tn)),
            ));
            let arg = j::Arg {
                name: None,
                kind: j::ArgKind::Expr(x),
                span: at,
            };
            ex(j::ExprKind::Call(Box::new(head), vec![arg]))
        };
        let call_of = |f: &str, x: j::Expr| {
            let arg = j::Arg {
                name: None,
                kind: j::ArgKind::Expr(x),
                span: at,
            };
            ex(j::ExprKind::Call(Box::new(name(f)), vec![arg]))
        };
        if f == "trunc_as" {
            let (_, from) = self.atom(xs[0], None)?;
            if from == Ty::Err {
                return Ok(err);
            }
            if from != Ty::Float {
                self.err(
                    "E201",
                    xs[0].span,
                    format!("`trunc_as` converts a `float`, not a `{from}`; use `wrap_as`, `sat_as` or `checked_as`"),
                );
                return Ok(err);
            }
            if tn == "u64" || tn == "u128" || tn == "i128" {
                return unsupported(format!("`trunc_as[{tn}]`"));
            }
            let e = checked(call_of("int", xs[0].clone()));
            let (r, t) = self.rhs(&e, None)?;
            return Ok((r, t, None));
        }
        let x = self.reusable(xs[0])?;
        let (_, from) = self.atom(&x, None)?;
        if from == Ty::Err {
            return Ok(err);
        }
        if !is_integer(&from) || from == Ty::BigInt {
            self.err(
                "E201",
                xs[0].span,
                format!("`try_as` converts an integer, not a `{from}`"),
            );
            return Ok(err);
        }
        let fname = from.scalar().expect("an integer");
        if fname == "i128" || tn == "i128" {
            return unsupported("a conversion to or from `i128`");
        }
        let (flo, fhi) = int_range(fname);
        let (tlo, thi) = int_range(tn);
        let bound = |op: &'static str, v: i128| {
            let lit = ex(j::ExprKind::Lit(j::Lit::Int(v.unsigned_abs().to_string())));
            let lit = if v < 0 {
                ex(j::ExprKind::Unary("-", Box::new(lit)))
            } else {
                lit
            };
            ex(j::ExprKind::Compare(op, Box::new(x.clone()), Box::new(lit)))
        };
        let mut conds = Vec::new();
        if tlo > flo {
            conds.push(bound(">=", tlo));
        }
        if thi < fhi {
            conds.push(bound("<=", thi));
        }
        let some = call_of("Some", checked(x.clone()));
        let want = Ty::Option(Box::new(target));
        let Some(first) = conds.pop() else {
            let (r, t) = self.rhs(&some, Some(&want))?;
            return Ok((r, t, None));
        };
        let cond = conds.into_iter().rfold(first, |acc, c| {
            ex(j::ExprKind::Binary("and", Box::new(c), Box::new(acc)))
        });
        let e = ex(j::ExprKind::Cond {
            then: Box::new(some),
            cond: Box::new(cond),
            els: Box::new(ex(j::ExprKind::Lit(j::Lit::None))),
        });
        let (r, t) = self.rhs(&e, Some(&want))?;
        Ok((r, t, None))
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
