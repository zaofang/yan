//! `Atomic.wait` and `notify` (RFC 0089 D3). A task of a `par concurrent` parks on the word's
//! address in the cooperative scheduler: `notify` releases the oldest waiters of an address
//! first, and the released task's retried `wait` returns. Every task parked with no task to
//! release it is a global deadlock (R396). A pool worker (RFC 0086) has no scheduler, so its
//! `wait` blocks the thread in the OS: Linux `futex`, macOS `os_sync_wait_on_address`, Windows
//! `WaitOnAddress`. One table serves both, so a `notify` from any thread reaches both kinds.

use crate::actor::TIMEOUT;
use crate::chan::{BLOCKED, CANCELLED};
use crate::sched;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

/// The park key of an address: apart from channels and actors (R959).
const KEY: usize = 1 << 62;

/// A cooperative task: its thread and path (R398).
type Task = (ThreadId, Vec<usize>);

#[derive(Default)]
struct State {
    /// Tasks parked in `wait`, oldest first, with the address.
    parked: Vec<(Task, usize)>,
    /// Tasks `notify` released that have not yet resumed.
    woken: Vec<Task>,
}

static STATE: Mutex<State> = Mutex::new(State {
    parked: Vec::new(),
    woken: Vec::new(),
});

/// Threads blocked in the OS, so a `notify` with none skips the system call.
static OS_WAITERS: AtomicUsize = AtomicUsize::new(0);

/// Counts releases of tasks on another thread, which that thread's scheduler cannot see.
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// Releases of tasks by other threads so far, added to the scheduler's progress count.
pub(crate) fn epoch() -> u64 {
    EPOCH.load(SeqCst)
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    f(&mut STATE.lock().expect("not poisoned"))
}

fn me() -> Task {
    (thread::current().id(), sched::path())
}

