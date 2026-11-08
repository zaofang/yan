//! Contract checks in a function body: `requires` on entry, `ensures` at every normal return (R882).

use super::*;

impl Fn<'_, '_> {
    /// Panic with kind `contract` unless predicate `e`, clause `clause` of the item, holds.
    pub(super) fn check_clause(&mut self, e: &y::Expr, clause: i128) -> R<()> {
        let t = self.expr(e)?;
        let mut fields = vec![("clause".to_owned(), Operand::Int(clause))];
        if let Some(h) = self.item.clone() {
            fields.push(("item".to_owned(), self.lit(&h)));
        }
        for n in free_names(e) {
            if let Some(v) = self.scalar_field(&y::Atom::Name(n.clone(), Default::default())) {
                fields.push((n, v));
            }
        }
        let fields = fields.into_iter().map(|(n, v)| (self.lit(&n), v)).collect();
        self.check_fields(t, "contract", fields);
        Ok(())
    }

    /// Snapshot each parameter `p` that an `ensures` reads as `old_p`, the entry value.
    pub(super) fn bind_olds(&mut self) {
        let mut olds = Vec::new();
        for (e, _) in &self.ensures {
            for n in free_names(e) {
                if let Some(p) = n.strip_prefix("old_")
                    && !self.names.contains_key(&n)
                    && let Some(v) = self.names.get(p)
                {
                    olds.push((n.clone(), v.clone()));
                }
            }
        }
        for (n, v) in olds {
            let var = self.var(v.shape.clone());
            self.copy_to(
                &var.locals.clone(),
                v.locals.iter().map(|l| Operand::Local(*l)).collect(),
            );
            self.names.insert(n, var);
        }
    }

    /// Check every `ensures` with `out` bound to the returned value `v` of shape `ok`.
    pub(super) fn check_ensures(&mut self, v: &[Operand], ok: &Shape) -> R<()> {
        let out = self.var(ok.clone());
        self.copy_to(&out.locals.clone(), v.to_vec());
        let shadowed = self.names.insert("out".to_owned(), out);
        for (e, clause) in self.ensures.clone() {
            self.check_clause(&e, clause)?;
        }
        match shadowed {
            Some(s) => self.names.insert("out".to_owned(), s),
            None => self.names.remove("out"),
        };
        Ok(())
    }
}
