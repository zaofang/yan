//! Scalar conversions.

/// `f` truncated toward zero, if that is an i64.
pub fn trunc_i64(f: f64) -> Option<i64> {
    in_i64(f.trunc())
}

/// `f` rounded half to even, if that is an i64.
pub fn round_i64(f: f64) -> Option<i64> {
    in_i64(f.round_ties_even())
}

/// `a / b` correctly rounded to f64 (3.4, SQ21). Converting both operands first rounds twice
/// once either exceeds 2^53.
pub fn div_i128(a: i128, b: i128) -> f64 {
    const EXACT: u128 = 1 << 53;
    let (n, d) = (a.unsigned_abs(), b.unsigned_abs());
    if b == 0 || (n <= EXACT && d <= EXACT) {
        return a as f64 / b as f64;
    }
    // 66 significant bits of n / d, the last one sticky, so the conversion below rounds once.
    let (mut m, mut r) = (n / d, n % d);
    let mut e = 0i32;
    if m >= 1 << 66 {
        let s = 128 - m.leading_zeros() - 66;
        let lost = m & ((1 << s) - 1);
        m >>= s;
        e = s as i32;
        if lost != 0 || r != 0 {
            m |= 1;
        }
    } else {
        while m < 1 << 65 {
            r <<= 1;
            let bit = r >= d;
            if bit {
                r -= d;
            }
            m = (m << 1) | u128::from(bit);
            e -= 1;
        }
        if r != 0 {
            m |= 1;
        }
    }
    let q = m as f64 * 2f64.powi(e);
    if (a < 0) != (b < 0) { -q } else { q }
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

#[cfg(test)]
mod tests {
    use super::div_i128;

    #[test]
    fn int_division_rounds_once() {
        // expected values are CPython's int / int, which is correctly rounded
        let cases: [(i128, i128, f64); 15] = [
            (27021597764222979, 3, 9007199254740992.0),
            (9007199254740993, 1, 9007199254740992.0),
            (9007199254740995, 1, 9007199254740996.0),
            (-9223372036854775808, -1, 9.223372036854776e+18),
            (9223372036854775807, 9223372036854775806, 1.0),
            (-4611686018427387905, 3, -1.5372286728091292e+18),
            (1, 3458764513820540928, 2.8912057932946783e-19),
            (
                170141183460469231731687303715884105727,
                3,
                5.671372782015641e+37,
            ),
            (
                -170141183460469231731687303715884105728,
                18446744073709551617,
                -9.223372036854776e+18,
            ),
            (6278314744523580143, 58621899350, 107098453.2084012),
            (-6329975792576927173, -955497375907, 6624796.626540431),
            (1363974157860798087, 239363611425, 5698335.472717303),
            (-4067832256650242452, -838204117599, 4853033.015755607),
            (-7896465698630544553, -142278089831, 55500222.89454464),
            (6869710717905637721, -1057909077179, -6493668.374813719),
        ];
        for (a, b, want) in cases {
            assert_eq!(div_i128(a, b), want, "{a} / {b}");
        }
        assert_ne!(
            27021597764222979f64 / 3.0,
            9007199254740992.0,
            "the case rounds twice"
        );
        assert_eq!(div_i128(7, 2), 3.5);
        assert_eq!(div_i128(1, 0), f64::INFINITY);
    }
}
