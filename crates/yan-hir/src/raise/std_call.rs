use super::*;

impl<'a> Fold<'a> {
    /// A left-to-right concatenation chain (R478) as an f-string when it shows a value or
    /// has literal pieces, and as `+` otherwise.
    pub(super) fn concat(&mut self, c: &y::Call) -> R<Expr> {
        let vals = self.values(c);
        let [a, b] = vals.as_slice() else {
            return no("`std.text.concat` with other than two arguments");
        };
        let mark = self.marks.call_mark(c, &Mark::Operator);
        let mut parts = Vec::new();
        let mut shown = false;
        for x in [a, b] {
            match self.atom(x)? {
                Expr {
                    kind: ExprKind::FStr { pieces, .. },
                    ..
                } if parts.is_empty() || pieces.len() == 1 => {
                    shown = true;
                    parts.extend(pieces)
                }
                Expr {
                    kind: ExprKind::Lit(Lit::Str { prefix, value }),
                    ..
                } if prefix.is_empty() => parts.push(FPiece::Lit(value)),
                x => parts.push(FPiece::Expr(x, None)),
            }
        }
        let lits = parts.iter().filter(|p| matches!(p, FPiece::Lit(_))).count();
        let fmt = shown || (lits > 0 && lits < parts.len());
        // Written with `+`: every part is a `str` operand, none a shown value or format spec.
        let plus = mark.is_some()
            && !shown
            && parts
                .iter()
                .all(|p| matches!(p, FPiece::Lit(_) | FPiece::Expr(_, None)));
        if let Some(m) = mark {
            if plus {
                self.marks.accept(m)
            } else {
                self.marks.refuse(m)
            }
        }
        if fmt && !plus {
            let mut merged: Vec<FPiece> = Vec::new();
            for p in parts {
                match (merged.last_mut(), p) {
                    (Some(FPiece::Lit(a)), FPiece::Lit(b)) => a.push_str(&b),
                    (_, p) => merged.push(p),
                }
            }
            return Ok(e(ExprKind::FStr {
                template: false,
                pieces: merged,
            }));
        }
        let mut xs = parts.into_iter().map(|p| match p {
            FPiece::Expr(x, _) => x,
            FPiece::Lit(value) => e(ExprKind::Lit(Lit::Str {
                prefix: String::new(),
                value,
            })),
        });
        let first = xs.next().expect("two parts");
        Ok(xs.fold(first, |a, b| {
            e(ExprKind::Binary("+", Box::new(a), Box::new(b)))
        }))
    }

    pub(super) fn std_call(&mut self, c: &y::Call, module: &str, f: &str) -> R<Expr> {
        let vals = self.values(c);
        // Collection constructors and builtins (13.3).
        match (module, f, vals.len()) {
            ("list", "new", 0) => return Ok(e(ExprKind::List(Vec::new()))),
            ("dict", "new", 0) => return Ok(e(ExprKind::Dict(Vec::new()))),
            ("set", "new", 0) => return Ok(call(name("set"), Vec::new())),
            (m, "len", 1) if TYPE_MODULES.contains(&m) => {
                let x = self.atom(vals[0])?;
                return Ok(match self.marks.spelling(c) {
                    Some(Mark::Method) => call(attr(x, "len"), Vec::new()),
                    _ => call(name("len"), vec![arg(x)]),
                });
            }
            // `len(s)` on a `str` elaborates to `char_count` (N21).
            ("text", "char_count", 1) if self.marks.spelling(c) == Some(Mark::Function) => {
                let x = self.atom(vals[0])?;
                return Ok(call(name("len"), vec![arg(x)]));
            }
            ("prelude", f, _) => {
                let args = self.std_args(c, &format!("std.prelude.{f}"), 0)?;
                return Ok(call(name(f), args));
            }
            ("cmp", f, 2) if binary(&format!("{f}_")).is_some_and(|o| COMPARE.contains(&o)) => {
                let op = binary(&format!("{f}_")).expect("matched");
                let (a, b) = (self.atom(vals[0])?, self.atom(vals[1])?);
                return Ok(e(ExprKind::Compare(op, Box::new(a), Box::new(b))));
            }
            // A pattern with a backslash is written raw.
            ("regex", "compile", 1) => {
                let mut x = self.atom(vals[0])?;
                raw_if_backslash(&mut x);
                return Ok(call(attr(name("regex"), f), vec![arg(x)]));
            }
            ("error", "context", 2) => {
                let recv = self.atom(vals[0])?;
                let args = self.args(&c.args[1..])?;
                return Ok(call(attr(recv, f), args));
            }
            ("list", "concat", 2) if self.marks.spelling(c) != Some(Mark::Method) => {
                let (a, b) = (self.atom(vals[0])?, self.atom(vals[1])?);
                return Ok(e(ExprKind::Binary("+", Box::new(a), Box::new(b))));
            }
            ("dict" | "collections.sorted_dict", "index", 2)
                if self.marks.spelling(c) != Some(Mark::Method) =>
            {
                let (d, k) = (self.atom(vals[0])?, self.atom(vals[1])?);
                return Ok(e(ExprKind::Index(Box::new(d), Box::new(k))));
            }
            (m, "contains" | "contains_key", 2) if TYPE_MODULES.contains(&m) => {
                let (coll, x) = (self.atom(vals[0])?, self.atom(vals[1])?);
                return Ok(e(ExprKind::Compare("in", Box::new(x), Box::new(coll))));
            }
            _ => {}
        }
        // A capability operation is a method of the capability.
        if let Some(y::Arg::Cap(cap, _)) = c.args.first() {
            let args = self.args(&c.args[1..])?;
            return Ok(call(attr(name(cap), f), args));
        }
        // A method of the type its module declares (17.2): its first parameter is `self`.
        let path = format!("std.{module}.{f}");
        if let Some(recv) = vals.first()
            && let Some(tm) = self.recv_module(recv)
            && (module == tm || module.starts_with(&format!("{tm}.")))
            && self
                .std
                .params(&path)
                .is_none_or(|ps| ps.first().is_some_and(|p| p.name == "self"))
        {
            let recv = self.atom(recv)?;
            let args = self.std_args(c, &format!("std.{module}.{f}"), 1)?;
            return Ok(call(attr(recv, f), args));
        }
        let args = self.std_args(c, &format!("std.{module}.{f}"), 0)?;
        let callee = if module == "test" {
            name(f)
        } else if let Some(t) = static_type(module, f) {
            attr(name(t), f)
        } else {
            attr(
                path_expr(&module.split('.').map(str::to_owned).collect::<Vec<_>>()),
                f,
            )
        };
        Ok(call(callee, args))
    }

