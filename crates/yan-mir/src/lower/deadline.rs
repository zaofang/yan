//! `with deadline(at): body` (9.4): the runtime keeps the open scopes, so every exit of the
//! body ends them, as for `with supervisor` (`actor.rs`).

use super::*;

impl<'m> Fn<'_, 'm> {
    pub(super) fn with_deadline(
        &mut self,
        at: &y::Atom,
        body: &'m y::Block,
        shield: bool,
    ) -> R<()> {
        let (v, _) = self.atom_any(at)?;
        let (Some(hi), Some(lo)) = (v.first().cloned(), v.get(1).cloned()) else {
            return unsupported("a `deadline` that is not a monotonic instant");
        };
        let depth = match self.deadline_depth {
            Some(d) => d,
            None => {
                let d = self.entry_local(i128::from(i64::MAX));
                self.deadline_depth = Some(d);
                d
            }
        };
        self.emit(Inst::Rt {
            dst: Some(depth),
            f: if shield {
                RtFn::ShieldBegin
            } else {
                RtFn::DeadlineBegin
            },
            args: vec![Operand::Local(depth), hi, lo],
        });
        self.scopes.push(Scope::Deadline);
        self.shields += u32::from(shield);
        self.block(body)?;
        self.shields -= u32::from(shield);
        self.scopes.pop();
        self.end_deadline();
        Ok(())
    }

    /// A `Timeout` that a shield's own expiry raised becomes `ShieldTimeout` as it leaves (R812).
    pub(super) fn shield_failure(&mut self, e: &Shape, v: Vec<Operand>) -> Vec<Operand> {
        let (Some((tag, ..)), Some(_)) = (e.variant("Timeout"), e.variant("ShieldTimeout")) else {
            return v;
        };
        let out: Vec<Local> = e.tys().into_iter().map(|t| self.local(t)).collect();
        self.copy_to(&out, v.clone());
        let join = self.block_new();
        let is_timeout = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(tag)]);
        let (timeout, other) = self.branch(is_timeout);
        self.cur = other;
        self.goto(join);
        self.cur = timeout;
        let hit = self.local(I64);
        self.emit(Inst::Rt {
            dst: Some(hit),
            f: RtFn::ShieldHit,
            args: Vec::new(),
        });
        let own = self.scalar_op(
            ScalarOp::Ne,
            I64,
            vec![Operand::Local(hit), Operand::Int(0)],
        );
        let (convert, keep) = self.branch(own);
        self.cur = keep;
        self.goto(join);
        self.cur = convert;
        self.copy_to(&out, Self::variant_value(e, "ShieldTimeout", Vec::new()));
        self.goto(join);
        out.into_iter().map(Operand::Local).collect()
    }

    pub(super) fn end_deadline(&mut self) {
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::DeadlineEnd,
            args: Vec::new(),
        });
    }

    /// End the `with deadline` scopes this function opened, before it leaves.
    pub(super) fn end_deadlines(&mut self) {
        if let Some(d) = self.deadline_depth {
            self.push(Inst::Rt {
                dst: None,
                f: RtFn::DeadlineEndTo,
                args: vec![Operand::Local(d)],
            });
        }
    }
}
