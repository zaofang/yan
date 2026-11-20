//! The drain phase of cancel scopes (9.4). Only the root scope, id 0, has a handle so far.
//! State is per thread, like the scheduler's.

use std::cell::Cell;

thread_local! {
    static ROOT_DRAINING: Cell<bool> = const { Cell::new(false) };
}

pub fn drain(id: i64) {
    debug_assert_eq!(id, 0, "only the root scope has a handle");
    ROOT_DRAINING.with(|d| d.set(true));
}

pub fn is_draining(id: i64) -> bool {
    debug_assert_eq!(id, 0, "only the root scope has a handle");
    ROOT_DRAINING.with(Cell::get)
}

pub(crate) fn reset() {
    ROOT_DRAINING.with(|d| d.set(false));
}

/// `std.scope.drain_id`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_scope_drain(id: i64) {
    drain(id);
}

/// `std.scope.is_draining_id`: 1 when draining.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_scope_draining(id: i64) -> i64 {
    i64::from(is_draining(id))
}
