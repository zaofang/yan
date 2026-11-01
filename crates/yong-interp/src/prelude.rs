//! The std functions elaborated Xin calls (paths per R475), bound by registry id (R561).

use std::cmp::Ordering;
use std::mem;

use yong_rt::{fmt, list, num, text};
use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{Value, panic};
use Value::*;

macro_rules! f {
    ($id:ident, |$a:ident| { $($pat:pat => $e:expr),+ $(,)? }) => {
        (
            $id,
            Bind::Plain(|$a| {
                Ok(match &mut *$a {
                    $($pat => $e,)+
                    #[allow(unreachable_patterns)]
                    _ => return bad_args($id),
                })
            }),
        )
    };
}

fn s(x: String) -> Value {
    Str(x.into())
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    f!(STD_FMT_SHOW, |a| { [v] => s(show(v, false)) }),
    f!(STD_TEXT_CONCAT, |a| { [Str(x), Str(y)] => s(format!("{x}{y}")) }),
    f!(STD_TEXT_BYTE_LEN, |a| { [Str(x)] => Int(x.len() as i128) }),
    f!(STD_TEXT_CHAR_COUNT, |a| { [Str(x)] => Int(x.chars().count() as i128) }),
    f!(STD_TEXT_GRAPHEME_COUNT, |a| { [Str(x)] => Int(text::grapheme_count(x) as i128) }),
    f!(STD_TEXT_UPPER, |a| { [Str(x)] => s(x.to_uppercase()) }),
    f!(STD_TEXT_LOWER, |a| { [Str(x)] => s(x.to_lowercase()) }),
    f!(STD_TEXT_STRIP, |a| { [Str(x)] => s(x.trim().to_owned()) }),
    f!(STD_TEXT_STARTSWITH, |a| { [Str(x), Str(p)] => Bool(x.starts_with(&**p)) }),
    f!(STD_TEXT_ENDSWITH, |a| { [Str(x), Str(p)] => Bool(x.ends_with(&**p)) }),
    f!(STD_TEXT_CONTAINS, |a| { [Str(x), Str(p)] => Bool(x.contains(&**p)) }),
    f!(STD_TEXT_REPLACE, |a| { [Str(x), Str(p), Str(q)] => s(x.replace(&**p, q)) }),
    f!(STD_TEXT_SPLIT, |a| {
        [Str(x), Data { fields, .. }] => List(match fields.first() {
            Some((_, Str(sep))) => x.split(&**sep).map(|p| Str(p.into())).collect(),
            _ => text::split_whitespace(x).map(|p| Str(p.into())).collect(),
        })
    }),
    f!(STD_TEXT_JOIN, |a| {
        [Str(sep), List(xs)] => {
            let parts: Vec<&str> = xs
                .iter()
                .filter_map(|x| match x {
                    Str(s) => Some(&**s),
                    _ => None,
                })
                .collect();
            s(parts.join(sep))
        }
    }),
    f!(STD_LIST_APPEND, |a| {
        [List(xs), v] => {
            xs.push(mem::replace(v, Unit));
            Unit
        }
    }),
    f!(STD_LIST_POP, |a| {
        [List(xs)] => match xs.pop() {
            Some(v) => v,
            None => return panic("bounds"),
        }
    }),
    f!(STD_LIST_CONCAT, |a| {
        [List(x), List(y)] => List(x.iter().chain(y.iter()).cloned().collect())
    }),
    f!(STD_LIST_CONTAINS, |a| { [List(xs), v] => Bool(xs.contains(v)) }),
    f!(STD_LIST_INSERT, |a| {
        [List(xs), Int(i), v] => {
            xs.insert(list::insert_at(*i, xs.len()), mem::replace(v, Unit));
            Unit
        }
    }),
    f!(STD_LIST_REVERSE, |a| {
        [List(xs)] => {
            xs.reverse();
            Unit
        }
    }),
    f!(STD_LIST_SLICE_CLAMPED, |a| {
        [List(xs), start, stop, step] => {
            let int = |v: &Value| match v {
                Data { fields, .. } => match fields.first() {
                    Some((_, Int(i))) => Some(*i),
                    _ => None,
                },
                _ => None,
            };
            let step = int(step).unwrap_or(1);
            if step == 0 {
                return panic("value");
            }
            List(
                list::slice_indices(xs.len(), int(start), int(stop), step)
                    .map(|k| xs[k].clone())
                    .collect(),
            )
        }
    }),
    f!(STD_LIST_SLICE_EXACT, |a| {
        [List(xs), Int(i), Int(j)] => match (usize::try_from(*i), usize::try_from(*j)) {
            (Ok(i), Ok(j)) if i <= j && j <= xs.len() => List(xs[i..j].to_vec()),
            _ => return panic("bounds"),
        }
    }),
    f!(STD_PRELUDE_INT, |a| {
        [Float(x)] => match num::trunc_i64(*x) {
            Some(i) => Int(i.into()),
            None => return panic("conversion"),
        }
    }),
    f!(STD_PRELUDE_FLOAT, |a| { [Int(i)] => Float(*i as f64) }),
    f!(STD_PRELUDE_ROUND, |a| {
        [Float(x)] => match num::round_i64(*x) {
            Some(i) => Int(i.into()),
            None => return panic("conversion"),
        }
    }),
    f!(STD_PRELUDE_ABS, |a| {
        [Int(i)] => match i64::try_from(*i).ok().and_then(i64::checked_abs) {
            Some(v) => Int(v.into()),
            None => return panic("overflow"),
        },
        [Float(x)] => Float(x.abs()),
    }),
    f!(STD_CMP_EQ, |a| { [x, y] => Bool(x == y) }),
    f!(STD_CMP_NE, |a| { [x, y] => Bool(x != y) }),
    f!(STD_CMP_LT, |a| { [x, y] => Bool(order(x, y) == Some(Ordering::Less)) }),
    f!(STD_CMP_LE, |a| {
        [x, y] => Bool(matches!(order(x, y), Some(Ordering::Less | Ordering::Equal)))
    }),
    f!(STD_CMP_GT, |a| { [x, y] => Bool(order(x, y) == Some(Ordering::Greater)) }),
    f!(STD_CMP_GE, |a| {
        [x, y] => Bool(matches!(order(x, y), Some(Ordering::Greater | Ordering::Equal)))
    }),
];

