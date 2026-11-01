//! std.rand: the `seeded` handler, the OS generator and the derived operations (RFC 0019 D3,
//! D4), all over `next_u64`.

use yong_rt::rand::{ChaCha8, Pcg64Dxsm, below, f64_from, split};
use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{Interp, R, Value, panic, unsupported};

pub(crate) enum Gen {
    ChaCha8(ChaCha8),
    Pcg64Dxsm(Pcg64Dxsm),
}

/// A `seeded` handler: the index of its generator in the interpreter.
pub(crate) fn seeded(it: &mut Interp, seed: u64, algorithm: &str) -> Value {
    it.rngs.push(match algorithm {
        "Pcg64Dxsm" => Gen::Pcg64Dxsm(Pcg64Dxsm::new(seed)),
        _ => Gen::ChaCha8(ChaCha8::new(seed)),
    });
    Value::Data {
        name: "Seeded".into(),
        fields: vec![
            ("rng".into(), Value::Int(it.rngs.len() as i128 - 1)),
            ("seed".into(), Value::Int(seed.into())),
            ("algorithm".into(), Value::Str(algorithm.into())),
        ],
    }
}

/// The next word under handler `h`; any other handler value is the OS generator.
fn next(it: &mut Interp, h: &Value) -> u64 {
    if let Value::Data { name, fields } = h
        && name == "Seeded"
        && let Value::Int(i) = fields[0].1
    {
        return match &mut it.rngs[i as usize] {
            Gen::ChaCha8(g) => g.next_u64(),
            Gen::Pcg64Dxsm(g) => g.next_u64(),
        };
    }
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("the OS generator");
    u64::from_le_bytes(b)
}

fn below_int(it: &mut Interp, h: &Value, n: i128) -> R<i128> {
    if n <= 0 {
        return panic("value");
    }
    Ok(below(n as u64, || next(it, h)).into())
}

/// The handler for branch `k` of a `par` (RFC 0019 D5).
fn branch(it: &mut Interp, h: &Value, k: i128) -> R<Value> {
    match h {
        Value::Data { name, fields } if name == "Seeded" => match &fields[1..] {
            [(_, Value::Int(seed)), (_, Value::Str(alg))] if &**alg == "ChaCha8" => {
                Ok(seeded(it, split(*seed as u64, k as u64), alg))
            }
            _ => unsupported("`seeded(seed, Pcg64Dxsm())` in a `par`, which cannot split (E402)"),
        },
        h => Ok(h.clone()),
    }
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_RAND_SPLIT,
        Bind::Cap(|it, a| match a {
            [h, Value::Int(k)] => {
                let h = h.clone();
                branch(it, &h, *k)
            }
            _ => bad_args(STD_RAND_SPLIT),
        }),
    ),
    (
        STD_TEST_SEEDED,
        Bind::Cap(|it, a| match a {
            [Value::Int(s), Value::Data { name, .. }] => Ok(seeded(it, *s as u64, name)),
            _ => bad_args(STD_TEST_SEEDED),
        }),
    ),
    (
        STD_RAND_NEXT_U64,
        Bind::Cap(|it, a| Ok(Value::Int(next(it, &a[0]).into()))),
    ),
    (
        STD_RAND_NEXT_F64,
        Bind::Cap(|it, a| Ok(Value::Float(f64_from(next(it, &a[0]))))),
    ),
    (
        STD_RAND_BOOL,
        Bind::Cap(|it, a| Ok(Value::Bool(next(it, &a[0]) >> 63 == 1))),
    ),
    (
        STD_RAND_BELOW,
        Bind::Cap(|it, a| match a {
            [h, Value::Int(n)] => {
                let h = h.clone();
                below_int(it, &h, *n).map(Value::Int)
            }
            _ => bad_args(STD_RAND_BELOW),
        }),
    ),
    (
        STD_RAND_RANGE,
        Bind::Cap(|it, a| match a {
            [h, Value::Int(lo), Value::Int(hi)] => {
                let h = h.clone();
                below_int(it, &h, *hi - *lo).map(|x| Value::Int(*lo + x))
            }
            _ => bad_args(STD_RAND_RANGE),
        }),
    ),
    (
        STD_RAND_CHOICE,
        Bind::Cap(|it, a| match a {
            [_, Value::List(xs)] if xs.is_empty() => panic("bounds"),
            [h, Value::List(xs)] => {
                let h = h.clone();
                let i = below(xs.len() as u64, || next(it, &h)) as usize;
                Ok(xs[i].clone())
            }
            _ => bad_args(STD_RAND_CHOICE),
        }),
    ),
    (
        STD_RAND_SHUFFLE,
        Bind::Cap(|it, a| match a {
            [h, Value::List(xs)] => {
                let h = h.clone();
                for i in (1..xs.len()).rev() {
                    let j = below(i as u64 + 1, || next(it, &h)) as usize;
                    xs.swap(i, j);
                }
                Ok(Value::Unit)
            }
            _ => bad_args(STD_RAND_SHUFFLE),
        }),
    ),
];
