use super::*;

impl Lower<'_, '_, '_> {
    /// A condition: a `bool`, or under the lenient reader a value tested by its type (N15).
    pub(super) fn cond(&mut self, e: &j::Expr) -> R<Atom> {
        let (a, t) = self.atom(e, Some(&Ty::Bool))?;
        if matches!(t, Ty::Bool | Ty::Err | Ty::Never) || self.cx.strict {
            self.expect(&t, &Ty::Bool, e.span);
            return Ok(a);
        }
        if matches!(t, Ty::List(_)) {
            self.sites.insert(e.span, Site::List);
        }
        let Some((test, how)) = self.truth(a, &t, e.span)? else {
            self.expect(&t, &Ty::Bool, e.span);
            return Ok(Atom::Lit(Literal::Bool(false), e.span));
        };
        let item = self.item.clone();
        self.cx.note(
            "N15",
            e.span,
            &item,
            format!(
                "a `{t}` as a condition normalized to {how}",
                t = self.ty_text(&t)
            ),
        );
        Ok(test)
    }

    /// The truth test of `a: t` that N15 writes out, and how it is spelled.
    pub(super) fn truth(&mut self, a: Atom, t: &Ty, at: Span) -> R<Option<(Atom, &'static str)>> {
        let zero = |s: &str| {
            Atom::Lit(
                if s.starts_with('f') {
                    Literal::Float("0.0".into())
                } else {
                    Literal::Int("0".into())
                },
                at,
            )
        };
        let (r, how) = match t {
            Ty::Int | Ty::Float | Ty::Fixed(_) => {
                let s = t.scalar().expect("a scalar");
                let c = call(
                    &format!("ne_{s}"),
                    vec![],
                    vec![let_arg(a), let_arg(zero(s))],
                    at,
                );
                (Rhs::Call(c), "`n != 0`")
            }
            Ty::BigInt => {
                let z = call("std.prelude.bigint", vec![], vec![let_arg(zero("i64"))], at);
                let z = self.tmp(&Ty::BigInt, Rhs::Call(z), at);
                let c = call("ne_bigint", vec![], vec![let_arg(a), let_arg(z)], at);
                (Rhs::Call(c), "`n != 0`")
            }
            Ty::Str => {
                let empty = Atom::Lit(Literal::Str(String::new()), at);
                let c = call(
                    "std.cmp.ne",
                    vec![t.targ()],
                    vec![let_arg(a), let_arg(empty)],
                    at,
                );
                (Rhs::Call(c), "`s != \"\"`")
            }
            Ty::List(_) | Ty::Set(_) | Ty::Dict(..) | Ty::Coll(..) => {
                let targs = match t {
                    Ty::List(e) | Ty::Set(e) => vec![e.targ()],
                    Ty::Dict(k, v) => vec![k.targ(), v.targ()],
                    _ => coll_targs(t),
                };
                let path = format!("std.{}.len", t.module().expect("a collection"));
                let n = self.tmp(
                    &Ty::Int,
                    Rhs::Call(call(&path, targs, vec![let_arg(a)], at)),
                    at,
                );
                let c = call("gt_i64", vec![], vec![let_arg(n), let_arg(zero("i64"))], at);
                (Rhs::Call(c), "`len(xs) > 0`")
            }
            // A nested optional has no single truth test (R795).
            Ty::Option(inner) if !matches!(**inner, Ty::Option(_)) => {
                let (v, _) = self.is_some(a, at)?;
                return Ok(Some((v, "`x is not None`")));
            }
            _ => return Ok(None),
        };
        Ok(Some((self.tmp(&Ty::Bool, r, at), how)))
    }

    /// Whether the optional `a` holds a value.
    pub(super) fn is_some(&mut self, a: Atom, at: Span) -> R<(Atom, Ty)> {
        self.join(at, |l, k| {
            let arm = |l: &mut Self, b: bool| {
                l.block_with(|l| {
                    l.jump(k, Atom::Lit(Literal::Bool(b), at), at);
                    Ok(())
                })
            };
            let some = y::Pat::Ctor {
                path: vec!["Some".into()],
                fields: Some((vec![("value".into(), y::Pat::Wild)], false)),
            };
            let arms = vec![
                (some, arm(l, true)?),
                (y::Pat::Bind("None".into()), arm(l, false)?),
            ];
            Ok((
                y::Stmt {
                    kind: StmtKind::Match { scrutinee: a, arms },
                    span: at,
                },
                Ty::Bool,
            ))
        })
    }

    /// The local an optional `n` is narrowed to here.
    pub(super) fn narrowed(&self, n: &str) -> Option<&str> {
        self.narrow
            .iter()
            .rev()
            .find(|m| m.0 == n)
            .map(|m| m.1.as_str())
    }

    /// The optional locals that `cond` being `holds` proves present, with their value types:
    /// `x is not None` when it holds, `x is None` when it does not, through `and` and `or`.
    pub(super) fn narrowings(&self, cond: &j::Expr, holds: bool) -> Vec<(String, Ty)> {
        use j::ExprKind as K;
        match &cond.kind {
            K::Compare(op @ ("is" | "is not"), x, none)
                if matches!(none.kind, K::Lit(j::Lit::None)) && (*op == "is not") == holds =>
            {
                match &x.kind {
                    K::Name(n) if self.narrowed(n).is_none() => match self.locals.get(n) {
                        Some(Local {
                            ty: Ty::Option(inner),
                        }) => vec![(n.clone(), (**inner).clone())],
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                }
            }
            K::Not(x) => self.narrowings(x, !holds),
            K::Binary("and", a, b) if holds => {
                let mut v = self.narrowings(a, true);
                v.extend(self.narrowings(b, true));
                v
            }
            K::Binary("or", a, b) if !holds => {
                let mut v = self.narrowings(a, false);
                v.extend(self.narrowings(b, false));
                v
            }
            _ => Vec::new(),
        }
    }

    /// From here to the end of the block, `x` names its unwrapped value.
    pub(super) fn narrow_to(&mut self, x: &str, inner: Ty, at: Span) {
        let o = self.fresh("n");
        let (v, _) = self
            .join(at, |l, k| {
                let some = y::Pat::Ctor {
                    path: vec!["Some".into()],
                    fields: Some((vec![("value".into(), y::Pat::Bind(o.clone()))], false)),
                };
                let some_b = l.block_with(|l| {
                    l.declare(&o, inner.clone());
                    l.jump(k, Atom::Name(o.clone(), at), at);
                    Ok(())
                })?;
                let none_b = y::Block {
                    stmts: vec![y::Stmt {
                        kind: StmtKind::Unreachable,
                        span: at,
                    }],
                };
                Ok((
                    y::Stmt {
                        kind: StmtKind::Match {
                            scrutinee: Atom::Name(x.to_owned(), at),
                            arms: vec![(some, some_b), (y::Pat::Bind("None".into()), none_b)],
                        },
                        span: at,
                    },
                    inner.clone(),
                ))
            })
            .expect("lowering a narrowing cannot fail");
        let Atom::Name(v, _) = v else {
            unreachable!("a join value is a name")
        };
        let depth = self.scopes.len();
        self.narrow.push((x.to_owned(), v, depth));
    }

    /// Bind a `for` target, one name or a tuple of names, to `cur` of type `ty`.
    pub(super) fn bind_target(
        &mut self,
        target: &[String],
        ty: &Ty,
        cur: Rhs,
        at: Span,
        span: Span,
    ) {
        match (target, ty) {
            ([n], _) => {
                let m = self.mutable.contains(n);
                self.bind(n, m, ty, cur, at, span);
            }
            (ns, Ty::Tuple(ts)) if ns.len() == ts.len() => {
                let e = self.tmp(ty, cur, at);
                let Atom::Name(e, _) = e else {
                    unreachable!("a projection is bound")
                };
                for (k, (n, t)) in ns.iter().zip(ts).enumerate() {
                    let p = Rhs::Proj(Place {
                        root: e.clone(),
                        root_span: at,
                        projs: vec![Proj::Field(format!("_{k}"), at)],
                    });
                    let m = self.mutable.contains(n);
                    self.bind(n, m, t, p, at, at);
                }
            }
            _ => {
                self.err(
                    "E201",
                    at,
                    format!(
                        "cannot unpack `{ty}` into {} names",
                        target.len(),
                        ty = self.ty_text(ty)
                    ),
                );
            }
        }
    }
}
