use super::*;

impl<'a> Fold<'a> {
    /// A call carrying `Const`, `Negated` or `Lenient`: the spelling the mark records when the
    /// node still has the shape the elaboration produced, else `None` and the default prints.
    pub(super) fn marked_math(&mut self, c: &y::Call) -> R<Option<Expr>> {
        let p = c.path.join(".");
        let vals = self.values(c);
        let is_math_call = |x: &Expr, fs: &[&str]| {
            matches!(&x.kind, ExprKind::Call(f, xs)
                if matches!(&f.kind, ExprKind::Attr(m, f, _)
                    if fs.contains(&f.as_str()) && matches!(&m.kind, ExprKind::Name(n) if n == "math"))
                    && !xs.is_empty())
        };
        if let Some(m) = self.marks.call_mark(c, &Mark::Const) {
            let inf =
                |a: &y::Atom| matches!(a, y::Atom::Lit(y::Literal::Float(s), _) if s == "1e999");
            if p == "sub_f64" && matches!(vals.as_slice(), [a, b] if inf(a) && inf(b)) {
                self.marks.accept(m);
                return Ok(Some(attr(name("math"), "nan")));
            }
            self.marks.refuse(m);
        }
        if let Some(m) = self.marks.call_mark(c, &Mark::Negated) {
            if let ("mul_f64", [y::Atom::Lit(y::Literal::Float(n), _), x]) =
                (p.as_str(), vals.as_slice())
                && n == "-1.0"
            {
                self.marks.accept(m);
                let x = self.atom(x)?;
                return Ok(Some(e(ExprKind::Unary("-", Box::new(x)))));
            }
            if let ("sub_checked_i64", [y::Atom::Lit(y::Literal::Int(z), _), x]) =
                (p.as_str(), vals.as_slice())
                && z == "0"
            {
                self.marks.accept(m);
                let x = self.atom(x)?;
                return Ok(Some(e(ExprKind::Unary("-", Box::new(x)))));
            }
            self.marks.refuse(m);
        }
        if let Some(m) = self.marks.call_mark(c, &Mark::Lenient("N39".into())) {
            match (p.as_str(), vals.as_slice()) {
                ("std.prelude.abs", [x]) => {
                    self.marks.accept(m);
                    let x = self.atom(x)?;
                    return Ok(Some(call(attr(name("math"), "abs"), vec![arg(x)])));
                }
                ("std.math.is_nan" | "std.math.is_inf" | "std.math.is_finite", [x]) => {
                    self.marks.accept(m);
                    let f = format!("is{}", &p["std.math.is_".len()..]);
                    let x = self.atom(x)?;
                    return Ok(Some(call(attr(name("math"), &f), vec![arg(x)])));
                }
                ("std.prelude.int", [x])
                    if self
                        .peek(x)
                        .is_some_and(|x| is_math_call(x, &["floor", "ceil", "trunc"])) =>
                {
                    self.marks.accept(m);
                    return Ok(Some(self.atom(x)?));
                }
                ("div_f64", [a, b])
                    if let (Some(a), Some(b)) = (self.peek(a), self.peek(b))
                        && is_math_call(a, &["log"])
                        && is_math_call(b, &["log"]) =>
                {
                    self.marks.accept(m);
                    let (a, b) = (self.atom(vals[0])?, self.atom(vals[1])?);
                    let (ExprKind::Call(_, mut xa), ExprKind::Call(_, xb)) = (a.kind, b.kind)
                    else {
                        unreachable!("checked above")
                    };
                    xa.extend(xb);
                    return Ok(Some(call(attr(name("math"), "log"), xa)));
                }
                _ => self.marks.refuse(m),
            }
        }
        Ok(None)
    }

