//! std.list kernels.

/// `index` as Python's `list.insert` reads it: negative from the end, clamped to `0..=len`.
pub fn insert_at(index: i128, len: usize) -> usize {
    let n = len as i128;
    (if index < 0 {
        (n + index).max(0)
    } else {
        index.min(n)
    }) as usize
}

/// The indices Python's `xs[start:stop:step]` takes from a sequence of `len`, clamped as
/// `slice.indices` does. `step` is not 0.
pub fn slice_indices(
    len: usize,
    start: Option<i128>,
    stop: Option<i128>,
    step: i128,
) -> impl Iterator<Item = usize> {
    let n = len as i128;
    let clamp = |i: i128, lo: i128, hi: i128| {
        if i < 0 { (i + n).max(lo) } else { i.min(hi) }
    };
    let (lo, hi) = if step > 0 { (0, n) } else { (-1, n - 1) };
    let start = start.map_or(if step > 0 { lo } else { hi }, |i| clamp(i, lo, hi));
    let stop = stop.map_or(if step > 0 { hi } else { lo }, |i| clamp(i, lo, hi));
    let mut i = start;
    std::iter::from_fn(move || {
        let more = if step > 0 { i < stop } else { i > stop };
        more.then(|| {
            let k = i as usize;
            i += step;
            k
        })
    })
}
