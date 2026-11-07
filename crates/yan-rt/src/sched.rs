//! The cooperative scheduler of `par concurrent` in native code (RFC 0043 D2), a port of the
//! interpreter's `run_tasks`: a task that parks yields to the lowest-index task that can
//! progress, and a task is retried only after some channel progress since it parked (R390).
//! Every task parked is a global deadlock (R396).

use std::cell::RefCell;

/// A park: (is a receive, channel).
pub(crate) type Park = (bool, usize);

#[derive(Default)]
struct Task {
    done: bool,
    park: Option<Park>,
    seen: Option<u64>,
}

struct Block {
    tasks: Vec<Task>,
    running: Option<usize>,
}

#[derive(Default)]
struct State {
    /// The `par concurrent` blocks running, innermost last.
    blocks: Vec<Block>,
    /// Counts channel progress: a transfer, or a task starting to wait for a receive.
    progress: u64,
    /// The channels tasks are parked receiving on, once per task.
    waiting_recv: Vec<usize>,
    /// Set by a blocking operation that parked.
    park: Option<Park>,
    /// Every task is parked: the next blocking operation cannot park.
    force: bool,
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

pub(crate) fn progressed() {
    with(|st| st.progress += 1);
}

pub(crate) fn waiting(c: usize) -> usize {
    with(|st| st.waiting_recv.iter().filter(|w| **w == c).count())
}

/// Park the running task on `p`; false when it cannot park (outside a task, or forced).
pub(crate) fn park(p: Park, in_task: bool) -> bool {
    with(|st| {
        if !in_task || st.force || st.blocks.is_empty() {
            return false;
        }
        st.park = Some(p);
        true
    })
}

/// Enter a `par concurrent` of `n` tasks.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_begin(n: u64) {
    with(|st| {
        st.blocks.push(Block {
            tasks: (0..n).map(|_| Task::default()).collect(),
            running: None,
        })
    });
}

/// The running task's branch ended.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_task_end() {
    with(|st| {
        let b = st.blocks.last_mut().expect("in a par block");
        let i = b.running.expect("a running task");
        b.tasks[i].done = true;
    });
}

/// Account for the step that just ended, then the task to run next, or the task count when
/// every task is done.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_next() -> u64 {
    let next = with(|st| {
        let parked = st.park.take();
        let b = st.blocks.last_mut().expect("in a par block");
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
                    st.progress += 1;
                }
                let b = st.blocks.last_mut().expect("in a par block");
                b.tasks[i].seen = Some(st.progress);
            } else if let Some((true, c)) = prev
                && unwait(st, c)
            {
                st.progress += 1;
            }
        }
        let progress = st.progress;
        let b = st.blocks.last_mut().expect("in a par block");
        let pick = b
            .tasks
            .iter()
            .position(|t| !t.done && t.seen.is_none_or(|s| s != progress));
        let (i, forced) = match pick {
            Some(i) => (i, false),
            None => match b.tasks.iter().position(|t| !t.done) {
                Some(i) => (i, true),
                None => return None,
            },
        };
        b.running = Some(i);
        st.force = forced;
        Some(i as u64)
    });
    match next {
        Some(i) => i,
        None => with(|st| st.blocks.last().expect("in a par block").tasks.len() as u64),
    }
}

/// Leave the innermost `par concurrent`, normally or through a task's early exit.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_par_end() {
    with(|st| {
        let b = st.blocks.pop().expect("in a par block");
        for t in &b.tasks {
            if let Some((true, c)) = t.park {
                unwait(st, c);
            }
        }
        st.force = false;
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
        yan_rt_par_begin(2);
        loop {
            let k = yan_rt_par_next() as usize;
            if k == 2 {
                break;
            }
            loop {
                if next[k] == 3 {
                    yan_rt_par_task_end();
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
        yan_rt_par_end();
        assert_eq!(trace, [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)]);
        // SAFETY: both ends, once each.
        unsafe {
            yan_rt_chan_drop(ch);
            yan_rt_chan_drop(ch);
        }
    }
}
