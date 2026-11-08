//! Native std functions over interpreter values, bound by registry id (R474, R475, R561).

use std::mem;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::Zero;
use yan_rt::text;
use yan_std::registry::id::*;

use super::bind::{Bind, bad_args};
use super::prelude::show;
use super::{Key, R, Value, data, panic, panic_fields};
use Value::*;

fn some(v: Value) -> Value {
    data("Some", "value", v)
}

fn none() -> Value {
    Value::Data {
        name: "None".into(),
        fields: Vec::new(),
    }
}

pub(super) fn opt(v: Option<Value>) -> Value {
    v.map_or_else(none, some)
}

fn take(v: &mut Value) -> Value {
    mem::replace(v, Value::Unit)
}

fn str_list<'a>(xs: impl Iterator<Item = &'a str>) -> Value {
    Value::List(xs.map(|s| Value::Str(s.into())).collect())
}

macro_rules! plain {
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

macro_rules! fallible {
    ($id:ident, |$a:ident| { $($pat:pat => $e:expr),+ $(,)? }) => {
        (
            $id,
            Bind::Fallible(|$a| {
                Ok(match $a {
                    $($pat => $e,)+
                    #[allow(unreachable_patterns)]
                    _ => return bad_args($id),
                })
            }),
        )
    };
}

pub(super) static BINDS: &[(yan_std::registry::Id, Bind)] = &[
    (
        STD_FS_READ,
        Bind::CapFallible(|it, a| match a {
            [h, Str(path)] => Ok(fs_read(h, path, it.test)),
            _ => bad_args(STD_FS_READ),
        }),
    ),
    (
        STD_FS_ONLY,
        Bind::Plain(|a| match a {
            [h, read, write] => Ok(Data {
                name: "OnlyFs".into(),
                fields: vec![
                    ("inner".into(), h.clone()),
                    ("read".into(), read.clone()),
                    ("write".into(), write.clone()),
                ],
            }),
            _ => bad_args(STD_FS_ONLY),
        }),
    ),
    (
        STD_FS_SUB,
        Bind::Plain(|a| match a {
            [h, dir] => Ok(Data {
                name: "SubFs".into(),
                fields: vec![("inner".into(), h.clone()), ("dir".into(), dir.clone())],
            }),
            _ => bad_args(STD_FS_SUB),
        }),
    ),
    fallible!(STD_ERROR_CONTEXT, |a| {
        [Data { name, fields }, Str(msg)] => match (name.as_str(), &fields[..]) {
            ("Ok", [(_, v)]) => Ok(v.clone()),
            (_, [(_, e)]) => {
                let Data { name, mut fields } = wrap(e) else {
                    unreachable!("wrap builds a record")
                };
                if let (_, List(chain)) = &mut fields[1] {
                    chain.insert(0, Str(msg.clone()));
                }
                Err(Data { name, fields })
            }
            _ => return bad_args(STD_ERROR_CONTEXT),
        },
    }),
    fallible!(STD_ERROR_WRAP, |a| { [e] => Ok(wrap(e)) }),
    fallible!(STD_REGEX_COMPILE, |a| { [Str(p)] => super::regex::compile(p) }),
    fallible!(STD_I18N_LOCALE, |a| {
        [Str(t)] => match yan_std::locale::canonical(t) {
            Ok(c) => Ok(Str(c.into())),
            Err(reason) => Err(Data {
                name: "LocaleError".into(),
                fields: vec![("tag".into(), Str(t.clone())), ("reason".into(), Str(reason.into()))],
            }),
        },
    }),
    plain!(STD_DICT_NEW, |a| { [] => Dict(Default::default()) }),
    plain!(STD_DICT_INSERT, |a| {
        [Dict(d), k, v] => {
            d.insert(Key(take(k)), take(v));
            Unit
        }
    }),
    plain!(STD_DICT_INDEX, |a| {
        [Dict(d), k] => match d.get(&Key(k.clone())) {
            Some(v) => v.clone(),
            None => return panic_fields("bounds", format!("key {} is not in the dict", show(k, true)), crate::missing_key(k)),
        }
    }),
    plain!(STD_DICT_GET, |a| { [Dict(d), k] => opt(d.get(&Key(k.clone())).cloned()) }),
    plain!(STD_DICT_CONTAINS_KEY, |a| { [Dict(d), k] => Bool(d.contains_key(&Key(k.clone()))) }),
    plain!(STD_DICT_LEN, |a| { [Dict(d)] => Int(d.len() as i128) }),
    plain!(STD_DICT_REMOVE, |a| { [Dict(d), k] => opt(d.shift_remove(&Key(k.clone()))) }),
    plain!(STD_DICT_SETDEFAULT, |a| {
        [Dict(d), k, v] => d.entry(Key(take(k))).or_insert_with(|| take(v)).clone()
    }),
    plain!(STD_DICT_ITEMS, |a| {
        [Dict(d)] => List(
            d.iter()
                .map(|(k, v)| Tuple(vec![k.0.clone(), v.clone()]))
                .collect(),
        )
    }),
    plain!(STD_DICT_KEYS, |a| { [Dict(d)] => List(d.keys().map(|k| k.0.clone()).collect()) }),
    plain!(STD_DICT_VALUES, |a| { [Dict(d)] => List(d.values().cloned().collect()) }),
    plain!(STD_SET_NEW, |a| { [] => Set(Default::default()) }),
    plain!(STD_SET_ADD, |a| {
        [Set(s), x] => {
            s.insert(Key(take(x)));
            Unit
        }
    }),
    plain!(STD_SET_DISCARD, |a| {
        [Set(s), x] => {
            s.shift_remove(&Key(x.clone()));
            Unit
        }
    }),
    plain!(STD_SET_CONTAINS, |a| { [Set(s), x] => Bool(s.contains(&Key(x.clone()))) }),
    plain!(STD_SET_LEN, |a| { [Set(s)] => Int(s.len() as i128) }),
    plain!(STD_SET_UNION, |a| { [Set(x), Set(y)] => Set(x.union(y).cloned().collect()) }),
    plain!(STD_SET_INTERSECTION, |a| {
        [Set(x), Set(y)] => Set(x.intersection(y).cloned().collect())
    }),
    plain!(STD_LIST_INDEX, |a| {
        [List(xs), v] => opt(xs.iter().position(|x| x == v).map(|i| Int(i as i128)))
    }),
    plain!(STD_LIST_GET, |a| {
        [List(xs), Int(i)] => opt(usize::try_from(*i).ok().and_then(|i| xs.get(i)).cloned())
    }),
    plain!(STD_TEXT_SPLITLINES, |a| { [Str(s)] => str_list(text::splitlines(s)) }),
    plain!(STD_TEXT_PARTITION, |a| {
        [Str(s), Str(sep)] => {
            let (x, m, y) = text::partition(s, sep);
            Tuple(vec![Str(x.into()), Str(m.into()), Str(y.into())])
        }
    }),
    plain!(STD_TEXT_PARSE_INT, |a| { [Str(s)] => opt(text::parse_int(s).map(Int)) }),
    plain!(STD_TEXT_PARSE_FLOAT, |a| { [Str(s)] => opt(text::parse_float(s).map(Float)) }),
    plain!(STD_PRELUDE_LIST, |a| { [List(xs)] => List(mem::take(xs)) }),
    plain!(STD_PRELUDE_RANGE, |a| {
        [Int(n)] => List((0..*n).map(Int).collect()),
        [Int(x), Int(y)] => List((*x..*y).map(Int).collect()),
    }),
    plain!(STD_PRELUDE_BIGINT, |a| { [Int(i)] => BigInt((*i).into()) }),
    plain!(STD_TEST_FAKE_FS, |a| { [Dict(files)] => Dict(mem::take(files)) }),
];

/// `e` as an `Error`: itself when it is one, else its shown text with no context.
fn wrap(e: &Value) -> Value {
    match e {
        Value::Data { name, .. } if name == "Error" => e.clone(),
        _ => Value::Data {
            name: "Error".into(),
            fields: vec![
                (
                    "message".into(),
                    Value::Str(super::prelude::show(e, false).into()),
                ),
                ("context".into(), Value::List(Vec::new())),
            ],
        },
    }
}

/// `path` without `.` and with each `..` taken against the component before it; `None` when
/// a `..` would climb above where the path starts.
fn lexical(path: &str) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            c => out.push(c),
        }
    }
    Some(out)
}

