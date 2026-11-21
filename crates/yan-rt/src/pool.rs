//! The worker pool of a plain `par` in native code (RFC 0086). A function whose `par` arms are
//! pool-safe (`yan-mir/src/pool.rs`) tests `yan_rt_pool_begin` and, when it says yes, hands the
//! arms to `yan_rt_pool_run`, which runs each on a worker and waits for all of them. Arms write
//! disjoint places (6.5), so the results do not depend on which worker ran what. An arm that
//! panics or leaves by `?`, `fail` or `return` is recorded; the lowest such arm decides the block
//! once every arm has settled, as it would if the arms ran in source order (RFC 0086 D8). Arms
//! above it that have not started are skipped; those that ran have their output and exits dropped.
//! A panicking arm parks its worker, so the heap blocks it still owns are freed first.
//!
//! The worker count is `YAN_THREADS`, one per core when unset; 1 runs every `par` inline.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// A worker's stack, as large as the machine thread's, for deep recursion in an arm.
const STACK: usize = 256 << 20;

thread_local! {
    /// The heap blocks the running arm took and has not freed, with how to free each.
    static OWNED: RefCell<HashMap<usize, unsafe fn(usize)>> = RefCell::new(HashMap::new());
    /// The arm this thread is running, when it is a pool worker.
    static ARM: Cell<Option<usize>> = const { Cell::new(None) };
    /// What the running arm printed, held back so the join can emit it in arm order.
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn track(p: usize, release: unsafe fn(usize)) {
    if ARM.with(Cell::get).is_some() {
        OWNED.with_borrow_mut(|o| o.insert(p, release));
    }
}

pub(crate) fn untrack(p: usize) {
    if ARM.with(Cell::get).is_some() {
        OWNED.with_borrow_mut(|o| o.remove(&p));
    }
}

/// This thread is a pool worker running an arm.
pub(crate) fn on_worker() -> bool {
    ARM.with(Cell::get).is_some()
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

/// How an arm left its block, other than by falling out of its end.
enum Left {
    /// A panic: its report on stderr and the exit status.
    Panic(String, i32),
    /// The code of the exit the arm took (`yan-mir/src/pool.rs`).
    Exit(i64),
}

#[derive(Default)]
struct Settled {
    count: usize,
    /// The lowest arm that left.
    failed: Option<(usize, Left)>,
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
        if r.state
            .lock()
            .expect("not poisoned")
            .failed
            .as_ref()
            .is_some_and(|f| f.0 < k)
        {
            settle(r, None);
            continue;
        }
        ARM.with(|a| a.set(Some(k)));
        OWNED.with_borrow_mut(HashMap::clear);
        CURRENT.with(|c| *c.borrow_mut() = Some(Arc::clone(r)));
        // SAFETY: words `0..n` of the frame are the arms' code addresses, and the frame
        // outlives the arm because the forking thread waits for every arm to settle.
        unsafe {
            let code = *(r.frame as *const usize).add(k);
            let code: extern "C" fn(*mut u64) -> i64 = std::mem::transmute(code);
            let exit = code(r.frame as *mut u64);
            ARM.with(|a| a.set(None));
            keep_output(r, k);
            settle(r, (exit != 0).then_some((k, Left::Exit(exit))));
        }
    }
}

fn keep_output(r: &Region, k: usize) {
    let bytes = OUT.with_borrow_mut(std::mem::take);
    r.out.lock().expect("not poisoned")[k] = bytes;
}

fn settle(r: &Region, failed: Option<(usize, Left)>) {
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
    // The arm's output is discarded and nothing resumes it, so what it still owns is freed here.
    for (p, release) in OWNED.with_borrow_mut(std::mem::take) {
        crate::heap::block_released(p, release);
    }
    settle(&r, Some((k, Left::Panic(report, code))));
    spawn(&r);
    loop {
        std::thread::park();
    }
}

/// Run the `n` arms in `frame` on `min(threads, n)` workers and wait for them. If the lowest arm
/// that left panicked, end the program as it does; if it left by an exit, return that exit's code.
/// Returns 0 when every arm fell out of its end.
///
/// # Safety
/// Words `0..n` of `frame` are the code addresses of arms `(frame) -> i64` that are pool-safe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_pool_run(frame: *mut u64, n: u64) -> i64 {
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
    match s.failed.take() {
        Some((_, Left::Panic(report, code))) => {
            eprintln!("{report}");
            std::process::exit(code);
        }
        Some((_, Left::Exit(code))) => code,
        None => 0,
    }
}
