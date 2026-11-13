//! `bigint` blocks: a leaked `Vec<u8>` of the value's two's-complement bytes, little endian.
//! The encoding is canonical, so equal values have equal bytes.

use crate::abi::{leak, panic, str_block};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

fn get(b: *const Vec<u8>) -> BigInt {
    // SAFETY: callers pass `bigint` blocks.
    BigInt::from_signed_bytes_le(unsafe { &*b })
}

fn put(x: &BigInt) -> *mut Vec<u8> {
    leak(x.to_signed_bytes_le())
}

/// Operation codes shared with the lowering.
pub const ADD: i64 = 0;
pub const SUB: i64 = 1;
pub const MUL: i64 = 2;
pub const FLOORDIV: i64 = 3;
pub const MOD: i64 = 4;
pub const AND: i64 = 5;
pub const OR: i64 = 6;
pub const XOR: i64 = 7;

pub const EQ: i64 = 0;
pub const NE: i64 = 1;
pub const LT: i64 = 2;
pub const LE: i64 = 3;
pub const GT: i64 = 4;
pub const GE: i64 = 5;

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_big_from_int(x: i64) -> *mut Vec<u8> {
    put(&BigInt::from(x))
}

/// # Safety
/// `a`, `b` are `bigint` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_big_bin(
    op: i64,
    a: *const Vec<u8>,
    b: *const Vec<u8>,
) -> *mut Vec<u8> {
    let (a, b) = (get(a), get(b));
    let r = match op {
        ADD => a + b,
        SUB => a - b,
        MUL => a * b,
        FLOORDIV | MOD => {
            if b.is_zero() {
                panic("div_zero")
            }
            let (q, r) = (&a / &b, &a % &b);
            let adjust = !r.is_zero() && (r.is_negative() != b.is_negative());
            match (op, adjust) {
                (FLOORDIV, true) => q - 1,
                (FLOORDIV, false) => q,
                (_, true) => r + b,
                (_, false) => r,
            }
        }
        AND => a & b,
        OR => a | b,
        XOR => a ^ b,
        _ => unreachable!("bigint op {op}"),
    };
    put(&r)
}

/// # Safety
/// `a`, `b` are `bigint` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_big_cmp(op: i64, a: *const Vec<u8>, b: *const Vec<u8>) -> bool {
    let (a, b) = (get(a), get(b));
    match op {
        EQ => a == b,
        NE => a != b,
        LT => a < b,
        LE => a <= b,
        GT => a > b,
        GE => a >= b,
        _ => unreachable!("bigint comparison {op}"),
    }
}

/// `a << n` when `left` is 1, else `a >> n`; a negative count panics.
///
/// # Safety
/// `a` is a `bigint` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_big_shift(left: i64, a: *const Vec<u8>, n: i64) -> *mut Vec<u8> {
    let Ok(n) = usize::try_from(n) else {
        panic("overflow")
    };
    let a = get(a);
    put(&if left == 1 { a << n } else { a >> n })
}

/// # Safety
/// `a` is a `bigint` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_big_pow(a: *const Vec<u8>, n: i64) -> *mut Vec<u8> {
    let Ok(n) = u32::try_from(n) else {
        panic("overflow")
    };
    put(&num_traits::pow::Pow::pow(get(a), n))
}

/// # Safety
/// `a` is a `bigint` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_big_not(a: *const Vec<u8>) -> *mut Vec<u8> {
    put(&!get(a))
}

/// `count_ones` of the magnitude when `width` is 0, else `bit_width`.
///
/// # Safety
/// `a` is a `bigint` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_big_bits(width: i64, a: *const Vec<u8>) -> i64 {
    let a = get(a);
    if width == 0 {
        a.magnitude().count_ones().to_i64().expect("fits")
    } else {
        a.magnitude().bits().to_i64().expect("fits")
    }
}

/// # Safety
/// `a` is a `bigint` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_big_str(a: *const Vec<u8>) -> *mut String {
    str_block(&get(a).to_string())
}
