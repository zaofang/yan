//! std.math (R500 onward). Transcendentals are CORE-MATH, correctly rounded to nearest. The
//! exact operations round in every mode by computing the exact value as a big integer times a
//! power of two and rounding it once.

use std::cmp::Ordering;

use num_bigint::BigUint;
use num_traits::{One, ToPrimitive, Zero};

use super::{R, Value, panic, unsupported};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Nearest,
    Up,
    Down,
    Zero,
}

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

/// A finite nonzero `x` as (negative, m, e) with `|x| = m * 2^e`.
fn parts(x: f64) -> (bool, u64, i64) {
    let b = x.to_bits();
    let exp = ((b >> 52) & 0x7ff) as i64;
    let frac = b & ((1 << 52) - 1);
    let (m, e) = if exp == 0 {
        (frac, -1074)
    } else {
        (frac | 1 << 52, exp - 1075)
    };
    (x.is_sign_negative(), m, e)
}

fn pow2(q: i64) -> f64 {
    if q >= -1022 {
        f64::from_bits(((q + 1023) as u64) << 52)
    } else {
        f64::from_bits(1 << (q + 1074))
    }
}

/// `m * 2^e`, plus something in `(0, 2^e)` when `sticky`, rounded to f64 in `mode`. Callers
/// with `sticky` pass at least 54 bits of `m`, so the sticky part is below half an ulp.
fn round(neg: bool, m: &BigUint, e: i64, sticky: bool, mode: Mode) -> f64 {
    let q = (e + m.bits() as i64 - 53).max(-1074);
    let (mut kept, half, inexact) = if q > e {
        let shift = (q - e) as usize;
        let kept = m >> shift;
        let rem = m - (&kept << shift);
        let half = rem.cmp(&(BigUint::one() << (shift - 1)));
        let half = if half.is_eq() && sticky {
            Ordering::Greater
        } else {
            half
        };
        (kept, half, sticky || !rem.is_zero())
    } else {
        (m << (e - q) as usize, Ordering::Less, false)
    };
    let away = match mode {
        Mode::Nearest => half.is_gt() || (half.is_eq() && kept.bit(0)),
        Mode::Up => inexact && !neg,
        Mode::Down => inexact && neg,
        Mode::Zero => false,
    };
    if away {
        kept += 1u32;
    }
    let sign = if neg { -1.0 } else { 1.0 };
    if kept.bits() as i64 + q > 1024 {
        let to_inf = match mode {
            Mode::Nearest => true,
            Mode::Up => !neg,
            Mode::Down => neg,
            Mode::Zero => false,
        };
        return sign * if to_inf { f64::INFINITY } else { f64::MAX };
    }
    sign * kept.to_f64().unwrap() * pow2(q)
}

/// `sum(x * y for x, y in terms)` exactly, rounded once, with the zero signs and special values
/// of IEEE 754 addition.
fn dot(terms: impl Iterator<Item = (f64, f64)>, mode: Mode) -> f64 {
    let (mut pos, mut neg) = (BigUint::zero(), BigUint::zero());
    let mut inf = 0.0f64;
    let (mut nan, mut nonzero, mut pos_zero, mut neg_zero) = (false, false, false, false);
    for (x, y) in terms {
        let p = x * y;
        if x.is_nan() || y.is_nan() {
            nan = true;
        } else if x.is_infinite() || y.is_infinite() {
            nan |= p.is_nan() || (inf != 0.0 && inf != p);
            inf = p;
        } else if p == 0.0 && (x == 0.0 || y == 0.0) {
            if p.is_sign_negative() {
                neg_zero = true
            } else {
                pos_zero = true
            }
        } else {
            nonzero = true;
            let ((nx, mx, ex), (ny, my, ey)) = (parts(x), parts(y));
            let t = (BigUint::from(mx) * my) << (ex + ey + 2148) as usize;
            if nx != ny { neg += t } else { pos += t }
        }
    }
    if nan {
        return f64::NAN;
    }
    if inf != 0.0 {
        return inf;
    }
    match pos.cmp(&neg) {
        Ordering::Greater => round(false, &(pos - neg), -2148, false, mode),
        Ordering::Less => round(true, &(neg - pos), -2148, false, mode),
        Ordering::Equal if !nonzero && neg_zero && !pos_zero => -0.0,
        Ordering::Equal if !nonzero && !neg_zero => 0.0,
        Ordering::Equal => {
            if mode == Mode::Down {
                -0.0
            } else {
                0.0
            }
        }
    }
}

