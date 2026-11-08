//! `deque` for native code: a `List` block that also holds its `maxlen` (17.3, R488 to R493).
//! Evicted elements go back to the caller, who owns any blocks they hold.

use crate::abi::{List, leak};
use crate::map::{Key, parse};
use std::cmp::Ordering;

fn len(l: &List) -> usize {
    l.data.len().checked_div(l.words).unwrap_or(0)
}

/// Set the `maxlen`.
///
/// # Safety
/// `l` is a `List` block nothing else borrows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_limit(l: *mut List, n: i64) {
    // SAFETY: the caller's contract.
    unsafe { (*l).limit = n }
}

/// The `maxlen`, or -1.
///
/// # Safety
/// `l` is a `List` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_maxlen(l: *const List) -> i64 {
    // SAFETY: the caller's contract.
    unsafe { (*l).limit }
}

/// Push `x` on the back (`front` 0) or front. When the deque is full, the element from the other
/// end is moved to `out` and 1 returned (`x` itself when `maxlen` is 0); else 0.
///
/// # Safety
/// `l` is a `List` block nothing else borrows; `x` and `out` hold an element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_push(
    l: *mut List,
    front: i64,
    x: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let w = l.words;
    // SAFETY: as above.
    let x = unsafe { std::slice::from_raw_parts(x, w) };
    let mut evicted = 0;
    if l.limit == 0 {
        // SAFETY: `out` has room for an element.
        unsafe { std::ptr::copy_nonoverlapping(x.as_ptr(), out, w) };
        return 1;
    }
    if l.limit > 0 && len(l) as i64 >= l.limit {
        let at = if front != 0 { l.data.len() - w } else { 0 };
        // SAFETY: the deque holds an element at `at`.
        unsafe { std::ptr::copy_nonoverlapping(l.data[at..].as_ptr(), out, w) };
        l.data.drain(at..at + w);
        evicted = 1;
    }
    if front != 0 {
        l.data.splice(0..0, x.iter().copied());
    } else {
        l.data.extend_from_slice(x);
    }
    evicted
}

/// Move the first (`front` 1) or last element to `out`: 1, or 0 when empty.
///
/// # Safety
/// `l` is a `List` block nothing else borrows; `out` has room for an element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_pop(l: *mut List, front: i64, out: *mut u64) -> i64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let w = l.words;
    if len(l) == 0 {
        return 0;
    }
    let at = if front != 0 { 0 } else { l.data.len() - w };
    // SAFETY: the deque holds an element at `at`.
    unsafe { std::ptr::copy_nonoverlapping(l.data[at..].as_ptr(), out, w) };
    l.data.drain(at..at + w);
    1
}

/// Append every element of `src` (freeing its block, not its elements); the elements pushed out
/// come back as a new `List` block.
///
/// # Safety
/// `l` and `src` are distinct `List` blocks of the same element width.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_extend(l: *mut List, src: *mut List) -> *mut List {
    crate::heap::block_freed();
    // SAFETY: the caller's contract; `src` came from `leak`.
    let (l, src) = unsafe { (&mut *l, *Box::from_raw(src)) };
    let w = l.words;
    let mut out = Vec::new();
    if w > 0 {
        for x in src.data.chunks(w) {
            if l.limit == 0 {
                out.extend_from_slice(x);
                continue;
            }
            if l.limit > 0 && len(l) as i64 >= l.limit {
                out.extend(l.data.drain(..w));
            }
            l.data.extend_from_slice(x);
        }
    }
    leak(List {
        words: w,
        data: out,
        limit: -1,
    })
}

/// Rotate right by `n` (left when negative).
///
/// # Safety
/// `l` is a `List` block nothing else borrows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_rotate(l: *mut List, n: i64) {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let (w, count) = (l.words, len(l));
    if count > 0 {
        let k = i128::from(n).rem_euclid(count as i128) as usize;
        l.data.rotate_right(k * w);
    }
}

/// All elements moved to a new `List` block, leaving the deque empty.
///
/// # Safety
/// `l` is a `List` block nothing else borrows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_take(l: *mut List) -> *mut List {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    leak(List {
        words: l.words,
        data: std::mem::take(&mut l.data),
        limit: -1,
    })
}

/// 1 when some element equals `x`, by the schema text `yan_rt::map` parses.
///
/// # Safety
/// `l` is a `List` block; `schema` a `Str` block; `x` holds an element's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_contains(
    l: *const List,
    schema: *const String,
    x: *const u64,
) -> i64 {
    // SAFETY: the caller's contract.
    let (l, schema) = unsafe { (&*l, &*schema) };
    let key: Key = parse(&mut schema.as_str());
    let w = l.words;
    // SAFETY: as above.
    let x = unsafe { std::slice::from_raw_parts(x, w) };
    i64::from(
        w > 0
            && l.data
                .chunks(w)
                // SAFETY: the elements are live and of the schema's shape.
                .any(|e| unsafe { key.cmp(e, x, &mut 0) } == Ordering::Equal),
    )
}

/// `, maxlen=N` for `show`, or the empty string.
///
/// # Safety
/// `l` is a `List` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_suffix(l: *const List) -> *mut String {
    // SAFETY: the caller's contract.
    let n = unsafe { (*l).limit };
    crate::abi::str_block(&if n >= 0 {
        format!(", maxlen={n}")
    } else {
        String::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deque(limit: i64, xs: &[u64]) -> List {
        List {
            words: 1,
            data: xs.to_vec(),
            limit,
        }
    }

    #[test]
    fn push_evicts_from_the_other_end() {
        let mut d = deque(2, &[1, 2]);
        let mut out = 0u64;
        // SAFETY: `d` is a live list of one-word elements and `out` has room for one.
        let (a, b) = unsafe { (yan_rt_dq_push(&mut d, 0, [3u64].as_ptr(), &mut out), out) };
        assert_eq!((a, b, d.data.clone()), (1, 1, vec![2, 3]));
        // SAFETY: as above.
        let a = unsafe { yan_rt_dq_push(&mut d, 1, [0u64].as_ptr(), &mut out) };
        assert_eq!((a, out, d.data), (1, 3, vec![0, 2]));
    }

    #[test]
    fn rotate_moves_the_tail_to_the_front() {
        let mut d = deque(-1, &[1, 2, 3, 4, 5]);
        // SAFETY: `d` is a live list.
        unsafe { yan_rt_dq_rotate(&mut d, 2) };
        assert_eq!(d.data, vec![4, 5, 1, 2, 3]);
        // SAFETY: as above.
        unsafe { yan_rt_dq_rotate(&mut d, -3) };
        assert_eq!(d.data, vec![2, 3, 4, 5, 1]);
    }
}
