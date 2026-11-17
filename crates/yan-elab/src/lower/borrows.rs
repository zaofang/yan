//! What a local view or `Ref` borrows from (4.5, 4.7): the escape check of a returned view
//! (E414) and the exclusivity of `r.set(v)` against a live `r.get()` view (E402, R1002).

use super::*;

impl Lower<'_, '_, '_> {
    /// The locals and parameters `value` borrows from when a local is bound to it: a view or
    /// `Ref` named directly, or a std method whose result is `from self`.
    pub(super) fn borrowed_by(&self, value: &j::Expr) -> Option<Vec<String>> {
        let recv = match &value.kind {
            j::ExprKind::Name(y) => y,
            j::ExprKind::Call(f, _) => {
                let j::ExprKind::Attr(recv, m, _) = &f.kind else {
                    return None;
                };
                let j::ExprKind::Name(y) = &recv.kind else {
                    return None;
                };
                let module = self.ty_of_local(recv).as_ref().and_then(Ty::module)?;
                let d = crate::stdlib::decl(&format!("std.{module}.{m}"))?;
                let from_self = d
                    .def
                    .ret
                    .as_ref()
                    .is_some_and(|r| r.from.iter().any(|f| f == "self"));
                if !from_self {
                    return None;
                }
                return Some(self.chain(y));
            }
            _ => return None,
        };
        let borrowing =
            self.borrows.contains_key(recv) || self.views.contains(recv) || self.is_ref_local(recv);
        borrowing.then(|| self.chain(recv))
    }

    /// Records what local `n` now borrows from. A copy-typed read is a copy, not a view, so it
    /// borrows nothing.
    pub(super) fn note_borrow(&mut self, n: &str, from: Option<Vec<String>>) {
        let copied = self
            .locals
            .get(n)
            .is_some_and(|l| l.ty.is_copy() && !matches!(l.ty, Ty::Coll("Ref", _)));
        match from {
            Some(from) if !copied => self.borrows.insert(n.to_owned(), from),
            _ => self.borrows.remove(n),
        };
    }

    /// `x` and everything it borrows from.
    fn chain(&self, x: &str) -> Vec<String> {
        let mut out = vec![x.to_owned()];
        out.extend(self.borrows.get(x).into_iter().flatten().cloned());
        out
    }

    fn is_ref_local(&self, n: &str) -> bool {
        self.locals
            .get(n)
            .is_some_and(|l| matches!(l.ty, Ty::Coll("Ref", _)))
    }

    /// The local that ends with the function and that `x` ultimately borrows from, if any.
    pub(super) fn local_root(&self, x: &str) -> Option<String> {
        self.chain(x).into_iter().find(|n| {
            !self.borrows.contains_key(n) && !self.sig.params.iter().any(|q| q.name == *n)
        })
    }

    /// E402: `r.set(v)` writes the target while a view from `r.get()` (through any alias of `r`)
    /// is read later (R1002).
    pub(super) fn ref_set_exclusive(&mut self, recv: &j::Expr, at: Span) {
        let j::ExprKind::Name(r) = &recv.kind else {
            return;
        };
        let refs = |x: &str| -> Vec<String> {
            self.chain(x)
                .into_iter()
                .filter(|n| self.is_ref_local(n) || self.sig.params.iter().any(|q| q.name == *n))
                .collect()
        };
        let mine = refs(r);
        let mut hits: Vec<(String, Span)> = Vec::new();
        let mut views: Vec<&String> = self.borrows.keys().collect();
        views.sort();
        for v in views {
            if self.is_ref_local(v) || !refs(v).iter().any(|x| mine.contains(x)) {
                continue;
            }
            if let Some(later) = self.use_after(v, recv.span, at) {
                hits.push((v.clone(), later));
            }
        }
        for (v, later) in hits {
            self.err(
                "E402",
                at,
                format!("`{r}.set` writes the target while the view `{v}` of it is live"),
            );
            self.cx.label(later, "view used here");
        }
    }

    /// The first read of `v` after the read starting at `from`, skipping reads inside `within`.
    fn use_after(&self, v: &str, from: Span, within: Span) -> Option<Span> {
        let inside = |s: Span| {
            (s.line, s.col) >= (within.line, within.col)
                && (s.end_line, s.end_col) <= (within.end_line, within.end_col)
        };
        let mut seen = HashSet::new();
        let mut at = from;
        while let Some(u) = self.next_use(v, at) {
            if !inside(u) {
                return Some(u);
            }
            if !seen.insert((u.line, u.col)) {
                return None;
            }
            at = u;
        }
        None
    }
}
