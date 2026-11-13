//! `std.actor` for native code (RFC 0056 D8), a port of the interpreter's `sync.rs` and
//! `supervise.rs`. An actor holds its state as slots and a mailbox of message closures. Native
//! code runs closures, never this module: an operation that needs one run returns [`WORK`] with
//! the closure in [`yan_rt_actor_arg`], and the caller runs it and retries. A handle is the
//! actor's index and owns nothing (9.6).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::sched;

/// Retry after running the closure in the work slots.
pub const WORK: u64 = 3;
/// A call fails `Stopped`.
pub const STOPPED: u64 = 4;
/// A call fails `Deadlock`.
pub const DEADLOCK: u64 = 5;
/// A wait outlived its deadline: `Timeout`.
pub const TIMEOUT: u64 = 6;
/// A send to a full `Reject` mailbox fails `Full`.
pub const FULL: u64 = 7;
/// A message through `Remote` was lost: `Unreachable` (RFC 0063 D5).
pub const UNREACHABLE: u64 = 8;
/// A send through `Remote` carried an argument the codec rejects: `SchemaMismatch` (R408).
pub const SCHEMA_MISMATCH: u64 = 9;
/// A `Block` send in a cancelled task fails `Cancelled`.
pub use crate::chan::CANCELLED;

/// The deadline of a call or a `Block` send outside every deadline scope (9.6).
const DEFAULT_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq)]
enum Policy {
    Block,
    DropOldest,
    DropNew,
    Reject,
}

/// Why an actor stopped (R977); natively never `Normal` or `NodeDown`.
enum Reason {
    Shutdown,
    Crashed(String),
}

/// A queued message: its thunk, closure code, captures block and arguments block.
type Message = [u64; 4];

fn free(m: Message) {
    // SAFETY: a message's captures and arguments blocks, from `yan_rt_alloc`, owned by the
    // mailbox; its thunk, called with a null code, drops the arguments it holds.
    unsafe {
        let drop: extern "C" fn(u64, u64, u64) = std::mem::transmute(m[0] as usize);
        drop(0, m[2], m[3]);
        crate::abi::yan_rt_free_env(m[2] as *mut u64);
        crate::abi::yan_rt_free_env(m[3] as *mut u64);
    }
}

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
    policy: Policy,
    mail: VecDeque<Message>,
    /// A queued message is running, so the actor takes no other from its mailbox.
    running: bool,
    /// The task path of the handler holding the state, while one does.
    holder: Option<Vec<usize>>,
    /// Why it stopped, until it restarts.
    stopped: Option<Reason>,
    /// Starts so far, so a `@reentrant` handler that lent the state cannot retake a restart's.
    starts: u64,
    /// Why the last start stopped, for a `@reentrant` handler that lent it the state.
    ended: Option<Reason>,
    /// The task paths of the `@reentrant` handlers that lent the state, innermost last, each
    /// marked once its retake failed.
    lent: Vec<(Vec<usize>, bool)>,
    sup: usize,
    /// The node it runs on (RFC 0063 D1).
    home: usize,
    restart: Restart,
    /// The restart thunk, then the start closure's code and captures block.
    start: [u64; 3],
}

impl Actor {
    /// Its queue is dropped, and its state with it (R813).
    fn stop(&mut self, why: Reason) {
        self.stopped = Some(why);
        self.state = None;
        self.holder = None;
        self.mail.drain(..).for_each(free);
    }

    /// Stopped for good: the start closure goes too.
    fn end(&mut self) {
        self.stop(Reason::Shutdown);
        self.release_start();
    }

    fn release_start(&mut self) {
        let env = std::mem::take(&mut self.start[2]);
        // SAFETY: the captures block of the start closure, which only a restart reads.
        unsafe { crate::abi::yan_rt_free_env(env as *mut u64) };
    }
}

pub(crate) struct Sup {
    strategy: Strategy,
    intensity: usize,
    period: i128,
    /// Its children, by actor, in start order.
    children: Vec<usize>,
    /// When each restart within the last `period` happened.
    restarts: VecDeque<i128>,
    /// For the scope of `with cluster = sim(..)`: the cluster and the root supervisor of each
    /// of its nodes 1 to n-1 (RFC 0063 D1).
    pub(crate) nodes: Option<(usize, Vec<usize>)>,
}

