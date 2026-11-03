//! Bit operators and the `std.bits` functions on fixed-width integers and `bigint`, as
//! `<op>_<type>` intrinsics. Fixed-width values are their two's-complement pattern.

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::{R, Value, int_range, panic};

fn width(ty: &str) -> Option<u32> {
    Some(match ty {
        "i8" | "u8" => 8,
        "i16" | "u16" => 16,
        "i32" | "u32" => 32,
        "i64" | "u64" | "Addr" => 64,
        "i128" => 128,
        _ => return None,
    })
}

fn mask(w: u32) -> u128 {
    u128::MAX >> (128 - w)
}

fn pattern(a: i128, w: u32) -> u128 {
    a as u128 & mask(w)
}

fn from_pattern(p: u128, w: u32, signed: bool) -> i128 {
    if signed && w < 128 && (p >> (w - 1)) & 1 == 1 {
        (p | !mask(w)) as i128
    } else {
        p as i128
    }
}

/// A shift amount in `0..w`, or an `overflow` panic.
fn amount(n: i128, w: u32) -> R<u32> {
    match u32::try_from(n) {
        Ok(n) if n < w => Ok(n),
        _ => panic("overflow"),
    }
}

/// `op` on fixed-width integer type `ty`; `None` when `op` is not a bit operation.
pub(crate) fn int(op: &str, ty: &str, args: &[Value]) -> Option<R<Value>> {
    let w = width(ty)?;
    let signed = ty.starts_with('i');
    let int = |p: u128| Ok(Value::Int(from_pattern(p, w, signed)));
    let count = |n: u32| Ok(Value::Int(n.into()));
    Some(match (op, args) {
        ("shl", [Value::Int(a), Value::Int(n)]) => amount(*n, w).and_then(|n| {
            let r = a.wrapping_shl(n);
            let (lo, hi) = int_range(ty).expect("an integer type");
            if r >> n != *a || r < lo || r > hi {
                panic("overflow")
            } else {
                Ok(Value::Int(r))
            }
        }),
        ("shr", [Value::Int(a), Value::Int(n)]) => amount(*n, w).map(|n| Value::Int(a >> n)),
        ("wrapping_shl", [Value::Int(a), Value::Int(n)]) => {
            amount(*n, w).and_then(|n| int(pattern(*a, w) << n & mask(w)))
        }
        ("rotate_left" | "rotate_right", [Value::Int(a), Value::Int(n)]) => {
            let n = n.rem_euclid(w.into()) as u32;
            let n = if op == "rotate_left" { n } else { (w - n) % w };
            let p = pattern(*a, w);
            int(if n == 0 {
                p
            } else {
                (p << n | p >> (w - n)) & mask(w)
            })
        }
        ("not", [Value::Int(a)]) => int(!pattern(*a, w) & mask(w)),
        ("swap_bytes", [Value::Int(a)]) => int(pattern(*a, w).swap_bytes() >> (128 - w)),
        ("reverse_bits", [Value::Int(a)]) => int(pattern(*a, w).reverse_bits() >> (128 - w)),
        ("count_ones", [Value::Int(a)]) => count(pattern(*a, w).count_ones()),
        ("count_zeros", [Value::Int(a)]) => count(w - pattern(*a, w).count_ones()),
        ("leading_zeros", [Value::Int(a)]) => count(pattern(*a, w).leading_zeros() - (128 - w)),
        ("trailing_zeros", [Value::Int(a)]) => count(pattern(*a, w).trailing_zeros().min(w)),
        ("bit_width", [Value::Int(a)]) => count(128 - pattern(*a, w).leading_zeros()),
        ("is_power_of_two", [Value::Int(a)]) => Ok(Value::Bool(*a > 0 && a & (a - 1) == 0)),
        _ => return None,
    })
}

/// `op` on `bigint`, as Python's infinite two's complement.
pub(crate) fn bigint(op: &str, args: &[Value]) -> Option<R<Value>> {
    let shift = |n: &i128| match usize::try_from(*n) {
        Ok(n) => Ok(n),
        Err(_) => panic("overflow"),
    };
    let big = |b: BigInt| Ok(Value::BigInt(b));
    Some(match (op, args) {
        ("and", [Value::BigInt(a), Value::BigInt(b)]) => big(a & b),
        ("or", [Value::BigInt(a), Value::BigInt(b)]) => big(a | b),
        ("xor", [Value::BigInt(a), Value::BigInt(b)]) => big(a ^ b),
        ("shl", [Value::BigInt(a), Value::Int(n)]) => shift(n).and_then(|n| big(a << n)),
        ("shr", [Value::BigInt(a), Value::Int(n)]) => shift(n).and_then(|n| big(a >> n)),
        ("not", [Value::BigInt(a)]) => big(!a),
        ("count_ones", [Value::BigInt(a)]) => Ok(Value::Int(a.magnitude().count_ones().into())),
        ("bit_width", [Value::BigInt(a)]) => Ok(Value::Int(if a.is_zero() {
            0
        } else {
            a.abs().bits().to_i128().expect("fits")
        })),
        _ => return None,
    })
}

/// The generic `std.bits` declarations. The elaborator lowers every call to the typed
/// `<f>_<type>` operation, so these run only for a call it could not type.
pub(crate) static BINDS: &[(yong_std::registry::Id, crate::bind::Bind)] = {
    use crate::bind::Bind;
    use yong_std::registry::id::*;
    const UNTYPED: Bind = Bind::Plain(|_| crate::unsupported("a `bits` call without a known type"));
    &[
        (STD_BITS_COUNT_ONES, UNTYPED),
        (STD_BITS_COUNT_ZEROS, UNTYPED),
        (STD_BITS_LEADING_ZEROS, UNTYPED),
        (STD_BITS_TRAILING_ZEROS, UNTYPED),
        (STD_BITS_BIT_WIDTH, UNTYPED),
        (STD_BITS_ROTATE_LEFT, UNTYPED),
        (STD_BITS_ROTATE_RIGHT, UNTYPED),
        (STD_BITS_SWAP_BYTES, UNTYPED),
        (STD_BITS_REVERSE_BITS, UNTYPED),
        (STD_BITS_IS_POWER_OF_TWO, UNTYPED),
        (STD_BITS_WRAPPING_SHL, UNTYPED),
    ]
};
