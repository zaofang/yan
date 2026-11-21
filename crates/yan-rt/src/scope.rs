//! Cancel scopes as values (9.4, `std.scope`): the root scope, each `par concurrent` block and
//! each deadline, timeout or shield scope has an id, and a scope drains when it or a scope
//! around it was drained. A drained scope is cancelled once its grace has passed on the bound
//! `time` handler's monotonic clock. The interpreter keeps its own [`Scopes`]; native code and the
//! machine share the thread's.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// The scope std binds around `main`; it never closes.
pub const ROOT: u64 = 0;

/// The cancel grace in nanoseconds (9.4): a task alive this long after its drained scope was
/// cancelled aborts the process with a panic of kind `shutdown`.
pub const CANCEL_GRACE: i128 = 10_000_000_000;

struct Scope {
    parent: u64,
    /// A shield holds the cancellation of the scopes around it (R811).
    shield: bool,
    /// Set by the first drain: the monotonic reading its grace ends at.
    cancel_at: Option<i128>,
}

/// The open scopes.
#[derive(Default)]
pub struct Scopes {
    last: u64,
    open: BTreeMap<u64, Scope>,
    root_cancel_at: Option<i128>,
}

impl Scopes {
    pub fn open(&mut self, parent: u64, shield: bool) -> u64 {
        self.last += 1;
        let s = Scope {
            parent,
            shield,
            cancel_at: None,
        };
        self.open.insert(self.last, s);
        self.last
    }

    pub fn close(&mut self, id: u64) {
        self.open.remove(&id);
    }

    /// Close `id` and the scopes around it, up to but not `outer`.
    pub fn close_to(&mut self, mut id: u64, outer: u64) {
        while id != outer {
            match self.open.remove(&id) {
                Some(s) => id = s.parent,
                None => break,
            }
        }
    }

    /// Drain `id`, to be cancelled at monotonic reading `cancel_at`. Draining a scope again, or
    /// one that has ended, does nothing.
    pub fn drain(&mut self, id: u64, cancel_at: i128) {
        if id == ROOT {
            if self.root_at().is_none() {
                self.root_cancel_at = Some(cancel_at);
            }
        } else if let Some(s) = self.open.get_mut(&id) {
            s.cancel_at.get_or_insert(cancel_at);
        }
    }

    /// When the root is cancelled: by its own drain, or by a signal's (9.4).
    fn root_at(&self) -> Option<i128> {
        self.root_cancel_at.or_else(crate::signal::root_cancel_at)
    }

    /// A scope that has ended is not draining.
    pub fn draining(&self, mut id: u64) -> bool {
        loop {
            if id == ROOT {
                return self.root_at().is_some();
            }
            match self.open.get(&id) {
                Some(s) if s.cancel_at.is_some() => return true,
                Some(s) => id = s.parent,
                None => return false,
            }
        }
    }

    /// The monotonic reading at which a task whose innermost scope is `id` is cancelled: the
    /// earliest grace end of the draining scopes from `id` out to the innermost shield.
    pub fn cancel_at(&self, mut id: u64) -> Option<i128> {
        let mut at: Option<i128> = None;
        let mut earliest = |c: Option<i128>| at = [at, c].into_iter().flatten().min();
        loop {
            if id == ROOT {
                earliest(self.root_at());
                break;
            }
            let Some(s) = self.open.get(&id) else { break };
            earliest(s.cancel_at);
            if s.shield {
                break;
            }
            id = s.parent;
        }
        at
    }

    /// The latest instant the innermost shield around `id` may run to: the end of the cancel
    /// grace of the draining scopes it holds back, if any drains (R812).
    pub fn shield_limit(&self, mut id: u64) -> Option<i128> {
        loop {
            let s = self.open.get(&id)?;
            if s.shield {
                return self.cancel_at(s.parent).map(|c| c + CANCEL_GRACE);
            }
            id = s.parent;
        }
    }
}

thread_local! {
    static SCOPES: RefCell<Scopes> = RefCell::default();
}

/// Open a scope inside the running task's innermost one.
pub(crate) fn open(shield: bool) -> u64 {
    let parent = yan_rt_scope_current();
    SCOPES.with(|s| s.borrow_mut().open(parent, shield))
}

