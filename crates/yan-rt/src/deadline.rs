//! The deadline scopes open in the running task (9.4): each entry is the earliest monotonic
//! reading of the scopes up to it, so the innermost entry is the deadline in force. `sched`
//! keeps one stack per task and a task starts with its block's enclosing deadline.

use std::cell::RefCell;

use crate::timeabi::join;

/// An open scope: its instant, and the count of enclosing `par` blocks whose cancellation it
/// holds (0 unless the scope is a shield).
pub(crate) type Stack = Vec<(i128, usize, bool)>;

thread_local! {
    static STACK: RefCell<Stack> = const { RefCell::new(Vec::new()) };
    /// Whether the deadline last read by an operation was a shield's own (R812).
    static HIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The deadline in force, if a scope is open.
pub fn current() -> Option<i128> {
    STACK.with(|s| {
        let s = s.borrow();
        HIT.with(|h| h.set(s.last().is_some_and(|e| e.2)));
        s.last().map(|e| e.0)
    })
}

/// The count of `par` blocks, from the outermost, whose cancellation a shield holds.
pub(crate) fn floor() -> usize {
    STACK.with(|s| s.borrow().iter().map(|e| e.1).max().unwrap_or(0))
}

pub(crate) fn take() -> Stack {
    STACK.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

pub(crate) fn install(stack: Stack) {
    STACK.with(|s| *s.borrow_mut() = stack);
}

pub(crate) fn snapshot() -> Stack {
    STACK.with(|s| s.borrow().clone())
}

/// Open a scope ending at `(hi, lo)`; the lowest open depth seen by the function, as for
/// `yan_rt_sup_begin`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_deadline_begin(low: u64, hi: i64, lo: i64) -> u64 {
    STACK.with(|s| {
        let mut s = s.borrow_mut();
        let depth = s.len() as u64;
        let at = join(hi, lo);
        let at = s.last().map_or(at, |d| d.0.min(at));
        s.push((at, 0, false));
        low.min(depth)
    })
}

/// Open a shield ending at `(hi, lo)`: the instant replaces the deadline in force, and the
/// cancellation of the `par` blocks around the running task is held until it ends (R811).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_shield_begin(low: u64, hi: i64, lo: i64) -> u64 {
    let held = crate::sched::depth();
    STACK.with(|s| {
        let mut s = s.borrow_mut();
        let depth = s.len() as u64;
        s.push((join(hi, lo), held, true));
        low.min(depth)
    })
}

/// Whether the last deadline an operation read was a shield's own, which a failure leaving the
/// shield reports as `ShieldTimeout`; reading clears it.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_shield_hit() -> u64 {
    u64::from(HIT.with(|h| h.take()))
}

/// End the innermost scope.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_deadline_end() {
    STACK.with(|s| {
        s.borrow_mut().pop();
    });
}

/// End the scopes past the first `depth`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_deadline_end_to(depth: u64) {
    STACK.with(|s| s.borrow_mut().truncate(depth as usize));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shield_replaces_the_deadline() {
        let low = yan_rt_deadline_begin(u64::MAX, 0, 20);
        yan_rt_shield_begin(low, 0, 90);
        assert_eq!(current(), Some(90));
        yan_rt_deadline_end_to(low);
        assert_eq!(current(), None);
    }

    #[test]
    fn a_nested_scope_keeps_the_earlier_deadline() {
        let low = yan_rt_deadline_begin(u64::MAX, 0, 50);
        yan_rt_deadline_begin(low, 0, 80);
        assert_eq!(current(), Some(50));
        yan_rt_deadline_end();
        yan_rt_deadline_begin(low, 0, 20);
        assert_eq!(current(), Some(20));
        yan_rt_deadline_end_to(low);
        assert_eq!(current(), None);
    }
}
