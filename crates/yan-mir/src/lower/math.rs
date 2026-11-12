//! Nearest-rounded `std.math` kernels called by index; the order is `yan_rt::math::MATH1`/`MATH2`.

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
];
