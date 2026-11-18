use super::*;

impl Lower<'_, '_, '_> {
    /// A constructor of the generic sum `s`. Its type arguments come from the expected type,
    /// else from its field arguments; an argument still unfixed is E241 (D4).
    pub(super) fn generic_ctor(
        &mut self,
        n: &str,
        s: &str,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<(Rhs, Ty)> {
        let params = self.g.type_params[s].clone();
        let generic = Ty::Inst(
            s.to_owned(),
            params.iter().map(|p| Ty::Param(p.clone())).collect(),
        );
        let ps = match self.g.records.get(n) {
            Some(fs) => {
                let pos = self.g.record_positional.get(n).copied().unwrap_or(0);
                field_params(fs, pos, at)
            }
            None => {
                let fs = &self.g.sums[s]
                    .iter()
                    .find(|(v, _)| v == n)
                    .expect("declared")
                    .1;
                field_params(fs, 0, at)
            }
        };
        let outer = mem::take(&mut self.arg_tys);
        if let Some(w @ Ty::Inst(ws, _)) = want
            && ws == s
        {
            self.arg_tys.push((generic, w.clone(), at));
        }
        let r = self.ctor(Some(s), n, &ps, args, at);
        let got = mem::replace(&mut self.arg_tys, outer);
        let r = r?;
        let mut map = Vec::new();
        for (w, t, _) in &got {
            let mut found = Vec::new();
            w.unify(t, &mut found);
            // A parameter of a callee still being inferred is no type: the fields decide it.
            let scope = self.g.scope.borrow();
            let known = |t: &Ty| !matches!(t, Ty::Param(q) if !scope.contains(q));
            found.retain(|(p, t)| known(t) && !map.iter().any(|(m, _)| m == p));
            map.extend(found);
        }
        let Some(targs) = params
            .iter()
            .map(|p| map.iter().find(|(m, _)| m == p).map(|(_, t)| t.clone()))
            .collect::<Option<Vec<Ty>>>()
        else {
            self.err(
                "E241",
                at,
                format!("annotate the type arguments of this `{n}`"),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
        };
        let r = match r {
            Rhs::Ctor(y::Ctor::Named { path, fields, .. }) => Rhs::Ctor(y::Ctor::Named {
                path,
                targs: targs.iter().map(Ty::targ).collect(),
                fields,
            }),
            r => r,
        };
        Ok((r, Ty::Inst(s.to_owned(), targs)))
    }
}
