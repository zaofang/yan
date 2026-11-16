//! `deque` for native code: a `List` block that also holds its `maxlen` (17.3, R488 to R493).
//! Evicted elements go back to the caller, who owns any blocks they hold.

use crate::abi::{List, leak};
use crate::map::{Key, parse};
use std::cmp::Ordering;

/// Push `x` on the back (`front` false) or front of `data`, elements of `w` slots. When full, the
/// element from the other end is returned (`x` itself when `limit` is 0).
pub fn push(data: &mut Vec<u64>, w: usize, limit: i64, front: bool, x: &[u64]) -> Option<Vec<u64>> {
    if limit == 0 {
        return Some(x.to_vec());
    }
    let mut evicted = None;
    if limit > 0 && data.len().checked_div(w).unwrap_or(0) as i64 >= limit {
        let at = if front { data.len() - w } else { 0 };
        evicted = Some(data.drain(at..at + w).collect());
    }
    if front {
        data.splice(0..0, x.iter().copied());
    } else {
        data.extend_from_slice(x);
    }
    evicted
}

/// Remove the first (`front`) or last element, or `None` when empty.
pub fn pop(data: &mut Vec<u64>, w: usize, front: bool) -> Option<Vec<u64>> {
    if data.is_empty() {
        return None;
    }
    let at = if front { 0 } else { data.len() - w };
    Some(data.drain(at..at + w).collect())
}

/// Append every element of `src`; the elements pushed out, in order.
pub fn extend(data: &mut Vec<u64>, w: usize, limit: i64, src: &[u64]) -> Vec<u64> {
    let mut out = Vec::new();
    if w > 0 {
        for x in src.chunks(w) {
            out.extend(push(data, w, limit, false, x).into_iter().flatten());
        }
    }
    out
}

/// Rotate right by `n` (left when negative).
pub fn rotate(data: &mut [u64], w: usize, n: i64) {
    let count = data.len().checked_div(w).unwrap_or(0);
    if count > 0 {
        let k = i128::from(n).rem_euclid(count as i128) as usize;
        data.rotate_right(k * w);
    }
}

/// `, maxlen=N` for `show`, or the empty string.
pub fn suffix(limit: i64) -> String {
    if limit >= 0 {
        format!(", maxlen={limit}")
    } else {
        String::new()
    }
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
    match push(&mut l.data, w, l.limit, front != 0, x) {
        Some(e) => {
            // SAFETY: `out` has room for an element.
            unsafe { std::ptr::copy_nonoverlapping(e.as_ptr(), out, w) };
            1
        }
        None => 0,
    }
}

/// Move the first (`front` 1) or last element to `out`: 1, or 0 when empty.
///
/// # Safety
/// `l` is a `List` block nothing else borrows; `out` has room for an element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dq_pop(l: *mut List, front: i64, out: *mut u64) -> i64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    match pop(&mut l.data, l.words, front != 0) {
        Some(e) => {
            // SAFETY: `out` has room for an element.
            unsafe { std::ptr::copy_nonoverlapping(e.as_ptr(), out, l.words) };
            1
        }
        None => 0,
    }
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
    leak(List {
        words: w,
        data: extend(&mut l.data, w, l.limit, &src.data),
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
    rotate(&mut l.data, l.words, n);
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
    crate::abi::str_block(&suffix(unsafe { (*l).limit }))
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
    fn core_push_pop_extend_respect_maxlen() {
        let mut d = vec![1, 2];
        assert_eq!(push(&mut d, 1, 3, false, &[3]), None);
        assert_eq!(push(&mut d, 1, 3, true, &[0]), Some(vec![3]));
        assert_eq!(d, vec![0, 1, 2]);
        assert_eq!(extend(&mut d, 1, 3, &[7, 8]), vec![0, 1]);
        assert_eq!(pop(&mut d, 1, true), Some(vec![2]));
        assert_eq!(push(&mut d, 1, 0, false, &[9]), Some(vec![9]));
        assert_eq!(suffix(-1), "");
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