impl Sup {
    /// A root supervisor: `one_for_one`, 1 restart per 5 s (R957).
    pub(crate) fn root() -> Self {
        Sup {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period: 5_000_000_000,
            children: Vec::new(),
            restarts: VecDeque::new(),
            nodes: None,
        }
    }
}

pub(crate) struct State {
    actors: Vec<Actor>,
    /// Every supervisor, the root first (R957), and the open `with supervisor` scopes.
    pub(crate) sups: Vec<Sup>,
    pub(crate) open: Vec<usize>,
    /// The node ids of each `sim` cluster built (RFC 0063 D3).
    pub(crate) sims: Vec<Vec<String>>,
    /// The nodes tasks entered, innermost last, by task path (D1).
    pub(crate) nodes: Vec<(Vec<usize>, usize)>,
    /// The node a `cluster.spawn` placed its start on, when not the caller's (D2).
    pub(crate) placed: Option<usize>,
    /// The faults of the last `sim` built and its messages, as `yan_rt::cluster` keeps them.
    pub(crate) faults: crate::cluster::Faults,
    /// Tasks parked on a busy actor, by task path, and the actor each waits for (RFC 0056 D6).
    waits: Vec<(Vec<usize>, usize)>,
    /// When each task parked on an actor began waiting, for its deadline.
    since: Vec<(Vec<usize>, Instant, Option<i128>)>,
    /// Callers that close a wait cycle as its youngest, failing `Deadlock` when they resume.
    doomed: Vec<Vec<usize>>,
    /// Children a crash restarts, in order.
    restarts: VecDeque<usize>,
    /// The closure the last [`WORK`] asks for.
    work: [u64; 5],
    start: Instant,
    /// `(actor, channel, node of the watcher)` of each monitor not yet delivered.
    pub(crate) monitors: Vec<(usize, usize, usize)>,
}

impl Default for State {
    fn default() -> Self {
        State {
            actors: Vec::new(),
            sups: vec![Sup::root()],
            open: Vec::new(),
            sims: Vec::new(),
            nodes: Vec::new(),
            placed: None,
            faults: Default::default(),
            waits: Vec::new(),
            since: Vec::new(),
            doomed: Vec::new(),
            restarts: VecDeque::new(),
            work: [0; 5],
            monitors: Vec::new(),
            start: Instant::now(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

pub(crate) fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

pub(crate) fn panic_with(kind: &str, msg: &str) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    eprintln!("panic({kind}): {msg}");
    std::process::exit(101)
}

pub(crate) fn unsupported(what: &str) -> ! {
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
    if a.running || a.state.is_none() {
        return false;
    }
    let Some([thunk, code, env, args]) = a.mail.pop_front() else {
        return false;
    };
    a.running = true;
    st.work = [thunk, code, env, args, id as u64];
    true
}

/// Whether some idle actor has a queued message (RFC 0056 D4).
pub(crate) fn idle_ready() -> bool {
    with(|st| {
        st.actors
            .iter()
            .any(|a| !a.running && a.state.is_some() && !a.mail.is_empty())
    })
}

/// One queued message of the first idle actor that has one, into the work slots.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_idle() -> u64 {
    with(|st| (0..st.actors.len()).any(|id| queued(st, id))).into()
}

/// Work slot `i` of the last [`WORK`]: a message's thunk, code, captures, arguments and actor;
/// or a restart's thunk, start code, start captures and actor. A thunk takes the three slots
/// after it.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_arg(i: u64) -> u64 {
    with(|st| st.work[i as usize])
}

/// The message in the work slots ran.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_ran() {
    with(|st| {
        let id = st.work[4] as usize;
        st.actors[id].running = false;
    });
    sched::progressed();
}