fn div(a: f64, b: f64, mode: Mode) -> f64 {
    if mode == Mode::Nearest || a == 0.0 || b == 0.0 || !a.is_finite() || !b.is_finite() {
        a / b
    } else {
        div_slow(a, b, mode)
    }
}

/// `a / b` for finite nonzero operands.
fn div_slow(a: f64, b: f64, mode: Mode) -> f64 {
    let ((na, ma, ea), (nb, mb, eb)) = (parts(a), parts(b));
    let k: usize = 128;
    let num = BigUint::from(ma) << k;
    let mb = BigUint::from(mb);
    let (q, r) = (&num / &mb, &num % &mb);
    round(na != nb, &q, ea - eb - k as i64, !r.is_zero(), mode)
}

fn sqrt(x: f64, mode: Mode) -> f64 {
    if mode == Mode::Nearest || x <= 0.0 || !x.is_finite() {
        x.sqrt()
    } else {
        sqrt_slow(x, mode)
    }
}

/// The square root of a finite positive `x`.
fn sqrt_slow(x: f64, mode: Mode) -> f64 {
    let (_, m, mut e) = parts(x);
    let mut m = BigUint::from(m);
    if e % 2 != 0 {
        m <<= 1;
        e -= 1;
    }
    let k: usize = 120;
    m <<= 2 * k;
    let s = m.sqrt();
    let exact = &s * &s == m;
    round(false, &s, e / 2 - k as i64, !exact, mode)
}

/// `hw` is the hardware result, correct for nearest and for any non-finite input.
fn exact(hw: f64, ins: &[f64], terms: &[(f64, f64)], mode: Mode) -> f64 {
    if mode == Mode::Nearest || ins.iter().any(|x| !x.is_finite()) {
        hw
    } else {
        dot(terms.iter().copied(), mode)
    }
}

fn pown(x: f64, n: i128) -> f64 {
    let r = core_math::pow(x.abs(), n as f64);
    if x.is_sign_negative() && n % 2 != 0 {
        -r
    } else {
        r
    }
}

