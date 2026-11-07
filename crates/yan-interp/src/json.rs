//! std.json: `Value`, `parse_value`, `dump_value`, `parse_float` and the accessors (R569 to R575).

use yan_rt::json::{self, Json, JsonError, Reason};
use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Key, Value, panic};
use Value::*;

fn data(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Data {
        name: name.into(),
        fields: fields.into_iter().map(|(n, v)| (n.into(), v)).collect(),
    }
}

fn to_value(j: Json) -> Value {
    match j {
        Json::Null => data("Null", vec![]),
        Json::Bool(b) => data("Bool", vec![("b", Bool(b))]),
        Json::Int(i) => data("Int", vec![("i", Int(i.into()))]),
        Json::Float(f) => data("Float", vec![("f", Float(f))]),
        Json::Str(s) => data("Str", vec![("s", Str(s.into()))]),
        Json::Array(xs) => data(
            "Array",
            vec![("items", List(xs.into_iter().map(to_value).collect()))],
        ),
        Json::Object(kv) => data(
            "Object",
            vec![(
                "fields",
                Dict(
                    kv.into_iter()
                        .map(|(k, v)| (Key(Str(k.into())), to_value(v)))
                        .collect(),
                ),
            )],
        ),
    }
}

/// `None` when `v` is not a `Value` the writer can take: an `Int` beyond `i64`.
fn from_value(v: &Value) -> Option<Json> {
    let Data { name, fields } = v else {
        unreachable!("a json.Value")
    };
    Some(match (name.as_str(), fields.as_slice()) {
        ("Null", []) => Json::Null,
        ("Bool", [(_, Bool(b))]) => Json::Bool(*b),
        ("Int", [(_, Int(i))]) => Json::Int(i64::try_from(*i).ok()?),
        ("Float", [(_, Float(f))]) => Json::Float(*f),
        ("Str", [(_, Str(s))]) => Json::Str(s.to_string()),
        ("Array", [(_, List(xs))]) => {
            Json::Array(xs.iter().map(from_value).collect::<Option<_>>()?)
        }
        ("Object", [(_, Dict(d))]) => Json::Object(
            d.iter()
                .map(|(k, v)| {
                    let Str(k) = &k.0 else {
                        unreachable!("a str key")
                    };
                    Some((k.to_string(), from_value(v)?))
                })
                .collect::<Option<_>>()?,
        ),
        _ => unreachable!("a json.Value"),
    })
}

pub(super) fn error(e: JsonError) -> Value {
    let reason = match e.reason {
        Reason::Syntax(msg) => data("Syntax", vec![("msg", Str(msg.into()))]),
        Reason::Depth => data("Depth", vec![]),
        Reason::Duplicate(key) => data("Duplicate", vec![("key", Str(key.into()))]),
        Reason::Range => data("Range", vec![]),
        _ => unreachable!("typed reasons come from jsonwire"),
    };
    data(
        "JsonError",
        vec![
            ("line", Int(e.line as i128)),
            ("col", Int(e.col as i128)),
            ("reason", reason),
        ],
    )
}

/// `line L, col C: reason` (R573).
pub(super) fn show_error(fields: &[(super::Name, Value)]) -> String {
    let [
        (_, Int(line)),
        (_, Int(col)),
        (_, Data { name, fields: rf }),
    ] = fields
    else {
        unreachable!("a JsonError")
    };
    let reason = match (name.as_str(), rf.as_slice()) {
        ("Syntax", [(_, Str(msg))]) => msg.to_string(),
        ("Duplicate", [(_, Str(key))]) => format!("duplicate key {key:?}"),
        ("Depth", _) => "nesting exceeds max_depth".into(),
        ("Mismatch", [(_, Str(p)), (_, Str(e)), (_, Str(f))]) => {
            format!("{p}: expected {e}, found {f}")
        }
        ("Missing", [(_, Str(p)), (_, Str(f))]) => format!("{p}: missing field {f:?}"),
        ("Unknown", [(_, Str(p)), (_, Str(f))]) => format!("{p}: unknown field {f:?}"),
        ("Invalid", [(_, Str(p)), (_, Str(m))]) => format!("{p}: {m}"),
        _ => "number out of range".into(),
    };
    format!("line {line}, col {col}: {reason}")
}

