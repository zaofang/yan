//! The std functions elaborated Jian calls (paths per R475), until yong-std provides them.

use std::cmp::Ordering;
use std::mem;

use unicode_segmentation::UnicodeSegmentation;

use super::{R, Value, panic};

pub(super) fn call(name: &str, args: &mut [Value]) -> Option<R<Value>> {
    use Value::*;
    let text = |s: String| Some(Ok(Str(s.into())));
    Some(Ok(match (name, &mut *args) {
        ("std.fmt.show", [v]) => return text(show(v, false)),
        ("std.text.concat", [Str(a), Str(b)]) => return text(format!("{a}{b}")),
        ("std.text.byte_len", [Str(s)]) => Int(s.len() as i128),
        ("std.text.char_count", [Str(s)]) => Int(s.chars().count() as i128),
        ("std.text.grapheme_count", [Str(s)]) => Int(s.graphemes(true).count() as i128),
        ("std.text.upper", [Str(s)]) => return text(s.to_uppercase()),
        ("std.text.lower", [Str(s)]) => return text(s.to_lowercase()),
        ("std.text.strip", [Str(s)]) => return text(s.trim().to_owned()),
        ("std.text.startswith", [Str(s), Str(p)]) => Bool(s.starts_with(&**p)),
        ("std.text.endswith", [Str(s), Str(p)]) => Bool(s.ends_with(&**p)),
        ("std.text.contains", [Str(s), Str(p)]) => Bool(s.contains(&**p)),
        ("std.text.replace", [Str(s), Str(a), Str(b)]) => return text(s.replace(&**a, b)),
        ("std.text.split", [Str(s), Str(sep)]) => {
            List(s.split(&**sep).map(|p| Str(p.into())).collect())
        }
        ("std.text.join", [Str(sep), List(xs)]) => {
            let parts: Vec<&str> = xs
                .iter()
                .filter_map(|x| match x {
                    Str(s) => Some(&**s),
                    _ => None,
                })
                .collect();
            return text(parts.join(sep));
        }
        ("std.list.append", [List(xs), v]) => {
            xs.push(mem::replace(v, Unit));
            Unit
        }
        ("std.list.pop", [List(xs)]) => match xs.pop() {
            Some(v) => v,
            None => return Some(panic("bounds")),
        },
        ("std.list.concat", [List(a), List(b)]) => {
            List(a.iter().chain(b.iter()).cloned().collect())
        }
        ("std.list.contains", [List(xs), v]) => Bool(xs.contains(v)),
        ("std.list.insert", [List(xs), Int(i), v]) => {
            let n = xs.len() as i128;
            let at = if *i < 0 { (n + *i).max(0) } else { (*i).min(n) };
            xs.insert(at as usize, mem::replace(v, Unit));
            Unit
        }
        ("std.list.sort", [List(xs)]) => {
            xs.sort_by(cmp);
            Unit
        }
        ("std.list.sort_unstable", [List(xs)]) => {
            xs.sort_unstable_by(cmp);
            Unit
        }
        ("std.list.reverse", [List(xs)]) => {
            xs.reverse();
            Unit
        }
        ("std.list.bisect_left", [List(xs), x]) => {
            Int(xs.partition_point(|y| cmp(y, x) == Ordering::Less) as i128)
        }
        ("std.list.bisect_right", [List(xs), x]) => {
            Int(xs.partition_point(|y| cmp(y, x) != Ordering::Greater) as i128)
        }
        ("std.list.insort", [List(xs), x]) => {
            let at = xs.partition_point(|y| cmp(y, x) != Ordering::Greater);
            xs.insert(at, mem::replace(x, Unit));
            Unit
        }
        ("std.list.slice_exact", [List(xs), Int(a), Int(b)]) => {
            match (usize::try_from(*a), usize::try_from(*b)) {
                (Ok(a), Ok(b)) if a <= b && b <= xs.len() => List(xs[a..b].to_vec()),
                _ => return Some(panic("bounds")),
            }
        }
        ("std.prelude.sorted", [List(xs)]) => {
            let mut v = xs.clone();
            v.sort_by(cmp);
            List(v)
        }
        ("std.prelude.int", [Float(f)]) => {
            let t = f.trunc();
            if t.is_nan() || t < i64::MIN as f64 || t >= i64::MAX as f64 {
                return Some(panic("conversion"));
            }
            Int(t as i128)
        }
        ("std.prelude.float", [Int(i)]) => Float(*i as f64),
        ("std.prelude.round", [Float(f)]) => {
            let r = f.round_ties_even();
            if r.is_nan() || r < i64::MIN as f64 || r >= i64::MAX as f64 {
                return Some(panic("conversion"));
            }
            Int(r as i128)
        }
        ("std.prelude.abs", [Int(i)]) if *i == i64::MIN.into() => return Some(panic("overflow")),
        ("std.prelude.abs", [Int(i)]) => Int(i.abs()),
        ("std.prelude.abs", [Float(f)]) => Float(f.abs()),
        ("std.prelude.min" | "std.prelude.max", [a, b]) => {
            let less = order(b, a) == Some(Ordering::Less);
            let pick_b = if name.ends_with("min") {
                less
            } else {
                order(b, a) == Some(Ordering::Greater)
            };
            if pick_b { b.clone() } else { a.clone() }
        }
        ("std.cmp.eq", [a, b]) => Bool(a == b),
        ("std.cmp.ne", [a, b]) => Bool(a != b),
        ("std.cmp.lt", [a, b]) => Bool(order(a, b) == Some(Ordering::Less)),
        ("std.cmp.le", [a, b]) => Bool(matches!(
            order(a, b),
            Some(Ordering::Less | Ordering::Equal)
        )),
        ("std.cmp.gt", [a, b]) => Bool(order(a, b) == Some(Ordering::Greater)),
        ("std.cmp.ge", [a, b]) => Bool(matches!(
            order(a, b),
            Some(Ordering::Greater | Ordering::Equal)
        )),
        _ => return None,
    }))
}

fn order(a: &Value, b: &Value) -> Option<Ordering> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => Some(x.cmp(y)),
        (BigInt(x), BigInt(y)) => Some(x.cmp(y)),
        (Float(x), Float(y)) => x.partial_cmp(y),
        (Bool(x), Bool(y)) => Some(x.cmp(y)),
        (Char(x), Char(y)) => Some(x.cmp(y)),
        (Str(x), Str(y)) => Some(x.cmp(y)),
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
        Float(f) => float_repr(*f),
        Char(c) if nested => repr(&c.to_string()),
        Char(c) => c.to_string(),
        Str(s) if nested => repr(s),
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
        Data { name, fields } => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, x)| format!("{n}={}", show(x, true)))
                .collect();
            format!("{name}({})", fs.join(", "))
        }
        Cap(n) => format!("<capability {n}>"),
        Closure(_) => "<function>".into(),
    }
}

fn repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

/// CPython float `repr`: shortest round-trip digits, exponent form outside [1e-4, 1e16).
pub(super) fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.into();
    }
    let a = f.abs();
    if a != 0.0 && !(1e-4..1e16).contains(&a) {
        let e = format!("{f:e}");
        let (m, x) = e.split_once('e').expect("exponent form");
        let (sign, digits) = match x.strip_prefix('-') {
            Some(d) => ('-', d),
            None => ('+', x),
        };
        return format!("{m}e{sign}{digits:0>2}");
    }
    let s = format!("{f}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// A total order for sorting; the checker admits only `Ord` element types.
pub(super) fn cmp(a: &Value, b: &Value) -> Ordering {
    order(a, b).unwrap_or(Ordering::Equal)
}
