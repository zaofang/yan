//! Nearest-rounded `std.math` kernels called by index; the order is `yan_rt::math::MATH1`/`MATH2`.

use crate::Operand;
use yan_core::ast as y;
use yan_std::registry::Id;
use yan_std::registry::id::*;

pub(super) static MATH1: &[Id] = &[
    STD_MATH_EXP,
    STD_MATH_EXP2,
    STD_MATH_EXP10,
    STD_MATH_EXPM1,
    STD_MATH_EXP2M1,
    STD_MATH_EXP10M1,
    STD_MATH_LOG,
    STD_MATH_LOG2,
    STD_MATH_LOG10,
    STD_MATH_LOG1P,
    STD_MATH_LOG2P1,
    STD_MATH_LOG10P1,
    STD_MATH_CBRT,
    STD_MATH_RSQRT,
    STD_MATH_SIN,
    STD_MATH_COS,
    STD_MATH_TAN,
    STD_MATH_SINPI,
    STD_MATH_COSPI,
    STD_MATH_TANPI,
    STD_MATH_ASIN,
    STD_MATH_ACOS,
    STD_MATH_ATAN,
    STD_MATH_ASINPI,
    STD_MATH_ACOSPI,
    STD_MATH_ATANPI,
    STD_MATH_SINH,
    STD_MATH_COSH,
    STD_MATH_TANH,
    STD_MATH_ASINH,
    STD_MATH_ACOSH,
    STD_MATH_ATANH,
    STD_MATH_ERF,
    STD_MATH_ERFC,
    STD_MATH_LGAMMA,
    STD_MATH_TGAMMA,
    STD_MATH_SQRT,
];

pub(super) static MATH2: &[Id] = &[
    STD_MATH_POWR,
    STD_MATH_HYPOT,
    STD_MATH_ATAN2,
    STD_MATH_ATAN2PI,
    STD_MATH_REMAINDER,
    STD_MATH_DIV,
];

/// The `yan_rt::math::rounded` op and operand count of a `std.math` function with a `Round` mode.
pub(super) fn rounded_op(id: Id) -> Option<(i128, usize)> {
    Some(match id {
        STD_MATH_ADD => (0, 2),
        STD_MATH_SUB => (1, 2),
        STD_MATH_MUL => (2, 2),
        STD_MATH_DIV => (3, 2),
        STD_MATH_FMA => (4, 3),
        STD_MATH_SQRT => (5, 1),
        _ => return None,
    })
}

/// The tag of a literal `math.Round` variant.
pub(super) fn round_tag(a: &y::Atom) -> Option<Operand> {
    let y::Atom::Name(n, _) = a else { return None };
    let tag = ["NearestEven", "Up", "Down", "TowardZero"]
        .iter()
        .position(|v| v == n)?;
    Some(Operand::Int(tag as i128))
}