/// `wait` on `addr`, whose word `equal`s the expected value now: 0 when it returns, or
/// [`BLOCKED`] when it parked the task (run it again on resume), [`CANCELLED`], or the status of
/// a wait that cannot park (an idle actor's [`crate::actor::WORK`], or a deadlock).
pub fn wait(addr: usize, equal: bool, in_task: bool) -> u64 {
    let me = me();
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

/// `wait` on a pool worker: block the thread while the word at `p` equals `expected`, until a
/// `notify` or `deadline`. 0 when it returns, or [`TIMEOUT`].
pub fn wait_os(p: *const u32, expected: u32, deadline: Option<Instant>) -> u64 {
    OS_WAITERS.fetch_add(1, SeqCst);
    let status = loop {
        let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if left == Some(Duration::ZERO) {
            break TIMEOUT;
        }
        match os::wait(p, expected, left) {
            os::Status::Woken => break 0,
            os::Status::TimedOut => break TIMEOUT,
            os::Status::Interrupted => {}
        }
    };
    OS_WAITERS.fetch_sub(1, SeqCst);
    status
}

/// Release up to `count` waiters on `addr`: parked tasks, oldest first, then threads in the OS.
pub fn notify(addr: usize, count: usize) {
    let here = thread::current().id();
    let (n, foreign) = with(|st| {
        let (mut n, mut foreign) = (0, false);
        let mut k = 0;
        while k < st.parked.len() && n < count {
            if st.parked[k].1 == addr {
                let (p, _) = st.parked.remove(k);
                foreign |= p.0 != here;
                st.woken.push(p);
                n += 1;
            } else {
                k += 1;
            }
        }
        (n, foreign)
    });
    if n > 0 {
        sched::progressed();
    }
    if foreign {
        EPOCH.fetch_add(1, SeqCst);
    }
    if count > n && OS_WAITERS.load(SeqCst) > 0 {
        os::wake(addr as *const u32, count - n);
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
    let equal = word.load(SeqCst) == expected as u32;
    if equal && crate::pool::on_worker() {
        return wait_os(p, expected as u32, None);
    }
    wait(p as usize, equal, in_task != 0)
}

/// `notify_one` (`count` 1) and `notify_all` (`count` `u64::MAX`) on the word at `p`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_futex_notify(p: *const u32, count: u64) {
    notify(p as usize, count as usize);
}

/// The OS primitives: block while a 32-bit word equals a value, and wake blocked threads.
mod os {
    use std::time::Duration;

    pub enum Status {
        /// Woken, or the word already differed.
        Woken,
        TimedOut,
        Interrupted,
    }

    #[cfg(target_os = "linux")]
    pub fn wait(p: *const u32, expected: u32, timeout: Option<Duration>) -> Status {
        let ts = timeout.map(|t| libc::timespec {
            tv_sec: t.as_secs() as libc::time_t,
            tv_nsec: t.subsec_nanos() as _,
        });
        let ts = ts.as_ref().map_or(std::ptr::null(), std::ptr::from_ref);
        // SAFETY: `p` is a live word (the caller's contract) and `ts` is null or live.
        let r = unsafe {
            libc::syscall(
                libc::SYS_futex,
                p,
                libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
                expected,
                ts,
            )
        };
        match r {
            0 => Status::Woken,
            _ => match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ETIMEDOUT) => Status::TimedOut,
                Some(libc::EINTR) => Status::Interrupted,
                _ => Status::Woken,
            },
        }
    }

    #[cfg(target_os = "linux")]
    pub fn wake(p: *const u32, count: usize) {
        let n = count.min(i32::MAX as usize) as i32;
        // SAFETY: waking an address needs no live word, only a valid syscall.
        unsafe {
            libc::syscall(
                libc::SYS_futex,
                p,
                libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
                n,
            );
        }
    }

    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        fn os_sync_wait_on_address(
            addr: *mut libc::c_void,
            value: u64,
            size: usize,
            flags: u32,
        ) -> libc::c_int;
        fn os_sync_wait_on_address_with_timeout(
            addr: *mut libc::c_void,
            value: u64,
            size: usize,
            flags: u32,
            clockid: u32,
            timeout_ns: u64,
        ) -> libc::c_int;
        fn os_sync_wake_by_address_any(
            addr: *mut libc::c_void,
            size: usize,
            flags: u32,
        ) -> libc::c_int;
        fn os_sync_wake_by_address_all(
            addr: *mut libc::c_void,
            size: usize,
            flags: u32,
        ) -> libc::c_int;
    }

    /// `OS_CLOCK_MACH_ABSOLUTE_TIME`.
    #[cfg(target_os = "macos")]
    const CLOCK: u32 = 32;

    #[cfg(target_os = "macos")]
    pub fn wait(p: *const u32, expected: u32, timeout: Option<Duration>) -> Status {
        let (a, v) = (p.cast_mut().cast(), u64::from(expected));
        // SAFETY: `p` is a live 4-byte word (the caller's contract).
        let r = unsafe {
            match timeout {
                Some(t) => os_sync_wait_on_address_with_timeout(
                    a,
                    v,
                    4,
                    0,
                    CLOCK,
                    t.as_nanos().min(u128::from(u64::MAX)) as u64,
                ),
                None => os_sync_wait_on_address(a, v, 4, 0),
            }
        };
        if r >= 0 {
            return Status::Woken;
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ETIMEDOUT) => Status::TimedOut,
            Some(libc::EINTR) => Status::Interrupted,
            _ => Status::Woken,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn wake(p: *const u32, count: usize) {
        // SAFETY: waking an address needs no live word.
        unsafe {
            if count == 1 {
                os_sync_wake_by_address_any(p.cast_mut().cast(), 4, 0);
            } else {
                os_sync_wake_by_address_all(p.cast_mut().cast(), 4, 0);
            }
        }
    }

    #[cfg(windows)]
    #[link(name = "synchronization")]
    unsafe extern "system" {
        fn WaitOnAddress(
            addr: *const core::ffi::c_void,
            compare: *const core::ffi::c_void,
            size: usize,
            millis: u32,
        ) -> i32;
        fn WakeByAddressSingle(addr: *const core::ffi::c_void);
        fn WakeByAddressAll(addr: *const core::ffi::c_void);
    }

    #[cfg(windows)]
    pub fn wait(p: *const u32, expected: u32, timeout: Option<Duration>) -> Status {
        let ms = timeout.map_or(u32::MAX, |t| {
            t.as_millis().min(u128::from(u32::MAX - 1)) as u32
        });
        // SAFETY: `p` is a live 4-byte word (the caller's contract) and `expected` outlives the call.
        let ok = unsafe { WaitOnAddress(p.cast(), (&raw const expected).cast(), 4, ms) };
        // ERROR_TIMEOUT
        if ok == 0 && std::io::Error::last_os_error().raw_os_error() == Some(1460) {
            Status::TimedOut
        } else {
            Status::Woken
        }
    }

    #[cfg(windows)]
    pub fn wake(p: *const u32, count: usize) {
        // SAFETY: waking an address needs no live word.
        unsafe {
            if count == 1 {
                WakeByAddressSingle(p.cast());
            } else {
                WakeByAddressAll(p.cast());
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub fn wait(_: *const u32, _: u32, _: Option<Duration>) -> Status {
        panic!("`Atomic.wait` on a pool worker is not supported on this target")
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub fn wake(_: *const u32, _: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn notify_wakes_an_os_waiter() {
        let w = Arc::new(AtomicU32::new(0));
        let w2 = Arc::clone(&w);
        let t = thread::spawn(move || {
            let p = w2.as_ptr().cast_const();
            while w2.load(SeqCst) == 0 {
                assert_eq!(wait_os(p, 0, None), 0);
            }
        });
        thread::sleep(Duration::from_millis(50));
        w.store(1, SeqCst);
        while !t.is_finished() {
            notify(w.as_ptr() as usize, 1);
            thread::sleep(Duration::from_millis(5));
        }
        t.join().unwrap();
    }

    #[test]
    fn wait_os_times_out() {
        let w = AtomicU32::new(0);
        let end = Instant::now() + Duration::from_millis(30);
        assert_eq!(wait_os(w.as_ptr(), 0, Some(end)), TIMEOUT);
        assert!(Instant::now() >= end);
    }

    #[test]
    fn wait_os_returns_when_the_word_differs() {
        let w = AtomicU32::new(1);
        assert_eq!(wait_os(w.as_ptr(), 0, None), 0);
    }
}
