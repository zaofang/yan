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