fn order(a: &Value, b: &Value) -> Option<Ordering> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Some(x.cmp(y)),
        (BigInt(x), BigInt(y)) => Some(x.cmp(y)),
        (Float(x), Float(y)) => x.partial_cmp(y),
        (Bool(x), Bool(y)) => Some(x.cmp(y)),
        (Char(x), Char(y)) => Some(x.cmp(y)),
        (Str(x), Str(y)) => Some(x.cmp(y)),
        (Bytes(x), Bytes(y)) => Some(x.cmp(y)),
        (Path(_), Path(_)) | (WindowsPath(_), WindowsPath(_)) => Some(super::path::order(a, b)),
        (Duration(x), Duration(y)) | (Instant(x), Instant(y)) | (Mono(x), Mono(y)) => {
            Some(x.cmp(y))
        }
        (PlainDate(x), PlainDate(y)) => Some(x.cmp(y)),
        (PlainTime(x), PlainTime(y)) => Some(x.cmp(y)),
        (PlainDateTime(x), PlainDateTime(y)) => Some(x.cmp(y)),
        (Tuple(xs), Tuple(ys)) | (List(xs), List(ys)) => {
            for (x, y) in xs.iter().zip(ys) {
                match order(x, y)? {
                    Ordering::Equal => {}
                    o => return Some(o),
                }
            }
            Some(xs.len().cmp(&ys.len()))
        }
        _ => None,
    }
}

/// CPython's `str` of a value at the top level and `repr` inside containers (SQ20).
pub(super) fn show(v: &Value, nested: bool) -> String {
    use Value::*;
    let join = |xs: &[Value]| {
        xs.iter()
            .map(|x| show(x, true))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match v {
        Unit => "None".into(),
        Bool(b) => if *b { "True" } else { "False" }.into(),
        Int(i) => i.to_string(),
        Float(f) => fmt::float_repr(*f),
        Char(c) if nested => fmt::repr(&c.to_string()),
        Char(c) => c.to_string(),
        Str(s) if nested => fmt::repr(s),
        Str(s) => s.to_string(),
        Tuple(xs) if xs.len() == 1 => format!("({},)", show(&xs[0], true)),
        Tuple(xs) => format!("({})", join(xs)),
        List(xs) => format!("[{}]", join(xs)),
        Dict(d) => {
            let kvs: Vec<String> = d
                .iter()
                .map(|(k, v)| format!("{}: {}", show(&k.0, true), show(v, true)))
                .collect();
            format!("{{{}}}", kvs.join(", "))
        }
        Deque(d, m) => {
            let xs: Vec<Value> = d.iter().cloned().collect();
            match m {
                Some(m) => format!("deque([{}], maxlen={m})", join(&xs)),
                None => format!("deque([{}])", join(&xs)),
            }
        }
        Heap(h) => format!("heap([{}])", join(&super::collections::ascending(h))),
        SortedMap(m) => {
            let kvs: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{}: {}", show(k, true), show(v, true)))
                .collect();
            format!("sorted_dict({{{}}})", kvs.join(", "))
        }
        SortedSet(s) if s.is_empty() => "sorted_set()".into(),
        SortedSet(s) => format!("sorted_set({{{}}})", join(s)),
        Set(s) if s.is_empty() => "set()".into(),
        Set(s) => {
            let xs: Vec<String> = s.iter().map(|x| show(&x.0, true)).collect();
            format!("{{{}}}", xs.join(", "))
        }
        BigInt(b) => b.to_string(),
        Data { name, fields } if name == "Some" && fields.len() == 1 => show(&fields[0].1, nested),
        Data { name, fields } if name == "None" && fields.is_empty() => "None".into(),
        Data { name, fields } if name == "EncodingError" => super::encoding::show_error(fields),
        Data { name, fields } if name == "TimeError" => super::time::show_error(fields),
        Data { name, .. } if name == "ZonedDateTime" => super::civil::show_zoned(v),
        Data { name, fields } => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, x)| format!("{n}={}", show(x, true)))
                .collect();
            format!("{name}({})", fs.join(", "))
        }
        Cap(n) => format!("<capability {n}>"),
        Closure(_) => "<function>".into(),
        Iter(_) => "<iter>".into(),
        Bytes(b) => super::encoding::repr(b),
        Secret(_) => "Secret(***)".into(),
        Path(_) | WindowsPath(_) => super::path::text(v).to_owned(),
        Duration(n) => super::time::show_duration(*n),
        Instant(n) => super::time::show_instant(*n),
        Mono(n) => format!("MonoInstant({n})"),
        PlainDate(d) => super::civil::show_date(*d),
        PlainTime(t) => super::civil::show_time(*t),
        PlainDateTime(dt) => super::civil::show_datetime(*dt),
        Period(p) => super::civil::show_period(p),
        Zone(z) => z.id().to_owned(),
        ByteArray(b) => format!("bytearray({})", super::encoding::repr(b)),
    }
}

/// A total order for sorting; the checker admits only `Ord` element types.
pub(super) fn cmp(a: &Value, b: &Value) -> Ordering {
    order(a, b).unwrap_or(Ordering::Equal)
}
