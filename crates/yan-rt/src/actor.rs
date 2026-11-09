//! `std.actor` for native code (RFC 0056 D8), a port of the interpreter's `sync.rs` and
//! `supervise.rs`. An actor holds its state as slots and a mailbox of message closures. Native
//! code runs closures, never this module: an operation that needs one run returns [`WORK`] with
//! the closure in [`yan_rt_actor_arg`], and the caller runs it and retries. A handle is the
//! actor's index and owns nothing (9.6).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Instant;

use crate::sched;

/// Retry after running the closure in the work slots.
pub const WORK: u64 = 3;
/// A call fails `Stopped`.
pub const STOPPED: u64 = 4;
/// A call fails `Deadlock`.
pub const DEADLOCK: u64 = 5;

#[derive(Clone, Copy, PartialEq)]
enum Restart {
    Permanent,
    Transient,
    Temporary,
}

#[derive(Clone, Copy, PartialEq)]
enum Strategy {
    OneForOne,
    OneForAll,
    RestForOne,
}

struct Actor {
    words: usize,
    /// `None` while a handler holds it, or once stopped.
    state: Option<Vec<u64>>,
    capacity: usize,
    /// Message closures: code and captures block.
    mail: VecDeque<[u64; 2]>,
    /// A queued message is running, so the actor takes no other from its mailbox.
    running: bool,
    /// The task path of the handler holding the state, while one does.
    holder: Option<Vec<usize>>,
    stopped: bool,
    sup: usize,
    restart: Restart,
    /// The restart thunk, then the start closure's code and captures block.
    start: [u64; 3],
}

impl Actor {
    /// Its queue is dropped, and its state with it (R813).
    fn stop(&mut self) {
        self.stopped = true;
        self.state = None;
        self.holder = None;
        for [_, env] in self.mail.drain(..) {
            // SAFETY: a message's captures block, from `yan_rt_alloc`, owned by the mailbox.
            unsafe { crate::abi::yan_rt_free_env(env as *mut u64) };
        }
    }
}

struct Sup {
    strategy: Strategy,
    intensity: usize,
    period: i128,
    /// Its children, by actor, in start order.
    children: Vec<usize>,
    /// When each restart within the last `period` happened.
    restarts: VecDeque<i128>,
}

struct State {
    actors: Vec<Actor>,
    /// Every supervisor, the root first (R957), and the open `with supervisor` scopes.
    sups: Vec<Sup>,
    open: Vec<usize>,
    /// Tasks parked on a busy actor, by task path, and the actor each waits for (RFC 0056 D6).
    waits: Vec<(Vec<usize>, usize)>,
    /// Callers that close a wait cycle as its youngest, failing `Deadlock` when they resume.
    doomed: Vec<Vec<usize>>,
    /// Children a crash restarts, in order.
    restarts: VecDeque<usize>,
    /// The closure the last [`WORK`] asks for.
    work: [u64; 4],
    start: Instant,
}

impl Default for State {
    fn default() -> Self {
        State {
            actors: Vec::new(),
            sups: vec![Sup {
                strategy: Strategy::OneForOne,
                intensity: 1,
                period: 5_000_000_000,
                children: Vec::new(),
                restarts: VecDeque::new(),
            }],
            open: Vec::new(),
            waits: Vec::new(),
            doomed: Vec::new(),
            restarts: VecDeque::new(),
            work: [0; 4],
            start: Instant::now(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn panic_with(kind: &str, msg: &str) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    eprintln!("panic({kind}): {msg}");
    std::process::exit(101)
}

fn unsupported(what: &str) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    eprintln!("yan: not supported yet: {what} in native code");
    std::process::exit(2)
}

/// The park key of a wait on actor `id`, apart from channels.
fn key(id: usize) -> (bool, usize) {
    (false, usize::MAX - 1 - id)
}

/// Pop actor `id`'s oldest message into the work slots if the actor is idle.
fn queued(st: &mut State, id: usize) -> bool {
    let a = &mut st.actors[id];
    if a.running || a.state.is_none() || a.stopped {
        return false;
    }
    let Some([code, env]) = a.mail.pop_front() else {
        return false;
    };
    a.running = true;
    st.work = [code, env, id as u64, 0];
    true
}

/// Whether some idle actor has a queued message (RFC 0056 D4).
pub(crate) fn idle_ready() -> bool {
    with(|st| {
        st.actors
            .iter()
            .any(|a| !a.running && a.state.is_some() && !a.stopped && !a.mail.is_empty())
    })
}

/// One queued message of the first idle actor that has one, into the work slots.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_idle() -> u64 {
    with(|st| (0..st.actors.len()).any(|id| queued(st, id))).into()
}

/// Work slot `i` of the last [`WORK`]: a message's code, captures and actor; or a restart's
/// thunk, start code, start captures and actor.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_arg(i: u64) -> u64 {
    with(|st| st.work[i as usize])
}

