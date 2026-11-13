//! `std.chan` for native code (RFC 0043): a channel is one block shared by its `Sender` and its
//! `Receiver`, holding a FIFO of element slots, freed with its last end. An operation that
//! cannot proceed parks its task (`sched`); outside a task, or with every task parked, nothing
//! can end the wait, which is a global deadlock (R396).

use crate::sched;

use std::collections::VecDeque;

pub struct Chan {
    cap: usize,
    words: usize,
    /// Queued elements, `words` slots each (one for a zero-slot element), oldest first.
    q: VecDeque<u64>,
    ends: u32,
}

impl Chan {
    fn stride(&self) -> usize {
        self.words.max(1)
    }

    fn len(&self) -> usize {
        self.q.len() / self.stride()
    }
}

pub(crate) fn deadlock() -> ! {
    let k = "deadlock";
    // SAFETY: a static string.
    unsafe { crate::abi::yan_rt_panic(k.as_ptr(), k.len()) }
}

/// `std.chan.Channel`: a channel of `words`-slot elements holding up to `cap` of them.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_chan_new(cap: u64, words: u64) -> *mut Chan {
    crate::heap::block_taken();
    Box::into_raw(Box::new(Chan {
        cap: cap as usize,
        words: words as usize,
        q: VecDeque::new(),
        ends: 2,
    }))
}

/// The status of an operation that parked its task: run it again when the task resumes.
pub const BLOCKED: u64 = 1;

/// The status of an operation in a cancelled task: it fails `Cancelled` (9.4).
pub const CANCELLED: u64 = 2;

/// A wait that cannot park leaves nothing else to run, so an idle actor runs a queued message
/// first, as it would at `par concurrent` quiescence (RFC 0056 D4): [`crate::actor::WORK`].
fn idle_or_deadlock() -> u64 {
    if crate::actor::yan_rt_actor_idle() == 0 {
        deadlock();
    }
    crate::actor::WORK
}

/// `send`: the element's slots from `buf`; 0, or [`BLOCKED`] when `in_task` (nonzero) lets it
/// park, or [`crate::actor::WORK`] when it cannot. A rendezvous channel (capacity 0) hands over only to a receiver already waiting.
///
/// # Safety
/// `ch` is a live channel and `buf` holds its element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_send(ch: *mut Chan, buf: *const u64, in_task: u64) -> u64 {
    if in_task != 0 && sched::cancelled() {
        return CANCELLED;
    }
    // SAFETY: the caller's contract.
    let c = unsafe { &mut *ch };
    let room = if c.cap == 0 {
        sched::waiting(ch as usize)
    } else {
        c.cap
    };
    if c.len() >= room {
        if sched::park((false, ch as usize), in_task != 0) {
            return BLOCKED;
        }
        return idle_or_deadlock();
    }
    // SAFETY: the caller's contract.
    let v = unsafe { std::slice::from_raw_parts(buf, c.words) };
    c.q.extend(v);
    if c.words == 0 {
        c.q.push_back(0);
    }
    sched::progressed();
    0
}

/// `recv`: the oldest element's slots into `out`; 0, or [`BLOCKED`] as for `send`.
///
/// # Safety
/// `ch` is a live channel and `out` has room for its element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_recv(ch: *mut Chan, out: *mut u64, in_task: u64) -> u64 {
    if in_task != 0 && sched::cancelled() {
        return CANCELLED;
    }
    // SAFETY: the caller's contract.
    let c = unsafe { &mut *ch };
    if c.len() == 0 {
        if sched::park((true, ch as usize), in_task != 0) {
            return BLOCKED;
        }
        return idle_or_deadlock();
    }
    for (i, w) in c.q.drain(..c.stride()).enumerate().take(c.words) {
        // SAFETY: the caller's contract.
        unsafe { *out.add(i) = w };
    }
    sched::progressed();
    0
}

/// Queues `v`, an element's slots, on channel `ch` as its sender would.
///
/// # Safety
/// `ch` is a live channel of `v.len()` slots.
pub(crate) unsafe fn push(ch: usize, v: &[u64]) {
    // SAFETY: the caller's contract.
    let c = unsafe { &mut *(ch as *mut Chan) };
    c.q.extend(v);
}

/// Drop one end; the channel goes with its last. Queued elements are not dropped (RFC 0043).
///
/// # Safety
/// `ch` is a live channel and this end is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_drop(ch: *mut Chan) {
    // SAFETY: the caller's contract.
    let c = unsafe { &mut *ch };
    c.ends -= 1;
    if c.ends == 0 {
        crate::heap::block_freed();
        // SAFETY: from `yan_rt_chan_new`, and both ends are gone.
        drop(unsafe { Box::from_raw(ch) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_come_out_in_send_order_and_the_last_end_frees() {
        let ch = yan_rt_chan_new(2, 2);
        let mut out = [0u64; 2];
        // SAFETY: a live channel and buffers of two words.
        unsafe {
            yan_rt_chan_send(ch, [1, 2].as_ptr(), 0);
            yan_rt_chan_send(ch, [3, 4].as_ptr(), 0);
            yan_rt_chan_recv(ch, out.as_mut_ptr(), 0);
            assert_eq!(out, [1, 2]);
            yan_rt_chan_recv(ch, out.as_mut_ptr(), 0);
            assert_eq!(out, [3, 4]);
            yan_rt_chan_drop(ch);
            yan_rt_chan_drop(ch);
        }
    }
}
