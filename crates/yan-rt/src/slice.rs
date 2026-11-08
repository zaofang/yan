//! `xs[a:b:c]` for native code (N50): Python's clamped slice. Each bound arrives as an
//! `Option[int]`'s tag (0 is `Some`) and value.

use crate::abi::{List, leak, yan_rt_panic};
use crate::list::slice_indices;

fn bound(tag: i64, v: i64) -> Option<i128> {
    (tag == 0).then_some(i128::from(v))
}

fn indices(
    len: usize,
    (a, av): (i64, i64),
    (b, bv): (i64, i64),
    (c, cv): (i64, i64),
) -> impl Iterator<Item = usize> {
    let step = bound(c, cv).unwrap_or(1);
    if step == 0 {
        let kind = "value";
        // SAFETY: a str is valid UTF-8.
        unsafe { yan_rt_panic(kind.as_ptr(), kind.len()) }
    }
    slice_indices(len, bound(a, av), bound(b, bv), step)
}

/// The chars of `s` picked by the slice, as a new `Str` block.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn yan_rt_str_slice(
    s: *const String,
    at: i64,
    av: i64,
    bt: i64,
    bv: i64,
    ct: i64,
    cv: i64,
) -> *mut String {
    // SAFETY: the caller's contract.
    let cs: Vec<char> = unsafe { &*s }.chars().collect();
    let out: String = indices(cs.len(), (at, av), (bt, bv), (ct, cv))
        .map(|k| cs[k])
        .collect();
    crate::abi::str_block(&out)
}

/// The elements picked by the slice, shallow-copied into a new `List` block.
///
/// # Safety
/// `l` is a `List` block.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn yan_rt_list_slice(
    l: *const List,
    at: i64,
    av: i64,
    bt: i64,
    bv: i64,
    ct: i64,
    cv: i64,
) -> *mut List {
    // SAFETY: the caller's contract.
    let l = unsafe { &*l };
    let w = l.words;
    let n = l.data.len().checked_div(w).unwrap_or(0);
    let data = indices(n, (at, av), (bt, bv), (ct, cv))
        .flat_map(|k| l.data[k * w..(k + 1) * w].iter().copied())
        .collect();
    leak(List {
        words: w,
        data,
        limit: -1,
    })
}
