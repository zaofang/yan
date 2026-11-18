//! `Atomic.wait` and `notify` over the cooperative scheduler (RFC 0089 D3): a task whose word
//! still equals the expected value parks on the word's address, `notify` releases the oldest
//! waiters of an address first, and the released task's retried `wait` returns. Every task
//! parked with no task to release it is a global deadlock (R396).

use crate::chan::{BLOCKED, CANCELLED};
use crate::sched;
use std::cell::RefCell;

/// The park key of an address: apart from channels and actors (R959).
const KEY: usize = 1 << 62;

#[derive(Default)]
struct State {
    /// Tasks parked in `wait`, oldest first: (task path, address).
    parked: Vec<(Vec<usize>, usize)>,
    /// Tasks `notify` released that have not yet resumed.
    woken: Vec<Vec<usize>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// `wait` on `addr`, whose word `equal`s the expected value now: 0 when it returns, or
/// [`BLOCKED`] when it parked the task (run it again on resume), [`CANCELLED`], or the status of
/// a wait that cannot park (an idle actor's [`crate::actor::WORK`], or a deadlock).
pub fn wait(addr: usize, equal: bool, in_task: bool) -> u64 {
    let me = sched::path();
    let leave = |st: &mut State| st.parked.retain(|(p, _)| *p != me);
    if in_task && sched::cancelled() {
        with(leave);
        return CANCELLED;
    }
    let released = with(|st| match st.woken.iter().position(|p| *p == me) {
        Some(k) => {
            st.woken.remove(k);
            true
        }
        None => false,
    });
    if released || !equal {
        with(leave);
        return 0;
    }
    with(|st| {
        if !st.parked.iter().any(|(p, _)| *p == me) {
            st.parked.push((me.clone(), addr));
        }
    });
    if sched::park((false, KEY + addr), in_task) {
        return BLOCKED;
    }
    with(leave);
    crate::chan::idle_or_deadlock()
}

/// Release up to `count` tasks parked on `addr`, oldest first.
pub fn notify(addr: usize, count: usize) {
    let n = with(|st| {
        let mut n = 0;
        let mut k = 0;
        while k < st.parked.len() && n < count {
            if st.parked[k].1 == addr {
                let (p, _) = st.parked.remove(k);
                st.woken.push(p);
                n += 1;
            } else {
                k += 1;
            }
        }
        n
    });
    if n > 0 {
        sched::progressed();
    }
}

/// `wait`: the 32-bit word at `p` against `expected`; the order is a load's, which the
/// cooperative scheduler runs sequentially consistent.
///
/// # Safety
/// `p` is a live, 4-aligned word.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_futex_wait(
    p: *const u32,
    expected: u64,
    _order: u64,
    in_task: u64,
) -> u64 {
    // SAFETY: the caller's contract.
    let word = unsafe { &*p.cast::<std::sync::atomic::AtomicU32>() };
    let equal = word.load(std::sync::atomic::Ordering::SeqCst) == expected as u32;
    wait(p as usize, equal, in_task != 0)
}

/// `notify_one` (`count` 1) and `notify_all` (`count` `u64::MAX`) on the word at `p`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_futex_notify(p: *const u32, count: u64) {
    notify(p as usize, count as usize);
}
