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
    /// Live `Sender`s; with none left and nothing queued a receive finds the channel closed.
    senders: u32,
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
    let ch = Box::into_raw(Box::new(Chan {
        cap: cap as usize,
        words: words as usize,
        q: VecDeque::new(),
        ends: 2,
        senders: 1,
    }));
    crate::heap::block_taken(ch as usize, crate::heap::release_box::<Chan>);
    ch
}

/// The status of an operation that parked its task: run it again when the task resumes.
pub const BLOCKED: u64 = 1;

/// The status of an operation in a cancelled task: it fails `Cancelled` (9.4).
pub const CANCELLED: u64 = 2;

/// The status of a `recv` on a closed, empty channel: it yields None (9.5).
pub const CLOSED: u64 = 3;

/// The status of a `try_recv` on an open channel with nothing queued, or a `try_send` on one
/// with no room (`Empty`, `Full`).
pub const NOT_READY: u64 = 4;

/// A wait that cannot park leaves nothing else to run, so an idle actor runs a queued message
/// first, as it would at `par concurrent` quiescence (RFC 0056 D4): [`crate::actor::WORK`].
pub(crate) fn idle_or_deadlock() -> u64 {
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
    // SAFETY: the caller's contract.
    let v = unsafe { std::slice::from_raw_parts(buf, (*ch).words) };
    send(ch as usize, v, in_task != 0)
}

/// The slots of `ch`'s elements.
pub fn words(ch: usize) -> usize {
    // SAFETY: a live channel, the contract of every operation on it.
    unsafe { (*(ch as *const Chan)).words }
}

/// `send` over the element's slots `v`.
pub fn send(ch: usize, v: &[u64], in_task: bool) -> u64 {
    if in_task && sched::cancelled() {
        return CANCELLED;
    }
    // SAFETY: a live channel, the contract of every operation on it.
    let c = unsafe { &mut *(ch as *mut Chan) };
    let ch = ch as *mut Chan;
    let room = if c.cap == 0 {
        sched::waiting(ch as usize)
    } else {
        c.cap
    };
    if c.len() >= room {
        if sched::park((false, ch as usize), in_task) {
            return BLOCKED;
        }
        return idle_or_deadlock();
    }
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
    let (status, v) = recv(ch as usize, in_task != 0);
    for (i, w) in v.into_iter().enumerate() {
        // SAFETY: the caller's contract.
        unsafe { *out.add(i) = w };
    }
    status
}

/// `recv`: the status and, when 0, the oldest element's slots.
pub fn recv(ch: usize, in_task: bool) -> (u64, Vec<u64>) {
    if in_task && sched::cancelled() {
        return (CANCELLED, Vec::new());
    }
    // SAFETY: a live channel, the contract of every operation on it.
    let c = unsafe { &mut *(ch as *mut Chan) };
    if c.len() == 0 {
        if c.senders == 0 {
            return (CLOSED, Vec::new());
        }
        if sched::park((true, ch), in_task) {
            return (BLOCKED, Vec::new());
        }
        return (idle_or_deadlock(), Vec::new());
    }
    let v = c.q.drain(..c.stride()).take(c.words).collect();
    sched::progressed();
    (0, v)
}

/// `try_send`: as `send`, but a channel with no room is [`NOT_READY`] instead of a wait.
///
/// # Safety
/// `ch` is a live channel and `buf` holds its element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_try_send(ch: *mut Chan, buf: *const u64, in_task: u64) -> u64 {
    // SAFETY: the caller's contract.
    let v = unsafe { std::slice::from_raw_parts(buf, (*ch).words) };
    try_send(ch as usize, v, in_task != 0)
}

/// `try_send` over the element's slots `v`.
pub fn try_send(ch: usize, v: &[u64], in_task: bool) -> u64 {
    if in_task && sched::cancelled() {
        return CANCELLED;
    }
    // SAFETY: a live channel, the contract of every operation on it.
    let c = unsafe { &mut *(ch as *mut Chan) };
    let room = if c.cap == 0 {
        sched::waiting(ch)
    } else {
        c.cap
    };
    if c.len() >= room {
        return NOT_READY;
    }
    send(ch, v, false)
}

/// `try_recv`: as `recv`, but an open channel with nothing queued is [`NOT_READY`] instead of a
/// wait; a closed one is [`CLOSED`].
///
/// # Safety
/// `ch` is a live channel and `out` has room for its element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_try_recv(ch: *mut Chan, out: *mut u64, in_task: u64) -> u64 {
    let (status, v) = try_recv(ch as usize, in_task != 0);
    for (i, w) in v.into_iter().enumerate() {
        // SAFETY: the caller's contract.
        unsafe { *out.add(i) = w };
    }
    status
}

/// `try_recv`: the status and, when 0, the oldest element's slots.
pub fn try_recv(ch: usize, in_task: bool) -> (u64, Vec<u64>) {
    // SAFETY: a live channel, the contract of every operation on it.
    let c = unsafe { &*(ch as *const Chan) };
    if c.len() == 0 && c.senders != 0 && !(in_task && sched::cancelled()) {
        return (NOT_READY, Vec::new());
    }
    recv(ch, in_task)
}

