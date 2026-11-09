use super::*;

impl Lower<'_, '_, '_> {
    pub(super) fn atom(&mut self, e: &j::Expr, want: Option<&Ty>) -> R<(Atom, Ty)> {
        if want == Some(&Ty::Type)
            && let Some(jt) = self.type_expr(e)
        {
            let t = self.g.handle(&jt, self.cx, e.span)?;
            // A function with a `Type` parameter runs only at stage 0 (10.4).
            if !self.item.starts_with("__stage0_")
                && !self.locals.values().any(|l| l.ty == Ty::Type)
            {
                self.err(
                    "E262",
                    e.span,
                    "a `Type` exists only at stage 0; evaluate this under `#`".into(),
                );
            }
            let r = Rhs::Call(call("std.meta.type_of", vec![t], vec![], e.span));
            return Ok((self.tmp(&Ty::Type, r, e.span), Ty::Type));
        }
        let (r, t) = self.rhs(e, want)?;
        let (r, t) = self.widen(r, t, want, e.span)?;
        let (r, t) = self.inject(r, t, want, e.span);
        Ok((self.tmp(&t, r, e.span), t))
    }

    /// `e` read as a type in argument position (RFC 0051 D1), unless it names a value.
    pub(super) fn type_expr(&self, e: &j::Expr) -> Option<j::Type> {
        Some(match &e.kind {
            j::ExprKind::Name(n) if self.locals.contains_key(n) || self.g.fns.contains_key(n) => {
                return None;
            }
            j::ExprKind::Name(n) => j::Type::Named {
                path: vec![core_type_name(n)],
                args: Vec::new(),
            },
            j::ExprKind::Index(b, k) => {
                let j::Type::Named { path, .. } = self.type_expr(b)? else {
                    return None;
                };
                let ks = match &k.kind {
                    j::ExprKind::Tuple(ks) => ks.iter().collect(),
                    _ => vec![&**k],
                };
                let mut args = Vec::new();
                for k in ks {
                    args.push(j::TArg::Type(self.type_expr(k)?));
                }
                j::Type::Named { path, args }
            }
            _ => return None,
        })
    }

    /// The type of the integer literal `text`: the expected integer type when its range holds
    /// the value (3.4), else `int`. A value outside the expected type's range is E201.
    pub(super) fn int_lit_ty(&mut self, text: &str, want: Option<&Ty>, at: Span) -> Ty {
        let t = match want {
            Some(w @ Ty::Fixed(n)) if !n.starts_with('f') => w.clone(),
            _ => Ty::Int,
        };
        let fits = text.parse::<i128>().ok().is_some_and(|v| {
            let (lo, hi) = int_range(t.scalar().expect("an integer type"));
            lo <= v && v <= hi
        });
        if !fits {
            let hint = if t == Ty::Int { "; use `bigint`" } else { "" };
            self.err(
                "E201",
                at,
                format!("`{text}` does not fit in `{t}`{hint}", t = self.ty_text(&t)),
            );
            return Ty::Err;
        }
        t
    }

    /// The `bigint` of decimal `text`, built from `int` pieces of at most 18 digits.
    pub(super) fn bigint_lit(&mut self, text: &str, at: Span) -> Rhs {
        let (neg, digits) = match text.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, text),
        };
        let piece = |l: &mut Self, d: &str| {
            let d = d.trim_start_matches('0');
            let lit = Atom::Lit(Literal::Int(if d.is_empty() { "0" } else { d }.into()), at);
            let c = call("std.prelude.bigint", vec![], vec![let_arg(lit)], at);
            l.tmp(&Ty::BigInt, Rhs::Call(c), at)
        };
        let head = digits.len() % 18;
        let (first, rest) = digits.split_at(if head == 0 {
            18.min(digits.len())
        } else {
            head
        });
        if rest.is_empty() && !neg {
            // One piece is the plain conversion `bigint(n)`, bound like one the author wrote.
            let d = first.trim_start_matches('0');
            let lit = Atom::Lit(Literal::Int(if d.is_empty() { "0" } else { d }.into()), at);
            self.cx
                .origins
                .push((at, yan_surface::Inserted::Conversion));
            return Rhs::Call(call("std.prelude.bigint", vec![], vec![let_arg(lit)], at));
        }
        let mut acc = piece(self, first);
        if !rest.is_empty() {
            let base = piece(self, "1000000000000000000");
            for k in (0..rest.len()).step_by(18) {
                let shifted = call(
                    "mul_bigint",
                    vec![],
                    vec![let_arg(acc), let_arg(base.clone())],
                    at,
                );
                let shifted = self.tmp(&Ty::BigInt, Rhs::Call(shifted), at);
                let p = piece(self, &rest[k..k + 18]);
                let sum = call("add_bigint", vec![], vec![let_arg(shifted), let_arg(p)], at);
                acc = self.tmp(&Ty::BigInt, Rhs::Call(sum), at);
            }
        }
        if !neg {
            return Rhs::Atom(acc);
        }
        let z = piece(self, "0");
        Rhs::Call(call(
            "sub_bigint",
            vec![],
            vec![let_arg(z), let_arg(acc)],
            at,
        ))
    }

    /// A `U` where `U | None` is expected is `Some` of it, once, even when `U` is optional
    /// (R794).
    pub(super) fn inject(&mut self, r: Rhs, t: Ty, want: Option<&Ty>, at: Span) -> (Rhs, Ty) {
        let Some(Ty::Option(inner)) = want else {
            return (r, t);
        };
        if matches!(t, Ty::Err | Ty::Never)
            || t.option_depth() >= (want.expect("matched")).option_depth()
        {
            return (r, t);
        }
        let mut level = &**inner;
        while let Ty::Option(x) = level {
            if level.option_depth() <= t.option_depth() {
                break;
            }
            level = x;
        }
        if !t.fits(level) {
            return (r, t);
        }
        if t.option_depth() < inner.option_depth() {
            let wanted = self.ty_text(want.expect("matched"));
            self.err(
                "E201",
                at,
                format!(
                    "a `{t}` where `{wanted}` is expected; write `Some(v)`",
                    t = self.ty_text(&t)
                ),
            );
            if let Some(text) = span_text(self.cx.src, at) {
                self.cx
                    .fix(Applicability::Auto, "wrap", at, &format!("Some({text})"));
            }
            return (r, Ty::Err);
        }
        let a = self.tmp(&t, r, at);
        let ctor = y::Ctor::Named {
            path: vec!["Some".into()],
            targs: Vec::new(),
            fields: vec![("value".into(), a)],
        };
        (Rhs::Ctor(ctor), Ty::Option(Box::new(t)))
    }

    /// The lenient reader's widenings (14.4): N36 takes a `char` where `str` is expected as its
    /// one-character string, a `list[char]` element by element; N37 collects an `iter` where a
    /// `list` is expected.
    pub(super) fn widen(&mut self, r: Rhs, t: Ty, want: Option<&Ty>, at: Span) -> R<(Rhs, Ty)> {
        let Some(w) = want.filter(|_| !self.cx.strict) else {
            return Ok((r, t));
        };
        match (&t, w) {
            (Ty::Char, Ty::Str) => {
                self.lenient(
                    "N36",
                    at,
                    "a `char` where `str` is expected normalized to `.to_str()`",
                );
                self.char_str(r, at)
            }
            (Ty::Coll("iter", _), Ty::List(_)) if self.can_collect() => {
                self.lenient(
                    "N37",
                    at,
                    "an `iter` where a `list` is expected normalized to `.collect()`",
                );
                let (r, t) = self.collect(r, &t, at)?;
                self.widen(r, t, want, at)
            }
            // An integer literal that is not exact as a float stays E201 (R656).
            (Ty::Int, Ty::Float) if !matches!(r, Rhs::Atom(Atom::Lit(..))) => {
                self.lenient(
                    "N38",
                    at,
                    "an `int` where `float` is expected normalized to `float(...)`",
                );
                self.int_float(r, at)
            }
            (Ty::Str, Ty::Data(w)) if w == "Regex" => {
                let Rhs::Atom(Atom::Lit(Literal::Str(value), sp)) = r else {
                    return Ok((r, t));
                };
                self.lenient(
                    "N40",
                    at,
                    "a pattern `str` where `Regex` is expected normalized to `regex.compile(...)`",
                );
                let lit = j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(j::Expr {
                        kind: j::ExprKind::Lit(j::Lit::Str {
                            prefix: String::new(),
                            value,
                        }),
                        span: sp,
                        paren: false,
                    }),
                    span: sp,
                };
                let (r, t, _) = self.module_call("regex", "compile", &[lit], at)?;
                self.mark_call(at, yan_surface::Mark::Lenient("N40".into()));
                Ok((r, t))
            }
            (Ty::List(e), Ty::List(we)) if **e == Ty::Char && **we == Ty::Str => {
                let xs = self.tmp(&t, r, at);
                let Atom::Name(xs, _) = xs else {
                    return Ok((Rhs::Atom(xs), t));
                };
                self.cx.marks.push(crate::marks::Pending {
                    span: at,
                    kind: crate::marks::Kind::Stmt,
                    call: None,
                    mark: yan_surface::Mark::Lenient("N36".into()),
                });
                let v = self.fresh("c");
                let ex = |kind| j::Expr {
                    kind,
                    span: at,
                    paren: false,
                };
                let comp = ex(j::ExprKind::Comp {
                    kind: j::CompKind::List,
                    par: None,
                    elem: Box::new(ex(j::ExprKind::Name(v.clone()))),
                    value: None,
                    target: vec![v],
                    iter: Box::new(ex(j::ExprKind::Name(xs))),
                    cond: None,
                });
                let before = self.cx.marks.len();
                let out = self.rhs_inner(&comp, want);
                // The element's own `show` shares the span of `list.new`, and the loop is not a
                // written comprehension; the statement mark stands for the whole rewrite.
                let mut at_k = 0;
                self.cx.marks.retain(|m| {
                    at_k += 1;
                    at_k <= before
                        || !(m.kind == crate::marks::Kind::Call
                            && m.mark == yan_surface::Mark::Lenient("N36".into())
                            || m.kind == crate::marks::Kind::Stmt
                                && m.mark == yan_surface::Mark::Comprehension)
                });
                out
            }
            _ => Ok((r, t)),
        }
    }

    /// `float(x)` of an `int`.
    pub(super) fn int_float(&mut self, r: Rhs, at: Span) -> R<(Rhs, Ty)> {
        let a = self.tmp(&Ty::Int, r, at);
        let c = call("std.prelude.float", vec![], vec![let_arg(a)], at);
        self.cx
            .origins
            .push((at, yan_surface::Inserted::Conversion));
        Ok((Rhs::Call(c), Ty::Float))
    }

    /// N36 on an operand whose other side is a `str`, N38 on one whose other side is a `float`.
    pub(super) fn widen_atom(&mut self, a: Atom, t: Ty, other: &Ty, at: Span) -> R<(Atom, Ty)> {
        if !matches!((&t, other), (Ty::Char, Ty::Str) | (Ty::Int, Ty::Float))
            || (t == Ty::Int && matches!(a, Atom::Lit(..)))
        {
            return Ok((a, t));
        }
        let (r, t) = self.widen(Rhs::Atom(a), t, Some(other), at)?;
        Ok((self.tmp(&t, r, at), t))
    }

    /// N40: Python's `re` flags (`re.I`, `flags=re.IGNORECASE | re.S`) on `re.m(...)` become
    /// inline flags at the front of the pattern; `None` when there are none.
    pub(super) fn re_flags(&mut self, m: &str, args: &[j::Arg]) -> Option<(Vec<j::Arg>, bool)> {
        fn flags(e: &j::Expr, out: &mut String) -> bool {
            match &e.kind {
                j::ExprKind::Binary("|", a, b) => flags(a, out) && flags(b, out),
                j::ExprKind::Attr(r, f, _) if matches!(&r.kind, j::ExprKind::Name(n) if n == "re") =>
                {
                    let c = match f.to_ascii_lowercase().as_str() {
                        "i" | "ignorecase" => 'i',
                        "m" | "multiline" => 'm',
                        "s" | "dotall" => 's',
                        "x" | "verbose" => 'x',
                        _ => return false,
                    };
                    if !out.contains(c) {
                        out.push(c);
                    }
                    true
                }
                _ => false,
            }
        }
        let pos = match m {
            "compile" => 1,
            "match" | "search" | "fullmatch" | "findall" | "finditer" | "find_all" => 2,
            _ => usize::MAX,
        };
        let mut fs = String::new();
        let k = args.iter().enumerate().position(|(k, a)| {
            (a.name.as_deref() == Some("flags") || (a.name.is_none() && k == pos))
                && matches!(&a.kind, j::ArgKind::Expr(e) if flags(e, &mut fs))
        })?;
        let keyword = args[k].name.is_some();
        let mut out = args.to_vec();
        out.remove(k);
        let j::ArgKind::Expr(p) = &out.first()?.kind else {
            return None;
        };
        let at = p.span;
        let lead = j::Lit::Str {
            prefix: String::new(),
            value: format!("(?{fs})"),
        };
        let kind = match &p.kind {
            j::ExprKind::Lit(j::Lit::Str { prefix, value })
                if matches!(prefix.as_str(), "" | "r") =>
            {
                j::ExprKind::Lit(j::Lit::Str {
                    prefix: prefix.clone(),
                    value: format!("(?{fs}){value}"),
                })
            }
            _ => j::ExprKind::Binary(
                "+",
                Box::new(j::Expr {
                    kind: j::ExprKind::Lit(lead),
                    span: at,
                    paren: false,
                }),
                Box::new(p.clone()),
            ),
        };
        out[0].kind = j::ArgKind::Expr(j::Expr {
            kind,
            span: at,
            paren: false,
        });
        self.lenient(
            "N40",
            at,
            &format!("`re` flags normalized to the inline flags `(?{fs})` in the pattern"),
        );
        Some((out, keyword))
    }

    pub(super) fn lenient(&mut self, code: &str, at: Span, msg: &str) {
        let item = self.item.clone();
        self.cx.note(code, at, &item, msg.into());
    }

    /// `c.to_str()`, or the one-character literal when `c` is one.
    pub(super) fn char_str(&mut self, r: Rhs, at: Span) -> R<(Rhs, Ty)> {
        match self.tmp(&Ty::Char, r, at) {
            Atom::Lit(Literal::Char(c), sp) => {
                self.cx.marks.push(crate::marks::Pending {
                    span: sp,
                    kind: crate::marks::Kind::Lit,
                    call: None,
                    mark: yan_surface::Mark::Lenient("N36".into()),
                });
                Ok((Rhs::Atom(Atom::Lit(Literal::Str(c.into()), sp)), Ty::Str))
            }
            a => {
                self.mark_call(at, yan_surface::Mark::Lenient("N36".into()));
                Ok((Rhs::Atom(self.show(a, &Ty::Char, at)), Ty::Str))
            }
        }
    }

    /// Whether std's `collect` is loaded, which it is when the module names an `iter` source.
    pub(super) fn can_collect(&self) -> bool {
        self.g.fns.contains_key("std.iter.ops.collect")
    }

    pub(super) fn collect(&mut self, r: Rhs, t: &Ty, at: Span) -> R<(Rhs, Ty)> {
        let a = self.tmp(t, r, at);
        let out = self.method_on(a, t, "collect", at)?;
        self.mark_call(at, yan_surface::Mark::Lenient("N37".into()));
        Ok(out)
    }

    /// `a.m()` for an already lowered receiver.
    pub(super) fn method_on(&mut self, a: Atom, t: &Ty, m: &str, at: Span) -> R<(Rhs, Ty)> {
        let Atom::Name(n, _) = a else {
            return unsupported(format!("`.{m}()` on a literal `{}`", self.ty_text(t)));
        };
        let recv = j::Expr {
            kind: j::ExprKind::Name(n),
            span: at,
            paren: false,
        };
        let (r, t, _) = self.method(&recv, m, at, &[], at)?;
        Ok((r, t))
    }
}