/// A new actor with the `words` slots at `state`, a child of the innermost supervisor, with
/// overflow `policy` 0 `Block`, 1 `DropOldest`, 2 `DropNew` or 3 `Reject` (R974): `restart` 0
/// permanent, 1 transient, 2 temporary, restarted by calling `thunk(code, env, actor)`.
///
/// # Safety
/// `state` holds `words` slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_spawn(
    state: *const u64,
    words: u64,
    cap: i64,
    policy: u64,
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
    let me = sched::path();
    with(|st| {
        let id = st.actors.len();
        let sup = adopter(st);
        let home = current(st, &me);
        st.sups[sup].children.push(id);
        st.actors.push(Actor {
            words: words as usize,
            state: Some(v),
            capacity: cap as usize,
            policy: match policy {
                1 => Policy::DropOldest,
                2 => Policy::DropNew,
                3 => Policy::Reject,
                _ => Policy::Block,
            },
            mail: VecDeque::new(),
            running: false,
            holder: None,
            stopped: None,
            starts: 0,
            ended: None,
            lent: Vec::new(),
            sup,
            home,
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
    st.since.retain(|(p, ..)| p != me);
}

/// Task `me` waits on actor `id`, keyed apart from channels (R959): 1 when it parked. Where it
/// cannot, nothing else runs, so it fails [`TIMEOUT`] 5 s after the wait began (9.6); native
/// code has no deadline scopes.
fn wait(id: usize, me: Vec<usize>, in_task: bool, what: &str) -> u64 {
    if sched::park(key(id), in_task) {
        with(|st| {
            if !st.since.iter().any(|(p, ..)| *p == me) {
                st.since
                    .push((me, Instant::now(), crate::civil::clocks_last()));
            }
        });
        return 1;
    }
    if !sched::forced() && !sched::idle() {
        unsupported(&format!("{what} that waits on a busy actor"));
    }
    let since = with(|st| {
        let s = st.since.iter().find(|(p, ..)| *p == me).map(|w| (w.1, w.2));
        unwait(st, &me);
        s
    });
    // Under `fixed_time` or `virtual` handlers the wait takes no host time: their clocks jump.
    if let Some(now) = since.and_then(|s| s.1).or_else(crate::civil::clocks_last) {
        crate::civil::clocks_advance_to(now + DEFAULT_DEADLINE.as_nanos() as i128);
        return TIMEOUT;
    }
    let end = since.map(|s| s.0).unwrap_or_else(Instant::now) + DEFAULT_DEADLINE;
    std::thread::sleep(end.saturating_duration_since(Instant::now()));
    TIMEOUT
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
/// [`crate::chan::BLOCKED`], [`WORK`], [`STOPPED`], [`DEADLOCK`], [`TIMEOUT`] or, for a call through
/// `Remote` whose request was lost, [`UNREACHABLE`]. A call is a barrier: the actor first runs
/// every message queued before it (R813).
///
/// # Safety
/// `out` has room for the state's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_take(id: u64, out: *mut u64, in_task: u64) -> u64 {
    let me = sched::path();
    if with(|st| st.faults.refused(&me)) {
        return UNREACHABLE;
    }
    // SAFETY: the caller's contract.
    let r = unsafe { take_state(id as usize, out, in_task != 0, true) };
    if ![0, WORK, crate::chan::BLOCKED].contains(&r) {
        with(|st| st.faults.forget(&me));
    }
    r
}

/// # Safety
/// `out` has room for the state's slots.
unsafe fn take_state(id: usize, out: *mut u64, in_task: bool, barrier: bool) -> u64 {
    let me = sched::path();
    let r = with(|st| {
        if let Some(k) = st.doomed.iter().position(|p| *p == me) {
            st.doomed.swap_remove(k);
            unwait(st, &me);
            return Some(DEADLOCK);
        }
        if barrier && queued(st, id) {
            return Some(WORK);
        }
        if st.actors[id].stopped.is_some() {
            unwait(st, &me);
            return Some(STOPPED);
        }
        if let Some(s) = st.actors[id].state.take() {
            // SAFETY: the caller's contract.
            unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), out, s.len()) };
            st.actors[id].holder = Some(me.clone());
            unwait(st, &me);
            // The handler runs on its actor's node (D1.3).
            let home = st.actors[id].home;
            st.nodes.push((me.clone(), home));
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
    let st = wait(id, me.clone(), in_task, "a handler call");
    if st == 1 {
        with(|s| {
            if !s.waits.iter().any(|(p, _)| *p == me) {
                s.waits.push((me, id));
            }
        });
    }
    st
}

/// A `@reentrant` handler lends the state slots at `state` back to actor `id` at a suspension
/// (9.6): the actor's start count, for [`yan_rt_actor_retake`].
///
/// # Safety
/// `state` holds the actor's state slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_lend(id: u64, state: *const u64) -> u64 {
    // SAFETY: the caller's contract.
    unsafe { yan_rt_actor_put(id, state) };
    with(|st| {
        let a = &mut st.actors[id as usize];
        a.lent.push((sched::path(), false));
        a.starts
    })
}

/// The `@reentrant` handler that lent actor `id`'s state while it had made `since` starts takes
/// it back into `out`, as [`yan_rt_actor_take`] without the barrier. It fails [`STOPPED`] when
/// the actor stopped or restarted meanwhile.
///
/// # Safety
/// `out` has room for the state's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_retake(
    id: u64,
    since: u64,
    out: *mut u64,
    in_task: u64,
) -> u64 {
    let id = id as usize;
    let restarted = with(|st| st.actors[id].starts != since && st.actors[id].stopped.is_none());
    let r = if restarted {
        STOPPED
    } else {
        // SAFETY: the caller's contract.
        unsafe { take_state(id, out, in_task != 0, false) }
    };
    if matches!(r, 0 | STOPPED | DEADLOCK | TIMEOUT) {
        let me = sched::path();
        with(|st| {
            let lent = &mut st.actors[id].lent;
            let k = lent
                .iter()
                .rposition(|(p, _)| *p == me)
                .expect("lent first");
            if r == 0 {
                lent.remove(k);
            } else {
                lent[k].1 = true;
            }
        });
    }
    r
}

/// A handler call ends: the state slots at `state` go back to actor `id` if this task holds it.
/// 1 when the handler's failure stops the actor (RFC 0060 D3): it held the state, or it is a
/// `@reentrant` handler that failed while it lent it; 0 when its retake failed.
///
/// # Safety
/// `state` holds the actor's state slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_give(id: u64, state: *const u64) -> u64 {
    let me = sched::path();
    let held = with(|st| st.actors[id as usize].holder.as_ref() == Some(&me));
    if held {
        // SAFETY: the caller's contract.
        unsafe { yan_rt_actor_put(id, state) };
        return 1;
    }
    with(|st| {
        leave(st, &me);
        let lent = &mut st.actors[id as usize].lent;
        match lent.iter().rposition(|(p, _)| *p == me) {
            Some(k) => u64::from(!lent.remove(k).1),
            None => 0,
        }
    })
}

