//! `heap` for native code: a `List` block kept as a 4-ary min-heap, ordered by the schema text
//! `yan_rt::map` parses (17.3, R488 to R493). The element order matches the interpreter's.

use crate::abi::List;
use crate::map::{Key, parse};
use std::cmp::Ordering;

const ARITY: usize = 4;

struct Heap<'a> {
    key: Key,
    l: &'a mut List,
}

impl Heap<'_> {
    fn len(&self) -> usize {
        self.l.data.len().checked_div(self.l.words).unwrap_or(0)
    }

    fn slots(&self, i: usize) -> &[u64] {
        &self.l.data[i * self.l.words..(i + 1) * self.l.words]
    }

    fn cmp_with(&self, i: usize, x: &[u64]) -> Ordering {
        // SAFETY: the lowering passes a list of live elements of the schema's shape.
        unsafe { self.key.cmp(self.slots(i), x, &mut 0) }
    }

    fn cmp(&self, i: usize, j: usize) -> Ordering {
        self.cmp_with(i, self.slots(j))
    }

    fn swap(&mut self, i: usize, j: usize) {
        let w = self.l.words;
        for k in 0..w {
            self.l.data.swap(i * w + k, j * w + k);
        }
    }

    fn up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / ARITY;
            if self.cmp(i, p) != Ordering::Less {
                break;
            }
            self.swap(i, p);
            i = p;
        }
    }

    fn down(&mut self, mut i: usize) {
        loop {
            let first = i * ARITY + 1;
            let Some(m) = (first..(first + ARITY).min(self.len())).min_by(|&a, &b| self.cmp(a, b))
            else {
                return;
            };
            if self.cmp(m, i) != Ordering::Less {
                return;
            }
            self.swap(i, m);
            i = m;
        }
    }
}

/// # Safety
/// `l` is a `List` block nothing else borrows; `schema` a `Str` block.
unsafe fn with<R>(l: *mut List, schema: *const String, f: impl FnOnce(&mut Heap) -> R) -> R {
    // SAFETY: the caller's contract.
    let (l, schema) = unsafe { (&mut *l, &*schema) };
    f(&mut Heap {
        key: parse(&mut schema.as_str()),
        l,
    })
}

/// Restore the order after the last element was appended.
///
/// # Safety
/// As [`with`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_heap_up(l: *mut List, schema: *const String) {
    // SAFETY: forwarded.
    unsafe {
        with(l, schema, |h| {
            let n = h.len();
            h.up(n - 1)
        })
    }
}

/// Heapify in place.
///
/// # Safety
/// As [`with`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_heap_fix(l: *mut List, schema: *const String) {
    // SAFETY: forwarded.
    unsafe {
        with(l, schema, |h| {
            for i in (0..h.len() / ARITY + 1).rev() {
                h.down(i);
            }
        })
    }
}

/// Move the least element's slots into `out`: 1, or 0 when the heap is empty.
///
/// # Safety
/// As [`with`]; `out` has room for an element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_heap_pop(
    l: *mut List,
    schema: *const String,
    out: *mut u64,
) -> i64 {
    // SAFETY: forwarded.
    unsafe {
        with(l, schema, |h| {
            let n = h.len();
            if n == 0 {
                return 0;
            }
            h.swap(0, n - 1);
            let w = h.l.words;
            let at = (n - 1) * w;
            std::ptr::copy_nonoverlapping(h.l.data[at..].as_ptr(), out, w);
            h.l.data.truncate(at);
            h.down(0);
            1
        })
    }
}

/// `x` pushed then the least popped into `out`, without growing the heap.
///
/// # Safety
/// As [`yan_rt_heap_pop`]; `x` is an element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_heap_pushpop(
    l: *mut List,
    schema: *const String,
    x: *const u64,
    out: *mut u64,
) {
    // SAFETY: forwarded.
    unsafe {
        with(l, schema, |h| {
            let w = h.l.words;
            let xs = std::slice::from_raw_parts(x, w);
            if h.len() > 0 && h.cmp_with(0, xs) == Ordering::Less {
                std::ptr::copy_nonoverlapping(h.l.data.as_ptr(), out, w);
                std::ptr::copy_nonoverlapping(x, h.l.data.as_mut_ptr(), w);
                h.down(0);
            } else {
                std::ptr::copy_nonoverlapping(x, out, w);
            }
        })
    }
}

/// Sort ascending in place (stable).
///
/// # Safety
/// As [`with`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_heap_sort(l: *mut List, schema: *const String) {
    // SAFETY: forwarded.
    unsafe {
        with(l, schema, |h| {
            let (n, w) = (h.len(), h.l.words);
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|&a, &b| h.cmp(a, b));
            let sorted: Vec<u64> = order.iter().flat_map(|&i| h.slots(i).to_vec()).collect();
            debug_assert_eq!(sorted.len(), n * w);
            h.l.data = sorted;
        })
    }
}
