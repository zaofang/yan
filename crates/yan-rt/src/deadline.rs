//! The deadline scopes open in the running task (9.4): each entry is the earliest monotonic
//! reading of the scopes up to it, so the innermost entry is the deadline in force. `sched`
//! keeps one stack per task and a task starts with its block's enclosing deadline.

use std::cell::RefCell;

use crate::timeabi::join;

thread_local! {
    static STACK: RefCell<Vec<i128>> = const { RefCell::new(Vec::new()) };
}

/// The deadline in force, if a scope is open.
pub fn current() -> Option<i128> {
    STACK.with(|s| s.borrow().last().copied())
}

pub(crate) fn take() -> Vec<i128> {
    STACK.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

pub(crate) fn install(stack: Vec<i128>) {
    STACK.with(|s| *s.borrow_mut() = stack);
}

pub(crate) fn snapshot() -> Vec<i128> {
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
        let at = s.last().map_or(at, |d| (*d).min(at));
        s.push(at);
        low.min(depth)
    })
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
