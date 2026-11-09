use super::*;

impl Lower<'_, '_, '_> {
    /// `+` and `-` on quantities of one dimension; other dimensions are E530 (11.4).
    pub(super) fn quantity_op(
        &mut self,
        op: &str,
        x: &Atom,
        tx: &Ty,
        y_: &Atom,
        ty: &Ty,
        at: Span,
    ) -> Option<R<(Rhs, Ty)>> {
        let dim = |t: &Ty| match t {
            Ty::Coll("Quantity", ts) => Some(ts[1].to_string()),
            _ => None,
        };
        let (dx, dy) = (dim(tx), dim(ty));
        if dx.is_none() && dy.is_none() {
            return None;
        }
        if !matches!(op, "+" | "-") {
            return Some(unsupported(format!("`{op}` on quantities")));
        }
        if dx != dy {
            let show = |d: Option<String>| d.unwrap_or_else(|| "a plain number".into());
            self.err(
                "E530",
                at,
                format!("`{op}` mixes dimension {} with {}", show(dx), show(dy)),
            );
            return Some(Ok((Rhs::Atom(x.clone()), Ty::Err)));
        }
        let p = float_op(op).expect("`+` and `-` on float");
        let c = call(p, vec![], vec![let_arg(x.clone()), let_arg(y_.clone())], at);
        Some(Ok((Rhs::Call(c), tx.clone())))
    }

    /// The waits a `par` block may not hold: in a plain one a branch receiving what a sibling sends
    /// (E713, R390), and a `Lub` read in a block that writes it (E717, R454).
    pub(super) fn par_waits(&mut self, branches: &[j::Stmt], concurrent: bool) {
        let mut ops: Vec<Vec<(String, String, Span)>> = Vec::new();
        for s in branches {
            let mut mine = Vec::new();
            for e in stmt_exprs(s) {
                walk(e, &mut |e| {
                    if let j::ExprKind::Call(f, _) = &e.kind
                        && let j::ExprKind::Attr(b, m, _) = &f.kind
                        && let j::ExprKind::Name(n) = &b.kind
                    {
                        mine.push((n.clone(), m.clone(), b.span));
                    }
                });
            }
            for b in sub_blocks(s) {
                let mut inner = Vec::new();
                for s in b {
                    for e in stmt_exprs(s) {
                        walk(e, &mut |e| {
                            if let j::ExprKind::Call(f, _) = &e.kind
                                && let j::ExprKind::Attr(b, m, _) = &f.kind
                                && let j::ExprKind::Name(n) = &b.kind
                            {
                                inner.push((n.clone(), m.clone(), b.span));
                            }
                        });
                    }
                }
                mine.extend(inner);
            }
            ops.push(mine);
        }
        let ty = |l: &Self, n: &str| l.locals.get(n).map(|x| x.ty.clone());
        let sends = ops.iter().enumerate().flat_map(|(k, os)| {
            os.iter()
                .filter(|(n, m, _)| {
                    m == "send" && matches!(ty(self, n), Some(Ty::Coll("Sender", _)))
                })
                .map(move |_| k)
        });
        let senders: Vec<usize> = sends.collect();
        let writes: Vec<String> = ops
            .iter()
            .flatten()
            .filter(|(n, m, _)| m == "put" && matches!(ty(self, n), Some(Ty::Coll("Lub", _))))
            .map(|(n, _, _)| n.clone())
            .collect();
        for (k, os) in ops.iter().enumerate() {
            for (n, m, sp) in os {
                if m == "recv"
                    && matches!(ty(self, n), Some(Ty::Coll("Receiver", _)))
                    && !concurrent
                    && senders.iter().any(|&j| j != k)
                {
                    self.err(
                        "E713",
                        *sp,
                        format!("`{n}.recv()` waits on a sibling branch of this `par`; use `par concurrent`"),
                    );
                }
                if m == "get" && writes.contains(n) {
                    self.err(
                        "E717",
                        *sp,
                        format!("`{n}.get()` in a `par` block that writes `{n}`; read it after the block"),
                    );
                }
            }
        }
    }

    /// `n * u` or `u * n` where `u` names a std unit (11.4) and no binding of that name.
    pub(super) fn unit_product<'e>(
        &self,
        a: &'e j::Expr,
        b: &'e j::Expr,
    ) -> Option<(&'e j::Expr, &'static str)> {
        let unit = |e: &j::Expr| match &e.kind {
            j::ExprKind::Name(n)
                if !self.locals.contains_key(n)
                    && !self.sig.caps.contains(n)
                    && !self.g.fns.contains_key(n)
                    && !self.g.consts.contains_key(n) =>
            {
                UNITS.iter().find(|(u, ..)| u == n).map(|(u, ..)| *u)
            }
            _ => None,
        };
        unit(b).map(|u| (a, u)).or_else(|| unit(a).map(|u| (b, u)))
    }

    /// An `int` times a duration unit is a `Duration`; any other number times a unit is a
    /// `Quantity` of the unit's dimension, held in SI base units (11.4, 11.6).
    pub(super) fn quantity(&mut self, n: &j::Expr, u: &'static str, at: Span) -> R<(Rhs, Ty)> {
        let &(_, dim, scale, ctor) = UNITS.iter().find(|(v, ..)| *v == u).expect("a unit");
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let int_n = match &n.kind {
            j::ExprKind::Lit(j::Lit::Int(_)) => true,
            j::ExprKind::Lit(_) => false,
            _ => self.ty_of_local(n) == Some(Ty::Int),
        };
        if let Some((ctor, k)) = ctor.filter(|_| int_n) {
            let mut arg = n.clone();
            if k != 1 {
                arg = ex(j::ExprKind::Binary(
                    "*",
                    Box::new(arg),
                    Box::new(ex(j::ExprKind::Lit(j::Lit::Int(k.to_string())))),
                ));
            }
            let f = ex(j::ExprKind::Attr(
                Box::new(ex(j::ExprKind::Name("Duration".into()))),
                ctor.into(),
                at,
            ));
            let c = ex(j::ExprKind::Call(
                Box::new(f),
                vec![j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(arg),
                    span: at,
                }],
            ));
            return self.rhs(&c, None);
        }
        let scaled = ex(j::ExprKind::Binary(
            "*",
            Box::new(n.clone()),
            Box::new(ex(j::ExprKind::Lit(j::Lit::Float(format!("{scale:e}"))))),
        ));
        let (r, t) = self.rhs(&scaled, Some(&Ty::Float))?;
        if t == Ty::Err {
            return Ok((r, t));
        }
        Ok((
            r,
            Ty::Coll("Quantity", vec![Ty::Float, Ty::Data(dim.into())]),
        ))
    }

    pub(super) fn arith(
        &mut self,
        op: &str,
        a: &j::Expr,
        b: &j::Expr,
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        if op == "*"
            && let Some((n, u)) = self.unit_product(a, b)
        {
            return self.quantity(n, u, at);
        }
        // W817: `day` and `week` are exact 24- and 168-hour durations, not calendar days.
        if op == "+"
            && self.ty_of_local(a) == Some(Ty::Data("ZonedDateTime".into()))
            && let j::ExprKind::Binary("*", l, r) = &b.kind
            && let Some((_, "day" | "week")) = self.unit_product(l, r)
        {
            let item = self.item.clone();
            self.cx.warn(
                "W817",
                at,
                &item,
                "a `day` or `week` duration is exact hours; add a `Period` for calendar days"
                    .into(),
            );
        }
        let want = want.filter(|w| !matches!(w, Ty::Float) || op != "/");
        // `2 * x`: a literal on the left takes the right operand's type (R656).
        let lit_left = is_int_lit(a) && !is_int_lit(b) && !matches!(op, "<<" | ">>" | "**");
        let (x, tx, right) = if lit_left {
            let (y_, ty) = self.atom(b, want)?;
            let (x, tx) = self.atom(a, Some(&ty))?;
            (x, tx, Some((y_, ty)))
        } else {
            let (x, tx) = self.atom(a, want)?;
            (x, tx, None)
        };
        if op == "**" && tx == Ty::Float {
            let (n, nt) = self.atom(b, Some(&Ty::Int))?;
            return self.float_pow(x, n, nt, at);
        }
        if op == "**" && matches!(tx, Ty::Int | Ty::BigInt) {
            let (n, nt) = self.atom(b, Some(&Ty::Int))?;
            let f = if tx == Ty::Int {
                "int_pow"
            } else {
                "bigint_pow"
            };
            return match nt {
                Ty::Err => Ok((Rhs::Atom(x), Ty::Err)),
                Ty::Int => Ok((
                    Rhs::Call(call(f, vec![], vec![let_arg(x), let_arg(n)], at)),
                    tx,
                )),
                Ty::Float if tx == Ty::Int && !self.cx.strict => {
                    let (x, _) = self.widen_atom(x, tx, &Ty::Float, a.span)?;
                    self.float_pow(x, n, nt, at)
                }
                _ => {
                    let msg = format!(
                        "`**` between `{}` and `{}`",
                        self.ty_text(&tx),
                        self.ty_text(&nt)
                    );
                    self.err("E201", at, msg);
                    Ok((Rhs::Atom(x), Ty::Err))
                }
            };
        }
        if matches!(op, "<<" | ">>") && is_integer(&tx) {
            // The amount is any integer type; a `bigint` shifts by an `int`.
            let hint = if tx == Ty::BigInt {
                Ty::Int
            } else {
                tx.clone()
            };
            let (n, nt) = self.atom(b, Some(&hint))?;
            if nt == Ty::Err {
                return Ok((Rhs::Atom(x), Ty::Err));
            }
            if !is_integer(&nt) || nt == Ty::BigInt {
                self.err(
                    "E201",
                    b.span,
                    format!("a shift amount must be an integer, not `{nt}`"),
                );
                return Ok((Rhs::Atom(x), Ty::Err));
            }
            let f = if op == "<<" { "shl" } else { "shr" };
            let p = format!("{f}_{}", tx.scalar().expect("an integer"));
            return Ok((
                Rhs::Call(call(&p, vec![], vec![let_arg(x), let_arg(n)], at)),
                tx,
            ));
        }
        let (y_, ty) = match right {
            Some(r) => r,
            None => self.atom(b, Some(&tx))?,
        };
        // N36: `+` with a `char` and a `str`, or two `char`s, concatenates their text.
        let (x, tx, y_, ty) = if op == "+"
            && !self.cx.strict
            && matches!(
                (&tx, &ty),
                (Ty::Char, Ty::Str | Ty::Char) | (Ty::Str, Ty::Char)
            ) {
            let (x, tx) = self.widen_atom(x, tx, &Ty::Str, a.span)?;
            let (y_, ty) = self.widen_atom(y_, ty, &Ty::Str, b.span)?;
            (x, tx, y_, ty)
        } else {
            (x, tx, y_, ty)
        };
        let (x, tx) = self.widen_atom(x, tx, &ty, a.span)?;
        let (y_, ty) = self.widen_atom(y_, ty, &tx, b.span)?;
        if let Some(r) = self.quantity_op(op, &x, &tx, &y_, &ty, at) {
            return r;
        }
        if op == "+" && matches!(tx, Ty::Str | Ty::List(_)) && self.not_last_use(a, at) {
            let item = self.item.clone();
            self.cx.warn(
                "W822",
                at,
                &item,
                "`+` in a loop copies its left operand, which is not at its last use (R388)".into(),
            );
        }
        if let (Ty::Array(sa, _), Ty::Array(sb, _)) = (&tx, &ty)
            && sa != sb
            && sa
                .iter()
                .chain(sb)
                .all(|d| matches!(d, Axis::Size(n) if n.chars().all(|c| c.is_ascii_digit())))
        {
            self.err(
                "E520",
                at,
                "positional axes broadcast only against scalars".into(),
            );
            return Ok((Rhs::Atom(x), Ty::Err));
        }
        let args = vec![let_arg(x.clone()), let_arg(y_)];
        let (path, rt, targs) = match (&tx, &ty) {
            (Ty::Err, _) | (_, Ty::Err) => return Ok((Rhs::Atom(x), Ty::Err)),
            (Ty::Int, Ty::Int) => match int_op(op) {
                Some((p, t)) => (p.to_owned(), t, vec![]),
                None => return unsupported(format!("`{op}` on `int`")),
            },
            (Ty::Float, Ty::Float) => match float_op(op) {
                Some(p) => (p.to_owned(), Ty::Float, vec![]),
                None => return unsupported(format!("`{op}` on `float`")),
            },
            (Ty::BigInt, Ty::BigInt) => match bigint_op(op) {
                Some(p) => (p.to_owned(), Ty::BigInt, vec![]),
                None => return unsupported(format!("`{op}` on `bigint`")),
            },
            (Ty::Fixed(n), Ty::Fixed(m)) if n == m && op != "/" => {
                let p = if n.starts_with('f') {
                    float_op(op).map(|p| p.replace("f64", n))
                } else {
                    int_op(op).map(|(p, _)| p.replace("i64", n))
                };
                match p {
                    Some(p) => (p, tx.clone(), vec![]),
                    None => {
                        return unsupported(format!("`{op}` on `{tx}`", tx = self.ty_text(&tx)));
                    }
                }
            }
            (Ty::Coll(c, _), _) if let Some((p, rt)) = time_op(c, &ty, op) => (p, rt, vec![]),
            (Ty::Data(z), Ty::Coll(c @ ("Duration" | "Period"), _))
                if z == "ZonedDateTime" && (op == "+" || (op == "-" && *c == "Duration")) =>
            {
                let f = match (*c, op) {
                    ("Period", _) => "zoned_add_period",
                    (_, "+") => "zoned_add",
                    _ => "zoned_sub",
                };
                (format!("std.time.{f}"), tx.clone(), vec![])
            }
            // `*` between a sequence and an `int`, in either order, repeats it (R660).
            (Ty::Str, Ty::Int) if op == "*" => ("std.text.repeat".to_owned(), Ty::Str, vec![]),
            (Ty::List(e), Ty::Int) if op == "*" => {
                ("std.list.repeat".to_owned(), tx.clone(), vec![e.targ()])
            }
            (Ty::Int, Ty::Str | Ty::List(_)) if op == "*" => {
                let (path, targs) = match &ty {
                    Ty::List(e) => ("std.list.repeat", vec![e.targ()]),
                    _ => ("std.text.repeat", vec![]),
                };
                let args = args.into_iter().rev().collect();
                return Ok((Rhs::Call(call(path, targs, args, at)), ty));
            }
            (Ty::Str, Ty::Str) if op == "+" => ("std.text.concat".to_owned(), Ty::Str, vec![]),
            (Ty::List(e), Ty::List(_)) if op == "+" => {
                ("std.list.concat".to_owned(), tx.clone(), vec![e.targ()])
            }
            (Ty::Option(_), _) | (_, Ty::Option(_)) => {
                let (t, sp) = if matches!(tx, Ty::Option(_)) {
                    (&tx, a.span)
                } else {
                    (&ty, b.span)
                };
                let default = match t {
                    Ty::Option(i) => match **i {
                        Ty::Int => Some("0"),
                        Ty::Float => Some("0.0"),
                        Ty::Str => Some("\"\""),
                        _ => None,
                    },
                    _ => None,
                };
                self.err(
                    "E201",
                    at,
                    format!("`{op}` on a `{t}`, which may be `None`; unwrap it first with `else`"),
                );
                if let Some(d) = default {
                    let fixed = format!("({} else {d})", self.cx.text(sp));
                    self.cx
                        .fix(Applicability::Choice, "unwrap with a default", sp, &fixed);
                }
                return Ok((Rhs::Atom(x), Ty::Err));
            }
            _ => {
                self.err(
                    "E201",
                    at,
                    format!(
                        "`{op}` between `{tx}` and `{ty}`",
                        ty = self.ty_text(&ty),
                        tx = self.ty_text(&tx)
                    ),
                );
                return Ok((Rhs::Atom(x), Ty::Err));
            }
        };
        Ok((Rhs::Call(call(&path, targs, args, at)), rt))
    }

    /// `x ** n` for a `float` `x`: `pown` for an `int` `n` (R657), `pow` for a `float` one.
    pub(super) fn float_pow(&mut self, x: Atom, n: Atom, nt: Ty, at: Span) -> R<(Rhs, Ty)> {
        let f = match nt {
            Ty::Err => return Ok((Rhs::Atom(x), Ty::Err)),
            Ty::Int => "pown",
            Ty::Float => "pow",
            _ if is_integer(&nt) => {
                return unsupported(format!("`**` with a `{}` exponent", self.ty_text(&nt)));
            }
            _ => {
                let msg = format!("`**` between `float` and `{}`", self.ty_text(&nt));
                self.err("E201", at, msg);
                return Ok((Rhs::Atom(x), Ty::Err));
            }
        };
        let path = format!("std.math.{f}");
        let p = crate::stdlib::decl(&path)
            .and_then(|d| d.plain())
            .expect("std.math declares pow and pown");
        let (_, round) = p.names[2];
        let (r, _) = self.atom(round.expect("a default rounding"), Some(&p.params[2].1))?;
        let args = vec![let_arg(x), let_arg(n), let_arg(r)];
        Ok((Rhs::Call(call(&path, vec![], args, at)), Ty::Float))
    }
}
