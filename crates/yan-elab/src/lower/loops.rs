use super::*;

impl Lower<'_, '_, '_> {
    /// `for` over an `iter[T]`: pull until it is done (R515).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn for_iter(
        &mut self,
        label: String,
        name: Option<&str>,
        target: &[String],
        it: Atom,
        e: Ty,
        span: Span,
        at: Span,
        body: impl FnOnce(&mut Self) -> R<()>,
    ) -> R<()> {
        self.loops.push((label.clone(), name.map(str::to_owned)));
        let b = self.block_with(|l| {
            let done = call(
                "std.iter.ops.done",
                vec![e.targ()],
                vec![let_arg(it.clone())],
                at,
            );
            let done = l.tmp(&Ty::Bool, Rhs::Call(done), at);
            let brk = y::Stmt {
                kind: StmtKind::Break {
                    label: label.clone(),
                    value: None,
                },
                span: at,
            };
            l.emit(
                StmtKind::If {
                    cond: done,
                    then: y::Block { stmts: vec![brk] },
                    els: None,
                },
                at,
            );
            let cur = call(
                "std.iter.ops.current",
                vec![e.targ()],
                vec![let_arg(it)],
                at,
            );
            l.bind_target(target, &e, Rhs::Call(cur), at, span);
            body(l)
        });
        self.loops.pop();
        let body = b?;
        self.emit(
            StmtKind::Loop {
                label,
                invariants: Vec::new(),
                decreases: Vec::new(),
                body,
            },
            at,
        );
        Ok(())
    }

    pub(super) fn for_(
        &mut self,
        name: Option<&str>,
        target: &[String],
        iter: &j::Expr,
        at: Span,
        body: impl FnOnce(&mut Self) -> R<()>,
    ) -> R<()> {
        let label = self.label();
        let i = self.fresh("i");
        let one = Atom::Lit(Literal::Int("1".into()), at);
        let is_range = matches!(&iter.kind, j::ExprKind::Call(f, _)
            if matches!(&f.kind, j::ExprKind::Name(n) if n == "range" && !self.locals.contains_key(n)));
        // `src` is the list and its element type; `None` iterates the counter itself.
        let mut descending = false;
        let (src, stop, step) = if is_range {
            let j::ExprKind::Call(_, args) = &iter.kind else {
                unreachable!("matched above")
            };
            let mut xs = Vec::new();
            for a in args {
                let (j::ArgKind::Expr(e), None) = (&a.kind, &a.name) else {
                    return unsupported("`range` with a keyword or reference argument");
                };
                let (x, t) = self.atom(e, Some(&Ty::Int))?;
                self.expect(&t, &Ty::Int, e.span);
                xs.push(x);
            }
            let zero = Atom::Lit(Literal::Int("0".into()), at);
            let (start, stop, step) = match xs.len() {
                1 => (zero, xs.remove(0), one.clone()),
                2 => {
                    let stop = xs.remove(1);
                    (xs.remove(0), stop, one.clone())
                }
                3 if matches!(&xs[2], Atom::Lit(Literal::Int(s), _) if s != "0") => {
                    descending =
                        matches!(&xs[2], Atom::Lit(Literal::Int(s), _) if s.starts_with('-'));
                    let step = xs.remove(2);
                    let stop = xs.remove(1);
                    (xs.remove(0), stop, step)
                }
                3 => return unsupported("`range` with a step that is not a nonzero literal"),
                _ => {
                    self.err("E202", iter.span, "`range` takes 1 to 3 arguments".into());
                    return Ok(());
                }
            };
            // A written `0` start gets its own span, which the `Explicit` mark joins.
            let written = match (&start, args.len()) {
                (Atom::Lit(Literal::Int(z), sp), 2) if z == "0" => Some(*sp),
                _ => None,
            };
            if let Some(sp) = written {
                self.cx.marks.push(crate::marks::Pending {
                    span: sp,
                    kind: crate::marks::Kind::Stmt,
                    call: None,
                    mark: yan_surface::Mark::Explicit,
                });
            }
            self.bind(
                &i,
                true,
                &Ty::Int,
                Rhs::Atom(start),
                written.unwrap_or(at),
                at,
            );
            (None, stop, step)
        } else {
            let (mut xs, t) = self.atom(iter, None)?;
            if let Ty::Coll("iter", ts) = &t {
                let e = ts[0].clone();
                return self.for_iter(label, name, target, xs, e, iter.span, at, body);
            }
            let mut len_path = "std.list.len".to_owned();
            let e = match &t {
                Ty::List(e) => (**e).clone(),
                Ty::Coll("heap", _) => {
                    self.err(
                        "E201",
                        iter.span,
                        "a `heap` is not iterable; use `h.into_sorted()`".into(),
                    );
                    return Ok(());
                }
                // Keys in ascending order, as a list.
                Ty::Coll("sorted_dict", ts) => {
                    let keys = call(
                        "std.collections.sorted_dict.keys",
                        coll_targs(&t),
                        vec![let_arg(xs)],
                        at,
                    );
                    xs = self.tmp(&Ty::List(Box::new(ts[0].clone())), Rhs::Call(keys), at);
                    ts[0].clone()
                }
                Ty::Coll(_, ts) => {
                    len_path = format!("std.{}.len", t.module().expect("a collection"));
                    ts[0].clone()
                }
                Ty::Str if self.cx.strict => {
                    let s = self.cx.text(iter.span);
                    self.err(
                        "E504",
                        iter.span,
                        format!("a `str` is not iterable; use `{s}.chars()`"),
                    );
                    return Ok(());
                }
                Ty::Str => {
                    let item = self.item.clone();
                    self.cx.note(
                        "N21",
                        iter.span,
                        &item,
                        "`for c in s` normalized to `for c in s.chars()`".into(),
                    );
                    let chars = call("std.text.chars", vec![], vec![let_arg(xs)], at);
                    let it = Ty::Coll("iter", vec![Ty::Char]);
                    let it = self.tmp(&it, Rhs::Call(chars), at);
                    return self.for_iter(label, name, target, it, Ty::Char, iter.span, at, body);
                }
                // A tuple of one type iterates as a list of its elements.
                Ty::Tuple(ts) if !ts.is_empty() && ts.iter().all(|x| *x == ts[0]) => {
                    let et = ts[0].clone();
                    let Atom::Name(root, sp) = xs else {
                        unreachable!("tuples are bound")
                    };
                    let lt = Ty::List(Box::new(et.clone()));
                    let l = self.fresh("t");
                    let new = call("std.list.new", vec![et.targ()], vec![], at);
                    self.bind(&l, true, &lt, Rhs::Call(new), at, at);
                    for k in 0..ts.len() {
                        let place = Place {
                            root: root.clone(),
                            root_span: sp,
                            projs: vec![Proj::Field(format!("_{k}"), sp)],
                        };
                        let a = self.tmp(&et, Rhs::Copy(place), at);
                        let push = call(
                            "std.list.append",
                            vec![et.targ()],
                            vec![
                                Arg::Value(Conv::Inout, Atom::Name(l.clone(), at), at),
                                Arg::Value(Conv::Sink, a, at),
                            ],
                            at,
                        );
                        self.emit(StmtKind::Rhs(Rhs::Call(push)), at);
                    }
                    xs = Atom::Name(l, at);
                    et
                }
                Ty::Tuple(_) => {
                    self.err(
                        "E201",
                        iter.span,
                        format!(
                            "a `{t}` has elements of different types; only a tuple of one type \
                             can be iterated",
                            t = self.ty_text(&t)
                        ),
                    );
                    return Ok(());
                }
                Ty::Err => return Ok(()),
                t => return unsupported(format!("iterating a `{t}`", t = self.ty_text(t))),
            };
            let zero = Atom::Lit(Literal::Int("0".into()), at);
            self.bind(&i, true, &Ty::Int, Rhs::Atom(zero), at, at);
            let len = call(&len_path, vec![e.targ()], vec![let_arg(xs.clone())], at);
            let n = self.tmp(&Ty::Int, Rhs::Call(len), at);
            (Some((xs, e)), n, one)
        };
        self.loops.push((label.clone(), name.map(str::to_owned)));
        let b = self.block_with(|l| {
            let ge = call(
                if descending { "le_i64" } else { "ge_i64" },
                vec![],
                vec![let_arg(Atom::Name(i.clone(), at)), let_arg(stop)],
                at,
            );
            let done = l.tmp(&Ty::Bool, Rhs::Call(ge), at);
            let brk = y::Stmt {
                kind: StmtKind::Break {
                    label: label.clone(),
                    value: None,
                },
                span: at,
            };
            l.emit(
                StmtKind::If {
                    cond: done,
                    then: y::Block { stmts: vec![brk] },
                    els: None,
                },
                at,
            );
            let (cur, ty) = match src {
                None => (Rhs::Atom(Atom::Name(i.clone(), at)), Ty::Int),
                Some((Atom::Name(xs, sp), e)) => (
                    Rhs::Proj(Place {
                        root: xs,
                        root_span: sp,
                        projs: vec![Proj::Index(Atom::Name(i.clone(), at))],
                    }),
                    e,
                ),
                Some(_) => return unsupported("iterating a literal"),
            };
            l.bind_target(target, &ty, cur, at, iter.span);
            let next = call(
                "add_checked_i64",
                vec![],
                vec![let_arg(Atom::Name(i.clone(), at)), let_arg(step)],
                at,
            );
            l.emit(
                StmtKind::Assign {
                    place: Place {
                        root: i.clone(),
                        root_span: at,
                        projs: Vec::new(),
                    },
                    rhs: Rhs::Call(next),
                    rhs_span: at,
                },
                at,
            );
            body(l)
        });
        self.loops.pop();
        let body = b?;
        self.emit(
            StmtKind::Loop {
                label,
                invariants: Vec::new(),
                decreases: Vec::new(),
                body,
            },
            at,
        );
        Ok(())
    }

    /// A loop that fills a fresh collection. Its element type is known only once the element
    /// is lowered inside the loop, so the collection's `let` is patched afterwards.
    /// `par` comprehensions run in source order, which is what their results observe (12.2).
    pub(super) fn comprehension(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Rhs, Ty)> {
        let j::ExprKind::Comp {
            kind,
            elem,
            value,
            target,
            iter,
            cond,
            ..
        } = &e.kind
        else {
            unreachable!("called on a comprehension")
        };
        let (kind, value, cond, at) = (*kind, value.as_deref(), cond.as_deref(), e.span);
        let elem_want = match (kind, want) {
            (j::CompKind::List, Some(Ty::List(t))) | (j::CompKind::Set, Some(Ty::Set(t))) => {
                (**t).clone()
            }
            _ => Ty::Err,
        };
        let acc = self.fresh("c");
        let new = match kind {
            j::CompKind::List => "std.list.new",
            j::CompKind::Set => "std.set.new",
            j::CompKind::Dict => "std.dict.new",
        };
        self.bind(
            &acc,
            true,
            &Ty::Err,
            Rhs::Call(call(new, vec![], vec![], at)),
            at,
            at,
        );
        let slot = self.out.len() - 1;
        self.cx.marks.push(crate::marks::Pending {
            span: at,
            kind: crate::marks::Kind::Stmt,
            call: None,
            mark: yan_surface::Mark::Comprehension,
        });
        let mut found: Option<(Ty, Option<Ty>)> = None;
        let acc_arg = Arg::Value(Conv::Inout, Atom::Name(acc.clone(), at), at);
        self.for_(None, target, iter, at, |l| {
            let mut push = |l: &mut Self| -> R<()> {
                let (x, xt) = l.atom(elem, None)?;
                let (x, xt) = l.widen_atom(x, xt, &elem_want, elem.span)?;
                let (path, mut args, mut targs) = match kind {
                    j::CompKind::List => {
                        ("std.list.append", vec![acc_arg.clone()], vec![xt.targ()])
                    }
                    j::CompKind::Set => ("std.set.add", vec![acc_arg.clone()], vec![xt.targ()]),
                    j::CompKind::Dict => {
                        ("std.dict.insert", vec![acc_arg.clone()], vec![xt.targ()])
                    }
                };
                let vt = match (kind, value) {
                    (j::CompKind::Dict, Some(v)) => {
                        let (va, vt) = l.atom(v, None)?;
                        targs.push(vt.targ());
                        args.push(let_arg(x));
                        args.push(sink_arg(va));
                        Some(vt)
                    }
                    _ => {
                        args.push(sink_arg(x));
                        None
                    }
                };
                found = Some((xt, vt));
                l.emit(StmtKind::Rhs(Rhs::Call(call(path, targs, args, at))), at);
                Ok(())
            };
            match cond {
                None => push(l),
                Some(c) => {
                    let (ca, ct) = l.atom(c, Some(&Ty::Bool))?;
                    l.expect(&ct, &Ty::Bool, c.span);
                    let then = l.block_with(push)?;
                    l.emit(
                        StmtKind::If {
                            cond: ca,
                            then,
                            els: None,
                        },
                        at,
                    );
                    Ok(())
                }
            }
        })?;
        let ty = match (kind, found) {
            (_, None) => Ty::Err,
            (j::CompKind::List, Some((x, _))) => Ty::List(Box::new(x)),
            (j::CompKind::Set, Some((x, _))) => Ty::Set(Box::new(x)),
            (j::CompKind::Dict, Some((k, v))) => {
                Ty::Dict(Box::new(k), Box::new(v.unwrap_or(Ty::Err)))
            }
        };
        let targs = match &ty {
            Ty::List(x) | Ty::Set(x) => vec![x.targ()],
            Ty::Dict(k, v) => vec![k.targ(), v.targ()],
            _ => vec![],
        };
        if let StmtKind::Let {
            ty: t,
            rhs: Rhs::Call(c),
            ..
        } = &mut self.out[slot].kind
        {
            *t = ty.core();
            c.targs = targs;
        }
        self.declare(&acc, ty.clone());
        Ok((Rhs::Atom(Atom::Name(acc, at)), ty))
    }
}