/// `fs.only` and `fs.sub` resolve paths beneath their grants, never above them (R674). A
/// `fake_fs` handler is its file map.
fn fs_read(h: &Value, path: &str, test: bool) -> Result<Value, Value> {
    let denied = || fs_error(path, "not granted");
    match h {
        Dict(files) => files
            .get(&Key(Str(path.into())))
            .cloned()
            .ok_or_else(|| fs_error(path, "not found")),
        Data { name, fields } if name == "SubFs" => {
            let Str(dir) = &fields[1].1 else {
                unreachable!("fs.sub takes a str")
            };
            let rel = lexical(path).filter(|_| !path.starts_with('/'));
            let Some(rel) = rel else { return Err(denied()) };
            let joined = format!("{}/{}", dir.trim_end_matches('/'), rel.join("/"));
            fs_read(&fields[0].1, &joined, test)
        }
        Data { name, fields } if name == "OnlyFs" => {
            let List(grants) = &fields[1].1 else {
                unreachable!("fs.only takes a list")
            };
            let abs = path.starts_with('/');
            let Some(p) = lexical(path) else {
                return Err(denied());
            };
            let beneath = grants.iter().any(|g| match g {
                Str(g) => {
                    g.starts_with('/') == abs && lexical(g).is_some_and(|g| p.starts_with(&g))
                }
                _ => false,
            });
            if !beneath {
                return Err(denied());
            }
            let resolved = format!("{}{}", if abs { "/" } else { "" }, p.join("/"));
            fs_read(&fields[0].1, &resolved, test)
        }
        Cap(_) if !test => std::fs::read_to_string(path)
            .map(|s| Str(s.into()))
            .map_err(|e| {
                let reason = match e.kind() {
                    std::io::ErrorKind::NotFound => "not found".to_owned(),
                    std::io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
                    _ => e.to_string(),
                };
                fs_error(path, &reason)
            }),
        _ => Err(fs_error(path, "no host fs handler")),
    }
}

fn fs_error(path: &str, reason: &str) -> Value {
    Value::Data {
        name: "FsError".into(),
        fields: vec![
            ("path".into(), Value::Str(path.into())),
            ("reason".into(), Value::Str(reason.into())),
        ],
    }
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
