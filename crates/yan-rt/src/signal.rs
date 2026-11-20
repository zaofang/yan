//! SIGINT and SIGTERM drain the root scope (9.4): the first one starts the root's drain with the
//! default grace, a second exits at once with status 128 plus the signal number. The handler only
//! records the signal and wakes the sleepers; the drain instant is taken on the bound `time`
//! handler's monotonic clock by the first task to look. A process still alive when the drain
//! grace and the cancel grace have both passed in host time aborts as a `shutdown` panic does.

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering::SeqCst};

/// The grace of a drain started by a signal, in nanoseconds.
pub const DRAIN_GRACE: i128 = 30_000_000_000;

/// The first SIGINT or SIGTERM received, else 0.
static PENDING: AtomicI32 = AtomicI32::new(0);
/// The read and write ends of the pipe a signal wakes sleepers through; -1 until [`install`].
static WAKE: [AtomicI32; 2] = [AtomicI32::new(-1), AtomicI32::new(-1)];
/// The monotonic reading the root's drain grace ends at, once a task has seen the signal.
static AT: Mutex<Option<i128>> = Mutex::new(None);

/// Note a pending signal: the root's drain starts at monotonic reading `now()`, read once, by
/// the first task to look after the signal. Tasks call it before they read the scopes.
pub fn observe(now: impl FnOnce() -> i128) {
    if PENDING.load(SeqCst) != 0 {
        AT.lock()
            .expect("not poisoned")
            .get_or_insert_with(|| now() + DRAIN_GRACE);
    }
}

/// The reading the grace of a signal's drain of the root ends at, once [`observe`] saw one.
pub fn root_cancel_at() -> Option<i128> {
    if PENDING.load(SeqCst) == 0 {
        return None;
    }
    *AT.lock().expect("not poisoned")
}

/// Sleep `ns` of host time, or until the first signal arrives.
pub fn sleep(ns: u64) {
    let d = std::time::Duration::from_nanos(ns);
    #[cfg(unix)]
    if PENDING.load(SeqCst) == 0 && WAKE[0].load(SeqCst) >= 0 {
        let end = std::time::Instant::now() + d;
        let mut fd = libc::pollfd {
            fd: WAKE[0].load(SeqCst),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            let left = end.saturating_duration_since(std::time::Instant::now());
            let Ok(ms) = i32::try_from(left.as_millis()) else {
                continue;
            };
            if ms == 0 {
                // poll counts milliseconds; the rest of the wait is shorter than one.
                return std::thread::sleep(left);
            }
            // SAFETY: one valid pollfd.
            let n = unsafe { libc::poll(&raw mut fd, 1, ms) };
            if n > 0 || PENDING.load(SeqCst) != 0 {
                return;
            }
        }
    }
    std::thread::sleep(d);
}

#[cfg(unix)]
extern "C" fn on_signal(signo: libc::c_int) {
    const LATE: &[u8] = b"panic(shutdown): a task outlived the cancel grace of a drained scope\n";
    // SAFETY: write, alarm and _exit are async-signal-safe.
    unsafe {
        if signo == libc::SIGALRM {
            libc::write(2, LATE.as_ptr().cast(), LATE.len());
            libc::_exit(134);
        }
        if PENDING.compare_exchange(0, signo, SeqCst, SeqCst).is_err() {
            libc::_exit(128 + signo);
        }
        libc::alarm(((DRAIN_GRACE + crate::scope::CANCEL_GRACE) / 1_000_000_000) as u32);
        libc::write(WAKE[1].load(SeqCst), [0u8].as_ptr().cast(), 1);
    }
}

/// Let SIGINT and SIGTERM drain the root scope of the program this process runs. Std calls it
/// before `main` on hosted platforms; a second call does nothing.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_signals() {
    #[cfg(unix)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let mut fds = [-1; 2];
            // SAFETY: `fds` holds two descriptors; the handler is async-signal-safe.
            unsafe {
                if libc::pipe(fds.as_mut_ptr()) != 0 {
                    return;
                }
                for fd in fds {
                    libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                    libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK);
                }
                WAKE[0].store(fds[0], SeqCst);
                WAKE[1].store(fds[1], SeqCst);
                let mut act: libc::sigaction = std::mem::zeroed();
                act.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
                libc::sigemptyset(&raw mut act.sa_mask);
                for s in [libc::SIGINT, libc::SIGTERM, libc::SIGALRM] {
                    libc::sigaction(s, &raw const act, std::ptr::null_mut());
                }
            }
        });
    }
}
