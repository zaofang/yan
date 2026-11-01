//! Scalar conversions.

/// `f` truncated toward zero, if that is an i64.
pub fn trunc_i64(f: f64) -> Option<i64> {
    in_i64(f.trunc())
}

/// `f` rounded half to even, if that is an i64.
pub fn round_i64(f: f64) -> Option<i64> {
    in_i64(f.round_ties_even())
}

fn in_i64(t: f64) -> Option<i64> {
    (!t.is_nan() && t >= i64::MIN as f64 && t < i64::MAX as f64).then_some(t as i64)
}

/// The sum of `xs` in the reduction order of 9.3 (R382): 64 lanes per 4096-element block, then a
/// fixed tree, so the result does not depend on the target.
pub fn sum_f64(xs: &[f64]) -> f64 {
    let blocks: Vec<f64> = xs
        .chunks(4096)
        .map(|b| {
            let mut lanes = [-0.0f64; 64];
            for (i, x) in b.iter().enumerate() {
                lanes[i % 64] += x;
            }
            tree(&lanes)
        })
        .collect();
    if blocks.is_empty() {
        -0.0
    } else {
        tree(&blocks)
    }
}

/// The fixed tree: split at the largest power of two below the length.
fn tree(xs: &[f64]) -> f64 {
    match xs.len() {
        1 => xs[0],
        n => {
            let k = 1 << (usize::BITS - 1 - (n - 1).leading_zeros());
            tree(&xs[..k]) + tree(&xs[k..])
        }
    }
}
