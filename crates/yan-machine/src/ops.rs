//! Scalar intrinsics on `i128` values, as native code computes them in 64-bit registers.

use crate::{Stop, Val, unsupported};
use yan_mir::{IntTy, ScalarOp, Ty};

pub fn wrap(t: IntTy, v: i128) -> i128 {
    let mask = (1i128 << t.bits) - 1;
    let u = v & mask;
    if t.signed && u >> (t.bits - 1) & 1 == 1 {
        u - (1i128 << t.bits)
    } else {
        u
    }
}

fn in_range(t: IntTy, v: i128) -> Result<i128, Stop> {
    let (lo, hi) = t.range();
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(Stop::Panic("overflow"))
    }
}

fn pattern(t: IntTy, v: i128) -> u128 {
    (v & ((1i128 << t.bits) - 1)) as u128
}

pub fn scalar(op: ScalarOp, ty: Ty, a: &[Val]) -> Result<Val, Stop> {
    use ScalarOp::*;
    let x = a.first().copied().unwrap_or(Val::U);
    let y = a.get(1).copied().unwrap_or(Val::U);
    let (xi, yi) = (x.int(), y.int());
    Ok(match (op, ty) {
        (Exclusive, _) => {
            let same = if ty == Ty::F64 {
                x.float() == y.float()
            } else {
                xi == yi
            };
            if same {
                return Err(Stop::Panic("exclusivity"));
            }
            Val::U
        }
        (CtSelect, _) => {
            if x.truth() {
                y
            } else {
                a[2]
            }
        }
        (CtEq, _) => Val::B(xi == yi),
        (CtLt, _) => Val::B(xi < yi),
        (Not, Ty::Bool) => Val::B(!x.truth()),
        (Not, Ty::Int(t)) => Val::I(wrap(t, !xi)),
        (And, Ty::Bool) => Val::B(x.truth() & y.truth()),
        (Or, Ty::Bool) => Val::B(x.truth() | y.truth()),
        (Xor, Ty::Bool) => Val::B(x.truth() ^ y.truth()),
        (And, _) => Val::I(xi & yi),
        (Or, _) => Val::I(xi | yi),
        (Xor, _) => Val::I(xi ^ yi),
        (Add, Ty::F64) => Val::F(x.float() + y.float()),
        (Sub, Ty::F64) => Val::F(x.float() - y.float()),
        (Mul, Ty::F64) => Val::F(x.float() * y.float()),
        (Div, Ty::F64) => Val::F(x.float() / y.float()),
        (Eq | Ne | Lt | Le | Gt | Ge, Ty::F64) => {
            let (p, q) = (x.float(), y.float());
            Val::B(match op {
                Eq => p == q,
                Ne => p != q,
                Lt => p < q,
                Le => p <= q,
                Gt => p > q,
                _ => p >= q,
            })
        }
        (Eq | Ne | Lt | Le | Gt | Ge, _) => Val::B(match op {
            Eq => xi == yi,
            Ne => xi != yi,
            Lt => xi < yi,
            Le => xi <= yi,
            Gt => xi > yi,
            _ => xi >= yi,
        }),
        (Add | Sub | Mul, Ty::Int(t)) => Val::I(wrap(
            t,
            match op {
                Add => xi.wrapping_add(yi),
                Sub => xi.wrapping_sub(yi),
                _ => xi.wrapping_mul(yi),
            },
        )),
        (Div, Ty::Int(t)) if t.bits > 32 => Val::F(yan_rt::num::div_i128(xi, yi)),
        (Div, Ty::Int(_)) => Val::F(xi as f64 / yi as f64),
        (AddChecked | SubChecked | MulChecked, Ty::Int(t)) => Val::I(in_range(
            t,
            match op {
                AddChecked => xi + yi,
                SubChecked => xi - yi,
                _ => xi * yi,
            },
        )?),
        (FloorDiv | Mod, Ty::Int(t)) => {
            if yi == 0 {
                return Err(Stop::Panic("div_zero"));
            }
            let (mut q, mut r) = (xi / yi, xi % yi);
            if r != 0 && (r < 0) != (yi < 0) {
                q -= 1;
                r += yi;
            }
            Val::I(in_range(t, if op == FloorDiv { q } else { r })?)
        }
        (Shl | Shr | WrappingShl, Ty::Int(t)) => {
            if !(0..i128::from(t.bits)).contains(&yi) {
                return Err(Stop::Panic("overflow"));
            }
            match op {
                Shr => Val::I(xi >> yi),
                WrappingShl => Val::I(wrap(t, xi << yi)),
                _ => Val::I(in_range(t, xi << yi)?),
            }
        }
        (RotateLeft | RotateRight, Ty::Int(t)) => {
            let w = u32::from(t.bits);
            let k = yi.rem_euclid(i128::from(w)) as u32;
            let k = if op == RotateRight { (w - k) % w } else { k };
            let p = pattern(t, xi);
            let r = if k == 0 {
                p
            } else {
                (p << k | p >> (w - k)) & ((1u128 << w) - 1)
            };
            Val::I(wrap(t, r as i128))
        }
        (
            CountOnes | CountZeros | LeadingZeros | TrailingZeros | BitWidth | SwapBytes
            | ReverseBits | IsPowerOfTwo,
            Ty::Int(t),
        ) => {
            let (w, p) = (u32::from(t.bits), pattern(t, xi));
            let pad = 128 - w;
            match op {
                CountOnes => Val::I(i128::from(p.count_ones())),
                CountZeros => Val::I(i128::from(w - p.count_ones())),
                LeadingZeros => Val::I(i128::from(p.leading_zeros() - pad)),
                TrailingZeros => Val::I(i128::from(if p == 0 { w } else { p.trailing_zeros() })),
                BitWidth => Val::I(i128::from(128 - p.leading_zeros())),
                SwapBytes => Val::I(wrap(t, (p.swap_bytes() >> pad) as i128)),
                ReverseBits => Val::I(wrap(t, (p.reverse_bits() >> pad) as i128)),
                _ => Val::B(xi > 0 && p & (p - 1) == 0),
            }
        }
        _ => return unsupported(format!("scalar {op:?} on {ty:?}")),
    })
}
