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