/// The message in the work slots ran.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_ran() {
    with(|st| {
        let id = st.work[2] as usize;
        st.actors[id].running = false;
    });
    sched::progressed();
}

/// A new actor with the `words` slots at `state`, a child of the innermost supervisor: `restart`
/// 0 permanent, 1 transient, 2 temporary, restarted by calling `thunk(code, env, actor)`.
///
/// # Safety
/// `state` holds `words` slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_spawn(
    state: *const u64,
    words: u64,
    cap: i64,
    restart: u64,
    thunk: u64,
    code: u64,
    env: u64,
) -> u64 {
    if cap < 1 {
        panic_with(
            "precondition `capacity >= 1` of `spawn` failed",
            crate::abi::panic_message(""),
        );
    }
    // SAFETY: the caller's contract.
    let v = unsafe { std::slice::from_raw_parts(state, words as usize) }.to_vec();
    with(|st| {
        let id = st.actors.len();
        let sup = st.open.last().copied().unwrap_or(0);
        st.sups[sup].children.push(id);
        st.actors.push(Actor {
            words: words as usize,
            state: Some(v),
            capacity: cap as usize,
            mail: VecDeque::new(),
            running: false,
            holder: None,
            stopped: false,
            sup,
            restart: match restart {
                0 => Restart::Permanent,
                1 => Restart::Transient,
                _ => Restart::Temporary,
            },
            start: [thunk, code, env],
        });
        id as u64
    })
}

fn unwait(st: &mut State, me: &[usize]) {
    st.waits.retain(|(p, _)| p != me);
}

/// The youngest caller of a wait cycle that `me` closes by waiting on actor `id`, if it closes
/// one (RFC 0056 D6).
fn cycle(st: &State, me: &[usize], id: usize) -> Option<Vec<usize>> {
    let mut callers = vec![me.to_vec()];
    let mut at = id;
    for _ in 0..=st.actors.len() {
        let h = st.actors[at].holder.as_ref()?;
        if callers.iter().any(|c| c.starts_with(h)) {
            return callers.into_iter().max();
        }
        at = st.waits.iter().find(|(p, _)| p == h)?.1;
        callers.push(h.clone());
    }
    None
}

/// Take actor `id`'s state for a handler call (R435): 0 with its slots in `out`, or
/// [`crate::chan::BLOCKED`], [`WORK`], [`STOPPED`] or [`DEADLOCK`]. A call is a barrier: the
/// actor first runs every message queued before it (R813).
///
/// # Safety
/// `out` has room for the state's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_take(id: u64, out: *mut u64, in_task: u64) -> u64 {
    let id = id as usize;
    let me = sched::path();
    let r = with(|st| {
        if let Some(k) = st.doomed.iter().position(|p| *p == me) {
            st.doomed.swap_remove(k);
            unwait(st, &me);
            return Some(DEADLOCK);
        }
        if queued(st, id) {
            return Some(WORK);
        }
        if st.actors[id].stopped {
            unwait(st, &me);
            return Some(STOPPED);
        }
        if let Some(s) = st.actors[id].state.take() {
            // SAFETY: the caller's contract.
            unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), out, s.len()) };
            st.actors[id].holder = Some(me.clone());
            unwait(st, &me);
            return Some(0);
        }
        match cycle(st, &me, id) {
            Some(y) if y == me => {
                unwait(st, &me);
                return Some(DEADLOCK);
            }
            Some(y) if !st.doomed.contains(&y) => {
                st.doomed.push(y);
                sched::progressed();
            }
            _ => {}
        }
        None
    });
    if let Some(r) = r {
        return r;
    }
    if sched::park(key(id), in_task != 0) {
        with(|st| {
            if !st.waits.iter().any(|(p, _)| *p == me) {
                st.waits.push((me, id));
            }
        });
        return 1;
    }
    with(|st| unwait(st, &me));
    if sched::forced() || sched::idle() {
        crate::chan::deadlock();
    }
    unsupported("a handler call that waits on a busy actor")
}

/// The state's `words` slots at `state` go back to actor `id` after a handler ran.
///
/// # Safety
/// `state` holds the actor's state slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_put(id: u64, state: *const u64) {
    with(|st| {
        let a = &mut st.actors[id as usize];
        // SAFETY: the caller's contract.
        let v = unsafe { std::slice::from_raw_parts(state, a.words) }.to_vec();
        a.state = Some(v);
        a.holder = None;
    });
    sched::progressed();
}