    /// `cli.parse_args[T](xs)` and `cli.parse[T]()` from a `Schema` mark, when the type argument
    /// names the type the schema literal describes.
    pub(super) fn marked_schema(&mut self, c: &y::Call) -> R<Option<Expr>> {
        if c.path.join(".") != "std.cli.parse_args" {
            return Ok(None);
        }
        let Some((m, ty_name)) = self.marks.call_schema(c) else {
            return Ok(None);
        };
        let vals = self.values(c);
        let described = |s: &str| {
            s.rfind("\"name\":\"")
                .is_some_and(|i| s[i + 8..].starts_with(&format!("{ty_name}\",\"prog\":")))
        };
        match (c.targs.as_slice(), vals.as_slice()) {
            ([y::TArg::Type(y::Type::Named(n))], [xs, y::Atom::Lit(y::Literal::Str(s), _)])
                if n.path == [ty_name.as_str()] && n.args.is_empty() && described(s) =>
            {
                self.marks.accept(m);
                let f = e(ExprKind::Index(
                    Box::new(attr(name("cli"), "parse_args")),
                    Box::new(name(&ty_name)),
                ));
                let is_args = |x: &Expr| {
                    matches!(&x.kind, ExprKind::Call(f, a) if a.is_empty()
                        && matches!(&f.kind, ExprKind::Attr(r, m, _)
                            if m == "args" && matches!(&r.kind, ExprKind::Name(n) if n == "env")))
                };
                if self.peek(xs).is_some_and(is_args) {
                    self.atom(xs)?;
                    let f = e(ExprKind::Index(
                        Box::new(attr(name("cli"), "parse")),
                        Box::new(name(&ty_name)),
                    ));
                    return Ok(Some(call(f, Vec::new())));
                }
                let xs = arg(self.atom(xs)?);
                Ok(Some(call(f, vec![xs])))
            }
            _ => {
                self.marks.refuse(m);
                Ok(None)
            }
        }
    }

    /// N35 and N40 on `std.regex` calls: the Python `re.f(pattern, ..)` spelling, with the flags
    /// the `Flags` marks record, or the literal a `Regex` was lifted from.
    pub(super) fn marked_re(&mut self, c: &y::Call) -> R<Option<Expr>> {
        let p = c.path.join(".");
        let vals = self.values(c);
        let n40 = Mark::Lenient("N40".into());
        if p == "std.regex.compile"
            && let Some(m) = self.marks.call_mark(c, &n40)
        {
            if let [a] = vals.as_slice() {
                self.marks.accept(m);
                let mut x = self.atom(a)?;
                raw_if_backslash(&mut x);
                return Ok(Some(x));
            }
            self.marks.refuse(m);
        }
        let Some(f) = p.strip_prefix("std.regex.") else {
            return Ok(None);
        };
        let n35 = Mark::Lenient("N35".into());
        let (m, lifted) = match (self.marks.call_mark(c, &n35), self.marks.call_mark(c, &n40)) {
            (Some(m), _) => (m, true),
            (None, Some(m)) => (m, false),
            _ => return Ok(None),
        };
        let flags = [(Mark::FlagsKeyword, true), (Mark::FlagsPositional, false)]
            .into_iter()
            .find_map(|(k, kw)| self.marks.call_mark(c, &k).map(|p| (p, kw)));
        let f = if f == "find_all" { "findall" } else { f };
        let Some(recv) = vals.first() else {
            self.marks.refuse(m);
            return Ok(None);
        };
        let mut head = if lifted {
            let is_compile = |x: &Expr| {
                matches!(&x.kind, ExprKind::Call(g, xs) if xs.len() == 1
                    && matches!(&g.kind, ExprKind::Attr(r, c, _)
                        if c == "compile" && matches!(&r.kind, ExprKind::Name(n) if n == "regex")))
            };
            if !self.peek(recv).is_some_and(is_compile) {
                self.marks.refuse(m);
                return Ok(None);
            }
            let ExprKind::Call(_, mut xs) = self.atom(recv)?.kind else {
                unreachable!("checked above")
            };
            match xs.remove(0).kind {
                ArgKind::Expr(x) => x,
                _ => unreachable!("a positional pattern"),
            }
        } else {
            self.atom(recv)?
        };
        let mut rest = self.std_args(c, &p, 1)?;
        if let Some((fm, keyword)) = flags {
            let letters = match &mut head.kind {
                ExprKind::Lit(Lit::Str { value, .. }) if value.starts_with("(?") => {
                    value.find(')').map(|k| {
                        let l = value[2..k].to_owned();
                        value.replace_range(..=k, "");
                        l
                    })
                }
                _ => None,
            };
            let ok = letters.as_deref().filter(|l| {
                !l.is_empty()
                    && l.chars().all(|c| "imsx".contains(c))
                    && (keyword || rest.len() == 1)
            });
            let Some(l) = ok else {
                self.marks.refuse(m);
                self.marks.refuse(fm);
                return Ok(None);
            };
            let flag = |c: char| {
                let n = match c {
                    'i' => "I",
                    'm' => "M",
                    's' => "S",
                    _ => "X",
                };
                attr(name("re"), n)
            };
            let mut it = l.chars();
            let first = flag(it.next().expect("non-empty"));
            let x = it.fold(first, |a, c| {
                e(ExprKind::Binary("|", Box::new(a), Box::new(flag(c))))
            });
            let mut a = arg(x);
            if keyword {
                a.name = Some("flags".into());
            }
            rest.push(a);
            self.marks.accept(fm);
        }
        raw_if_backslash(&mut head);
        self.marks.accept(m);
        let mut args = vec![arg(head)];
        args.extend(rest);
        Ok(Some(call(attr(name("re"), f), args)))
    }

