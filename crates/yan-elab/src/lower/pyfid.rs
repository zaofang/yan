//! Python constructor names that are real functions (R807): `bool(x)`, `tuple(xs)`, `dict()`,
//! `set()` and `sum(xs, start)`.

use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn py_ctor(
        &mut self,
        n: &str,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<Option<(Rhs, Ty)>> {
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        match n {
            "dict" if args.is_empty() => {
                self.rhs(&ex(j::ExprKind::Dict(Vec::new())), want).map(Some)
            }
            "set" if args.is_empty() => {
                let Some(Ty::Set(e)) = want else {
                    if !matches!(want, Some(Ty::Err)) {
                        self.err("E241", at, "annotate the type of this empty set".into());
                    }
                    return Ok(Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)));
                };
                let c = call("std.set.new", vec![e.targ()], Vec::new(), at);
                Ok(Some((Rhs::Call(c), want.cloned().expect("matched above"))))
            }
            "bool" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)));
                };
                let (a, t) = self.atom(xs[0], None)?;
                let x = match &a {
                    Atom::Name(n, sp) => j::Expr {
                        kind: j::ExprKind::Name(n.clone()),
                        span: *sp,
                        paren: false,
                    },
                    Atom::Lit(..) => xs[0].clone(),
                };
                let lit = |l| Box::new(ex(j::ExprKind::Lit(l)));
                let e = match &t {
                    Ty::Err => return Ok(Some((Rhs::Atom(a), Ty::Err))),
                    Ty::Bool => return Ok(Some((Rhs::Atom(a), Ty::Bool))),
                    Ty::Int => ex(j::ExprKind::Compare(
                        "!=",
                        Box::new(x),
                        lit(j::Lit::Int("0".into())),
                    )),
                    Ty::Float => ex(j::ExprKind::Compare(
                        "!=",
                        Box::new(x),
                        lit(j::Lit::Float("0.0".into())),
                    )),
                    Ty::Str => {
                        let empty = j::Lit::Str {
                            prefix: String::new(),
                            value: String::new(),
                        };
                        ex(j::ExprKind::Compare("!=", Box::new(x), lit(empty)))
                    }
                    Ty::List(_) | Ty::Set(_) | Ty::Dict(..) => {
                        let len = ex(j::ExprKind::Call(
                            Box::new(ex(j::ExprKind::Name("len".into()))),
                            vec![j::Arg {
                                name: None,
                                kind: j::ArgKind::Expr(x),
                                span: at,
                            }],
                        ));
                        ex(j::ExprKind::Compare(
                            "!=",
                            Box::new(len),
                            lit(j::Lit::Int("0".into())),
                        ))
                    }
                    _ => {
                        self.err(
                            "E201",
                            xs[0].span,
                            format!(
                                "`bool` does not take a `{t}`; compare it explicitly",
                                t = self.ty_text(&t)
                            ),
                        );
                        return Ok(Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)));
                    }
                };
                self.rhs(&e, want).map(Some)
            }
            "tuple" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)));
                };
                let (a, t) = self.atom(xs[0], None)?;
                match t {
                    Ty::Tuple(_) | Ty::Err => Ok(Some((Rhs::Atom(a), t))),
                    t => {
                        self.err(
                            "E201",
                            xs[0].span,
                            format!(
                                "`tuple` of a `{t}`: a tuple has a fixed arity; index or destructure the elements",
                                t = self.ty_text(&t)
                            ),
                        );
                        Ok(Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)))
                    }
                }
            }
            // `sum(xs, start)` is `sum([start] + xs)`: Python adds `start` first.
            "sum" if args.len() == 2 => {
                let Some(xs) = self.positional(args, 2, n, at)? else {
                    return Ok(None);
                };
                let items = self.reusable(xs[0])?;
                let start = self.reusable(xs[1])?;
                let head = ex(j::ExprKind::List(vec![start]));
                let joined = ex(j::ExprKind::Call(
                    Box::new(ex(j::ExprKind::Attr(Box::new(head), "concat".into(), at))),
                    vec![j::Arg {
                        name: None,
                        kind: j::ArgKind::Expr(items),
                        span: at,
                    }],
                ));
                let sum = ex(j::ExprKind::Call(
                    Box::new(ex(j::ExprKind::Name("sum".into()))),
                    vec![j::Arg {
                        name: None,
                        kind: j::ArgKind::Expr(joined),
                        span: at,
                    }],
                ));
                self.rhs(&sum, want).map(Some)
            }
            _ => Ok(None),
        }
    }
}