/// The state's `words` slots at `state` go back to actor `id` after a handler ran.
///
/// # Safety
/// `state` holds the actor's state slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_put(id: u64, state: *const u64) {
    let me = sched::path();
    with(|st| {
        leave(st, &me);
        let a = &mut st.actors[id as usize];
        // SAFETY: the caller's contract.
        let v = unsafe { std::slice::from_raw_parts(state, a.words) }.to_vec();
        a.state = Some(v);
        a.holder = None;
    });
    sched::progressed();
}

/// Enqueue on actor `id` the message `thunk(code, env, args)` (R813): 0, 1 when parked on a full
/// mailbox, [`WORK`] to run the actor's oldest message first, or why it is undelivered:
/// [`UNREACHABLE`], [`STOPPED`], [`FULL`], [`CANCELLED`] or [`TIMEOUT`], leaving the message to
/// the caller (R976). A full mailbox does what the actor's policy says (R974).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_send(
    id: u64,
    thunk: u64,
    code: u64,
    env: u64,
    args: u64,
    in_task: u64,
) -> u64 {
    let id = id as usize;
    let m = [thunk, code, env, args];
    let me = sched::path();
    let r = with(|st| {
        if let Some(s) = st.faults.lost_send(&me) {
            return Some(s);
        }
        let a = &mut st.actors[id];
        if a.stopped.is_some() {
            return Some(STOPPED);
        }
        if a.mail.len() >= a.capacity {
            match a.policy {
                Policy::Reject => return Some(FULL),
                Policy::DropNew => {
                    free(m);
                    return Some(0);
                }
                Policy::DropOldest => a.mail.pop_front().into_iter().for_each(free),
                // A full mailbox parks a `Block` sender, so the actor runs its oldest message
                // first; waiting is a cancellation point (9.6).
                Policy::Block => {
                    if sched::cancelled() {
                        return Some(CANCELLED);
                    }
                    return Some(if queued(st, id) { WORK } else { WAIT });
                }
            }
        }
        st.actors[id].mail.push_back(m);
        None
    });
    match r {
        Some(WAIT) => wait(id, me, in_task != 0, "a send"),
        Some(r) => r,
        None => {
            with(|st| unwait(st, &me));
            sched::progressed();
            0
        }
    }
}

