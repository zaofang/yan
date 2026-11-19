//! `?` (14.1): the failure conversion of a propagated `E`, and `?` on a reified `Result` value.

use super::*;

impl Lower<'_, '_, '_> {
    /// The `From` function or `Error` wrapper a propagated `e` goes through to become this
    /// function's failure, with E310 and E313 reported on the way.
    pub(super) fn fail_conversion(&mut self, e: &Ty, at: Span) -> Option<Vec<String>> {
        if self.sig.fail.is_none() {
            self.err(
                "E310",
                at,
                format!(
                    "`?` passes a `{e}` failure up, but `{}` cannot fail; \
                     declare `fail {e}` on it or handle the failure with `else`",
                    self.item
                ),
            );
        }
        // `?` injects any failure into `Error` with no `From` (R432).
        let into_error = *e != Ty::Data("Error".into())
            && self.sig.fail == Some(Ty::Data("Error".into()))
            && self.g.std_records.contains("Error");
        let from = self
            .sig
            .fail
            .as_ref()
            .and_then(|own| self.g.froms.get(&(e.to_string(), own.to_string())));
        let injects = matches!(&self.sig.fail, Some(Ty::Union(ms)) if ms.contains(e));
        if from.is_some() && injects {
            self.err(
                "E313",
                at,
                format!(
                    "`?` could inject the `{e}` failure into the union or convert it \
                     with `From`; remove the `From` implementation"
                ),
            );
        }
        match from {
            Some(f) => Some(vec![f.clone()]),
            None => into_error.then(|| vec!["std".into(), "error".into(), "wrap".into()]),
        }
    }

    /// `v?` for a `Result[T, E]` value `v`: `match v { Ok(x) => x, Err(e) => fail e }`, with the
    /// same conversion of `e` as a call's `?`.
    pub(super) fn propagate_value(&mut self, x: &j::Expr, at: Span) -> R<(Rhs, Ty)> {
        let (v, t) = self.atom(x, None)?;
        let (ok_ty, err_ty) = match t {
            Ty::Result(o, e) => (*o, *e),
            Ty::Err => return Ok((Rhs::Atom(v), Ty::Err)),
            Ty::Option(_) => return unsupported("`?` on an optional"),
            t => {
                self.err(
                    "E201",
                    x.span,
                    format!(
                        "`?` needs a call that can fail or a `Result` value, found `{t}`; \
                         the value cannot fail",
                        t = self.ty_text(&t)
                    ),
                );
                return Ok((Rhs::Atom(v), Ty::Err));
            }
        };
        let via = self.fail_conversion(&err_ty, at);
        let (out, ty) = self.join(at, |l, k| {
            let ok = l.fresh("o");
            let ok_block = l.block_with(|l| {
                l.declare(&ok, ok_ty.clone());
                l.jump(k, Atom::Name(ok.clone(), at), at);
                Ok(())
            })?;
            let e = l.fresh("e");
            let err_block = l.block_with(|l| {
                l.declare(&e, err_ty.clone());
                let mut f = Atom::Name(e.clone(), at);
                if let (Some(path), Some(own)) = (&via, l.sig.fail.clone()) {
                    let targs = if path.len() == 3 {
                        vec![err_ty.targ()]
                    } else {
                        vec![]
                    };
                    let c = y::Call {
                        path: path.clone(),
                        span: at,
                        targs,
                        args: vec![sink_arg(f)],
                    };
                    f = l.tmp(&own, Rhs::Call(c), at);
                }
                l.emit(StmtKind::Fail(f), at);
                Ok(())
            })?;
            let pat = |c: &str, f: &str, n: &str| y::Pat::Ctor {
                path: vec![c.into()],
                fields: Some((vec![(f.into(), y::Pat::Bind(n.into()))], false)),
            };
            let m = y::Stmt {
                kind: StmtKind::Match {
                    scrutinee: v,
                    arms: vec![
                        (pat("Ok", "value", &ok), ok_block),
                        (pat("Err", "error", &e), err_block),
                    ],
                },
                span: at,
            };
            Ok((m, ok_ty.clone()))
        })?;
        Ok((Rhs::Atom(out), ty))
    }
}
