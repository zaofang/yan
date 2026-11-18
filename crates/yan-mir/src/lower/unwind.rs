//! Leaving the function from a `par concurrent` branch. Every task that leaves runs its own
//! drops, but the names bound outside its branch belong to the frame the block returns from
//! once: the task drops them behind an `Unwind` target, which runs only for the result the
//! block keeps (R394).

use super::*;

impl Fn<'_, '_> {
    /// Start the frame of a `par` branch; `plain` for a branch without a scheduler.
    pub(super) fn enter_frame(&mut self, plain: bool) {
        self.task_starts.push(self.locals.len() as Local);
        self.frame_plain.push(plain);
    }

    pub(super) fn leave_frame(&mut self) {
        self.task_starts.pop();
        self.frame_plain.pop();
    }

    /// Whether the code is in a branch of a `par concurrent`.
    pub(super) fn in_task(&self) -> bool {
        self.frame_plain.iter().any(|p| !p)
    }

    /// How many enclosing branches bind `n`: 0 for a name of the function itself.
    fn depth(&self, n: &str) -> usize {
        self.names
            .get(n)
            .and_then(|v| v.locals.first())
            .map_or(0, |l| {
                self.task_starts.iter().take_while(|s| l >= *s).count()
            })
    }

    /// `drop n` in a branch: a name of an enclosing frame waits for the `return` or `fail` that
    /// follows, so that it is dropped once.
    pub(super) fn drop_stmt(&mut self, n: &y::Name) {
        if self.depth(n) < self.task_starts.len() {
            self.deferred.push(n.clone());
        } else {
            self.drop_owned(n);
        }
    }

    /// `if _dN { drop n }` for the flag of `n`, the form the elaborator writes it in.
    pub(super) fn flagged_drop<'s>(&self, k: &'s y::StmtKind) -> Option<&'s y::Name> {
        let y::StmtKind::If {
            cond: y::Atom::Name(f, _),
            then,
            els: None,
        } = k
        else {
            return None;
        };
        match then.stmts.as_slice() {
            [
                y::Stmt {
                    kind: y::StmtKind::Drop(n),
                    ..
                },
            ] if self.flags.get(n) == Some(f) => Some(n),
            _ => None,
        }
    }

    /// Drop `n`, under its flag if it has one: a drop list or a deferred drop names a flagged
    /// local that may have been moved (R871).
    fn drop_owned(&mut self, n: &str) {
        let Some(flag) = self.flags.get(n).and_then(|f| self.names.get(f)) else {
            self.drop_name(n);
            return self.mark_name(n);
        };
        let (set, unset) = self.branch(Operand::Local(flag.locals[0]));
        self.cur = set;
        self.drop_name(n);
        self.mark_name(n);
        self.goto(unset);
    }

    /// Drops deferred by [`Self::drop_stmt`] that no exit followed.
    pub(super) fn flush_deferred(&mut self) {
        for n in std::mem::take(&mut self.deferred) {
            self.drop_owned(&n);
        }
    }

    /// Drop `drops` and the deferred drops, reporting the failure `ev` of shape `e`, if any,
    /// to each enclosing `par concurrent`. The caller then returns.
    pub(super) fn unwind(&mut self, drops: &[y::Name], failure: Option<(&Shape, &[Operand])>) {
        let mut drops = drops.to_vec();
        drops.append(&mut self.deferred);
        for d in (0..=self.task_starts.len()).rev() {
            for n in &drops {
                if self.depth(n) == d {
                    self.drop_owned(n);
                }
            }
            if d == 0 {
                break;
            }
            let drops = self.block_new();
            if self.frame_plain[d - 1] {
                self.emit(Inst::Arm(ArmEvent::Leave));
                self.finish(Term::Jump(drops));
            } else {
                if let Some((e, ev)) = failure {
                    self.exit_kind(e, ev);
                }
                self.finish(Term::Unwind { drops });
            }
            self.cur = drops;
        }
    }
}
