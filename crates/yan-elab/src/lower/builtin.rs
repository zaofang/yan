use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn builtin(
        &mut self,
        n: &str,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err);
        match n {
            "len" | "sorted" | "int" | "float" | "round" | "abs" | "str" | "bigint" | "list" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(err);
                };
                if n == "len" {
                    self.mark_call(at, yan_surface::Mark::Function);
                }
                let (a, t) = self.atom(xs[0], None)?;
                let (path, targs, ret) = match (n, &t) {
                    (_, Ty::Err) => return Ok((Rhs::Atom(a), Ty::Err)),
                    ("len", Ty::List(e)) => ("std.list.len", vec![e.targ()], Ty::Int),
                    ("len", Ty::Set(e)) => ("std.set.len", vec![e.targ()], Ty::Int),
                    ("len", Ty::Dict(k, v)) => ("std.dict.len", vec![k.targ(), v.targ()], Ty::Int),
                    ("len", Ty::Coll(..)) => {
                        let path = format!("std.{}.len", t.module().expect("a collection"));
                        let c = call(&path, coll_targs(&t), vec![let_arg(a)], at);
                        return Ok((Rhs::Call(c), Ty::Int));
                    }
                    ("len", Ty::Str) if self.cx.strict => {
                        let s = self.cx.text(xs[0].span);
                        self.err(
                            "E504",
                            at,
                            format!("`len` of a `str`; use `{s}.char_count()` or `{s}.byte_len()`"),
                        );
                        return Ok(err);
                    }
                    ("len", Ty::Str) => {
                        let item = self.item.clone();
                        self.cx.note(
                            "N21",
                            at,
                            &item,
                            "`len(s)` normalized to `s.char_count()`".into(),
                        );
                        ("std.text.char_count", vec![], Ty::Int)
                    }
                    ("sorted", Ty::List(e)) => ("std.prelude.sorted", vec![e.targ()], t.clone()),
                    ("list", Ty::List(e)) => {
                        let c = call("std.prelude.list", vec![e.targ()], vec![sink_arg(a)], at);
                        return Ok((Rhs::Call(c), t.clone()));
                    }
                    ("list", Ty::Coll("iter", _)) => {
                        let (r, ty) = self.method_on(a, &t, "collect", at)?;
                        self.lenient("N37", at, "`list(it)` normalized to `it.collect()`");
                        return Ok((r, ty));
                    }
                    ("bigint", Ty::Int) => ("std.prelude.bigint", vec![], Ty::BigInt),
                    ("bigint", Ty::BigInt) => return Ok((Rhs::Atom(a), t)),
                    ("int", Ty::Int) | ("float", Ty::Float) => return Ok((Rhs::Atom(a), t)),
                    ("int", Ty::Float) => ("std.prelude.int", vec![], Ty::Int),
                    ("int", Ty::Str) if !self.cx.strict => {
                        self.lenient("N51", at, "`int(s)` normalized to `std.text.int_of(s)`");
                        ("std.text.int_of", vec![], Ty::Int)
                    }
                    ("float", Ty::Int) => ("std.prelude.float", vec![], Ty::Float),
                    ("round", Ty::Float) => ("std.prelude.round", vec![], Ty::Int),
                    ("abs", Ty::Int | Ty::Float) => ("std.prelude.abs", vec![t.targ()], t.clone()),
                    ("str", _) => return Ok((Rhs::Atom(self.show(a, &t, at)), Ty::Str)),
                    _ => {
                        self.err(
                            "E201",
                            xs[0].span,
                            format!("`{n}` does not take a `{t}`", t = self.ty_text(&t)),
                        );
                        return Ok(err);
                    }
                };
                Ok((Rhs::Call(call(path, targs, vec![let_arg(a)], at)), ret))
            }
            "min" | "max" => {
                let Some(xs) = self.positional(args, 2, n, at)? else {
                    return Ok(err);
                };
                let (a, t) = self.atom(xs[0], None)?;
                let (b, u) = self.atom(xs[1], Some(&t))?;
                self.expect(&u, &t, xs[1].span);
                let path = format!("std.prelude.{n}");
                Ok((
                    Rhs::Call(call(
                        &path,
                        vec![t.targ()],
                        vec![let_arg(a), let_arg(b)],
                        at,
                    )),
                    t,
                ))
            }
            "pow" => {
                let Some(xs) = self.positional(args, 2, n, at)? else {
                    return Ok(err);
                };
                let r = self.arith("**", xs[0], xs[1], at, want)?;
                if matches!(r.0, Rhs::Call(_)) {
                    self.mark_call(at, yan_surface::Mark::Prelude);
                }
                Ok(r)
            }
            // `(a // b, a % b)`, each operand evaluated once (R801).
            "divmod" => {
                let Some(xs) = self.positional(args, 2, n, at)? else {
                    return Ok(err);
                };
                let a = self.reusable(xs[0])?;
                let b = self.reusable(xs[1])?;
                let ex = |kind| j::Expr {
                    kind,
                    span: at,
                    paren: false,
                };
                let op = |o| {
                    ex(j::ExprKind::Binary(
                        o,
                        Box::new(a.clone()),
                        Box::new(b.clone()),
                    ))
                };
                self.cx.marks.push(crate::marks::Pending {
                    span: at,
                    kind: crate::marks::Kind::Stmt,
                    call: None,
                    mark: yan_surface::Mark::Prelude,
                });
                self.rhs(&ex(j::ExprKind::Tuple(vec![op("//"), op("%")])), want)
            }
            "print" => {
                let (r, t, _) = self.cap_call("io", "print", args, at)?;
                Ok((r, t))
            }
            "evolve" => {
                let Some(first) = args.first().filter(|a| a.name.is_none()) else {
                    self.err("E202", at, "`evolve` takes a record first".into());
                    return Ok(err);
                };
                let j::ArgKind::Expr(c) = &first.kind else {
                    return unsupported("`evolve` of a reference");
                };
                let (p, t) = self.place(c)?;
                let Ty::Data(r) = &t else {
                    self.err(
                        "E201",
                        c.span,
                        format!("`evolve` takes a record, found `{t}`", t = self.ty_text(&t)),
                    );
                    return Ok(err);
                };
                let Some(fs) = self.g.records.get(r) else {
                    self.err(
                        "E201",
                        c.span,
                        format!("`evolve` takes a record, found `{t}`", t = self.ty_text(&t)),
                    );
                    return Ok(err);
                };
                let fs: Vec<(String, Ty)> =
                    fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
                let v = self.fresh("t");
                self.bind(&v, true, &t, Rhs::Copy(p), at, at);
                for a in &args[1..] {
                    let (Some(name), j::ArgKind::Expr(e)) = (&a.name, &a.kind) else {
                        self.err("E202", a.span, "`evolve` takes fields by keyword".into());
                        continue;
                    };
                    let Some((_, ft)) = fs.iter().find(|(f, _)| f == name) else {
                        let names = fs.iter().map(|(f, _)| f.as_str());
                        let msg = format!("`{r}` has no field `{name}`");
                        self.err_near("E203", a.span, msg, name, names);
                        continue;
                    };
                    let (x, xt) = self.atom(e, Some(ft))?;
                    self.expect(&xt, ft, e.span);
                    self.emit(
                        StmtKind::Assign {
                            place: Place {
                                root: v.clone(),
                                root_span: at,
                                projs: vec![Proj::Field(name.clone(), a.span)],
                            },
                            rhs: Rhs::Atom(x),
                            rhs_span: e.span,
                        },
                        a.span,
                    );
                }
                Ok((Rhs::Atom(Atom::Name(v, at)), t))
            }
            "Ok" | "Err" | "Some" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(err);
                };
                let (field, inner) = match (n, want) {
                    ("Ok", Some(Ty::Result(t, _))) => ("value", (**t).clone()),
                    ("Err", Some(Ty::Result(_, e))) => ("error", (**e).clone()),
                    ("Some", Some(Ty::Option(t))) => ("value", (**t).clone()),
                    _ => return unsupported(format!("`{n}(...)` without an expected type")),
                };
                let (a, t) = self.atom(xs[0], Some(&inner))?;
                self.expect(&t, &inner, xs[0].span);
                Ok((
                    Rhs::Ctor(y::Ctor::Named {
                        path: vec![n.to_owned()],
                        targs: Vec::new(),
                        fields: vec![(field.to_owned(), a)],
                    }),
                    want.cloned().expect("matched above"),
                ))
            }
            "range" => {
                let n = args.len();
                if !(1..=2).contains(&n) {
                    self.err(
                        "E202",
                        at,
                        format!("`range` takes 1 or 2 arguments, {n} given"),
                    );
                    return Ok(err);
                }
                let Some(xs) = self.positional(args, n, "range", at)? else {
                    return Ok(err);
                };
                let mut out = Vec::new();
                for x in xs {
                    let (a, t) = self.atom(x, Some(&Ty::Int))?;
                    self.expect(&t, &Ty::Int, x.span);
                    out.push(let_arg(a));
                }
                let c = call("std.prelude.range", vec![], out, at);
                Ok((Rhs::Call(c), Ty::List(Box::new(Ty::Int))))
            }
            "deque" | "heap" | "sorted_dict" | "sorted_set" => self.collection(n, args, at, want),
            "Max" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(err);
                };
                let (a, t) = self.atom(xs[0], None)?;
                let c = call("std.lattice.max_of", vec![t.targ()], vec![sink_arg(a)], at);
                Ok((Rhs::Call(c), Ty::Coll("Max", vec![t])))
            }
            "Secret" => {
                let Some(xs) = self.positional(args, 1, n, at)? else {
                    return Ok(err);
                };
                let inner = match want {
                    Some(Ty::Coll("Secret", ts)) => ts.first().cloned(),
                    _ => None,
                };
                let (a, t) = self.atom(xs[0], inner.as_ref())?;
                let c = call("std.prelude.Secret", vec![t.targ()], vec![sink_arg(a)], at);
                Ok((Rhs::Call(c), Ty::Coll("Secret", vec![t])))
            }
            "dict" | "set" | "enumerate" | "zip" | "reversed" | "isinstance" | "hash" | "repr"
            | "atan2" | "hypot" => unsupported(format!("`{n}`")),
            // std.test is in scope without an import (assumption A7).
            _ if let Some((path, p)) = ["prelude", "test"].iter().find_map(|m| {
                let path = format!("std.{m}.{n}");
                let p = crate::stdlib::decl(&path)?.plain()?;
                Some((path, p))
            }) =>
            {
                if p.fail.is_some() {
                    return unsupported(format!("`{path}`, a std function that can fail"));
                }
                self.civil_literal(n, args, at);
                let (r, t, _) = self.std_call(&path, p, Vec::new(), args, n, at)?;
                Ok((r, t))
            }
            _ => {
                let mut names: Vec<&str> = self.g.fns.keys().map(String::as_str).collect();
                names.extend(BUILTINS);
                names.extend(crate::stdlib::members("prelude"));
                names.extend(crate::stdlib::members("test"));
                self.err_near("E200", at, format!("cannot find function `{n}`"), n, names);
                for a in args {
                    if let j::ArgKind::Expr(e) | j::ArgKind::Ref(e) = &a.kind {
                        self.atom(e, None)?;
                    }
                }
                Ok(err)
            }
        }
    }
}