/// The arm kinds in the low two bits of a `select_wait` key, the channel above: a receive that
/// is ready on a message or on close, a send, a receive that wants a message (`is Some(..)`),
/// and one that wants close (`is None`). A timer arm is -1.
const SEL_RECV: i64 = 0;
const SEL_SEND: i64 = 1;
const SEL_VALUE: i64 = 2;

/// `select_wait` (15.3): the index of the first ready arm in source order into `out`, as the
/// interpreter's; 0, or [`BLOCKED`], [`CANCELLED`] or [`crate::actor::WORK`] as for `recv`.
/// A timer arm takes the next of `at`'s readings (two words each) and is ready once the clock
/// reaches it; with nothing else able to end the wait the clock runs up to the earliest.
///
/// # Safety
/// `keys` and `at` are live lists of arm keys and readings, and `out` has a slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_select_wait(
    keys: *const crate::abi::List,
    at: *const crate::abi::List,
    out: *mut u64,
    in_task: u64,
) -> u64 {
    // SAFETY: the caller's contract.
    let (keys, at) = unsafe { (&(*keys).data, &(*at).data) };
    let (status, arm) = select_wait(keys, at, in_task != 0);
    if status == 0 {
        // SAFETY: the caller's contract.
        unsafe { *out = arm };
    }
    status
}

/// `select_wait` over the arm keys and timer readings: the status and, when 0, the ready arm.
pub fn select_wait(keys: &[u64], at: &[u64], in_task: bool) -> (u64, u64) {
    let at: Vec<i128> = at
        .chunks(2)
        .map(|w| (i128::from(w[0] as i64) << 64) | i128::from(w[1]))
        .collect();
    loop {
        if in_task && sched::cancelled() {
            return (CANCELLED, 0);
        }
        let now = crate::civil::mono_now();
        let mut timers = at.iter();
        let mut wake: Option<i128> = None;
        for (i, &k) in keys.iter().enumerate() {
            let k = k as i64;
            let ready = if k < 0 {
                let t = *timers.next().expect("a reading per timer arm");
                wake = Some(wake.map_or(t, |w| w.min(t)));
                t <= now
            } else {
                // SAFETY: a key is a live channel with its arm kind in the low bits.
                let c = unsafe { &*((k & !3) as *const Chan) };
                match k & 3 {
                    SEL_RECV => c.senders == 0 || c.len() > 0,
                    SEL_SEND => {
                        let room = if c.cap == 0 {
                            sched::waiting((k & !3) as usize)
                        } else {
                            c.cap
                        };
                        c.len() < room
                    }
                    SEL_VALUE => c.len() > 0,
                    _ => c.senders == 0 && c.len() == 0,
                }
            };
            if ready {
                return (0, i as u64);
            }
        }
        let first_recv = keys
            .iter()
            .map(|&k| k as i64)
            .find(|&k| k >= 0 && k & 3 != SEL_SEND);
        let on = first_recv.map_or((false, usize::MAX), |k| (true, (k & !3) as usize));
        if sched::park_until(on, wake, in_task) {
            return (BLOCKED, 0);
        }
        if crate::actor::yan_rt_actor_idle() != 0 {
            return (crate::actor::WORK, 0);
        }
        match wake {
            Some(w) => crate::civil::wait_until(w),
            None => deadlock(),
        }
    }
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
        crate::heap::block_freed(ch as usize);
        // SAFETY: from `yan_rt_chan_new`, and both ends are gone.
        drop(unsafe { Box::from_raw(ch) });
    }
}

/// Drop a `Sender`: the receivers see the channel closed once its last is gone. `close` drops
/// one early and leaves the null channel in its place, whose drop and share do nothing.
///
/// # Safety
/// As [`yan_rt_chan_drop`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_drop_sender(ch: *mut Chan) {
    // A `Sender` that `close` ended.
    if ch.is_null() {
        return;
    }
    // SAFETY: the caller's contract.
    unsafe { (*ch).senders -= 1 };
    sched::progressed();
    // SAFETY: as above.
    unsafe { yan_rt_chan_drop(ch) };
}

/// A copy of a `Sender`.
///
/// # Safety
/// `ch` is a live channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_clone_sender(ch: *mut Chan) {
    if ch.is_null() {
        return;
    }
    // SAFETY: the caller's contract.
    unsafe { (*ch).senders += 1 };
    // SAFETY: as above.
    unsafe { yan_rt_chan_clone(ch) };
}

/// A copy of an end: one more end of the same channel.
///
/// # Safety
/// `ch` is a live channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_clone(ch: *mut Chan) {
    // SAFETY: the caller's contract.
    unsafe { (*ch).ends += 1 };
}

/// The end operations over a channel word, for callers that hold no raw pointers.
pub fn end(ch: usize, op: End) {
    let ch = ch as *mut Chan;
    // SAFETY: a live channel with this end held, the contract of every operation on it.
    unsafe {
        match op {
            End::Drop => yan_rt_chan_drop(ch),
            End::DropSender => yan_rt_chan_drop_sender(ch),
            End::Clone => yan_rt_chan_clone(ch),
            End::CloneSender => yan_rt_chan_clone_sender(ch),
        }
    }
}

#[derive(Clone, Copy)]
pub enum End {
    Drop,
    DropSender,
    Clone,
    CloneSender,
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
