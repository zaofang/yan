//! The worker pool of a plain `par` in native code (RFC 0086). A function whose `par` arms are
//! pool-safe (`yan-mir/src/pool.rs`) tests `yan_rt_pool_begin` and, when it says yes, hands the
//! arms to `yan_rt_pool_run`, which runs each on a worker and waits for all of them. Arms write
//! disjoint places (6.5), so the results do not depend on which worker ran what. A panic in an
//! arm is recorded and the lowest arm's panic ends the program once every arm has settled, as
//! it would if the arms ran in source order.
//!
//! The worker count is `YAN_THREADS`, one per core when unset; 1 runs every `par` inline.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// A worker's stack, as large as the machine thread's, for deep recursion in an arm.
const STACK: usize = 256 << 20;

thread_local! {
    /// The arm this thread is running, when it is a pool worker.
    static ARM: Cell<Option<usize>> = const { Cell::new(None) };
    /// What the running arm printed, held back so the join can emit it in arm order.
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Hold `bytes` for the join when this thread runs an arm; false when it should write them.
pub(crate) fn capture(bytes: &[u8]) -> bool {
    if ARM.with(Cell::get).is_none() {
        return false;
    }
    OUT.with_borrow_mut(|o| o.extend_from_slice(bytes));
    true
}

/// The worker count: `YAN_THREADS` when it is a number, else one per core.
pub fn threads() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("YAN_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map_or_else(
                || std::thread::available_parallelism().map_or(1, |n| n.get()),
                |n| n.max(1),
            )
    })
}

struct Region {
    n: usize,
    frame: usize,
    next: AtomicUsize,
    state: Mutex<Settled>,
    /// Per arm, what it printed.
    out: Mutex<Vec<Vec<u8>>>,
    done: Condvar,
}

#[derive(Default)]
struct Settled {
    count: usize,
    /// The lowest arm that panicked, with its report and exit status.
    failed: Option<(usize, String, i32)>,
}

type Arms = Arc<Region>;

thread_local! {
    /// The region this worker's arm belongs to.
    static CURRENT: RefCell<Option<Arms>> = const { RefCell::new(None) };
}

/// 1 when a plain `par` of `n` arms runs on the pool: the pool is on, there is more than one
/// arm, and this is not a worker (arms of a nested `par` run inline) nor a `pct` scheduler.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_pool_begin(n: u64) -> u64 {
    u64::from(n > 1 && threads() > 1 && ARM.with(Cell::get).is_none() && !crate::sched::pct_bound())
}

fn work(r: &Arms) {
    loop {
        let k = r.next.fetch_add(1, SeqCst);
        if k >= r.n {
            return;
        }
        ARM.with(|a| a.set(Some(k)));
        CURRENT.with(|c| *c.borrow_mut() = Some(Arc::clone(r)));
        // SAFETY: words `0..n` of the frame are the arms' code addresses, and the frame
        // outlives the arm because the forking thread waits for every arm to settle.
        unsafe {
            let code = *(r.frame as *const usize).add(k);
            let code: extern "C" fn(*mut u64) -> i64 = std::mem::transmute(code);
            code(r.frame as *mut u64);
        }
        ARM.with(|a| a.set(None));
        keep_output(r, k);
        settle(r, None);
    }
}

fn keep_output(r: &Region, k: usize) {
    let bytes = OUT.with_borrow_mut(std::mem::take);
    r.out.lock().expect("not poisoned")[k] = bytes;
}

fn settle(r: &Region, failed: Option<(usize, String, i32)>) {
    let mut s = r.state.lock().expect("not poisoned");
    s.count += 1;
    if let Some(f) = failed
        && s.failed.as_ref().is_none_or(|g| f.0 < g.0)
    {
        s.failed = Some(f);
    }
    r.done.notify_all();
}

fn spawn(r: &Arms) {
    let r = Arc::clone(r);
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || work(&r))
        .expect("a worker starts");
}

/// The running arm panicked with `report` on stderr and exit status `code`. Inside a pool arm
/// the panic is kept for the join, a replacement worker takes over this one's share, and this
/// thread parks for good; otherwise it returns.
pub(crate) fn arm_failed(report: String, code: i32) {
    let Some(k) = ARM.with(Cell::get) else {
        return;
    };
    let r = CURRENT
        .with(|c| c.borrow().clone())
        .expect("a worker has its region");
    keep_output(&r, k);
    settle(&r, Some((k, report, code)));
    spawn(&r);
    loop {
        std::thread::park();
    }
}

/// Run the `n` arms in `frame` on `min(threads, n)` workers and wait for them. If any arm
/// panicked, end the program as the lowest panicking arm does.
///
/// # Safety
/// Words `0..n` of `frame` are the code addresses of arms `(frame) -> i64` that are pool-safe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_pool_run(frame: *mut u64, n: u64) {
    let n = n as usize;
    let r = Arc::new(Region {
        n,
        frame: frame as usize,
        next: AtomicUsize::new(0),
        state: Mutex::default(),
        out: Mutex::new(vec![Vec::new(); n]),
        done: Condvar::new(),
    });
    for _ in 0..threads().min(n) {
        spawn(&r);
    }
    let mut s = r.state.lock().expect("not poisoned");
    while s.count < n {
        s = r.done.wait(s).expect("not poisoned");
    }
    // Output as the arms would have written it in source order: all of it, or up to and
    // including the lowest failing arm.
    let upto = s.failed.as_ref().map_or(n, |f| f.0 + 1);
    {
        use std::io::Write;
        let out = r.out.lock().expect("not poisoned");
        let mut w = std::io::stdout().lock();
        let wrote = out[..upto].iter().try_for_each(|b| w.write_all(b));
        if wrote.and_then(|()| w.flush()).is_err() {
            std::process::exit(1);
        }
    }
    if let Some((_, report, code)) = s.failed.take() {
        eprintln!("{report}");
        std::process::exit(code);
    }
}
