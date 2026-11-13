//! The cooperative scheduler of `par concurrent` in native code (RFC 0043 D2), a port of the
//! interpreter's `run_tasks`: a task that parks yields to the lowest-index task that can
//! progress, and a task is retried only after some channel progress since it parked (R390).
//! Every task parked is a global deadlock (R396). A task that leaves the function cancels the
//! others, whose blocking operations then fail `Cancelled` until they end (9.4, R394).

use std::cell::RefCell;

/// A park: (is a receive, channel).
pub(crate) type Park = (bool, usize);

/// The park of a task whose nested `par concurrent` yielded: every task in it is parked.
const NESTED: Park = (false, usize::MAX);

#[derive(Default)]
struct Task {
    done: bool,
    park: Option<Park>,
    seen: Option<u64>,
    /// The PCT priority (R999); unused without a `pct` scheduler.
    prio: i64,
}

struct Block {
    tasks: Vec<Task>,
    running: Option<usize>,
    /// The block runs in a task that can suspend, so it may yield instead of forcing.
    can_yield: bool,
    /// The block whose task was running when this one began.
    parent: Option<usize>,
    cancelled: bool,
    /// The kept exit: its task and kind (0 returned, 1 failed, 2 failed `Cancelled`).
    kept: Option<(usize, u64)>,
}

#[derive(Default)]
struct State {
    /// Set by `ParExitKind` for the exit that follows.
    exit_kind: u64,
    /// The `par concurrent` blocks entered and not left, by handle. A block that yielded
    /// stays while its enclosing task runs others.
    blocks: Vec<Option<Block>>,
    /// Counts channel progress: a transfer, or a task starting to wait for a receive.
    progress: u64,
    /// The channels tasks are parked receiving on, once per task.
    waiting_recv: Vec<usize>,
    /// Set by a blocking operation that parked.
    park: Option<Park>,
    /// The block whose tasks are all parked and that runs one anyway: blocking operations
    /// cannot park, and blocks nested in that task cannot yield.
    force: Option<usize>,
    /// The block whose task is running.
    cur: Option<usize>,
    /// The PCT scheduler last built, which chooses the next task (R999).
    pct: Option<crate::pct::Pct>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn unwait(st: &mut State, c: usize) -> bool {
    match st.waiting_recv.iter().position(|w| *w == c) {
        Some(k) => {
            st.waiting_recv.swap_remove(k);
            true
        }
        None => false,
    }
}

fn block(st: &mut State, h: u64) -> &mut Block {
    st.blocks[h as usize].as_mut().expect("a live par block")
}

pub(crate) fn progressed() {
    with(|st| st.progress += 1);
}

/// The running task is cancelled: its block or one around it is.
pub(crate) fn cancelled() -> bool {
    with(|st| {
        let mut at = st.cur;
        while let Some(h) = at {
            let b = st.blocks[h].as_ref().expect("a live par block");
            if b.cancelled {
                return true;
            }
            at = b.parent;
        }
        false
    })
}

pub(crate) fn waiting(c: usize) -> usize {
    with(|st| st.waiting_recv.iter().filter(|w| **w == c).count())
}

/// The running task's path (R398): the index of the running task of each block from the
/// outermost in.
pub(crate) fn path() -> Vec<usize> {
    with(|st| {
        let mut p = Vec::new();
        let mut at = st.cur;
        while let Some(h) = at {
            let b = st.blocks[h].as_ref().expect("a live par block");
            p.extend(b.running);
            at = b.parent;
        }
        p.reverse();
        p
    })
}

/// A block runs a task although every task is parked.
pub(crate) fn forced() -> bool {
    with(|st| st.force.is_some())
}

/// No `par concurrent` block is running.
pub(crate) fn idle() -> bool {
    with(|st| st.blocks.iter().all(Option::is_none))
}

/// Park the running task on `p`; false when it cannot park (outside a task, or forced).
pub(crate) fn park(p: Park, in_task: bool) -> bool {
    with(|st| {
        if !in_task || st.force.is_some() || st.blocks.iter().all(Option::is_none) {
            return false;
        }
        st.park = Some(p);
        true
    })
}

/// Enter a `par concurrent` of `n` tasks; its handle. `can_yield` (nonzero) when it runs in a
/// task that can suspend.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_begin(n: u64, can_yield: u64) -> u64 {
    with(|st| {
        let prios = match st.pct.as_mut() {
            Some(p) => p.priorities(n as usize),
            None => vec![0; n as usize],
        };
        let b = Block {
            tasks: prios
                .into_iter()
                .map(|prio| Task {
                    prio,
                    ..Task::default()
                })
                .collect(),
            running: None,
            can_yield: can_yield != 0,
            parent: st.cur,
            cancelled: false,
            kept: None,
        };
        match st.blocks.iter().position(Option::is_none) {
            Some(h) => {
                st.blocks[h] = Some(b);
                h as u64
            }
            None => {
                st.blocks.push(Some(b));
                st.blocks.len() as u64 - 1
            }
        }
    })
}

/// The running task of block `h` ended its branch.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_task_end(h: u64) {
    with(|st| {
        let b = block(st, h);
        let i = b.running.expect("a running task");
        b.tasks[i].done = true;
    });
}

/// The task about to leave the function failed (1) or failed `Cancelled` (2).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_exit_kind(kind: u64) {
    with(|st| st.exit_kind = kind);
}

