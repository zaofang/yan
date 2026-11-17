//! `heap` for native code: a `List` block kept as a 4-ary min-heap, ordered by the schema text
//! `yan_rt::map` parses (17.3, R488 to R493). The element order matches the interpreter's.
//! [`Heap`] is the pure core, shared with the machine, which reads blocks through its own memory.

use crate::abi::List;
use crate::map::parse;
use std::cmp::Ordering;

const ARITY: usize = 4;

/// A 4-ary min-heap over `words`-wide elements in `data`, ordered by `cmp`.
pub struct Heap<'a, C: Fn(&[u64], &[u64]) -> Ordering> {
    pub data: &'a mut Vec<u64>,
    pub words: usize,
    pub cmp: C,
}

impl<C: Fn(&[u64], &[u64]) -> Ordering> Heap<'_, C> {
    pub fn len(&self) -> usize {
        self.data.len().checked_div(self.words).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn slots(&self, i: usize) -> &[u64] {
        &self.data[i * self.words..(i + 1) * self.words]
    }

    fn cmp_with(&self, i: usize, x: &[u64]) -> Ordering {
        (self.cmp)(self.slots(i), x)
    }

    fn cmp_at(&self, i: usize, j: usize) -> Ordering {
        self.cmp_with(i, self.slots(j))
    }

    fn swap(&mut self, i: usize, j: usize) {
        let w = self.words;
        for k in 0..w {
            self.data.swap(i * w + k, j * w + k);
        }
    }

    /// Restore the order after the last element was appended.
    pub fn up_last(&mut self) {
        let mut i = self.len() - 1;
        while i > 0 {
            let p = (i - 1) / ARITY;
            if self.cmp_at(i, p) != Ordering::Less {
                break;
            }
            self.swap(i, p);
            i = p;
        }
    }

    fn down(&mut self, mut i: usize) {
        loop {
            let first = i * ARITY + 1;
            let Some(m) =
                (first..(first + ARITY).min(self.len())).min_by(|&a, &b| self.cmp_at(a, b))
            else {
                return;
            };
            if self.cmp_at(m, i) != Ordering::Less {
                return;
            }
            self.swap(i, m);
            i = m;
        }
    }

    /// Heapify in place.
    pub fn fix(&mut self) {
        for i in (0..self.len() / ARITY + 1).rev() {
            self.down(i);
        }
    }

    /// The least element's slots, or `None` when empty.
    pub fn pop(&mut self) -> Option<Vec<u64>> {
        let n = self.len();
        if n == 0 {
            return None;
        }
        self.swap(0, n - 1);
        let out = self.data.split_off((n - 1) * self.words);
        self.down(0);
        Some(out)
    }

    /// `x` pushed then the least popped, without growing the heap.
    pub fn pushpop(&mut self, x: &[u64]) -> Vec<u64> {
        if !self.is_empty() && self.cmp_with(0, x) == Ordering::Less {
            let out = self.slots(0).to_vec();
            self.data[..self.words].copy_from_slice(x);
            self.down(0);
            out
        } else {
            x.to_vec()
        }
    }

    /// Sort ascending (stable).
    pub fn sort(&mut self) {
        let mut order: Vec<usize> = (0..self.len()).collect();
        order.sort_by(|&a, &b| self.cmp_at(a, b));
        let sorted: Vec<u64> = order.iter().flat_map(|&i| self.slots(i).to_vec()).collect();
        *self.data = sorted;
    }
}

/// # Safety
/// `l` is a `List` block nothing else borrows; `schema` a `Str` block.
unsafe fn with<R>(
    l: *mut List,
    schema: *const String,
    f: impl FnOnce(&mut Heap<'_, &dyn Fn(&[u64], &[u64]) -> Ordering>) -> R,
) -> R {
    // SAFETY: the caller's contract.
    let (l, schema) = unsafe { (&mut *l, &*schema) };
    let key = parse(&mut schema.as_str());
    // SAFETY: the lowering passes a list of live elements of the schema's shape.
    let cmp = |a: &[u64], b: &[u64]| unsafe { key.cmp(a, b, &mut 0) };
    f(&mut Heap {
        words: l.words,
        data: &mut l.data,
        cmp: &cmp,
    })
}

/// Restore the order after the last element was appended.
///
/// # Safety
/// As [`with`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_heap_up(l: *mut List, schema: *const String) {
    // SAFETY: forwarded.
    unsafe { with(l, schema, |h| h.up_last()) }
}

/// Heapify in place.
///
/// # Safety
/// As [`with`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_heap_fix(l: *mut List, schema: *const String) {
    // SAFETY: forwarded.
    unsafe { with(l, schema, |h| h.fix()) }
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
        with(l, schema, |h| match h.pop() {
            Some(x) => {
                std::ptr::copy_nonoverlapping(x.as_ptr(), out, x.len());
                1
            }
            None => 0,
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
            let r = h.pushpop(std::slice::from_raw_parts(x, h.words));
            std::ptr::copy_nonoverlapping(r.as_ptr(), out, r.len());
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
    unsafe { with(l, schema, |h| h.sort()) }
}
