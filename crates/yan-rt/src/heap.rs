//! Live heap bytes for the heap budget (R676), counted when a binary installs [`Counting`] as
//! its global allocator; otherwise [`live`] stays 0 and no heap budget trips.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static BLOCKS: AtomicUsize = AtomicUsize::new(0);

pub struct Counting;

pub fn live() -> usize {
    LIVE.load(Relaxed)
}

/// Blocks handed to native code by the runtime ABI and not yet freed. Independent of
/// [`Counting`]: the interpreter never takes blocks, so this stays 0 there.
pub fn live_blocks() -> usize {
    BLOCKS.load(Relaxed)
}

/// Count the block at `p`; `release` frees it shallowly, should a panicking pool arm be
/// discarded while it still owns the block (`pool::arm_failed`).
pub fn block_taken(p: usize, release: unsafe fn(usize)) {
    BLOCKS.fetch_add(1, Relaxed);
    crate::pool::track(p, release);
}

pub fn block_freed(p: usize) {
    BLOCKS.fetch_sub(1, Relaxed);
    crate::pool::untrack(p);
}

/// Free a block that [`block_taken`] counted, without counting it twice.
pub(crate) fn block_released(p: usize, release: unsafe fn(usize)) {
    // SAFETY: `p` is a live block of the kind `release` frees.
    unsafe { release(p) };
    BLOCKS.fetch_sub(1, Relaxed);
}

/// Releases a block that is a `Box<T>`.
pub(crate) unsafe fn release_box<T>(p: usize) {
    // SAFETY: the caller's contract.
    drop(unsafe { Box::from_raw(p as *mut T) });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size(), Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size(), Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, size) };
        if !q.is_null() {
            LIVE.fetch_add(size, Relaxed);
            LIVE.fetch_sub(l.size(), Relaxed);
        }
        q
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn blocks_count_up_and_down() {
        let n = super::live_blocks();
        super::block_taken(0, super::release_box::<u8>);
        assert_eq!(super::live_blocks(), n + 1);
        super::block_freed(0);
        assert_eq!(super::live_blocks(), n);
    }
}