/// The running task of block `h` leaves the function: it ends and cancels the others. 1 if
/// its result is the one the block returns: the first exit, or a failure that is not
/// `Cancelled` with a lower index than the failure kept (R394); 0 otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_cancel(h: u64) -> u64 {
    with(|st| {
        let kind = std::mem::take(&mut st.exit_kind);
        let b = block(st, h);
        let i = b.running.expect("a running task");
        b.tasks[i].done = true;
        b.cancelled = true;
        let keep = match b.kept {
            None => true,
            Some((j, k)) => kind == 1 && (k != 1 || i < j),
        };
        if keep {
            b.kept = Some((i, kind));
        }
        u64::from(keep)
    })
}

/// Account for the step of block `h` that just ended, then the task to run next: the task
/// count when every task is done, one past it when every task is parked and the block yields
/// to its enclosing task, or two past it to run an idle actor's message first.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_next(h: u64) -> u64 {
    with(|st| {
        let parked = st.park.take();
        let b = block(st, h);
        if let Some(i) = b.running.take() {
            let prev = b.tasks[i].park.take();
            if let Some(now) = parked {
                b.tasks[i].park = Some(now);
                if prev != Some(now) {
                    if let Some((true, c)) = prev {
                        unwait(st, c);
                    }
                    if now.0 {
                        st.waiting_recv.push(now.1);
                    }
                    if now != NESTED {
                        st.progress += 1;
                    }
                }
                let progress = st.progress;
                block(st, h).tasks[i].seen = Some(progress);
            } else if let Some((true, c)) = prev
                && unwait(st, c)
            {
                st.progress += 1;
            }
        }
        let progress = st.progress;
        let inherited = st.force.is_some_and(|f| f != h as usize);
        let b = st.blocks[h as usize].as_mut().expect("a live par block");
        let parent = b.parent;
        let Some(first) = b.tasks.iter().position(|t| !t.done) else {
            let n = b.tasks.len() as u64;
            st.cur = parent;
            return n;
        };
        // A cancelled task runs on to fail its next wait.
        let cancelled = b.cancelled;
        let runnable: Vec<bool> = b
            .tasks
            .iter()
            .map(|t| !t.done && (cancelled || t.seen.is_none_or(|s| s != progress)))
            .collect();
        let pick = match st.pct.as_mut() {
            Some(p) => {
                let mut prios: Vec<i64> = b.tasks.iter().map(|t| t.prio).collect();
                let i = p.pick(&mut prios, &runnable);
                b.tasks.iter_mut().zip(prios).for_each(|(t, q)| t.prio = q);
                i
            }
            None => runnable.iter().position(|r| *r),
        };
        // No task can progress: an idle actor runs a queued message, as an actor task would
        // (RFC 0056 D4), and the parked tasks retry.
        if pick.is_none() && crate::actor::idle_ready() {
            let n = b.tasks.len() as u64;
            st.cur = parent;
            return n + 2;
        }
        let (i, forced) = match pick {
            Some(i) => (i, false),
            None if b.can_yield && !inherited => {
                let n = b.tasks.len() as u64;
                st.park = Some(NESTED);
                st.cur = parent;
                return n + 1;
            }
            None => (first, true),
        };
        b.running = Some(i);
        st.force = forced.then_some(h as usize);
        st.cur = Some(h as usize);
        i as u64
    })
}

/// Leave block `h`, normally or through a task's early exit.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_end(h: u64) {
    with(|st| {
        let b = st.blocks[h as usize].take().expect("a live par block");
        for t in &b.tasks {
            if let Some((true, c)) = t.park {
                unwait(st, c);
            }
        }
        if st.force == Some(h as usize) {
            st.force = None;
        }
        st.park = None;
        st.cur = b.parent;
    });
}

/// `pct(depth, seed, steps)`: the scheduler of every `par concurrent` that begins after it,
/// as the interpreter keeps the last one built.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_sim_pct(depth: i64, seed: i64, steps: i64) {
    if depth < 1 || steps < 1 {
        crate::actor::panic_with(
            "precondition `depth >= 1 and steps >= 1` of `pct` failed",
            crate::abi::panic_message(""),
        );
    }
    with(|st| st.pct = Some(crate::pct::Pct::new(seed as u64, depth, steps)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chan::*;

    /// The R390 case by hand: task 0 sends 0, 1, 2 on a rendezvous channel and task 1 receives
    /// them; the trace is the order of completed operations, as the interpreter runs them.
    #[test]
    fn a_rendezvous_hands_over_in_send_order() {
        let ch = yan_rt_chan_new(0, 1);
        let mut next = [0u64; 2];
        let mut trace = Vec::new();
        let h = yan_rt_par_begin(2, 0);
        loop {
            let k = yan_rt_par_next(h) as usize;
            if k == 2 {
                break;
            }
            loop {
                if next[k] == 3 {
                    yan_rt_par_task_end(h);
                    break;
                }
                let mut w = [next[k]];
                // SAFETY: a live channel and one-word buffers.
                let st = unsafe {
                    if k == 0 {
                        yan_rt_chan_send(ch, w.as_ptr(), 1)
                    } else {
                        yan_rt_chan_recv(ch, w.as_mut_ptr(), 1)
                    }
                };
                if st == BLOCKED {
                    break;
                }
                trace.push((k, w[0]));
                next[k] += 1;
            }
        }
        yan_rt_par_end(h);
        assert_eq!(trace, [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)]);
        // SAFETY: both ends, once each.
        unsafe {
            yan_rt_chan_drop(ch);
            yan_rt_chan_drop(ch);
        }
    }
}