fn powr(x: f64, y: f64) -> f64 {
    let undefined = x.is_nan()
        || y.is_nan()
        || x < 0.0
        || (y == 0.0 && (x == 0.0 || x.is_infinite()))
        || (x == 1.0 && y.is_infinite());
    if undefined {
        f64::NAN
    } else {
        core_math::pow(x.abs(), y)
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

fn libm(name: &str, x: f64) -> Option<f64> {
    use core_math as c;
    Some(match name {
        "std.math.exp" => c::exp(x),
        "std.math.exp2" => c::exp2(x),
        "std.math.exp10" => c::exp10(x),
        "std.math.expm1" => c::expm1(x),
        "std.math.exp2m1" => c::exp2m1(x),
        "std.math.exp10m1" => c::exp10m1(x),
        "std.math.log" => c::log(x),
        "std.math.log2" => c::log2(x),
        "std.math.log10" => c::log10(x),
        "std.math.log1p" => c::log1p(x),
        "std.math.log2p1" => c::log2p1(x),
        "std.math.log10p1" => c::log10p1(x),
        "std.math.cbrt" => c::cbrt(x),
        "std.math.rsqrt" => c::rsqrt(x),
        "std.math.sin" => c::sin(x),
        "std.math.cos" => c::cos(x),
        "std.math.tan" => c::tan(x),
        "std.math.sinpi" => c::sinpi(x),
        "std.math.cospi" => c::cospi(x),
        "std.math.tanpi" => c::tanpi(x),
        "std.math.asin" => c::asin(x),
        "std.math.acos" => c::acos(x),
        "std.math.atan" => c::atan(x),
        "std.math.asinpi" => c::asinpi(x),
        "std.math.acospi" => c::acospi(x),
        "std.math.atanpi" => c::atanpi(x),
        "std.math.sinh" => c::sinh(x),
        "std.math.cosh" => c::cosh(x),
        "std.math.tanh" => c::tanh(x),
        "std.math.asinh" => c::asinh(x),
        "std.math.acosh" => c::acosh(x),
        "std.math.atanh" => c::atanh(x),
        "std.math.erf" => c::erf(x),
        "std.math.erfc" => c::erfc(x),
        "std.math.lgamma" => c::lgamma(x),
        "std.math.tgamma" => c::tgamma(x),
        _ => return None,
    })
}

fn libm2(name: &str, x: f64, y: f64) -> Option<f64> {
    Some(match name {
        "std.math.pow" => core_math::pow(x, y),
        "std.math.powr" => powr(x, y),
        "std.math.hypot" => core_math::hypot(x, y),
        "std.math.atan2" => core_math::atan2(x, y),
        "std.math.atan2pi" => core_math::atan2pi(x, y),
        _ => return None,
    })
}

fn nearest_only(name: &str, m: Mode) -> R<()> {
    if m == Mode::Nearest {
        Ok(())
    } else {
        unsupported(format!("directed rounding in `{name}` before codegen"))
    }
}

pub(super) fn call(name: &str, args: &mut [Value]) -> Option<R<Value>> {
    use Value::{Bool, Float, Int, Tuple};
    Some(Ok(Float(match (name, &*args) {
        ("std.math.add", [Float(a), Float(b), r]) => {
            exact(a + b, &[*a, *b], &[(*a, 1.0), (*b, 1.0)], mode(r))
        }
        ("std.math.sub", [Float(a), Float(b), r]) => {
            exact(a - b, &[*a, *b], &[(*a, 1.0), (-*b, 1.0)], mode(r))
        }
        ("std.math.mul", [Float(a), Float(b), r]) => exact(a * b, &[*a, *b], &[(*a, *b)], mode(r)),
        ("std.math.div", [Float(a), Float(b), r]) => div(*a, *b, mode(r)),
        ("std.math.fma", [Float(a), Float(b), Float(c), r]) => exact(
            a.mul_add(*b, *c),
            &[*a, *b, *c],
            &[(*a, *b), (*c, 1.0)],
            mode(r),
        ),
        ("std.math.sqrt", [Float(x), r]) => sqrt(*x, mode(r)),
        ("std.math.sum_exact", [xs, r]) => dot(floats(xs)?.into_iter().map(|x| (x, 1.0)), mode(r)),
        ("std.math.dot_exact", [xs, ys, r]) => {
            let (xs, ys) = (floats(xs)?, floats(ys)?);
            if xs.len() != ys.len() {
                return Some(panic("value"));
            }
            dot(xs.into_iter().zip(ys), mode(r))
        }
        ("std.math.pown", [Float(x), Int(n), r]) => {
            if let Err(e) = nearest_only(name, mode(r)) {
                return Some(Err(e));
            }
            pown(*x, *n)
        }
        ("std.math.floor", [Float(x)]) => x.floor(),
        ("std.math.ceil", [Float(x)]) => x.ceil(),
        ("std.math.trunc", [Float(x)]) => x.trunc(),
        ("std.math.fmod", [Float(x), Float(y)]) => x % y,
        ("std.math.remainder", [Float(x), Float(y)]) => libm::remainder(*x, *y),
        ("std.math.copysign", [Float(x), Float(y)]) => x.copysign(*y),
        ("std.math.ldexp", [Float(x), Int(n)]) => {
            libm::scalbn(*x, (*n).clamp(-10_000, 10_000) as i32)
        }
        ("std.math.next_up", [Float(x)]) => x.next_up(),
        ("std.math.next_down", [Float(x)]) => x.next_down(),
        ("std.math.frexp", [Float(x)]) => {
            let (m, e) = libm::frexp(*x);
            return Some(Ok(Tuple(vec![Float(m), Int(e.into())])));
        }
        ("std.math.is_nan", [Float(x)]) => return Some(Ok(Bool(x.is_nan()))),
        ("std.math.is_inf", [Float(x)]) => return Some(Ok(Bool(x.is_infinite()))),
        ("std.math.is_finite", [Float(x)]) => return Some(Ok(Bool(x.is_finite()))),
        (_, [Float(x), r]) => {
            let v = libm(name, *x)?;
            if let Err(e) = nearest_only(name, mode(r)) {
                return Some(Err(e));
            }
            v
        }
        (_, [Float(x), Float(y), r]) => {
            let v = libm2(name, *x, *y)?;
            if let Err(e) = nearest_only(name, mode(r)) {
                return Some(Err(e));
            }
            v
        }
        _ => return None,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Floats from raw bits, so subnormals, extremes and ties all turn up.
    fn floats(n: usize) -> Vec<f64> {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut out = vec![
            1.0,
            -1.0,
            3.0,
            0.1,
            f64::MAX,
            f64::MIN_POSITIVE,
            5e-324,
            1e-310,
        ];
        while out.len() < n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let f = match x % 4 {
                0 => f64::from_bits(x),
                1 => f64::from_bits(x >> 12 | 0x3ff0_0000_0000_0000) - 1.0,
                2 => f64::from_bits(x & 0x800f_ffff_ffff_ffff),
                _ => (x % 1000) as f64 / 7.0,
            };
            if f.is_finite() && f != 0.0 {
                out.push(f);
            }
        }
        out
    }

    /// Nearest agrees with the hardware; down and up bracket it and are adjacent or equal.
    fn check(hw: f64, f: impl Fn(Mode) -> f64, what: &str) {
        if !hw.is_finite() {
            return;
        }
        let (n, d, u, z) = (f(Mode::Nearest), f(Mode::Down), f(Mode::Up), f(Mode::Zero));
        assert_eq!(n.to_bits(), hw.to_bits(), "{what}: nearest");
        assert!(d <= n && n <= u, "{what}: {d} {n} {u}");
        assert!(d == u || d.next_up() == u, "{what}: {d} {u} not adjacent");
        assert_eq!(z, if u > 0.0 { d } else { u }, "{what}: toward zero");
    }

    #[test]
    fn exact_ops_round_in_every_mode() {
        let xs = floats(400);
        for (i, &a) in xs.iter().enumerate() {
            let c = xs[(i * 7 + 3) % xs.len()];
            for &b in &xs[..60] {
                check(
                    a + b,
                    |m| dot([(a, 1.0), (b, 1.0)].into_iter(), m),
                    &format!("{a} + {b}"),
                );
                check(
                    a * b,
                    |m| dot([(a, b)].into_iter(), m),
                    &format!("{a} * {b}"),
                );
                check(a / b, |m| div_slow(a, b, m), &format!("{a} / {b}"));
                let f = a.mul_add(b, c);
                check(
                    f,
                    |m| dot([(a, b), (c, 1.0)].into_iter(), m),
                    &format!("fma {a} {b} {c}"),
                );
            }
            check(
                a.abs().sqrt(),
                |m| sqrt_slow(a.abs(), m),
                &format!("sqrt {a}"),
            );
        }
    }

    #[test]
    fn exact_sums() {
        let s = |xs: &[f64], m| dot(xs.iter().map(|x| (*x, 1.0)), m);
        assert_eq!(s(&[0.1; 10], Mode::Nearest), 1.0);
        assert_eq!(s(&[1e100, 1.0, -1e100], Mode::Nearest), 1.0);
        assert_eq!(s(&[1e308, 1e308, -1e308], Mode::Nearest), 1e308);
        assert_eq!(s(&[f64::MAX, f64::MAX], Mode::Zero), f64::MAX);
        assert_eq!(s(&[f64::MAX, f64::MAX], Mode::Nearest), f64::INFINITY);
        assert!(s(&[f64::INFINITY, f64::NEG_INFINITY], Mode::Nearest).is_nan());
        assert!(s(&[-0.0, -0.0], Mode::Nearest).is_sign_negative());
        assert!(s(&[], Mode::Down).is_sign_positive());
        assert!(s(&[1.0, -1.0], Mode::Down).is_sign_negative());
        assert!(s(&[1.0, -1.0], Mode::Up).is_sign_positive());
        assert_eq!(s(&[1.0, 1e-30], Mode::Up), 1.0f64.next_up());
    }
}
