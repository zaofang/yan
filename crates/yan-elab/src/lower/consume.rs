use super::*;

impl Lower<'_, '_, '_> {
    /// Lowering `value` with no expected type gives `t`: a literal of its default type, a local,
    /// or a call of a non-generic function of this module.
    pub(super) fn inferred_alone(&self, value: &j::Expr, t: &Ty) -> bool {
        match &value.kind {
            j::ExprKind::Lit(l) => !matches!(l, j::Lit::None) && lit_ty(l) == *t,
            j::ExprKind::Name(n) => self.locals.get(n).is_some_and(|l| l.ty == *t),
            j::ExprKind::Call(f, _) => matches!(&f.kind, j::ExprKind::Name(n)
                if self.g.fns.get(n).is_some_and(|s| s.tparams.is_empty() && s.ret == *t)),
            _ => false,
        }
    }

    /// W818: an `else` after `call` that neither binds nor re-fails its `Cancelled` (R341).
    pub(super) fn discards_cancelled(&mut self, call: Span) {
        let line = self
            .cx
            .src
            .lines()
            .nth(call.end_line as usize - 1)
            .unwrap_or("");
        let from = (call.end_col as usize).saturating_sub(1).min(line.len());
        let Some(k) = line[from..].find("else").map(|k| k + from) else {
            return;
        };
        let col = k as u32 + 1;
        let at = Span {
            line: call.end_line,
            col,
            end_line: call.end_line,
            end_col: col + 4,
        };
        let item = self.item.clone();
        self.cx.warn(
            "W818",
            at,
            &item,
            "this `else` discards `Cancelled`; bind the error or `fail` it".into(),
        );
    }

    /// The type after `name:` on the line of `name`.
    pub(super) fn annotation_span(&self, name: Span) -> Span {
        let line = self
            .cx
            .src
            .lines()
            .nth(name.line as usize - 1)
            .unwrap_or("");
        let from = (name.end_col as usize).saturating_sub(1).min(line.len());
        let rest = &line[from..];
        let skip = rest.find(':').map_or(0, |k| {
            k + 1 + rest[k + 1..].len() - rest[k + 1..].trim_start().len()
        });
        let col = (from + skip + 1) as u32;
        let end = line[from + skip..]
            .find(" =")
            .map_or(col, |k| col + k as u32);
        Span {
            col,
            end_col: end,
            end_line: name.line,
            ..name
        }
    }

    /// R769: a non-copy local bound or assigned to a name leaves by `move` at its last use and
    /// by `copy` otherwise, or when it is a borrowed parameter.
    pub(super) fn consume(&self, r: Rhs, t: &Ty, at: Span) -> Rhs {
        let (x, sp) = match &r {
            Rhs::Atom(Atom::Name(x, sp)) => (x, sp),
            Rhs::Proj(p) if p.projs.is_empty() => (&p.root, &p.root_span),
            _ => return r,
        };
        if t.is_copy() || !self.locals.contains_key(x) {
            return r;
        }
        let place = Place {
            root: x.clone(),
            root_span: *sp,
            projs: Vec::new(),
        };
        let borrowed = self
            .sig
            .params
            .iter()
            .any(|p| p.name == *x && p.conv != Conv::Sink);
        if borrowed || self.next_use(x, at).is_some() {
            Rhs::Copy(place)
        } else {
            Rhs::Move(place)
        }
    }

    /// Inside a loop, `a` names a local that this statement does not reassign and that is read
    /// again, around the loop or after it.
    pub(super) fn not_last_use(&self, a: &j::Expr, at: Span) -> bool {
        let j::ExprKind::Name(n) = &a.kind else {
            return false;
        };
        !self.loops.is_empty()
            && self.locals.contains_key(n)
            && self.target.as_ref() != Some(n)
            && self.next_use(n, at).is_some()
    }

    /// The side of `f(x) == Ok(v)` that is not the constructor: a fallible call is its
    /// `Result`, so the constructor gets an expected type (R309 `ex` lines compare this way).
    pub(super) fn operand(&mut self, e: &j::Expr) -> R<(Atom, Ty)> {
        let j::ExprKind::Call(f, args) = &e.kind else {
            return self.atom(e, None);
        };
        let (r, t, fail) = self.call_expr(f, args, e.span, None)?;
        let (r, t) = match (fail, r) {
            (Some(fl), Rhs::Call(c)) => (Rhs::Try(c), Ty::Result(Box::new(t), Box::new(fl))),
            (_, r) => (r, t),
        };
        Ok((self.tmp(&t, r, e.span), t))
    }

    /// A call of a function of this module that can fail.
    pub(super) fn fallible_call(&self, e: &j::Expr) -> bool {
        matches!(&e.kind, j::ExprKind::Call(f, _)
            if matches!(&f.kind, j::ExprKind::Name(n)
                if !self.locals.contains_key(n)
                    && self.g.fns.get(n).is_some_and(|s| s.fail.is_some())))
    }

    pub(super) fn compare(&mut self, op: &str, a: &j::Expr, b: &j::Expr, at: Span) -> R<(Rhs, Ty)> {
        let none = |e: &j::Expr| matches!(e.kind, j::ExprKind::Lit(j::Lit::None));
        if matches!(op, "is" | "is not") {
            if !none(b) {
                return unsupported("`is` with other than `None`");
            }
            let (x, t) = self.atom(a, None)?;
            let present = match t {
                Ty::Option(_) => self.is_some(x, at)?.0,
                Ty::Err => return Ok((Rhs::Atom(x), Ty::Bool)),
                Ty::Unit => {
                    self.err(
                        "E201",
                        a.span,
                        "this is always `None`; annotate the binding with its type, as \
                         `x: int | None = None`"
                            .into(),
                    );
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
                }
                t => {
                    self.err(
                        "E201",
                        a.span,
                        format!("a `{t}` is never `None`", t = self.ty_text(&t)),
                    );
                    return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
                }
            };
            if op == "is not" {
                return Ok((Rhs::Atom(present), Ty::Bool));
            }
            let c = call("not_bool", vec![], vec![let_arg(present)], at);
            return Ok((Rhs::Call(c), Ty::Bool));
        }
        if matches!(op, "in" | "not in") {
            let a_lit = matches!(a.kind, j::ExprKind::Lit(j::Lit::Str { .. }));
            // A one-character literal on the left takes the element type, so the container goes first.
            let (x, xt, c, ct) = if a_lit {
                let (c, ct) = self.atom(b, None)?;
                let (x, xt) = self.atom(a, in_elem(&ct).as_ref())?;
                (x, xt, c, ct)
            } else {
                let (x, xt) = self.atom(a, None)?;
                let (c, ct) = self.atom(b, None)?;
                (x, xt, c, ct)
            };
            if let Some(e) = in_elem(&ct) {
                self.expect(&xt, &e, a.span);
            }
            let (path, targs) = match &ct {
                Ty::List(e) => ("std.list.contains".to_owned(), vec![e.targ()]),
                Ty::Set(e) => ("std.set.contains".to_owned(), vec![e.targ()]),
                Ty::Dict(k, v) => ("std.dict.contains_key".to_owned(), vec![k.targ(), v.targ()]),
                Ty::Coll(n, _) if *n != "heap" => {
                    let m = if *n == "sorted_dict" {
                        "contains_key"
                    } else {
                        "contains"
                    };
                    let module = ct.module().expect("a collection");
                    (format!("std.{module}.{m}"), coll_targs(&ct))
                }
                Ty::Str => ("std.text.contains".to_owned(), vec![]),
                Ty::Err => return Ok((Rhs::Atom(x), Ty::Err)),
                t => {
                    self.err(
                        "E201",
                        b.span,
                        format!("`in` on a `{t}`", t = self.ty_text(t)),
                    );
                    return Ok((Rhs::Atom(x), Ty::Err));
                }
            };
            let r = Rhs::Call(call(&path, targs, vec![let_arg(c), let_arg(x)], at));
            if op == "in" {
                return Ok((r, Ty::Bool));
            }
            let t = self.tmp(&Ty::Bool, r, at);
            return Ok((
                Rhs::Call(call("not_bool", vec![], vec![let_arg(t)], at)),
                Ty::Bool,
            ));
        }
        let Some(name) = cmp_name(op) else {
            return unsupported(format!("`{op}`"));
        };
        let (x, tx, y_, ty) = if (matches!(a.kind, j::ExprKind::Lit(j::Lit::Str { .. }))
            && !matches!(b.kind, j::ExprKind::Lit(_))
            && !is_outcome(b))
            || (is_empty_literal(a) && !is_empty_literal(b))
        {
            let (y_, ty) = self.atom(b, None)?;
            let (x, tx) = self.atom(a, Some(&ty))?;
            (x, tx, y_, ty)
        } else if is_outcome(a) && !is_outcome(b) {
            let (y_, ty) = self.operand(b)?;
            let (x, tx) = self.atom(a, Some(&ty))?;
            (x, tx, y_, ty)
        } else {
            let fallible = matches!(op, "==" | "!=") && self.fallible_call(a);
            let (x, tx) = if is_outcome(b) || fallible {
                self.operand(a)?
            } else {
                self.atom(a, None)?
            };
            // `f(x) == v` means `f(x) == Ok(v)` when `v` is not itself an outcome.
            let ok_b;
            let mut b = b;
            if fallible
                && !is_outcome(b)
                && let Ty::Result(..) = tx
                && !self.probe(b).is_some_and(|t| t.fits(&tx))
            {
                ok_b = j::Expr {
                    kind: j::ExprKind::Call(
                        Box::new(j::Expr {
                            kind: j::ExprKind::Name("Ok".into()),
                            span: b.span,
                            paren: false,
                        }),
                        vec![j::Arg {
                            name: None,
                            kind: j::ArgKind::Expr(b.clone()),
                            span: b.span,
                        }],
                    ),
                    span: b.span,
                    paren: false,
                };
                b = &ok_b;
            }
            let (y_, ty) = self.atom(b, Some(&tx))?;
            (x, tx, y_, ty)
        };
        if matches!(tx, Ty::Err) || matches!(ty, Ty::Err) {
            return Ok((Rhs::Atom(x), Ty::Bool));
        }
        // A multi-character literal never equals a `char`, so it stays E201 (R802).
        let lit = |e: &j::Expr| matches!(e.kind, j::ExprKind::Lit(_));
        let (x, tx) = if lit(b) {
            (x, tx)
        } else {
            self.widen_atom(x, tx, &ty, a.span)?
        };
        let (y_, ty) = if lit(a) {
            (y_, ty)
        } else {
            self.widen_atom(y_, ty, &tx, b.span)?
        };
        if !ty.fits(&tx) && !tx.fits(&ty) {
            self.err(
                "E201",
                b.span,
                format!(
                    "cannot compare `{tx}` with `{ty}`",
                    ty = self.ty_text(&ty),
                    tx = self.ty_text(&tx)
                ),
            );
            return Ok((Rhs::Atom(x), Ty::Bool));
        }
        if matches!(name, "lt" | "le" | "gt" | "ge") && !self.implements(&tx, "Ord") {
            self.err(
                "E221",
                at,
                format!("`{tx}` does not implement `Ord`", tx = self.ty_text(&tx)),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
        }
        let args = vec![let_arg(x), let_arg(y_)];
        let c = match tx.scalar() {
            Some("bool") if !matches!(name, "eq" | "ne") => {
                self.err("E201", at, format!("`{op}` on `bool`"));
                return Ok((Rhs::Atom(Atom::Lit(Literal::Bool(false), at)), Ty::Bool));
            }
            Some(s) => call(&format!("{name}_{s}"), vec![], args, at),
            None => call(&format!("std.cmp.{name}"), vec![tx.targ()], args, at),
        };
        Ok((Rhs::Call(c), Ty::Bool))
    }
}
