//! Leaving the function from a `par concurrent` branch. Every task that leaves runs its own
//! drops, but the names bound outside its branch belong to the frame the block returns from
//! once: the task drops them behind an `Unwind` target, which runs only for the result the
//! block keeps (R394).

use super::*;

impl Fn<'_, '_> {
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
            self.drop_name(n);
        }
    }

    /// Drops deferred by [`Self::drop_stmt`] that no exit followed.
    pub(super) fn flush_deferred(&mut self) {
        for n in std::mem::take(&mut self.deferred) {
            self.drop_name(&n);
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
                    self.drop_name(n);
                }
            }
            if d == 0 {
                break;
            }
            if let Some((e, ev)) = failure {
                self.exit_kind(e, ev);
            }
            let drops = self.block_new();
            self.finish(Term::Unwind { drops });
            self.cur = drops;
        }
    }
}