/// Enqueue the message closure `code`, `env` on actor `id` (R813): 0, 1 when parked on a full
/// mailbox, or [`WORK`] to run the actor's oldest message first.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_send(id: u64, code: u64, env: u64, in_task: u64) -> u64 {
    let id = id as usize;
    let r = with(|st| {
        let a = &st.actors[id];
        if a.stopped {
            return None;
        }
        if a.mail.len() >= a.capacity {
            return Some(if queued(st, id) { WORK } else { 1 });
        }
        st.actors[id].mail.push_back([code, env]);
        Some(0)
    });
    match r {
        None => unsupported("a send to a stopped actor, before sends have a fail type"),
        Some(1) => {
            if sched::park(key(id), in_task != 0) {
                return 1;
            }
            if sched::forced() || sched::idle() {
                crate::chan::deadlock();
            }
            unsupported("a send that waits on a busy actor")
        }
        Some(0) => {
            sched::progressed();
            0
        }
        Some(r) => r,
    }
}

fn now(st: &State) -> i128 {
    st.start.elapsed().as_nanos() as i128
}

/// A handler of actor `id` failed, which stops it (RFC 0060 D3). Its supervisor queues the
/// restarts its strategy asks for, which [`yan_rt_actor_restart_next`] hands out, unless that
/// exceeds the restart intensity: then every child stops and the run panics `supervisor`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_crash(id: u64) {
    let id = id as usize;
    let over = with(|st| {
        st.actors[id].stop();
        let (s, restart) = (st.actors[id].sup, st.actors[id].restart);
        if restart == Restart::Temporary {
            return None;
        }
        let t = now(st);
        let sup = &mut st.sups[s];
        while sup.restarts.front().is_some_and(|r| t - r >= sup.period) {
            sup.restarts.pop_front();
        }
        if sup.restarts.len() >= sup.intensity {
            let (n, p) = (sup.intensity, sup.period);
            for c in sup.children.clone().into_iter().rev() {
                st.actors[c].stop();
            }
            return Some((n, p));
        }
        sup.restarts.push_back(t);
        let at = sup.children.iter().position(|c| *c == id).expect("a child");
        let group = match sup.strategy {
            Strategy::OneForOne => vec![id],
            Strategy::OneForAll => sup.children.clone(),
            Strategy::RestForOne => sup.children[at..].to_vec(),
        };
        for &c in group.iter().rev() {
            st.actors[c].stop();
        }
        st.restarts.extend(
            group
                .into_iter()
                .filter(|c| st.actors[*c].restart != Restart::Temporary),
        );
        None
    });
    if let Some((n, p)) = over {
        panic_with(
            "supervisor",
            &format!("a supervisor exceeded its restart intensity of {n} per {p} ns"),
        );
    }
}

/// The next queued restart into the work slots: 1, or 0 when none is left.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_restart_next() -> u64 {
    with(|st| match st.restarts.pop_front() {
        Some(c) => {
            let [thunk, code, env] = st.actors[c].start;
            st.work = [thunk, code, env, c as u64];
            1
        }
        None => 0,
    })
}

/// A restart of actor `id` from the state slots at `state`, with an empty mailbox (R958).
///
/// # Safety
/// `state` holds the actor's state slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_restarted(id: u64, state: *const u64) {
    with(|st| {
        let a = &mut st.actors[id as usize];
        // SAFETY: the caller's contract.
        let v = unsafe { std::slice::from_raw_parts(state, a.words) }.to_vec();
        a.stopped = false;
        a.state = Some(v);
    });
}

/// `with supervisor(spec):` opens a scope with strategy `strategy` (0 one for one, 1 one for
/// all, 2 rest for one), `intensity` restarts per `period` ns. Its result is the lower of
/// `low` and the open scope count before it, for [`yan_rt_sup_end_to`] at the frame's exit.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_sup_begin(low: u64, strategy: u64, intensity: i64, period: i64) -> u64 {
    with(|st| {
        let depth = st.open.len() as u64;
        st.sups.push(Sup {
            strategy: match strategy {
                1 => Strategy::OneForAll,
                2 => Strategy::RestForOne,
                _ => Strategy::OneForOne,
            },
            intensity: intensity.max(0) as usize,
            period: i128::from(period),
            children: Vec::new(),
            restarts: VecDeque::new(),
        });
        st.open.push(st.sups.len() - 1);
        low.min(depth)
    })
}

/// The scopes past the first `depth` end: their children stop in reverse start order (9.6).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_sup_end_to(depth: u64) {
    with(|st| {
        while st.open.len() as u64 > depth {
            let s = st.open.pop().expect("an open scope");
            for c in st.sups[s].children.clone().into_iter().rev() {
                st.actors[c].stop();
            }
        }
    });
}

/// The innermost scope ends.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_sup_end() {
    let n = with(|st| st.open.len() as u64);
    yan_rt_sup_end_to(n.saturating_sub(1));
}