fn depth(n: &i128) -> usize {
    usize::try_from(*n).unwrap_or(0)
}

/// The payload of variant `want` in `v`.
fn field<'a>(v: &'a Value, want: &str) -> Option<&'a Value> {
    match v {
        Data { name, fields } if name == want => fields.first().map(|f| &f.1),
        _ => None,
    }
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_JSON_PARSE_VALUE,
        Bind::Fallible(|a| match a {
            [Str(s), Int(n)] => Ok(json::parse(s, depth(n)).map(to_value).map_err(error)),
            _ => bad_args(STD_JSON_PARSE_VALUE),
        }),
    ),
    (
        STD_JSON_PARSE_FLOAT,
        Bind::Fallible(|a| match a {
            [Str(s), Int(n)] => Ok(json::parse_float(s, depth(n)).map(Float).map_err(error)),
            _ => bad_args(STD_JSON_PARSE_FLOAT),
        }),
    ),
    (
        STD_JSON_DUMP_VALUE,
        Bind::Plain(|a| {
            let (v, indent) = match a {
                [v, Data { name, .. }] if name == "None" => (v, None),
                [v, Data { fields, .. }] => match fields.as_slice() {
                    [(_, Int(n))] => (v, Some(depth(n))),
                    _ => return bad_args(STD_JSON_DUMP_VALUE),
                },
                _ => return bad_args(STD_JSON_DUMP_VALUE),
            };
            let Some(j) = from_value(v) else {
                return panic("value");
            };
            match json::dump(&j, indent) {
                Ok(s) => Ok(Str(s.into())),
                Err(json::NonFinite) => panic("value"),
            }
        }),
    ),
    (
        STD_JSON_GET,
        Bind::Plain(|a| match a {
            [v, Str(k)] => Ok(opt(match field(v, "Object") {
                Some(Dict(d)) => d.get(&Key(Str(k.clone()))).cloned(),
                _ => None,
            })),
            _ => bad_args(STD_JSON_GET),
        }),
    ),
    (
        STD_JSON_AT,
        Bind::Plain(|a| match a {
            [v, Int(i)] => Ok(opt(match field(v, "Array") {
                Some(List(xs)) => usize::try_from(*i).ok().and_then(|i| xs.get(i)).cloned(),
                _ => None,
            })),
            _ => bad_args(STD_JSON_AT),
        }),
    ),
    (
        STD_JSON_AS_BOOL,
        Bind::Plain(|a| Ok(opt(a.first().and_then(|v| field(v, "Bool")).cloned()))),
    ),
    (
        STD_JSON_AS_INT,
        Bind::Plain(|a| Ok(opt(a.first().and_then(|v| field(v, "Int")).cloned()))),
    ),
    (
        STD_JSON_AS_FLOAT,
        Bind::Plain(|a| {
            let v = a.first();
            Ok(opt(match v.and_then(|v| field(v, "Float")) {
                Some(f) => Some(f.clone()),
                // An `Int` widens exactly or gives `None` (R574).
                None => match v.and_then(|v| field(v, "Int")) {
                    Some(Int(i)) if i.unsigned_abs() <= 1 << 53 => Some(Float(*i as f64)),
                    _ => None,
                },
            }))
        }),
    ),
    (
        STD_JSON_AS_STR,
        Bind::Plain(|a| Ok(opt(a.first().and_then(|v| field(v, "Str")).cloned()))),
    ),
    (
        STD_JSON_AS_LIST,
        Bind::Plain(|a| Ok(opt(a.first().and_then(|v| field(v, "Array")).cloned()))),
    ),
    (
        STD_JSON_AS_DICT,
        Bind::Plain(|a| Ok(opt(a.first().and_then(|v| field(v, "Object")).cloned()))),
    ),
    (
        STD_JSON_IS_NULL,
        Bind::Plain(|a| {
            Ok(Bool(
                matches!(a.first(), Some(Data { name, .. }) if name == "Null"),
            ))
        }),
    ),
];
