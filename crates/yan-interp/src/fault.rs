//! std.fault: `maybe` under the `never` and `sim(seed)` handlers of std.test (7.1, 7.3).

use yan_std::registry::{Id, id::*};

use super::Value;
use super::bind::{Bind, bad_args};

fn handler(seed: Option<i128>) -> Value {
    Value::Data {
        name: "FaultHandler".into(),
        fields: match seed {
            Some(s) => vec![("seed".into(), Value::Int(s))],
            None => Vec::new(),
        },
    }
}

/// Whether point `name` fires under `sim(seed)`: one bit of a hash of the seed and the name.
fn fires(seed: i128, name: &str) -> bool {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed as u64;
    for b in name.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 27;
    h & 1 == 1
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (STD_TEST_NEVER, Bind::Plain(|_| Ok(handler(None)))),
    (
        STD_TEST_SIM,
        Bind::Plain(|a| match a {
            [Value::Int(s)] => Ok(handler(Some(*s))),
            _ => bad_args(STD_TEST_SIM),
        }),
    ),
    (
        STD_FAULT_MAYBE,
        Bind::Cap(|_, a| match a {
            [Value::Data { fields, .. }, Value::Str(n)] => Ok(Value::Bool(match fields.as_slice() {
                [(_, Value::Int(s))] => fires(*s, n),
                _ => false,
            })),
            [_, Value::Str(_)] => Ok(Value::Bool(false)),
            _ => bad_args(STD_FAULT_MAYBE),
        }),
    ),
];
