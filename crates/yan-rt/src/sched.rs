//! The cooperative scheduler of `par concurrent` in native code (RFC 0043 D2), a port of the
//! interpreter's `run_tasks`: a task that parks yields to the lowest-index task that can
//! progress, and a task is retried only after some channel progress since it parked (R390).
//! Every task parked is a global deadlock (R396).

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
}

struct Block {
    tasks: Vec<Task>,
    running: Option<usize>,
    /// The block runs in a task that can suspend, so it may yield instead of forcing.
    can_yield: bool,
}

#[derive(Default)]
struct State {
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

pub(crate) fn waiting(c: usize) -> usize {
    with(|st| st.waiting_recv.iter().filter(|w| **w == c).count())
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
        let b = Block {
            tasks: (0..n).map(|_| Task::default()).collect(),
            running: None,
            can_yield: can_yield != 0,
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

/// Account for the step of block `h` that just ended, then the task to run next: the task
/// count when every task is done, or one past it when every task is parked and the block
/// yields to its enclosing task.
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
        let b = block(st, h);
        let Some(first) = b.tasks.iter().position(|t| !t.done) else {
            return b.tasks.len() as u64;
        };
        let pick = b
            .tasks
            .iter()
            .position(|t| !t.done && t.seen.is_none_or(|s| s != progress));
        let (i, forced) = match pick {
            Some(i) => (i, false),
            None if b.can_yield && !inherited => {
                let n = b.tasks.len() as u64;
                st.park = Some(NESTED);
                return n + 1;
            }
            None => (first, true),
        };
        b.running = Some(i);
        st.force = forced.then_some(h as usize);
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
    });
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
