//! `with deadline(at): body` (9.4): the runtime keeps the open scopes, so every exit of the
//! body ends them, as for `with supervisor` (`actor.rs`).

use super::*;

impl<'m> Fn<'_, 'm> {
    pub(super) fn with_deadline(&mut self, at: &y::Atom, body: &'m y::Block) -> R<()> {
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
            f: RtFn::DeadlineBegin,
            args: vec![Operand::Local(depth), hi, lo],
        });
        self.scopes.push(Scope::Deadline);
        self.block(body)?;
        self.scopes.pop();
        self.end_deadline();
        Ok(())
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
