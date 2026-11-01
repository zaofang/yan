//! std.math over interpreter values; the kernels are in `yong_rt::math` (R561).

use yong_rt::math::{self as m, Mode};
use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{R, Value, panic, unsupported};
use Value::{Bool, Float, Int, Tuple};

fn mode(v: &Value) -> Mode {
    match v {
        Value::Data { name, .. } => match &**name {
            "Up" => Mode::Up,
            "Down" => Mode::Down,
            "TowardZero" => Mode::Zero,
            _ => Mode::Nearest,
        },
        _ => Mode::Nearest,
    }
}

fn floats(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::List(xs) => xs
            .iter()
            .map(|x| match x {
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn nearest_only(id: Id, r: &Value) -> R<()> {
    if mode(r) == Mode::Nearest {
        Ok(())
    } else {
        let name = yong_std::registry::entry(id).path;
        unsupported(format!("directed rounding in `{name}` before codegen"))
    }
}

macro_rules! f {
    ($id:ident, |$a:ident| $pat:pat => $e:expr) => {
        (
            $id,
            Bind::Plain(|$a| match &*$a {
                $pat => Ok($e),
                #[allow(unreachable_patterns)]
                _ => bad_args($id),
            }),
        )
    };
}

/// A CORE-MATH function of one argument, rounded to nearest only before codegen.
macro_rules! un {
    ($id:ident, $f:path) => {
        f!($id, |a| [Float(x), r] => {
            nearest_only($id, r)?;
            Float($f(*x))
        })
    };
}

macro_rules! bin {
    ($id:ident, $f:path) => {
        f!($id, |a| [Float(x), Float(y), r] => {
            nearest_only($id, r)?;
            Float($f(*x, *y))
        })
    };
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    f!(STD_MATH_ADD, |a| [Float(x), Float(y), r] => {
        Float(m::exact(x + y, &[*x, *y], &[(*x, 1.0), (*y, 1.0)], mode(r)))
    }),
    f!(STD_MATH_SUB, |a| [Float(x), Float(y), r] => {
        Float(m::exact(x - y, &[*x, *y], &[(*x, 1.0), (-*y, 1.0)], mode(r)))
    }),
    f!(STD_MATH_MUL, |a| [Float(x), Float(y), r] => {
        Float(m::exact(x * y, &[*x, *y], &[(*x, *y)], mode(r)))
    }),
    f!(STD_MATH_DIV, |a| [Float(x), Float(y), r] => Float(m::div(*x, *y, mode(r)))),
    f!(STD_MATH_FMA, |a| [Float(x), Float(y), Float(z), r] => Float(m::exact(
        x.mul_add(*y, *z),
        &[*x, *y, *z],
        &[(*x, *y), (*z, 1.0)],
        mode(r),
    ))),
    f!(STD_MATH_SQRT, |a| [Float(x), r] => Float(m::sqrt(*x, mode(r)))),
    f!(STD_MATH_SUM_EXACT, |a| [xs, r] => match floats(xs) {
        Some(xs) => Float(m::dot(xs.into_iter().map(|x| (x, 1.0)), mode(r))),
        None => return bad_args(STD_MATH_SUM_EXACT),
    }),
    f!(STD_MATH_DOT_EXACT, |a| [xs, ys, r] => match (floats(xs), floats(ys)) {
        (Some(xs), Some(ys)) if xs.len() == ys.len() => {
            Float(m::dot(xs.into_iter().zip(ys), mode(r)))
        }
        (Some(_), Some(_)) => return panic("value"),
        _ => return bad_args(STD_MATH_DOT_EXACT),
    }),
    f!(STD_MATH_POWN, |a| [Float(x), Int(n), r] => {
        nearest_only(STD_MATH_POWN, r)?;
        Float(m::pown(*x, *n))
    }),
    f!(STD_MATH_FLOOR, |a| [Float(x)] => Float(x.floor())),
    f!(STD_MATH_CEIL, |a| [Float(x)] => Float(x.ceil())),
    f!(STD_MATH_TRUNC, |a| [Float(x)] => Float(x.trunc())),
    f!(STD_MATH_FMOD, |a| [Float(x), Float(y)] => Float(x % y)),
    f!(STD_MATH_REMAINDER, |a| [Float(x), Float(y)] => Float(m::remainder(*x, *y))),
    f!(STD_MATH_COPYSIGN, |a| [Float(x), Float(y)] => Float(x.copysign(*y))),
    f!(STD_MATH_LDEXP, |a| [Float(x), Int(n)] => Float(m::ldexp(*x, *n))),
    f!(STD_MATH_NEXT_UP, |a| [Float(x)] => Float(x.next_up())),
    f!(STD_MATH_NEXT_DOWN, |a| [Float(x)] => Float(x.next_down())),
    f!(STD_MATH_FREXP, |a| [Float(x)] => {
        let (mant, e) = m::frexp(*x);
        Tuple(vec![Float(mant), Int(e.into())])
    }),
    f!(STD_MATH_IS_NAN, |a| [Float(x)] => Bool(x.is_nan())),
    f!(STD_MATH_IS_INF, |a| [Float(x)] => Bool(x.is_infinite())),
    f!(STD_MATH_IS_FINITE, |a| [Float(x)] => Bool(x.is_finite())),
    un!(STD_MATH_EXP, m::exp),
    un!(STD_MATH_EXP2, m::exp2),
    un!(STD_MATH_EXP10, m::exp10),
    un!(STD_MATH_EXPM1, m::expm1),
    un!(STD_MATH_EXP2M1, m::exp2m1),
    un!(STD_MATH_EXP10M1, m::exp10m1),
    un!(STD_MATH_LOG, m::log),
    un!(STD_MATH_LOG2, m::log2),
    un!(STD_MATH_LOG10, m::log10),
    un!(STD_MATH_LOG1P, m::log1p),
    un!(STD_MATH_LOG2P1, m::log2p1),
    un!(STD_MATH_LOG10P1, m::log10p1),
    un!(STD_MATH_CBRT, m::cbrt),
    un!(STD_MATH_RSQRT, m::rsqrt),
    un!(STD_MATH_SIN, m::sin),
    un!(STD_MATH_COS, m::cos),
    un!(STD_MATH_TAN, m::tan),
    un!(STD_MATH_SINPI, m::sinpi),
    un!(STD_MATH_COSPI, m::cospi),
    un!(STD_MATH_TANPI, m::tanpi),
    un!(STD_MATH_ASIN, m::asin),
    un!(STD_MATH_ACOS, m::acos),
    un!(STD_MATH_ATAN, m::atan),
    un!(STD_MATH_ASINPI, m::asinpi),
    un!(STD_MATH_ACOSPI, m::acospi),
    un!(STD_MATH_ATANPI, m::atanpi),
    un!(STD_MATH_SINH, m::sinh),
    un!(STD_MATH_COSH, m::cosh),
    un!(STD_MATH_TANH, m::tanh),
    un!(STD_MATH_ASINH, m::asinh),
    un!(STD_MATH_ACOSH, m::acosh),
    un!(STD_MATH_ATANH, m::atanh),
    un!(STD_MATH_ERF, m::erf),
    un!(STD_MATH_ERFC, m::erfc),
    un!(STD_MATH_LGAMMA, m::lgamma),
    un!(STD_MATH_TGAMMA, m::tgamma),
    bin!(STD_MATH_POW, m::pow),
    bin!(STD_MATH_POWR, m::powr),
    bin!(STD_MATH_HYPOT, m::hypot),
    bin!(STD_MATH_ATAN2, m::atan2),
    bin!(STD_MATH_ATAN2PI, m::atan2pi),
];
