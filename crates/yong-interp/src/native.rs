//! Native std functions over interpreter values, by Yong path (R474, R475).

use std::mem;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::Zero;

use super::{Key, R, Value, data, panic};

fn some(v: Value) -> Value {
    data("Some", "value", v)
}

fn none() -> Value {
    Value::Data {
        name: "None".into(),
        fields: Vec::new(),
    }
}

fn opt(v: Option<Value>) -> Value {
    v.map_or_else(none, some)
}

fn take(v: &mut Value) -> Value {
    mem::replace(v, Value::Unit)
}

fn str_list<'a>(xs: impl Iterator<Item = &'a str>) -> Value {
    Value::List(xs.map(|s| Value::Str(s.into())).collect())
}

pub(super) fn call(name: &str, args: &mut [Value]) -> Option<R<Value>> {
    use Value::*;
    Some(Ok(match (name, &mut *args) {
        ("std.dict.new", []) => Dict(Default::default()),
        ("std.dict.insert", [Dict(d), k, v]) => {
            d.insert(Key(take(k)), take(v));
            Unit
        }
        ("std.dict.index", [Dict(d), k]) => match d.get(&Key(k.clone())) {
            Some(v) => v.clone(),
            None => return Some(panic("bounds")),
        },
        ("std.dict.get", [Dict(d), k]) => opt(d.get(&Key(k.clone())).cloned()),
        ("std.dict.contains_key", [Dict(d), k]) => Bool(d.contains_key(&Key(k.clone()))),
        ("std.dict.len", [Dict(d)]) => Int(d.len() as i128),
        ("std.dict.remove", [Dict(d), k]) => opt(d.shift_remove(&Key(k.clone()))),
        ("std.dict.setdefault", [Dict(d), k, v]) => {
            d.entry(Key(take(k))).or_insert_with(|| take(v)).clone()
        }
        ("std.dict.items", [Dict(d)]) => List(
            d.iter()
                .map(|(k, v)| Tuple(vec![k.0.clone(), v.clone()]))
                .collect(),
        ),
        ("std.dict.keys", [Dict(d)]) => List(d.keys().map(|k| k.0.clone()).collect()),
        ("std.dict.values", [Dict(d)]) => List(d.values().cloned().collect()),
        ("std.set.new", []) => Set(Default::default()),
        ("std.set.add", [Set(s), x]) => {
            s.insert(Key(take(x)));
            Unit
        }
        ("std.set.discard", [Set(s), x]) => {
            s.shift_remove(&Key(x.clone()));
            Unit
        }
        ("std.set.contains", [Set(s), x]) => Bool(s.contains(&Key(x.clone()))),
        ("std.set.len", [Set(s)]) => Int(s.len() as i128),
        ("std.set.union", [Set(a), Set(b)]) => Set(a.union(b).cloned().collect()),
        ("std.set.intersection", [Set(a), Set(b)]) => Set(a.intersection(b).cloned().collect()),
        ("std.list.index", [List(xs), v]) => {
            opt(xs.iter().position(|x| x == v).map(|i| Int(i as i128)))
        }
        ("std.list.get", [List(xs), Int(i)]) => {
            opt(usize::try_from(*i).ok().and_then(|i| xs.get(i)).cloned())
        }
        ("std.text.splitlines", [Str(s)]) => str_list(splitlines(s)),
        ("std.text.partition", [Str(s), Str(sep)]) => {
            let (a, m, b) = match s.split_once(&**sep) {
                Some((a, b)) => (a, &**sep, b),
                None => (&**s, "", ""),
            };
            Tuple(vec![Str(a.into()), Str(m.into()), Str(b.into())])
        }
        ("std.text.parse_int", [Str(s)]) => opt(parse_int(s).map(Int)),
        ("std.text.parse_float", [Str(s)]) => opt(parse_float(s).map(Float)),
        ("std.prelude.list", [List(xs)]) => List(mem::take(xs)),
        ("std.prelude.range", [Int(n)]) => List((0..*n).map(Int).collect()),
        ("std.prelude.range", [Int(a), Int(b)]) => List((*a..*b).map(Int).collect()),
        ("std.prelude.bigint", [Int(i)]) => BigInt((*i).into()),
        ("std.math.sqrt", [Float(x)]) => Float(x.sqrt()),
        _ => return None,
    }))
}

/// Python's `str.splitlines()` boundaries, with no empty last line.
fn splitlines(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .find([
                '\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}',
                '\u{2029}',
            ])
            .unwrap_or(rest.len());
        let line = &rest[..end];
        let skip = match rest[end..].chars().next() {
            None => 0,
            Some('\r') if rest[end + 1..].starts_with('\n') => 2,
            Some(c) => c.len_utf8(),
        };
        rest = &rest[end + skip..];
        Some(line)
    })
}

/// An optional sign then ASCII digits, within `int` (i64).
fn parse_int(s: &str) -> Option<i128> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok().map(i128::from)
}

fn parse_float(s: &str) -> Option<f64> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let named = ["inf", "infinity", "nan"].contains(&body.to_ascii_lowercase().as_str());
    if !named
        && !body
            .bytes()
            .all(|b| b.is_ascii_digit() || b"._eE+-".contains(&b))
    {
        return None;
    }
    s.parse().ok()
}

/// `<op>_bigint` intrinsics; floor division and modulo round toward negative infinity.
pub(super) fn bigint_op(op: &str, a: &BigInt, b: &BigInt) -> Option<R<Value>> {
    use Value::{BigInt as B, Bool};
    Some(Ok(match op {
        "add" => B(a + b),
        "sub" => B(a - b),
        "mul" => B(a * b),
        "floordiv" | "mod" if b.is_zero() => return Some(panic("div_zero")),
        "floordiv" => B(a.div_floor(b)),
        "mod" => B(a.mod_floor(b)),
        "eq" => Bool(a == b),
        "ne" => Bool(a != b),
        "lt" => Bool(a < b),
        "le" => Bool(a <= b),
        "gt" => Bool(a > b),
        "ge" => Bool(a >= b),
        _ => return None,
    }))
}
