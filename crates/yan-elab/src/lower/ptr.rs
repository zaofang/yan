use super::*;

/// The result type of a `core.ptr` function: a pointer to `T`, an address or a callback's.
const ADDR: Ty = Ty::Fixed("u64");

/// The functions that expose provenance (6.6), legal only in ffi modules.
pub(super) fn exposes(f: &str) -> bool {
    matches!(f, "expose_provenance" | "with_exposed_provenance")
}

impl Lower<'_, '_, '_> {
    /// `ptr.f(...)` or `ptr.f[T](...)` (6.6); `T` comes from the explicit argument, the pointer
    /// argument or the expected type.
    pub(super) fn ptr_call(
        &mut self,
        f: &str,
        explicit: Option<&j::Expr>,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        if !self.cx.ffi {
            self.err(
                "E419",
                at,
                format!("`ptr.{f}` exposes provenance, which belongs in an ffi module, under the package's `ffi/` directory"),
            );
            return Ok(err);
        }
        let arity = match f {
            "dangling" => 0,
            "with_addr" | "map_addr" => 2,
            "addr" | "without_provenance" | "expose_provenance" | "with_exposed_provenance" => 1,
            _ => {
                self.err("E203", at, format!("`ptr` has no function `{f}`"));
                return Ok(err);
            }
        };
        let Some(xs) = self.positional(args, arity, f, at)? else {
            return Ok(err);
        };
        let mut t = match explicit {
            Some(e) => Some(self.ptr_pointee(e)?),
            None => match want {
                Some(Ty::Foreign("Ptr", ts)) => ts.first().cloned(),
                _ => None,
            },
        };
        let mut out = Vec::new();
        for (i, x) in xs.iter().enumerate() {
            let pointer = i == 0 && !matches!(f, "without_provenance" | "with_exposed_provenance");
            let (a, at_) = if pointer {
                let (a, got) = self.atom(x, None)?;
                match &got {
                    Ty::Err => return Ok(err),
                    Ty::Foreign("Ptr", ts) => {
                        let found = ts.first().cloned();
                        if let Some(want) = &t
                            && t != found
                        {
                            self.err("E201", x.span, format!("`ptr.{f}` takes a `Ptr[{want}]`"));
                            return Ok(err);
                        }
                        t = found;
                    }
                    _ => {
                        self.err(
                            "E201",
                            x.span,
                            format!("`ptr.{f}` takes a `Ptr`, not `{got}`"),
                        );
                        return Ok(err);
                    }
                }
                (a, got)
            } else if f == "map_addr" {
                let want = Ty::Fn(false, vec![ADDR], Box::new(ADDR));
                let (a, got) = self.atom(x, Some(&want))?;
                self.expect(&got, &want, x.span);
                (a, got)
            } else {
                let (a, got) = self.atom(x, Some(&ADDR))?;
                self.expect(&got, &ADDR, x.span);
                (a, got)
            };
            let _ = at_;
            out.push(let_arg(a));
        }
        let Some(t) = t else {
            self.err(
                "E205",
                at,
                format!("`ptr.{f}` needs the pointee type: write `ptr.{f}[T](...)`"),
            );
            return Ok(err);
        };
        let rt = match f {
            "addr" | "expose_provenance" => ADDR,
            _ => Ty::Foreign("Ptr", vec![t.clone()]),
        };
        let c = call(&format!("std.ptr.{f}"), vec![t.targ()], out, at);
        Ok((Rhs::Call(c), rt, None))
    }

    pub(super) fn ptr_pointee(&mut self, e: &j::Expr) -> R<Ty> {
        let j::ExprKind::Name(tn) = &e.kind else {
            return unsupported("a `ptr` type argument that is not a name");
        };
        let named = j::Type::Named {
            path: vec![tn.clone()],
            args: Vec::new(),
        };
        self.g.ty(&named, self.cx, e.span)
    }

    /// `Ptr.of(&place)`: the address of a local, with its provenance (6.6).
    pub(super) fn ptr_of(&mut self, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        if !self.cx.ffi {
            self.err(
                "E419",
                at,
                "`Ptr.of` belongs in an ffi module, under the package's `ffi/` directory".into(),
            );
            return Ok(err);
        }
        let [
            j::Arg {
                kind: j::ArgKind::Ref(e),
                ..
            },
        ] = args
        else {
            self.err("E202", at, "`Ptr.of` takes one argument, `&place`".into());
            return Ok(err);
        };
        let j::ExprKind::Name(n) = &e.kind else {
            return unsupported("`Ptr.of` of a field or element");
        };
        let Some(t) = self.locals.get(n).map(|l| l.ty.clone()) else {
            self.err("E200", e.span, format!("`{n}` is not a local variable"));
            return Ok(err);
        };
        let c = call(
            "std.ptr.of",
            vec![t.targ()],
            vec![Arg::Value(
                Conv::Inout,
                Atom::Name(n.clone(), e.span),
                e.span,
            )],
            at,
        );
        Ok((Rhs::Call(c), Ty::Foreign("Ptr", vec![t]), None))
    }

    /// `p.cast[U]()`: the same address and provenance, read as a `U`.
    pub(super) fn ptr_cast(
        &mut self,
        recv: &j::Expr,
        to: &j::Expr,
        args: &[j::Arg],
        at: Span,
    ) -> R<(Rhs, Ty, Option<Ty>)> {
        let err = (Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None);
        if !args.is_empty() {
            self.err("E202", at, "`cast` takes no arguments".into());
            return Ok(err);
        }
        let (a, got) = self.atom(recv, None)?;
        let Ty::Foreign("Ptr", ts) = &got else {
            if got != Ty::Err {
                self.err(
                    "E201",
                    recv.span,
                    format!("`cast` takes a `Ptr`, not `{got}`"),
                );
            }
            return Ok(err);
        };
        let from = ts[0].clone();
        let u = self.ptr_pointee(to)?;
        let c = call(
            "std.ptr.cast",
            vec![from.targ(), u.targ()],
            vec![let_arg(a)],
            at,
        );
        Ok((Rhs::Call(c), Ty::Foreign("Ptr", vec![u]), None))
    }
}
