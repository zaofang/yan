use super::*;

impl Lower<'_, '_, '_> {
    /// A lambda becomes a closure; its parameter types come from the expected callable type.
    /// Locals capture by copy, `var` parameters by reference and capabilities by name (4.6).
    pub(super) fn lambda(
        &mut self,
        names: &[String],
        body: &j::Expr,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        // An optional callable parameter (`key: ... | None`) takes a lambda as its callable.
        let want = match want {
            Some(Ty::Option(f)) => Some(&**f),
            w => w,
        };
        let (par, ps, ret) = match want {
            Some(Ty::Fn(par, ps, ret)) if ps.len() == names.len() && !ps.iter().any(has_param) => {
                (*par, ps.clone(), (!has_param(ret)).then(|| (**ret).clone()))
            }
            _ => return unsupported("a lambda without a known callable type"),
        };
        let mut captures: Vec<(Conv, String, Span)> = Vec::new();
        walk(body, &mut |e| {
            let j::ExprKind::Name(n) = &e.kind else {
                return;
            };
            if names.contains(n) || captures.iter().any(|(_, m, _)| m == n) {
                return;
            }
            let inout = self
                .sig
                .params
                .iter()
                .any(|p| p.name == *n && p.conv == Conv::Inout);
            if inout {
                captures.push((Conv::Inout, n.clone(), e.span));
            } else if self.locals.contains_key(n)
                || self.sig.caps.contains(n)
                || self.handlers.contains(n)
            {
                captures.push((Conv::Let, n.clone(), e.span));
            }
        });
        let mut rt = Ty::Err;
        let block = self.block_with(|l| {
            for (n, t) in names.iter().zip(&ps) {
                l.declare(n, t.clone());
            }
            let (a, t) = l.atom(body, ret.as_ref())?;
            if let Some(r) = &ret {
                l.expect(&t, r, body.span);
            }
            rt = ret.clone().unwrap_or(t);
            l.emit(StmtKind::Ret(a), body.span);
            Ok(())
        })?;
        let params = names
            .iter()
            .zip(&ps)
            .map(|(n, t)| y::Param::Value {
                comptime: false,
                conv: Conv::Let,
                name: n.clone(),
                ty: t.core(),
            })
            .collect();
        let clo = y::Closure {
            captures,
            params,
            ret: rt.core(),
            uses: None,
            fail: None,
            body: block,
        };
        Ok((Rhs::Closure(Box::new(clo)), Ty::Fn(par, ps, Box::new(rt))))
    }

    pub(super) fn fstr(&mut self, pieces: &[j::FPiece], at: Span) -> R<(Rhs, Ty)> {
        // Each piece joins the accumulator as it is shown, the order of the `+` chain it equals.
        let mut acc: Option<Atom> = None;
        for p in pieces {
            let part = match p {
                j::FPiece::Lit(s) if s.is_empty() => continue,
                j::FPiece::Lit(s) => Atom::Lit(Literal::Str(s.clone()), at),
                j::FPiece::Expr(e, Some(spec)) => {
                    let (a, t) = self.atom(e, None)?;
                    let (fill, align, width) = pad_spec(spec, &t)?;
                    let shown = self.show(a, &t, e.span);
                    let args = vec![
                        let_arg(shown),
                        let_arg(Atom::Lit(Literal::Int(width.to_string()), e.span)),
                        let_arg(Atom::Lit(Literal::Int(align.to_string()), e.span)),
                        let_arg(Atom::Lit(Literal::Str(fill.to_string()), e.span)),
                    ];
                    let c = call("fmt_pad", vec![], args, e.span);
                    self.tmp(&Ty::Str, Rhs::Call(c), e.span)
                }
                j::FPiece::Expr(e, None) => {
                    let (a, t) = self.atom(e, None)?;
                    self.show(a, &t, e.span)
                }
            };
            acc = Some(match acc {
                None => part,
                Some(prev) => {
                    let c = call(
                        "std.text.concat",
                        vec![],
                        vec![let_arg(prev), let_arg(part)],
                        at,
                    );
                    self.tmp(&Ty::Str, Rhs::Call(c), at)
                }
            });
        }
        let acc = acc.unwrap_or(Atom::Lit(Literal::Str(String::new()), at));
        Ok((Rhs::Atom(acc), Ty::Str))
    }

    pub(super) fn show(&mut self, a: Atom, t: &Ty, at: Span) -> Atom {
        if matches!(t, Ty::Str) {
            return a;
        }
        let c = call("std.fmt.show", vec![t.targ()], vec![let_arg(a)], at);
        self.tmp(&Ty::Str, Rhs::Call(c), at)
    }

    pub(super) fn fallback(
        &mut self,
        x: &j::Expr,
        fb: &j::Fallback,
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        // The scrutinee, the success type, and the failure type (`None` for an optional).
        let (s, ok_ty, err_ty) = match &x.kind {
            j::ExprKind::Call(f, args) => {
                let (r, t, fail) = self.call_expr(f, args, x.span, None)?;
                match (fail, r) {
                    (Some(e), Rhs::Call(c)) => {
                        let rt = Ty::Result(Box::new(t.clone()), Box::new(e.clone()));
                        (self.tmp(&rt, Rhs::Try(c), x.span), t, Some(e))
                    }
                    (_, r) => match t {
                        Ty::Option(inner) => (
                            self.tmp(&Ty::Option(inner.clone()), r, x.span),
                            *inner,
                            None,
                        ),
                        Ty::Data(n) if n == "Regex" && !self.cx.strict => {
                            self.lenient(
                                "N40",
                                at,
                                "`else` on `regex.compile` of a literal, which cannot fail, removed",
                            );
                            return Ok((r, Ty::Data(n)));
                        }
                        Ty::Err => return Ok((r, Ty::Err)),
                        t => {
                            self.err(
                                "E201",
                                x.span,
                                format!(
                                    "`else` needs an optional or a fallible call, found `{t}`",
                                    t = self.ty_text(&t)
                                ),
                            );
                            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
                        }
                    },
                }
            }
            _ => {
                let (a, t) = self.atom(x, None)?;
                match t {
                    Ty::Option(inner) => (a, *inner, None),
                    Ty::Err => return Ok((Rhs::Atom(a), Ty::Err)),
                    t => {
                        self.err(
                            "E201",
                            x.span,
                            format!(
                                "`else` needs an optional or a fallible call, found `{t}`",
                                t = self.ty_text(&t)
                            ),
                        );
                        return Ok((Rhs::Atom(a), Ty::Err));
                    }
                }
            }
        };
        let cancels = |e: &Ty| match e {
            Ty::Union(ms) => ms.contains(&Ty::Data("Cancelled".into())),
            e => *e == Ty::Data("Cancelled".into()),
        };
        if err_ty.as_ref().is_some_and(cancels)
            && !matches!(
                fb,
                j::Fallback::Fail(_) | j::Fallback::Bind(..) | j::Fallback::Block(Some(_), _)
            )
        {
            self.discards_cancelled(x.span);
        }
        let want = want.cloned().unwrap_or_else(|| ok_ty.clone());
        let fb = fb.clone();
        let (v, t) = self.join(at, |l, k| {
            let ok = l.fresh("o");
            let ok_pat = y::Pat::Ctor {
                path: vec![if err_ty.is_some() { "Ok" } else { "Some" }.into()],
                fields: Some((vec![("value".into(), y::Pat::Bind(ok.clone()))], false)),
            };
            let ok_block = l.block_with(|l| {
                l.declare(&ok, ok_ty.clone());
                l.expect(&ok_ty, &want, x.span);
                l.jump(k, Atom::Name(ok.clone(), at), at);
                Ok(())
            })?;
            let (err_name, err_pat) = match (&fb, &err_ty) {
                (j::Fallback::Bind(n, _) | j::Fallback::Block(Some(n), _), Some(_)) => (
                    Some(n.clone()),
                    y::Pat::Ctor {
                        path: vec!["Err".into()],
                        fields: Some((vec![("error".into(), y::Pat::Bind(n.clone()))], false)),
                    },
                ),
                (_, Some(_)) => (
                    None,
                    y::Pat::Ctor {
                        path: vec!["Err".into()],
                        fields: Some((vec![("error".into(), y::Pat::Wild)], false)),
                    },
                ),
                (_, None) => (None, y::Pat::Bind("None".into())),
            };
            let err_block = l.block_with(|l| {
                if let (Some(n), Some(e)) = (&err_name, &err_ty) {
                    l.declare(n, e.clone());
                }
                match &fb {
                    j::Fallback::Value(h) | j::Fallback::Bind(_, h) => {
                        let (a, t) = l.atom(h, Some(&want))?;
                        l.expect(&t, &want, h.span);
                        l.jump(k, a, h.span);
                    }
                    j::Fallback::Return(e) => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Return(e.clone()),
                        span: at,
                    })?,
                    j::Fallback::Fail(e) => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Fail(e.clone()),
                        span: at,
                    })?,
                    j::Fallback::Break => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Break,
                        span: at,
                    })?,
                    j::Fallback::Continue => l.stmt(&j::Stmt {
                        kind: j::StmtKind::Continue,
                        span: at,
                    })?,
                    j::Fallback::Block(_, stmts) => {
                        let exits = matches!(
                            stmts.last().map(|s| &s.kind),
                            Some(
                                j::StmtKind::Return(_)
                                    | j::StmtKind::Fail(_)
                                    | j::StmtKind::Become(_)
                                    | j::StmtKind::Unreachable
                                    | j::StmtKind::Break
                                    | j::StmtKind::Continue
                                    | j::StmtKind::BreakTo(_)
                                    | j::StmtKind::ContinueTo(_)
                            )
                        );
                        if !exits {
                            return unsupported("a fallback block that does not exit");
                        }
                        l.stmts(stmts)?;
                    }
                }
                Ok(())
            })?;
            Ok((
                y::Stmt {
                    kind: StmtKind::Match {
                        scrutinee: s,
                        arms: vec![(ok_pat, ok_block), (err_pat, err_block)],
                    },
                    span: at,
                },
                want.clone(),
            ))
        })?;
        Ok((Rhs::Atom(v), t))
    }
}
