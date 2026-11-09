use super::*;

impl Lower<'_, '_, '_> {
    /// A call of the callable bound to the local `n`, of type `t`.
    pub(super) fn call_local(
        &mut self,
        n: &str,
        t: Ty,
        f_span: Span,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let Ty::Fn(par, ps, ret) = t else {
            unreachable!("a callable")
        };
        if args.len() != ps.len() || args.iter().any(|a| a.name.is_some()) {
            self.err(
                "E202",
                at,
                format!("`{n}` takes {} positional arguments", ps.len()),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        }
        let mut out = Vec::new();
        for (a, p) in args.iter().zip(&ps) {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported("`&` or `become` arguments to a callable");
            };
            let (x, t) = self.atom(e, Some(p))?;
            self.expect(&t, p, a.span);
            out.push(let_arg(x));
        }
        if !par {
            self.fn_calls.push((n.to_owned(), f_span));
        }
        Ok((Rhs::Call(call(n, vec![], out, at)), *ret, None))
    }

    /// `f(...).context(msg)`: the call's failure as an `Error` with `msg` added (R432).
    pub(super) fn context(
        &mut self,
        f: &j::Expr,
        fargs: &[j::Arg],
        fat: Span,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let (r, t, fail) = self.call_expr(f, fargs, fat, None)?;
        let (Some(e), Rhs::Call(c)) = (fail, r) else {
            return unsupported("`.context` on a call that cannot fail");
        };
        if !self.g.std_records.contains("Error") {
            return unsupported("`.context` where the module defines its own `Error`");
        }
        let Some(xs) = self.positional(args, 1, "context", at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let (msg, mt) = self.atom(xs[0], Some(&Ty::Str))?;
        self.expect(&mt, &Ty::Str, xs[0].span);
        let rt = Ty::Result(Box::new(t.clone()), Box::new(e.clone()));
        let tried = self.tmp(&rt, Rhs::Try(c), fat);
        let c = call(
            "std.error.context",
            vec![t.targ(), e.targ()],
            vec![sink_arg(tried), let_arg(msg)],
            at,
        );
        Ok((Rhs::Call(c), t, Some(Ty::Data("Error".into()))))
    }

    /// `bits.f(x, ...)` as the typed `<f>_<type>` operation for `x`'s integer type.
    pub(super) fn bits_call(
        &mut self,
        f: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let n = if matches!(f, "rotate_left" | "rotate_right" | "wrapping_shl") {
            2
        } else {
            1
        };
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let Some(xs) = self.positional(args, n, f, at)? else {
            return Ok(err);
        };
        let (x, t) = self.atom(xs[0], None)?;
        if t == Ty::Err {
            return Ok(err);
        }
        let big_ok = matches!(f, "count_ones" | "bit_width");
        if !is_integer(&t) || (t == Ty::BigInt && !big_ok) {
            self.err(
                "E201",
                xs[0].span,
                format!("`bits.{f}` does not take a `{t}`"),
            );
            return Ok(err);
        }
        let mut out = vec![let_arg(x)];
        if let Some(e) = xs.get(1) {
            let (a, nt) = self.atom(e, Some(&Ty::Int))?;
            if !is_integer(&nt) || nt == Ty::BigInt {
                if nt != Ty::Err {
                    self.err(
                        "E201",
                        e.span,
                        format!("`bits.{f}` needs an integer amount, not `{nt}`"),
                    );
                }
                return Ok(err);
            }
            out.push(let_arg(a));
        }
        let rt = match f {
            "is_power_of_two" => Ty::Bool,
            "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" | "bit_width" => {
                Ty::Int
            }
            _ => t.clone(),
        };
        let p = format!("{f}_{}", t.scalar().expect("an integer"));
        Ok((Rhs::Call(call(&p, vec![], out, at)), rt, None))
    }

    /// `module.f(...)`, a free std function called by its module's name, as `math.sqrt(x)`.
    pub(super) fn module_call(
        &mut self,
        module: &str,
        f: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        if module == "bits" {
            return self.bits_call(f, args, at);
        }
        let path = format!("std.{module}.{f}");
        if let Some(name) = crate::stdlib::decl(&path).and_then(|d| d.preview.as_ref())
            && !self.cx.previews.contains(name)
        {
            self.err(
                "E001",
                at,
                format!("`{module}.{f}` is a preview; add `preview=[\"{name}\"]` to the package"),
            );
        }
        if crate::stdlib::decl(&path).is_some_and(|d| d.typed_at_call()) {
            return self.user_call(&path, None, args, at);
        }
        let Some(mut p) = crate::stdlib::decl(&path).and_then(|d| d.plain()) else {
            return unsupported(format!("`{path}`"));
        };
        if f == "strptime"
            && let Some(n) = match module {
                "time.date" => Some("PlainDate"),
                "time.plain_time" => Some("PlainTime"),
                "time.plain_date_time" => Some("PlainDateTime"),
                _ => None,
            }
        {
            self.pattern_check(n, args.get(1));
        }
        // A literal pattern is checked here, so the call cannot fail (R484).
        if path == "std.regex.compile"
            && let [
                j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(e),
                    ..
                },
            ] = args
            && let j::ExprKind::Lit(j::Lit::Str { prefix, value }) = &e.kind
            && matches!(prefix.as_str(), "" | "r")
        {
            if let Err(reason) = yan_std::regex::build(value) {
                self.err(
                    "E545",
                    e.span,
                    format!("invalid regular expression: {reason}"),
                );
            }
            p.fail = None;
        }
        if path == "std.i18n.Locale"
            && let [
                j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(e),
                    ..
                },
            ] = args
            && let j::ExprKind::Lit(j::Lit::Str { value, .. }) = &e.kind
        {
            if let Err(reason) = yan_std::locale::canonical(value) {
                self.err("E550", e.span, format!("malformed locale: {reason}"));
            }
            p.fail = None;
        }
        self.std_call(&path, p, Vec::new(), args, f, at)
    }

    /// An operation of a std capability (`io.print(s)`, `fs.read(p)`), declared in its module.
    pub(super) fn cap_call(
        &mut self,
        cap: &str,
        op: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let path = format!("std.{cap}.{op}");
        if path == "std.rand.choice" || path == "std.rand.shuffle" {
            return self.rand_list_op(cap, op, args, at);
        }
        if path == "std.crypto_rand.fill_bytes" {
            return self.fill_bytes_call(cap, args, at);
        }
        if cap == "log" && matches!(op, "debug" | "info" | "warn" | "error" | "span") {
            return self.log_call(op, args, at);
        }
        // A capability trait of this module: its method takes the binding as `self`.
        let t = self.sig.cap_trait(cap);
        if let Some(ms) = self.g.traits.get(&t) {
            let Some((_, s)) = ms.iter().find(|(n, _)| n == op) else {
                self.err("E203", at, format!("`{t}` has no operation `{op}`"));
                return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
            };
            let map = [("Self".to_owned(), Ty::Data(t.clone()))];
            let mut s = s.clone();
            for q in &mut s.params {
                q.ty = q.ty.subst(&map);
            }
            s.ret = s.ret.subst(&map);
            let recv = j::Expr {
                kind: j::ExprKind::Name(cap.to_owned()),
                span: at,
                paren: false,
            };
            return self.call_sig(op, &s, Some(&recv), args, at);
        }
        // `print` takes any `Show` value and prints `show(x)` (R655).
        if path == "std.io.print"
            && let [
                j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(e),
                    ..
                },
            ] = args
        {
            let (a, t) = self.atom(e, None)?;
            if t == Ty::Err {
                return Ok((Rhs::Atom(a), Ty::Err, None));
            }
            let s = self.show(a, &t, e.span);
            let c = call(
                &path,
                vec![],
                vec![Arg::Cap(cap.to_owned(), at), let_arg(s)],
                at,
            );
            return Ok((Rhs::Call(c), Ty::Unit, None));
        }
        let Some(p) = crate::stdlib::decl(&path)
            .filter(|d| d.cap)
            .and_then(|d| d.plain())
        else {
            return unsupported(format!("capability operation `{cap}.{op}`"));
        };
        self.std_call(&path, p, vec![Arg::Cap(cap.to_owned(), at)], args, op, at)
    }

    /// `log.info(msg, key=value, ...)` as `log.event(InfoLevel(), msg, [("key", show(value))])`,
    /// and `log.span(name, key=value)` as `log.span(name, [...])`.
    pub(super) fn log_call(
        &mut self,
        op: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let mut msg = None;
        let mut fields = Vec::new();
        for a in args {
            let j::ArgKind::Expr(e) = &a.kind else {
                return unsupported(format!("a reference argument to `log.{op}`"));
            };
            match &a.name {
                None if msg.is_none() => msg = Some(self.log_message(e)),
                None => {
                    self.err(
                        "E202",
                        a.span,
                        format!("`log.{op}` fields are keyword arguments"),
                    );
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
                }
                Some(n) => fields.push(ex(j::ExprKind::Tuple(vec![
                    ex(j::ExprKind::Lit(j::Lit::Str {
                        prefix: String::new(),
                        value: n.clone(),
                    })),
                    ex(j::ExprKind::FStr {
                        template: false,
                        pieces: vec![j::FPiece::Expr(e.clone(), None)],
                    }),
                ]))),
            }
        }
        let Some(msg) = msg else {
            self.err("E202", at, format!("`log.{op}` needs a message"));
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let arg = |e| j::Arg {
            name: None,
            kind: j::ArgKind::Expr(e),
            span: at,
        };
        let mut synth = Vec::new();
        let f = if op == "span" {
            "span"
        } else {
            let level = format!("{}{}Level", op[..1].to_uppercase(), &op[1..]);
            synth.push(arg(ex(j::ExprKind::Call(
                Box::new(ex(j::ExprKind::Name(level))),
                Vec::new(),
            ))));
            "event"
        };
        synth.push(arg(msg));
        synth.push(arg(ex(j::ExprKind::List(fields))));
        let path = format!("std.log.{f}");
        let p = crate::stdlib::decl(&path)
            .and_then(|d| d.plain())
            .expect("std.log declares the operation");
        self.std_call(&path, p, vec![Arg::Cap("log".into(), at)], &synth, op, at)
    }

    /// A log message is a `Template` (R452): an interpolating f-string is E233, and a t-string
    /// interpolating an exposed `Secret` is W803. The template is logged as its rendered text.
    pub(super) fn log_message(&mut self, e: &j::Expr) -> j::Expr {
        let j::ExprKind::FStr { template, pieces } = &e.kind else {
            return e.clone();
        };
        let interpolates = pieces.iter().any(|p| matches!(p, j::FPiece::Expr(..)));
        if !template && interpolates {
            self.err(
                "E233",
                e.span,
                "an f-string where a `Template` is expected; write `t\"...\"`".into(),
            );
        }
        for p in pieces {
            if let j::FPiece::Expr(x, _) = p
                && let j::ExprKind::Call(f, _) = &x.kind
                && matches!(&f.kind, j::ExprKind::Attr(_, m, _) if m == "expose")
            {
                let item = self.item.clone();
                self.cx.warn(
                    "W803",
                    x.span,
                    &item,
                    "an exposed `Secret` flows into `log`".into(),
                );
            }
        }
        j::Expr {
            kind: j::ExprKind::FStr {
                template: false,
                pieces: pieces.clone(),
            },
            ..e.clone()
        }
    }

    /// A type written as an argument to `json.parse[T]`: a name, or `list`, `set`, `dict`,
    /// `Option` and `tuple` of those.
    pub(super) fn written_ty(&self, e: &j::Expr) -> Option<Ty> {
        Some(match &e.kind {
            j::ExprKind::Name(n) => match n.as_str() {
                "bool" => Ty::Bool,
                "int" => Ty::Int,
                "float" => Ty::Float,
                "str" => Ty::Str,
                "char" => Ty::Char,
                n => Ty::Data(n.to_owned()),
            },
            j::ExprKind::Index(b, k) if let j::ExprKind::Name(c) = &b.kind => {
                let ks: Vec<&j::Expr> = match &k.kind {
                    j::ExprKind::Tuple(xs) => xs.iter().collect(),
                    _ => vec![k],
                };
                let mut ts = ks
                    .iter()
                    .map(|x| self.written_ty(x))
                    .collect::<Option<Vec<_>>>()?;
                match (c.as_str(), ts.len()) {
                    ("list", 1) => Ty::List(Box::new(ts.remove(0))),
                    ("set", 1) => Ty::Set(Box::new(ts.remove(0))),
                    ("Option", 1) => Ty::Option(Box::new(ts.remove(0))),
                    ("dict", 2) => {
                        let v = ts.remove(1);
                        Ty::Dict(Box::new(ts.remove(0)), Box::new(v))
                    }
                    _ => return None,
                }
            }
            _ => return None,
        })
    }

    /// `json.parse[T](s)` and `json.dump[T](x)` over `derive(Wire)` types (R874 to R877).
    pub(super) fn json_call(
        &mut self,
        op: &str,
        t: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let Some(xs) = self.positional(args, 1, op, at)? else {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        };
        let want = if op == "parse" { Some(Ty::Str) } else { None };
        let (a, ta) = self.atom(xs[0], want.as_ref())?;
        let rt = match (op, t) {
            ("parse", None) => return unsupported("`json.parse` without a type"),
            ("parse", Some(t)) => self.written_ty(t),
            (_, Some(t)) => self.written_ty(t),
            _ => Some(ta.clone()),
        };
        let Some(rt) = rt else {
            return unsupported("a type that is not a name or a collection of names");
        };
        if op == "parse" {
            self.expect(&ta, &Ty::Str, xs[0].span);
        } else {
            self.expect(&ta, &rt, xs[0].span);
        }
        if let Ty::Data(n) = &rt
            && !self.g.wires.contains(n)
        {
            self.err("E201", at, format!("`{n}` does not derive `Wire`"));
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
        }
        let Some(schema) = crate::jsonwire::schema(self.g, &rt) else {
            return unsupported(format!("`json.{op}` of `{rt}`"));
        };
        let c = call(
            &format!("std.json.{op}"),
            vec![rt.targ()],
            vec![let_arg(a), let_arg(Atom::Lit(Literal::Str(schema), at))],
            at,
        );
        if op == "parse" {
            Ok((Rhs::Call(c), rt, Some(Ty::Data("JsonError".into()))))
        } else {
            Ok((Rhs::Call(c), Ty::Str, None))
        }
    }

    /// `csv.parse[T](s, delimiter=",")` and `csv.dump[T](rows, delimiter=",")` over `derive(Wire)`
    /// row types (R878 to R880).
    pub(super) fn csv_call(
        &mut self,
        op: &str,
        t: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let unit = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let n = if args.len() == 2 { 2 } else { 1 };
        let Some(xs) = self.positional(args, n, op, at)? else {
            return Ok(unit);
        };
        let first = (op == "parse").then_some(Ty::Str);
        let (a, ta) = self.atom(xs[0], first.as_ref())?;
        let delim = match xs.get(1) {
            Some(d) => {
                let (d, td) = self.atom(d, Some(&Ty::Str))?;
                self.expect(&td, &Ty::Str, xs[1].span);
                d
            }
            None => Atom::Lit(Literal::Str(",".into()), at),
        };
        let rt = match (t, &ta) {
            (Some(t), _) => self.written_ty(t),
            (None, Ty::List(e)) if op == "dump" => Some((**e).clone()),
            _ => return unsupported("`csv.parse` without a type"),
        };
        let Some(rt) = rt else {
            return unsupported("a type that is not a name");
        };
        if op == "parse" {
            self.expect(&ta, &Ty::Str, xs[0].span);
        } else {
            self.expect(&ta, &Ty::List(Box::new(rt.clone())), xs[0].span);
        }
        if let Ty::Data(n) = &rt
            && !self.g.wires.contains(n)
        {
            self.err("E201", at, format!("`{n}` does not derive `Wire`"));
            return Ok(unit);
        }
        let schema = match crate::csvwire::schema(self.g, &rt) {
            Ok(s) => s,
            Err(m) => {
                self.err("E330", at, m);
                return Ok(unit);
            }
        };
        let c = call(
            &format!("std.csv.{op}"),
            vec![rt.targ()],
            vec![
                let_arg(a),
                let_arg(delim),
                let_arg(Atom::Lit(Literal::Str(schema), at)),
            ],
            at,
        );
        if op == "parse" {
            Ok((
                Rhs::Call(c),
                Ty::List(Box::new(rt)),
                Some(Ty::Data("CsvError".into())),
            ))
        } else {
            Ok((Rhs::Call(c), Ty::Str, None))
        }
    }

    /// `cli.parse_args[T](args)` with `T`'s schema, and `cli.parse[T]()` over `env.args()`.
    pub(super) fn cli_call(
        &mut self,
        op: &str,
        t: &j::Expr,
        args: &[j::Arg],
        at: Span,
        written: Option<&str>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let j::ExprKind::Name(n) = &t.kind else {
            return unsupported(format!("`cli.{op}` of a type that is not a name"));
        };
        // After an error the call is still lowered, so what follows it checks.
        let schema = if let Some(w) = written {
            w.to_owned()
        } else if self.g.clis.contains_key(n) {
            // An error here was reported at the derive.
            crate::cli::schema(self.g, n).unwrap_or_default()
        } else {
            self.err("E201", t.span, format!("`{n}` does not derive `Cli`"));
            String::new()
        };
        let xs = if op == "parse" {
            if !args.is_empty() {
                self.err(
                    "E202",
                    at,
                    "`cli.parse` takes no arguments; it reads `env.args()`".into(),
                );
                return Ok(err);
            }
            let (r, t, _) = self.cap_call("env", "args", &[], at)?;
            self.tmp(&t, r, at)
        } else {
            let Some(xs) = self.positional(args, 1, "parse_args", at)? else {
                return Ok(err);
            };
            let want = Ty::List(Box::new(Ty::Str));
            let (a, t) = self.atom(xs[0], Some(&want))?;
            self.expect(&t, &want, xs[0].span);
            a
        };
        let rt = Ty::Data(n.clone());
        self.cx.marks.push(crate::marks::Pending {
            span: at,
            kind: crate::marks::Kind::Call,
            call: None,
            mark: yan_surface::Mark::Schema(n.clone()),
        });
        let c = call(
            "std.cli.parse_args",
            vec![rt.targ()],
            vec![let_arg(xs), let_arg(Atom::Lit(Literal::Str(schema), at))],
            at,
        );
        Ok((Rhs::Call(c), rt, Some(Ty::Data("CliError".into()))))
    }

    /// `rand.choice(xs)` and `rand.shuffle(&xs)`, the generic operations of `Rand`.
    pub(super) fn rand_list_op(
        &mut self,
        cap: &str,
        op: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let [a] = args else {
            self.err("E202", at, format!("`rand.{op}` takes one list"));
            return Ok(err);
        };
        let path = format!("std.rand.{op}");
        let mut out = vec![Arg::Cap(cap.to_owned(), at)];
        let (t, ret) = match (op, &a.kind) {
            ("choice", j::ArgKind::Expr(e)) => {
                let (x, t) = self.atom(e, None)?;
                out.push(let_arg(x));
                let el = match &t {
                    Ty::List(el) => (**el).clone(),
                    _ => Ty::Err,
                };
                (t, el)
            }
            ("shuffle", j::ArgKind::Ref(e)) => {
                let (pl, t) = self.place(e)?;
                if !pl.projs.is_empty() {
                    return unsupported("`rand.shuffle` of a projection");
                }
                out.push(Arg::Value(
                    Conv::Inout,
                    Atom::Name(pl.root, pl.root_span),
                    a.span,
                ));
                (t, Ty::Unit)
            }
            ("shuffle", _) => {
                self.err("E404", a.span, "`rand.shuffle` takes `&place`".into());
                return Ok(err);
            }
            _ => {
                self.err(
                    "E405",
                    a.span,
                    "`rand.choice` takes the list, not `&`".into(),
                );
                return Ok(err);
            }
        };
        let Ty::List(el) = &t else {
            if t != Ty::Err {
                self.err(
                    "E201",
                    a.span,
                    format!("`rand.{op}` takes a list, not `{t}`"),
                );
            }
            return Ok(err);
        };
        let targs = vec![el.targ()];
        Ok((Rhs::Call(call(&path, targs, out, at)), ret, None))
    }

    /// `crypto_rand.fill_bytes(&buf)`: fills the whole `bytearray` in place.
    pub(super) fn fill_bytes_call(
        &mut self,
        cap: &str,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        let [
            j::Arg {
                kind: j::ArgKind::Ref(e),
                span,
                ..
            },
        ] = args
        else {
            self.err(
                "E404",
                at,
                "`crypto_rand.fill_bytes` takes `&bytearray`".into(),
            );
            return Ok(err);
        };
        let (pl, t) = self.place(e)?;
        if !pl.projs.is_empty() {
            return unsupported("`crypto_rand.fill_bytes` of a projection");
        }
        if t != Ty::Coll("bytearray", vec![]) {
            if t != Ty::Err {
                self.err(
                    "E201",
                    *span,
                    format!("`crypto_rand.fill_bytes` takes a bytearray, not `{t}`"),
                );
            }
            return Ok(err);
        }
        let out = vec![
            Arg::Cap(cap.to_owned(), at),
            Arg::Value(Conv::Inout, Atom::Name(pl.root, pl.root_span), *span),
        ];
        let c = call("std.crypto_rand.fill_bytes", vec![], out, at);
        Ok((Rhs::Call(c), Ty::Unit, None))
    }
}