/// A full `Block` mailbox with no message ready to run: the sender waits.
const WAIT: u64 = u64::MAX;

fn now(st: &State) -> i128 {
    st.start.elapsed().as_nanos() as i128
}

/// A handler of actor `id` failed with the shown failure `error`, which stops it `Crashed`
/// (RFC 0060 D3, R977). Its supervisor queues the restarts its strategy asks for, which
/// [`yan_rt_actor_restart_next`] hands out, unless that exceeds the restart intensity: then
/// every child stops and the run panics `supervisor`.
///
/// # Safety
/// `error` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_actor_crash(id: u64, error: *const String) {
    let id = id as usize;
    // SAFETY: the caller's contract.
    let error = unsafe { &*error }.clone();
    let over = with(|st| {
        let over = crash(st, id, error);
        settle(st);
        over
    });
    if let Some((n, p)) = over {
        panic_with(
            "supervisor",
            &format!("a supervisor exceeded its restart intensity of {n} per {p} ns"),
        );
    }
}

fn crash(st: &mut State, id: usize, error: String) -> Option<(usize, i128)> {
    {
        st.actors[id].stop(Reason::Crashed(error));
        let (s, restart) = (st.actors[id].sup, st.actors[id].restart);
        if restart == Restart::Temporary {
            st.actors[id].release_start();
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
                st.actors[c].end();
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
        for &c in group.iter().rev().filter(|c| **c != id) {
            if st.actors[c].restart == Restart::Temporary {
                st.actors[c].end();
            } else {
                st.actors[c].stop(Reason::Shutdown);
            }
        }
        st.restarts.extend(
            group
                .into_iter()
                .filter(|c| st.actors[*c].restart != Restart::Temporary),
        );
        None
    }
}

/// The next queued restart into the work slots: 1, or 0 when none is left.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_restart_next() -> u64 {
    with(|st| match st.restarts.pop_front() {
        Some(c) => {
            let [thunk, code, env] = st.actors[c].start;
            st.work = [thunk, code, env, c as u64, 0];
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
        a.ended = a.stopped.take();
        a.starts += 1;
        a.state = Some(v);
    });
}

/// Why actor `id` stopped, or why its last start did once it restarted.
fn why(a: &Actor) -> Option<&Reason> {
    a.stopped.as_ref().or(a.ended.as_ref())
}

/// Delivers the `Stopped` of each monitor whose actor has stopped, and the `NodeDown` of each
/// whose watcher a partition cuts from it (RFC 0063 D7): a `Stopped` is a `StopReason` tag
/// (0 `Normal`, 1 `Shutdown`, 2 `Crashed`, 3 `NodeDown`) and the `Crashed` error.
pub(crate) fn settle(st: &mut State) {
    let t = st.faults.since();
    let mut kept = Vec::new();
    for (id, ch, owner) in std::mem::take(&mut st.monitors) {
        let a = &st.actors[id];
        let word = match &a.stopped {
            Some(Reason::Shutdown) => Some([1, 0]),
            Some(Reason::Crashed(e)) => Some([2, crate::abi::str_block(e) as u64]),
            None if owner != a.home && st.faults.cut(owner, a.home, t) => Some([3, 0]),
            None => None,
        };
        let Some(v) = word else {
            kept.push((id, ch, owner));
            continue;
        };
        // SAFETY: a live channel of a `Stopped`'s two slots, whose sender end is ours.
        unsafe {
            crate::chan::push(ch, &v);
            crate::chan::yan_rt_chan_drop(ch as *mut crate::chan::Chan);
        }
        crate::sched::progressed();
    }
    st.monitors = kept;
}

/// Why stopped actor `id` stopped: 1 `Shutdown`, 2 `Crashed`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_reason(id: u64) -> u64 {
    with(|st| match why(&st.actors[id as usize]) {
        Some(Reason::Crashed(_)) => 2,
        _ => 1,
    })
}

