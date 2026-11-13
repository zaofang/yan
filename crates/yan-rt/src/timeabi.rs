//! C-ABI entry points of `Duration` and `Instant` for native code. Both are 128-bit
//! nanosecond counts held as two words, `hi` (signed) then `lo` (unsigned) (R593 to R597).

use crate::abi::{panic, str_block};
use crate::time::{
    NS, div_half_even, floor_div, in_duration, in_instant, show_duration, show_instant,
};
use std::cell::Cell;

pub const DADD: i64 = 0;
pub const DSUB: i64 = 1;
pub const DMUL: i64 = 2;
pub const DABS: i64 = 3;
pub const DNEG: i64 = 4;
pub const DFLOORDIV: i64 = 5;
pub const DREM: i64 = 6;
pub const DDIV: i64 = 7;
pub const DQUOT: i64 = 8;
pub const IADD: i64 = 9;
pub const ISUB: i64 = 10;
pub const ISINCE: i64 = 11;
pub const UNIT: i64 = 12;
pub const FROM_UNIX_NANOS: i64 = 13;
pub const FROM_UNIX_SECONDS: i64 = 14;
pub const UNIX_SECONDS: i64 = 15;
pub const NANOS: i64 = 16;
pub const SECONDS: i64 = 17;

thread_local! {
    static OUT: Cell<[i64; 3]> = const { Cell::new([0; 3]) };
}

fn join(hi: i64, lo: i64) -> i128 {
    (i128::from(hi) << 64) | i128::from(lo as u64)
}

/// A result `v` as the words `hi`, `lo` and whether it was in range.
fn put(v: Option<i128>) -> *const i64 {
    let n = v.unwrap_or(0);
    OUT.with(|c| {
        c.set([(n >> 64) as i64, n as i64, i64::from(v.is_some())]);
        c.as_ptr().cast::<i64>().cast_const()
    })
}

fn int(n: i128) -> Option<i128> {
    i64::try_from(n).ok().map(i128::from)
}

fn zero_check(d: i128) {
    if d == 0 {
        panic("div_zero");
    }
}

/// `a op b` on the time values `(ahi, alo)` and `(bhi, blo)`; `b` is a plain `int` for the
/// operations that take one. Returns `[hi, lo, ok]`, valid until the next call; `ok` is 0 when
/// the result is out of range.
///
/// # Safety
/// None beyond the C ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_time(
    op: i64,
    ahi: i64,
    alo: i64,
    bhi: i64,
    blo: i64,
) -> *const i64 {
    let (a, b) = (join(ahi, alo), join(bhi, blo));
    put(match op {
        DADD => a.checked_add(b).and_then(in_duration),
        DSUB => a.checked_sub(b).and_then(in_duration),
        DMUL => a.checked_mul(b).and_then(in_duration),
        DABS => Some(a.abs()),
        DNEG => Some(-a),
        DFLOORDIV => {
            zero_check(b);
            Some(floor_div(a, b))
        }
        DREM => {
            zero_check(b);
            Some(a - b * floor_div(a, b))
        }
        DDIV => {
            zero_check(b);
            Some(div_half_even(a, b))
        }
        DQUOT => {
            zero_check(b);
            int(floor_div(a, b))
        }
        IADD => in_instant(a + b),
        ISUB => in_instant(a - b),
        ISINCE => Some(a - b),
        UNIT => a.checked_mul(b).and_then(in_duration),
        FROM_UNIX_NANOS => in_instant(a),
        FROM_UNIX_SECONDS => a.checked_mul(NS).and_then(in_instant),
        UNIX_SECONDS => int(a.div_euclid(NS)),
        NANOS => Some(a),
        SECONDS => int(a / NS),
        _ => unreachable!("time op {op}"),
    })
}

/// `as_seconds_float` (0) and `ratio` (1).
///
/// # Safety
/// None beyond the C ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_time_f64(op: i64, ahi: i64, alo: i64, bhi: i64, blo: i64) -> f64 {
    let (a, b) = (join(ahi, alo), join(bhi, blo));
    if op == 0 {
        return a as f64 / 1e9;
    }
    zero_check(b);
    a as f64 / b as f64
}

/// The text of a `Duration` (`kind` 0), an `Instant` (1) or an `i128` (2).
///
/// # Safety
/// None beyond the C ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_time_show(kind: i64, hi: i64, lo: i64) -> *mut String {
    let n = join(hi, lo);
    str_block(&match kind {
        0 => show_duration(n),
        1 => show_instant(n),
        _ => n.to_string(),
    })
}
