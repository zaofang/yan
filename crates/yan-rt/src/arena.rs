//! `Arena` (4.7, 5.5) in native code: a block holding the slots `alloc` handed out, freed
//! together with it.

use crate::abi::{yan_rt_alloc, yan_rt_free_env};

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_arena_new() -> *mut Vec<*mut u64> {
    crate::heap::block_taken();
    Box::into_raw(Box::default())
}

/// # Safety
/// `a` is from [`yan_rt_arena_new`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_arena_alloc(a: *mut Vec<*mut u64>, words: u64) -> *mut u64 {
    let slot = yan_rt_alloc(words);
    // SAFETY: the caller's contract.
    unsafe { (*a).push(slot) };
    slot
}

/// # Safety
/// `a` is from [`yan_rt_arena_new`], and no `Ref` into it is used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_arena_free(a: *mut Vec<*mut u64>) {
    crate::heap::block_freed();
    // SAFETY: the caller's contract.
    let slots = unsafe { Box::from_raw(a) };
    for s in slots.into_iter() {
        // SAFETY: each slot is from `yan_rt_alloc` and owned by the arena.
        unsafe { yan_rt_free_env(s) };
    }
}