    pub(super) fn recv_module(&self, a: &y::Atom) -> Option<String> {
        if self.atom_is_str(a) || matches!(a, y::Atom::Lit(y::Literal::Char(_), _)) {
            return Some("text".into());
        }
        let t = self.atom_ty(a)?;
        if let Some(m) = type_module(t) {
            return Some(m.into());
        }
        match t {
            y::Type::Named(n) if n.path.len() == 1 => self.std.type_module(&n.path[0]),
            _ => None,
        }
    }

    /// A std call's arguments from the `skip`th on: those equal to the parameter's default
    /// are dropped, and later ones are named, as Yan Agent writes them (13.3).
    pub(super) fn std_args(&mut self, c: &y::Call, path: &str, skip: usize) -> R<Vec<Arg>> {
        let mut args = self.args(&c.args)?;
        let marks = self.marks.args(c);
        let module = match c.path.as_slice() {
            [_, m, _] => Some(m.as_str()),
            _ => None,
        };
        for (a, q) in args.iter_mut().zip(self.marks.qualified(c)) {
            let Some(q) = q else { continue };
            if let (ArgKind::Expr(x), Some(module)) = (&mut a.kind, module)
                && let ExprKind::Call(f, xs) = &x.kind
                && xs.is_empty()
                && let ExprKind::Name(v) = &f.kind
                && variant(v)
                && std_sum_variant(module, v)
            {
                *x = call(attr(name(module), v), Vec::new());
                self.marks.accept(q);
            } else {
                self.marks.refuse(q);
            }
        }
        let Some(params) = self.std.params(path) else {
            return Ok(args.split_off(skip.min(args.len())));
        };
        let mut out = Vec::new();
        // Yan Agent passes an optional parameter by keyword, except the first argument after the
        // receiver or a positional-only one, and leaves it out when it has its default.
        for (i, a) in args.into_iter().enumerate().skip(skip) {
            let (Some(p), ArgKind::Expr(x)) = (params.get(i), &a.kind) else {
                out.push(a);
                continue;
            };
            // yan-elab passes a list or a `str` to an `iter` parameter as `x.iter()`.
            let a = match &x.kind {
                ExprKind::Call(f, xs) if p.iter && xs.is_empty() && !self.marks.sourced => {
                    match &f.kind {
                        ExprKind::Attr(r, m, _) if m == "iter" => Arg {
                            kind: ArgKind::Expr((**r).clone()),
                            ..a
                        },
                        _ => a,
                    }
                }
                _ => a,
            };
            let ArgKind::Expr(x) = &a.kind else {
                unreachable!("an expression argument")
            };
            let mark = marks.get(i).and_then(Option::as_ref).map(|(m, _)| m);
            match (mark, &p.default) {
                // A marked argument was written, even if it equals the default.
                (Some(Mark::Keyword), _) if !p.positional => out.push(Arg {
                    name: Some(p.name.clone()),
                    ..a
                }),
                (Some(Mark::Keyword), _) => {
                    self.marks.reject(&marks[i]);
                    out.push(a)
                }
                (Some(_), _) => out.push(a),
                (None, Some(d)) if same(x, d) => {}
                (None, Some(_)) if i > skip && !p.positional => out.push(Arg {
                    name: Some(p.name.clone()),
                    ..a
                }),
                _ => out.push(a),
            }
        }
        Ok(out)
    }
}
