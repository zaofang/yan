//! std.collections kernels (R488 to R493): the 4-ary min-heap and sorted-sequence searches,
//! generic over the element order.

use std::cmp::Ordering;
use std::mem;

const ARITY: usize = 4;

pub fn sift_up<T>(h: &mut [T], mut i: usize, cmp: impl Fn(&T, &T) -> Ordering) {
    while i > 0 {
        let p = (i - 1) / ARITY;
        if cmp(&h[i], &h[p]) != Ordering::Less {
            break;
        }
        h.swap(i, p);
        i = p;
    }
}

pub fn sift_down<T>(h: &mut [T], mut i: usize, cmp: impl Fn(&T, &T) -> Ordering) {
    loop {
        let first = i * ARITY + 1;
        let Some(m) = (first..(first + ARITY).min(h.len())).min_by(|&a, &b| cmp(&h[a], &h[b]))
        else {
            return;
        };
        if cmp(&h[m], &h[i]) != Ordering::Less {
            return;
        }
        h.swap(i, m);
        i = m;
    }
}

pub fn heapify<T>(mut h: Vec<T>, cmp: impl Fn(&T, &T) -> Ordering) -> Vec<T> {
    for i in (0..h.len() / ARITY + 1).rev() {
        sift_down(&mut h, i, &cmp);
    }
    h
}

pub fn heap_pop<T>(h: &mut Vec<T>, cmp: impl Fn(&T, &T) -> Ordering) -> Option<T> {
    let last = h.pop()?;
    if h.is_empty() {
        return Some(last);
    }
    let top = mem::replace(&mut h[0], last);
    sift_down(h, 0, cmp);
    Some(top)
}

/// From a binary search's result, the last index at most the key.
pub fn floor(at: Result<usize, usize>) -> Option<usize> {
    at.map_or_else(|i| i.checked_sub(1), Some)
}

/// From a binary search's result over `n` items, the first index at least the key.
pub fn ceiling(n: usize, at: Result<usize, usize>) -> Option<usize> {
    let i = at.unwrap_or_else(|i| i);
    (i < n).then_some(i)
}

/// The index range `lo <= x < hi` from the searches for `lo` and `hi`; empty when `hi < lo`.
pub fn range(lo: Result<usize, usize>, hi: Result<usize, usize>) -> (usize, usize) {
    let a = lo.unwrap_or_else(|i| i);
    (a, hi.unwrap_or_else(|i| i).max(a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_pops_in_order() {
        let mut h = heapify(vec![5, 3, 9, 1, 7, 2, 8, 6, 4, 0, 11, 10], i32::cmp);
        let mut out = Vec::new();
        while let Some(v) = heap_pop(&mut h, i32::cmp) {
            out.push(v);
        }
        assert_eq!(out, (0..12).collect::<Vec<_>>());
    }

    #[test]
    fn floor_and_ceiling_at_the_edges() {
        let s = [2, 4, 6];
        let at = |x| s.binary_search(&x);
        assert_eq!(floor(at(1)), None);
        assert_eq!(floor(at(5)), Some(1));
        assert_eq!(floor(at(6)), Some(2));
        assert_eq!(ceiling(3, at(5)), Some(2));
        assert_eq!(ceiling(3, at(7)), None);
        assert_eq!(range(at(3), at(1)), (1, 1));
    }
}