    pub(super) fn call_marked(&mut self, c: &y::Call) -> R<Expr> {
        if let Some(m) = self.marks.call_mark(c, &Mark::Prelude) {
            let vals = self.values(c);
            if let ("int_pow" | "bigint_pow" | "std.math.pow" | "std.math.pown", [a, b, ..]) =
                (c.path.join(".").as_str(), vals.as_slice())
            {
                self.marks.accept(m);
                let args = vec![arg(self.atom(a)?), arg(self.atom(b)?)];
                return Ok(call(name("pow"), args));
            }
            self.marks.refuse(m);
        }
        if let ("std.prelude.float" | "std.prelude.bigint", [x]) =
            (c.path.join(".").as_str(), self.values(c).as_slice())
            && self.marks.call_origin(c) == Some(Inserted::Conversion)
        {
            return self.atom(x);
        }
        // An `.iter()` elaboration added to a collection passed as an `iter` argument (R511).
        if let (Some("iter"), [x]) = (c.path.last().map(String::as_str), self.values(c).as_slice())
            && self.marks.call_origin(c) == Some(Inserted::Conversion)
        {
            return self.atom(x);
        }
        if let Some(x) = self.marked_math(c)? {
            return Ok(x);
        }
        if let Some(x) = self.marked_re(c)? {
            return Ok(x);
        }
        if let Some(x) = self.marked_schema(c)? {
            return Ok(x);
        }
        let p = c.path.join(".");
        let vals = self.values(c);
        // `int_pow` and `bigint_pow` have no call form in Yan Agent; `math.pow` does, so it needs
        // the mark; without name-table facts, the module carrying `int_pow` is the sign of a `**`.
        let mark = self.marks.call_mark(c, &Mark::Operator);
        let pow = match (p.as_str(), vals.len()) {
            ("int_pow" | "bigint_pow", 2) => true,
            ("std.math.pow" | "std.math.pown", 3) => {
                mark.is_some() || (!self.marks.sourced && self.marks.fns.contains_key("int_pow"))
            }
            _ => false,
        };
        if pow {
            mark.into_iter().for_each(|m| self.marks.accept(m));
            let (a, b) = (self.atom(vals[0])?, self.atom(vals[1])?);
            return Ok(e(ExprKind::Binary("**", Box::new(a), Box::new(b))));
        }
        if p == "std.text.concat" {
            return self.concat(c);
        }
        if let Some(m) = mark {
            self.marks.refuse(m);
        }
        if let ([a, b], Some(op)) = (vals.as_slice(), binary(&p)) {
            let (a, b) = (self.atom(a)?, self.atom(b)?);
            return Ok(e(if COMPARE.contains(&op) {
                ExprKind::Compare(op, Box::new(a), Box::new(b))
            } else {
                ExprKind::Binary(op, Box::new(a), Box::new(b))
            }));
        }
        if let Some((f, ty)) = p.rsplit_once('_')
            && ty != "bool"
            && !p.contains('.')
        {
            if f == "not" && vals.len() == 1 {
                let x = self.atom(vals[0])?;
                return Ok(e(ExprKind::Unary("~", Box::new(x))));
            }
            if yan_std::intrinsics::BITS.contains(&f) {
                let args = vals
                    .iter()
                    .map(|v| self.atom(v).map(arg))
                    .collect::<R<Vec<_>>>()?;
                return Ok(call(attr(name("bits"), f), args));
            }
        }
        if p == "not_bool" && vals.len() == 1 {
            let x = self.atom(vals[0])?;
            return Ok(match x.kind {
                ExprKind::Compare("in", a, b) => e(ExprKind::Compare("not in", a, b)),
                _ => e(ExprKind::Not(Box::new(x))),
            });
        }
        if let Some(op) = p.strip_prefix("neg_").map(|_| "-")
            && vals.len() == 1
        {
            let x = self.atom(vals[0])?;
            return Ok(e(ExprKind::Unary(op, Box::new(x))));
        }
        if p == "std.text.concat" {
            return self.concat(c);
        }
        if let Some(m) = self.marks.call_mark(c, &Mark::Lenient("N37".into())) {
            if p == "std.iter.ops.collect" && vals.len() == 1 {
                self.marks.accept(m);
                return self.atom(vals[0]);
            }
            self.marks.refuse(m);
        }
        if let Some(m) = self.marks.call_mark(c, &Mark::Lenient("N36".into())) {
            if p == "std.fmt.show" && vals.len() == 1 {
                self.marks.accept(m);
                return self.atom(vals[0]);
            }
            self.marks.refuse(m);
        }
        // `str(x)` lowers the same way; the f-string is the more common spelling.
        if p == "std.fmt.show" && vals.len() == 1 {
            let x = self.atom(vals[0])?;
            return Ok(e(ExprKind::FStr {
                template: false,
                pieces: vec![FPiece::Expr(x, None)],
            }));
        }
        if let Some((m, f)) = p.strip_prefix("std.").and_then(|s| s.rsplit_once('.')) {
            // A float `sum` is its own function only below Yan Agent.
            let f = if f == "sum_float" { "sum" } else { f };
            if let ([a], true) = (vals.as_slice(), m.starts_with("time.") && f == "neg") {
                let a = self.atom(a)?;
                return Ok(e(ExprKind::Unary("-", Box::new(a))));
            }
            if let ([a, b], Some(op)) = (vals.as_slice(), time_operator(m, f)) {
                let (a, b) = (self.atom(a)?, self.atom(b)?);
                return Ok(e(ExprKind::Binary(op, Box::new(a), Box::new(b))));
            }
            return self.std_call(c, m, f);
        }
        let mut args = self.args(&c.args)?;
        let marks = self.marks.args(c);
        let names = self.marks.fns.get(&p);
        for (i, a) in args.iter_mut().enumerate() {
            match (marks.get(i), names.and_then(|n| n.get(i))) {
                (Some(Some((Mark::Keyword, _))), Some(n)) => a.name = Some(n.clone()),
                (Some(m @ Some((Mark::Keyword, _))), None) => self.marks.reject(m),
                _ => {}
            }
        }
        Ok(call(path_expr(&c.path), args))
    }
}
