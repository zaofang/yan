//! Cancel scopes as values (9.4, `std.scope`): the root scope, each `par concurrent` block and
//! each deadline, timeout or shield scope has an id, and a scope drains when it or a scope
//! around it was drained. The interpreter keeps its own [`Scopes`]; native code and the
//! machine share the thread's.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// The scope std binds around `main`; it never closes.
pub const ROOT: u64 = 0;

/// The open scopes: each one's parent and whether it was drained.
#[derive(Default)]
pub struct Scopes {
    last: u64,
    open: BTreeMap<u64, (u64, bool)>,
    root_drained: bool,
}

impl Scopes {
    pub fn open(&mut self, parent: u64) -> u64 {
        self.last += 1;
        self.open.insert(self.last, (parent, false));
        self.last
    }

    pub fn close(&mut self, id: u64) {
        self.open.remove(&id);
    }

    /// Close `id` and the scopes around it, up to but not `outer`.
    pub fn close_to(&mut self, mut id: u64, outer: u64) {
        while id != outer {
            match self.open.remove(&id) {
                Some((parent, _)) => id = parent,
                None => break,
            }
        }
    }

    /// Draining a scope that has ended does nothing.
    pub fn drain(&mut self, id: u64) {
        if id == ROOT {
            self.root_drained = true;
        } else if let Some(e) = self.open.get_mut(&id) {
            e.1 = true;
        }
    }

    /// A scope that has ended is not draining.
    pub fn draining(&self, mut id: u64) -> bool {
        loop {
            if id == ROOT {
                return self.root_drained;
            }
            match self.open.get(&id) {
                Some((_, true)) => return true,
                Some((parent, false)) => id = *parent,
                None => return false,
            }
        }
    }
}

thread_local! {
    static SCOPES: RefCell<Scopes> = RefCell::default();
}

/// Open a scope inside the running task's innermost one.
pub(crate) fn open() -> u64 {
    let parent = yan_rt_scope_current();
    SCOPES.with(|s| s.borrow_mut().open(parent))
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

/// `Scope.drain`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_scope_drain(id: u64) {
    SCOPES.with(|s| s.borrow_mut().drain(id));
}

/// `Scope.is_draining`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_scope_draining(id: u64) -> u64 {
    u64::from(SCOPES.with(|s| s.borrow().draining(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drain_reaches_the_scopes_inside() {
        let mut s = Scopes::default();
        let a = s.open(ROOT);
        let b = s.open(a);
        let c = s.open(ROOT);
        s.drain(a);
        assert!(s.draining(a) && s.draining(b));
        assert!(!s.draining(c) && !s.draining(ROOT));
        s.drain(ROOT);
        assert!(s.draining(c));
    }

    #[test]
    fn an_ended_scope_is_not_draining() {
        let mut s = Scopes::default();
        let a = s.open(ROOT);
        let b = s.open(a);
        s.drain(a);
        s.close_to(b, ROOT);
        assert!(!s.draining(a) && !s.draining(b));
        s.drain(a);
        assert!(!s.draining(a));
        assert_eq!(s.open(ROOT), 3);
    }
}