/// The shown failure that crashed actor `id`, as a new string.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_actor_reason_text(id: u64) -> *mut String {
    let s = with(|st| match why(&st.actors[id as usize]) {
        Some(Reason::Crashed(s)) => s.clone(),
        _ => String::new(),
    });
    crate::abi::leak(s)
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
            nodes: None,
        });
        st.open.push(st.sups.len() - 1);
        low.min(depth)
    })
}

/// The scopes past the first `depth` end: their children stop in reverse start order (9.6),
/// and for a `with cluster` scope each node's children, the last node first (RFC 0063 D1).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_sup_end_to(depth: u64) {
    with(|st| {
        while st.open.len() as u64 > depth {
            let s = st.open.pop().expect("an open scope");
            let nodes = st.sups[s].nodes.as_ref().map(|n| n.1.clone());
            for s in nodes.unwrap_or_default().into_iter().rev().chain([s]) {
                for c in st.sups[s].children.clone().into_iter().rev() {
                    st.actors[c].end();
                }
            }
        }
        settle(st);
    });
}

/// The program ends: the root supervisor's children stop, the last started first (R957).
pub fn end_root() {
    with(|st| {
        for c in st.sups[0].children.clone().into_iter().rev() {
            st.actors[c].end();
        }
        settle(st);
        // A monitor of an actor that never stopped holds its channel's sender end.
        for (_, ch, _) in std::mem::take(&mut st.monitors) {
            // SAFETY: a live channel whose sender end is the monitor's.
            unsafe { crate::chan::yan_rt_chan_drop(ch as *mut crate::chan::Chan) };
        }
    });
}

/// The node task `me` runs on: the innermost node entered by it or a task it descends from
/// (D1.2), else node 0, the binding task's (D1.1).
pub(crate) fn current(st: &State, me: &[usize]) -> usize {
    st.nodes
        .iter()
        .rev()
        .find(|(p, _)| me.starts_with(p))
        .map_or(0, |n| n.1)
}

/// The node actor `id` runs on.
pub(crate) fn home(st: &State, id: usize) -> usize {
    st.actors[id].home
}

/// Task `me` returns from the node it last entered.
pub(crate) fn leave(st: &mut State, me: &[usize]) {
    if let Some(k) = st.nodes.iter().rposition(|(p, _)| p == me) {
        st.nodes.remove(k);
    }
}

/// The supervisor of an actor started now: the innermost open scope that is not a cluster's,
/// or for a start placed on another node, that node's root supervisor (RFC 0063 D2).
fn adopter(st: &State) -> usize {
    let mut open = st.open.iter().rev().map(|s| (*s, &st.sups[*s]));
    match st.placed {
        Some(0) => 0,
        Some(k) => open
            .find_map(|(_, s)| s.nodes.as_ref())
            .and_then(|n| n.1.get(k - 1).copied())
            .unwrap_or(0),
        None => open.find(|(_, s)| s.nodes.is_none()).map_or(0, |s| s.0),
    }
}

/// The innermost scope ends.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_sup_end() {
    let n = with(|st| st.open.len() as u64);
    yan_rt_sup_end_to(n.saturating_sub(1));
}