/// When the running task is cancelled by a drain, if a scope it is in drains.
pub(crate) fn cancel_at() -> Option<i128> {
    crate::signal::observe(crate::civil::mono_now);
    let id = yan_rt_scope_current();
    SCOPES.with(|s| s.borrow().cancel_at(id))
}

/// Where a drain clamps the shield the running task is in.
pub(crate) fn shield_limit() -> Option<i128> {
    crate::signal::observe(crate::civil::mono_now);
    let id = yan_rt_scope_current();
    SCOPES.with(|s| s.borrow().shield_limit(id))
}

/// The grace of a drain around the running task has passed (9.4).
pub(crate) fn cancelled() -> bool {
    cancel_at().is_some_and(|c| crate::civil::mono_now() >= c)
}

/// As [`cancelled`], at a cancellation point: a task still alive once the cancel grace has also
/// passed aborts the process.
pub(crate) fn cancelled_or_abort() -> bool {
    let Some(c) = cancel_at() else {
        return false;
    };
    let now = crate::civil::mono_now();
    if now >= c + CANCEL_GRACE {
        let k = "shutdown";
        // SAFETY: a static string.
        unsafe { crate::abi::yan_rt_panic(k.as_ptr(), k.len()) }
    }
    now >= c
}

/// The program ends: the next one on this thread starts with a fresh root.
pub(crate) fn reset() {
    SCOPES.with(|s| *s.borrow_mut() = Scopes::default());
}

pub(crate) fn close(id: u64) {
    SCOPES.with(|s| s.borrow_mut().close(id));
}

/// `std.scope.current`: the innermost cancel scope open in the running task.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_scope_current() -> u64 {
    crate::deadline::scope().unwrap_or_else(crate::sched::scope)
}

/// `Scope.drain`: the grace `(hi, lo)` runs on the innermost `time` handler's clock.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_scope_drain(id: u64, hi: i64, lo: i64) {
    crate::signal::observe(crate::civil::mono_now);
    let at = crate::civil::mono_now().saturating_add(crate::timeabi::join(hi, lo));
    SCOPES.with(|s| s.borrow_mut().drain(id, at));
}

/// `Scope.is_draining`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_scope_draining(id: u64) -> u64 {
    crate::signal::observe(crate::civil::mono_now);
    u64::from(SCOPES.with(|s| s.borrow().draining(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drain_reaches_the_scopes_inside() {
        let mut s = Scopes::default();
        let a = s.open(ROOT, false);
        let b = s.open(a, false);
        let c = s.open(ROOT, false);
        s.drain(a, 5);
        assert!(s.draining(a) && s.draining(b));
        assert!(!s.draining(c) && !s.draining(ROOT));
        s.drain(ROOT, 9);
        assert!(s.draining(c));
        assert_eq!((s.cancel_at(b), s.cancel_at(c)), (Some(5), Some(9)));
        s.drain(a, 1);
        assert_eq!(s.cancel_at(a), Some(5));
    }

    #[test]
    fn an_ended_scope_is_not_draining() {
        let mut s = Scopes::default();
        let a = s.open(ROOT, false);
        let b = s.open(a, false);
        s.drain(a, 5);
        s.close_to(b, ROOT);
        assert!(!s.draining(a) && !s.draining(b));
        s.drain(a, 5);
        assert!(!s.draining(a));
        assert_eq!(s.open(ROOT, false), 3);
    }

    #[test]
    fn a_shield_holds_the_cancel_of_the_scopes_around_it() {
        let mut s = Scopes::default();
        let a = s.open(ROOT, false);
        let sh = s.open(a, true);
        let in_sh = s.open(sh, false);
        s.drain(a, 5);
        assert!(s.draining(in_sh));
        assert_eq!(s.cancel_at(in_sh), None);
        assert_eq!(s.shield_limit(in_sh), Some(5 + CANCEL_GRACE));
        assert_eq!(s.shield_limit(a), None);
        s.drain(sh, 7);
        assert_eq!(s.cancel_at(in_sh), Some(7));
    }
}
