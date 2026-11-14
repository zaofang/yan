use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn match_(&mut self, scrutinee: &j::Expr, cases: &[j::Case], at: Span) -> R<()> {
        let (a, t) = self.atom(scrutinee, None)?;
        let catch_all = |p: &j::Pat| match p {
            j::Pat::Wild => true,
            j::Pat::Path { path, args: None } => {
                path.len() == 1 && !self.g.variant_of.contains_key(&path[0])
            }
            _ => false,
        };
        let open = matches!(&t, Ty::Data(d) if self.g.std_sums.contains(d) && crate::stdlib::non_exhaustive(d));
        if let Ty::Data(d) = &t
            && self.g.sums.contains_key(d)
            && !open
            && let Some(c) = cases
                .iter()
                .find(|c| matches!(c.pat, j::Pat::Wild) && c.span != at)
        {
            if self.cx.yn {
                let item = self.item.clone();
                self.cx.warn(
                    "W811",
                    c.span,
                    &item,
                    format!("`_` on the sum `{d}`; name each variant"),
                );
            } else {
                self.err(
                    "E502",
                    c.span,
                    format!("`case _` on the sum `{d}`; name each variant"),
                );
            }
        }
        let scalar = matches!(t, Ty::Int | Ty::Float | Ty::Str | Ty::Char | Ty::Fixed(_));
        let bools = |b: bool| {
            cases.iter().any(|c| {
                matches!(&c.pat, j::Pat::Lit(e) if matches!(e.kind, j::ExprKind::Lit(j::Lit::Bool(x)) if x == b))
            })
        };
        let int_cov = if matches!(t, Ty::Int) {
            crate::coverage::int_match(scrutinee, cases, |n| self.g.variant_of.contains_key(n))
        } else {
            None
        };
        let covered = cases.iter().any(|c| c.guard.is_none() && catch_all(&c.pat))
            || (matches!(t, Ty::Bool) && bools(true) && bools(false))
            || int_cov.as_ref().is_some_and(|(_, all)| *all);
        if (scalar || open || matches!(t, Ty::Bool)) && !covered {
            self.err(
                "E503",
                at,
                format!("this `match` on `{t}` needs `case _`", t = self.ty_text(&t)),
            );
        }
        let mut seen_variants: Vec<&str> = Vec::new();
        let mut closed = false;
        for (i, c) in cases.iter().enumerate() {
            let dup = matches!(&c.pat, j::Pat::Path { path, args: None }
                if path.len() == 1 && seen_variants.contains(&path[0].as_str()));
            let empty = int_cov.as_ref().is_some_and(|(dead, _)| dead[i]);
            if closed || dup || empty {
                let item = self.item.clone();
                self.cx.warn(
                    "W853",
                    c.span,
                    &item,
                    "this arm can match no value left by the arms before it; remove it".to_owned(),
                );
                self.cx.fix_remove_block(
                    yan_diag::Applicability::Auto,
                    "remove the arm",
                    c.span.line,
                );
            }
            if c.guard.is_none() {
                closed |= catch_all(&c.pat);
                if let j::Pat::Path { path, args: None } = &c.pat
                    && path.len() == 1
                    && self.g.variant_of.contains_key(&path[0])
                {
                    seen_variants.push(path[0].as_str());
                }
            }
        }
        if cases.iter().any(|c| c.guard.is_some()) {
            return self.guarded_match(a, &t, cases, at);
        }
        let mut arms = Vec::new();
        for c in cases {
            let mut pat = y::Pat::Wild;
            let b = self.block_with(|l| {
                let (inner, alias) = split_as(&c.pat);
                let (p, binds) = l.pat(inner, &t, at)?;
                pat = p;
                for (n, ty) in binds {
                    l.declare(&n, ty);
                }
                if let Some(n) = alias {
                    l.bind(n, false, &t, Rhs::Atom(a.clone()), c.span, at);
                }
                l.stmts(&c.body.stmts)
            })?;
            arms.push((pat, b));
        }
        self.emit(StmtKind::Match { scrutinee: a, arms }, at);
        Ok(())
    }

    /// Yan's `match` has no guards, so a guarded one is a block that tries each case in turn:
    /// a one-case match per case whose arm runs the body when the guard holds and leaves.
    pub(super) fn guarded_match(&mut self, a: Atom, t: &Ty, cases: &[j::Case], at: Span) -> R<()> {
        let done = self.label();
        let leave = |at| y::Stmt {
            kind: StmtKind::Break {
                label: done.clone(),
                value: None,
            },
            span: at,
        };
        let b = self.block_with(|l| {
            for c in cases {
                let mut pat = y::Pat::Wild;
                let arm = l.block_with(|l| {
                    let (inner, alias) = split_as(&c.pat);
                    let (p, binds) = l.pat(inner, t, at)?;
                    pat = p;
                    for (n, ty) in binds {
                        l.declare(&n, ty);
                    }
                    if let Some(n) = alias {
                        l.bind(n, false, t, Rhs::Atom(a.clone()), c.span, at);
                    }
                    let body = l.block_with(|l| {
                        l.stmts(&c.body.stmts)?;
                        if !ends(&c.body.stmts) {
                            l.out.push(leave(c.span));
                        }
                        Ok(())
                    })?;
                    match &c.guard {
                        None => l.out.extend(body.stmts),
                        Some(g) => {
                            let (cond, gt) = l.atom(g, Some(&Ty::Bool))?;
                            l.expect(&gt, &Ty::Bool, g.span);
                            l.emit(
                                StmtKind::If {
                                    cond,
                                    then: body,
                                    els: None,
                                },
                                c.span,
                            );
                        }
                    }
                    Ok(())
                })?;
                let rest = y::Block { stmts: Vec::new() };
                l.emit(
                    StmtKind::Match {
                        scrutinee: a.clone(),
                        arms: vec![(pat, arm), (y::Pat::Wild, rest)],
                    },
                    c.span,
                );
            }
            l.out.push(leave(at));
            Ok(())
        })?;
        self.emit(
            StmtKind::Loop {
                label: done.clone(),
                invariants: Vec::new(),
                decreases: Vec::new(),
                body: b,
            },
            at,
        );
        Ok(())
    }

    pub(super) fn pat(&mut self, p: &j::Pat, ty: &Ty, at: Span) -> R<(y::Pat, Vec<(String, Ty)>)> {
        Ok(match p {
            j::Pat::Wild => (y::Pat::Wild, Vec::new()),
            // `None` against an option is the variant, not a unit literal.
            j::Pat::Lit(e)
                if matches!(e.kind, j::ExprKind::Lit(j::Lit::None))
                    && matches!(ty, Ty::Option(_)) =>
            {
                let none = j::Pat::Path {
                    path: vec!["None".to_owned()],
                    args: None,
                };
                return self.pat(&none, ty, at);
            }
            j::Pat::Lit(e) => {
                let (l, lt) = match &e.kind {
                    j::ExprKind::Lit(l) => (literal(l)?, lit_ty(l)),
                    j::ExprKind::Unary("-", x) => match &x.kind {
                        j::ExprKind::Lit(j::Lit::Int(s)) => {
                            (Literal::Int(format!("-{}", int_text(s)?)), Ty::Int)
                        }
                        _ => return unsupported("this literal pattern"),
                    },
                    _ => return unsupported("this literal pattern"),
                };
                self.expect(&lt, ty, e.span);
                (y::Pat::Lit(l), Vec::new())
            }
            j::Pat::Path { path, args } if path.len() == 1 => {
                // A choice's result sum names its variants by label (R634).
                let labelled = match ty {
                    Ty::Data(s) if args.is_some() => {
                        let v = format!("{s}_{}", path[0]);
                        self.g
                            .variant_of
                            .get(&v)
                            .is_some_and(|o| o == s)
                            .then_some(v)
                    }
                    _ => None,
                };
                let n = labelled.as_ref().unwrap_or(&path[0]);
                let fields: Option<Vec<(String, Ty)>> = match (ty, n.as_str()) {
                    (Ty::Option(_), "None") if args.is_none() => Some(Vec::new()),
                    (Ty::Option(t), "Some") => Some(vec![("value".into(), (**t).clone())]),
                    (Ty::Result(t, _), "Ok") => Some(vec![("value".into(), (**t).clone())]),
                    (Ty::Result(_, e), "Err") => Some(vec![("error".into(), (**e).clone())]),
                    (Ty::Data(s), _) if self.g.variant_of.get(n) == Some(s) => self.g.sums[s]
                        .iter()
                        .find(|(v, _)| v == n)
                        .map(|(_, fs)| fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect()),
                    (Ty::Data(r), _) if r == n && self.g.layout_bits.contains(r) => {
                        return unsupported(format!(
                            "a pattern on `@layout(bits)` record `{r}`; read its fields instead"
                        ));
                    }
                    (Ty::Data(r), _) if r == n && self.g.records.contains_key(r) => Some(
                        self.g.records[r]
                            .iter()
                            .map(|f| (f.name.clone(), f.ty.clone()))
                            .collect(),
                    ),
                    _ => None,
                };
                match (args, fields) {
                    (None, Some(fs)) if fs.is_empty() => (y::Pat::Bind(n.clone()), Vec::new()),
                    (None, _) => (y::Pat::Bind(n.clone()), vec![(n.clone(), ty.clone())]),
                    (Some(_), None) if matches!(ty, Ty::Err) => (y::Pat::Wild, Vec::new()),
                    (Some(_), None) => {
                        self.err(
                            "E201",
                            at,
                            format!("pattern `{n}` cannot match a `{ty}`", ty = self.ty_text(ty)),
                        );
                        (y::Pat::Wild, Vec::new())
                    }
                    (Some(ps), Some(fs)) if fs.is_empty() && ps.is_empty() => {
                        (y::Pat::Bind(n.clone()), Vec::new())
                    }
                    (Some(ps), Some(fs)) => {
                        let mut pairs = Vec::new();
                        let mut binds = Vec::new();
                        for (k, (name, sub)) in ps.iter().enumerate() {
                            let f = match name {
                                Some(name) => fs.iter().find(|(f, _)| f == name),
                                None => fs.get(k),
                            };
                            let Some((fname, fty)) = f else {
                                self.err(
                                    "E203",
                                    at,
                                    format!("`{n}` has no field for this pattern"),
                                );
                                continue;
                            };
                            let (sp, mut b) = self.pat(sub, fty, at)?;
                            pairs.push((fname.clone(), sp));
                            binds.append(&mut b);
                        }
                        let rest = pairs.len() < fs.len();
                        (
                            y::Pat::Ctor {
                                path: vec![n.clone()],
                                fields: Some((pairs, rest)),
                            },
                            binds,
                        )
                    }
                }
            }
            j::Pat::Tuple(ps) => {
                let Ty::Tuple(ts) = ty else {
                    self.err(
                        "E201",
                        at,
                        format!(
                            "a tuple pattern cannot match a `{ty}`",
                            ty = self.ty_text(ty)
                        ),
                    );
                    return Ok((y::Pat::Wild, Vec::new()));
                };
                if ts.len() != ps.len() {
                    self.err(
                        "E201",
                        at,
                        format!(
                            "a {}-tuple pattern cannot match a `{ty}`",
                            ps.len(),
                            ty = self.ty_text(ty)
                        ),
                    );
                    return Ok((y::Pat::Wild, Vec::new()));
                }
                let mut out = Vec::new();
                let mut binds = Vec::new();
                for (p, t) in ps.iter().zip(ts) {
                    let (sp, mut b) = self.pat(p, t, at)?;
                    out.push(sp);
                    binds.append(&mut b);
                }
                (y::Pat::Tuple(out), binds)
            }
            _ => return unsupported("qualified and `as` patterns"),
        })
    }
}
