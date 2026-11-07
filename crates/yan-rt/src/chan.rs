//! `std.chan` for native code (RFC 0043): a channel is one block shared by its `Sender` and its
//! `Receiver`, holding a FIFO of element slots, freed with its last end. Without a scheduler a
//! wait can never end, so an operation that cannot proceed is a global deadlock (R396).

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

fn deadlock() -> ! {
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

/// `send`: the element's slots from `buf`. A rendezvous channel (capacity 0) hands over only to
/// a receiver already waiting, and none waits without a scheduler.
///
/// # Safety
/// `ch` is a live channel and `buf` holds its element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_send(ch: *mut Chan, buf: *const u64) {
    // SAFETY: the caller's contract.
    let c = unsafe { &mut *ch };
    if c.len() >= c.cap {
        deadlock();
    }
    // SAFETY: the caller's contract.
    let v = unsafe { std::slice::from_raw_parts(buf, c.words) };
    c.q.extend(v);
    if c.words == 0 {
        c.q.push_back(0);
    }
}

/// `recv`: the oldest element's slots into `out`.
///
/// # Safety
/// `ch` is a live channel and `out` has room for its element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_chan_recv(ch: *mut Chan, out: *mut u64) {
    // SAFETY: the caller's contract.
    let c = unsafe { &mut *ch };
    if c.len() == 0 {
        deadlock();
    }
    for (i, w) in c.q.drain(..c.stride()).enumerate().take(c.words) {
        // SAFETY: the caller's contract.
        unsafe { *out.add(i) = w };
    }
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
            yan_rt_chan_send(ch, [1, 2].as_ptr());
            yan_rt_chan_send(ch, [3, 4].as_ptr());
            yan_rt_chan_recv(ch, out.as_mut_ptr());
            assert_eq!(out, [1, 2]);
            yan_rt_chan_recv(ch, out.as_mut_ptr());
            assert_eq!(out, [3, 4]);
            yan_rt_chan_drop(ch);
            yan_rt_chan_drop(ch);
        }
    }
}
