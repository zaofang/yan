//! `json.parse[T]` and `json.dump[T]` over `derive(Wire)` types (17.3, R874 to R877). The
//! elaborator passes the type's schema as JSON; see `yan-elab/src/jsonwire.rs` for its shape.

use serde_json::Value as S;
use yan_rt::json::{self, Json, Pos, Reason};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Key, Value, panic};
use Value::*;

/// The tag key of a sum's object, `@wire(tag: ...)` or `tag` (R886).
fn tag(def: &S) -> &str {
    def["tag"].as_str().unwrap_or("tag")
}

/// Whether the type has `@wire(unknown: "ignore")` (R886).
fn ignores(def: &S) -> bool {
    def["unk"].as_bool().unwrap_or(false)
}

type Fail = (usize, Reason);
type D<T> = Result<T, Fail>;

struct Cx<'a> {
    schema: &'a S,
}

fn data(name: &str, fields: Vec<(String, Value)>) -> Value {
    Data {
        name: name.into(),
        fields: fields.into_iter().collect(),
    }
}

/// The key or tag value a field or variant has on the wire.
fn wire(d: &S) -> &str {
    d["w"].as_str().or_else(|| d["n"].as_str()).unwrap_or("")
}

fn kind(j: &Json) -> &'static str {
    match j {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

fn expected(t: &S) -> String {
    match t {
        S::String(s) => s.clone(),
        S::Object(o) => match o.iter().next() {
            Some((k, S::String(n))) if k == "named" => n.clone(),
            Some((k, _)) => match k.as_str() {
                "opt" | "list" | "set" => "array".into(),
                "dict" => "object".into(),
                "tuple" => "array".into(),
                _ => k.clone(),
            },
            None => String::new(),
        },
        _ => String::new(),
    }
}

fn from_schema(s: &S) -> Json {
    match s {
        S::Null => Json::Null,
        S::Bool(b) => Json::Bool(*b),
        S::Number(n) => n
            .as_i64()
            .map_or_else(|| Json::Float(n.as_f64().unwrap_or(0.0)), Json::Int),
        S::String(s) => Json::Str(s.clone()),
        S::Array(xs) => Json::Array(xs.iter().map(from_schema).collect()),
        S::Object(o) => Json::Object(o.iter().map(|(k, v)| (k.clone(), from_schema(v))).collect()),
    }
}

fn kid(p: &Pos, i: usize) -> &Pos {
    p.kids.get(i).unwrap_or(p)
}

impl Cx<'_> {
    fn def(&self, n: &str) -> &S {
        &self.schema["defs"][n]
    }

    fn mismatch(&self, path: &str, t: &S, j: &Json, p: &Pos) -> Fail {
        (
            p.at,
            Reason::Mismatch {
                path: path.into(),
                expected: expected(t),
                found: kind(j).into(),
            },
        )
    }

    fn decode(&self, t: &S, j: &Json, p: &Pos, path: &str) -> D<Value> {
        let bad = || self.mismatch(path, t, j, p);
        let invalid = |msg: &str| {
            (
                p.at,
                Reason::Invalid {
                    path: path.into(),
                    msg: msg.into(),
                },
            )
        };
        Ok(match (t, j) {
            (S::String(n), _) => match (n.as_str(), j) {
                ("bool", Json::Bool(b)) => Bool(*b),
                ("int", Json::Int(i)) => Int(i128::from(*i)),
                ("float", Json::Float(f)) => Float(*f),
                ("float", Json::Int(i)) if i.unsigned_abs() <= 1 << 53 => Float(*i as f64),
                ("float", Json::Int(_)) => return Err((p.at, Reason::Range)),
                ("str", Json::Str(s)) => Str(s.as_str().into()),
                ("char", Json::Str(s)) if s.chars().count() == 1 => {
                    Char(s.chars().next().unwrap_or_default())
                }
                ("char", Json::Str(_)) => return Err(invalid("expected one character")),
                _ => return Err(bad()),
            },
            (S::Object(o), _) => {
                let (k, v) = o.iter().next().expect("a schema");
                match (k.as_str(), v, j) {
                    ("opt", _, Json::Null) => opt(None),
                    ("opt", t, _) => opt(Some(self.decode(t, j, p, path)?)),
                    ("list", t, Json::Array(xs)) => List(
                        xs.iter()
                            .enumerate()
                            .map(|(i, x)| self.decode(t, x, kid(p, i), &format!("{path}[{i}]")))
                            .collect::<D<_>>()?,
                    ),
                    ("set", t, Json::Array(xs)) => {
                        let mut out = indexmap::IndexSet::default();
                        for (i, x) in xs.iter().enumerate() {
                            let v = self.decode(t, x, kid(p, i), &format!("{path}[{i}]"))?;
                            if !out.insert(Key(v)) {
                                return Err((
                                    kid(p, i).at,
                                    Reason::Invalid {
                                        path: format!("{path}[{i}]"),
                                        msg: "duplicate element".into(),
                                    },
                                ));
                            }
                        }
                        Set(out)
                    }
                    ("tuple", ts, Json::Array(xs)) => {
                        let ts = ts.as_array().expect("a schema");
                        if ts.len() != xs.len() {
                            return Err(bad());
                        }
                        Tuple(
                            ts.iter()
                                .zip(xs)
                                .enumerate()
                                .map(|(i, (t, x))| {
                                    self.decode(t, x, kid(p, i), &format!("{path}[{i}]"))
                                })
                                .collect::<D<_>>()?,
                        )
                    }
                    ("dict", kv, Json::Object(fs)) if kv[0] == "str" => {
                        let mut out = indexmap::IndexMap::default();
                        for (i, (name, x)) in fs.iter().enumerate() {
                            let v = self.decode(&kv[1], x, kid(p, i), &format!("{path}.{name}"))?;
                            out.insert(Key(Str(name.as_str().into())), v);
                        }
                        Dict(out)
                    }
                    ("dict", kv, Json::Array(xs)) if kv[0] != "str" => {
                        let mut out = indexmap::IndexMap::default();
                        for (i, x) in xs.iter().enumerate() {
                            let here = format!("{path}[{i}]");
                            let Json::Array(pair) = x else {
                                return Err(self.mismatch(&here, &S::from("array"), x, kid(p, i)));
                            };
                            if pair.len() != 2 {
                                return Err(self.mismatch(&here, &S::from("array"), x, kid(p, i)));
                            }
                            let q = kid(p, i);
                            let k =
                                self.decode(&kv[0], &pair[0], kid(q, 0), &format!("{here}[0]"))?;
                            let v =
                                self.decode(&kv[1], &pair[1], kid(q, 1), &format!("{here}[1]"))?;
                            if out.insert(Key(k), v).is_some() {
                                return Err((
                                    q.at,
                                    Reason::Invalid {
                                        path: here,
                                        msg: "duplicate key".into(),
                                    },
                                ));
                            }
                        }
                        Dict(out)
                    }
                    ("named", S::String(n), _) => self.named(n, t, j, p, path)?,
                    _ => return Err(bad()),
                }
            }
            _ => return Err(bad()),
        })
    }

    fn named(&self, n: &str, t: &S, j: &Json, p: &Pos, path: &str) -> D<Value> {
        let Json::Object(fs) = j else {
            return Err(self.mismatch(path, t, j, p));
        };
        let def = self.def(n);
        if let Some(rec) = def.get("rec") {
            return Ok(data(n, self.fields(rec, fs, p, path, None, ignores(def))?));
        }
        let vs = def["sum"].as_array().expect("a schema");
        let names = || vs.iter().map(wire).collect::<Vec<_>>().join("|");
        let key = tag(def);
        let Some(i) = fs.iter().position(|(k, _)| k == key) else {
            return Err((
                p.at,
                Reason::Missing {
                    path: path.into(),
                    field: key.into(),
                },
            ));
        };
        let tag_path = format!("{path}.{key}");
        let Json::Str(tag) = &fs[i].1 else {
            return Err((
                kid(p, i).at,
                Reason::Mismatch {
                    path: tag_path,
                    expected: "str".into(),
                    found: kind(&fs[i].1).into(),
                },
            ));
        };
        let Some(v) = vs.iter().find(|v| wire(v) == tag.as_str()) else {
            return Err((
                kid(p, i).at,
                Reason::Mismatch {
                    path: tag_path,
                    expected: names(),
                    found: tag.clone(),
                },
            ));
        };
        let name = v["n"].as_str().unwrap_or("");
        Ok(data(
            name,
            self.fields(&v["fs"], fs, p, path, Some(i), ignores(def))?,
        ))
    }

    /// The members of `fs` against the declared fields `decl`; `skip` is the tag member.
    fn fields(
        &self,
        decl: &S,
        fs: &[(String, Json)],
        p: &Pos,
        path: &str,
        skip: Option<usize>,
        ignore: bool,
    ) -> D<Vec<(String, Value)>> {
        let decl = decl.as_array().expect("a schema");
        for (i, (k, _)) in fs.iter().enumerate() {
            if !ignore && Some(i) != skip && !decl.iter().any(|f| wire(f) == k.as_str()) {
                return Err((
                    kid(p, i).at,
                    Reason::Unknown {
                        path: path.into(),
                        field: k.clone(),
                    },
                ));
            }
        }
        let mut out = Vec::new();
        for f in decl {
            let name = f["n"].as_str().unwrap_or("");
            let key = wire(f);
            let here = format!("{path}.{key}");
            let v = match fs.iter().position(|(k, _)| k == key) {
                Some(i) => self.decode(&f["t"], &fs[i].1, kid(p, i), &here)?,
                None if f.get("d").is_some() => self.decode(
                    &f["t"],
                    &from_schema(&f["d"]),
                    &Pos {
                        at: p.at,
                        kids: vec![],
                    },
                    &here,
                )?,
                None if f["t"].get("opt").is_some() => opt(None),
                None => {
                    return Err((
                        p.at,
                        Reason::Missing {
                            path: path.into(),
                            field: key.into(),
                        },
                    ));
                }
            };
            out.push((name.to_owned(), v));
        }
        Ok(out)
    }

    fn encode(&self, t: &S, v: &Value) -> Result<Json, ()> {
        Ok(match (t, v) {
            (S::String(n), _) => match (n.as_str(), v) {
                ("bool", Bool(b)) => Json::Bool(*b),
                ("int", Int(i)) => Json::Int(i64::try_from(*i).map_err(|_| ())?),
                ("float", Float(f)) if f.is_finite() => Json::Float(*f),
                ("str", Str(s)) => Json::Str(s.to_string()),
                ("char", Char(c)) => Json::Str(c.to_string()),
                ("unit", Unit) => Json::Null,
                _ => return Err(()),
            },
            (S::Object(o), _) => {
                let (k, t) = o.iter().next().expect("a schema");
                match (k.as_str(), v) {
                    ("opt", Data { name, fields }) => match (name.as_str(), fields.as_slice()) {
                        ("None", _) => Json::Null,
                        (_, [(_, x)]) => self.encode(t, x)?,
                        _ => return Err(()),
                    },
                    ("list", List(xs)) => Json::Array(
                        xs.iter()
                            .map(|x| self.encode(t, x))
                            .collect::<Result<_, _>>()?,
                    ),
                    ("set", Set(xs)) => Json::Array(
                        xs.iter()
                            .map(|x| self.encode(t, &x.0))
                            .collect::<Result<_, _>>()?,
                    ),
                    ("tuple", Tuple(xs)) => Json::Array(
                        t.as_array()
                            .expect("a schema")
                            .iter()
                            .zip(xs)
                            .map(|(t, x)| self.encode(t, x))
                            .collect::<Result<_, _>>()?,
                    ),
                    ("dict", Dict(d)) if t[0] == "str" => Json::Object(
                        d.iter()
                            .map(|(k, x)| match &k.0 {
                                Str(s) => Ok((s.to_string(), self.encode(&t[1], x)?)),
                                _ => Err(()),
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                    ("dict", Dict(d)) => Json::Array(
                        d.iter()
                            .map(|(k, x)| {
                                Ok(Json::Array(vec![
                                    self.encode(&t[0], &k.0)?,
                                    self.encode(&t[1], x)?,
                                ]))
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                    ("named", Data { name, fields }) => {
                        let S::String(n) = t else { return Err(()) };
                        let def = self.def(n);
                        let mut out = Vec::new();
                        let decl = if let Some(rec) = def.get("rec") {
                            rec
                        } else {
                            let vs = def["sum"].as_array().expect("a schema");
                            let v = vs.iter().find(|v| v["n"] == name.as_str()).ok_or(())?;
                            out.push((tag(def).to_owned(), Json::Str(wire(v).into())));
                            &v["fs"]
                        };
                        for (f, (_, x)) in decl.as_array().expect("a schema").iter().zip(fields) {
                            out.push((wire(f).to_owned(), self.encode(&f["t"], x)?));
                        }
                        Json::Object(out)
                    }
                    _ => return Err(()),
                }
            }
            _ => return Err(()),
        })
    }
}

fn error(s: &str, (at, reason): Fail) -> Value {
    let (line, col) = json::line_col(s, at);
    let text = |s: &str| Str(s.into());
    let reason = match reason {
        Reason::Syntax(msg) => data("Syntax", vec![("msg".into(), text(&msg))]),
        Reason::Depth => data("Depth", vec![]),
        Reason::Duplicate(key) => data("Duplicate", vec![("key".into(), text(&key))]),
        Reason::Range => data("Range", vec![]),
        Reason::Mismatch {
            path,
            expected,
            found,
        } => data(
            "Mismatch",
            vec![
                ("path".into(), text(&path)),
                ("expected".into(), text(&expected)),
                ("found".into(), text(&found)),
            ],
        ),
        Reason::Missing { path, field } => data(
            "Missing",
            vec![("path".into(), text(&path)), ("field".into(), text(&field))],
        ),
        Reason::Unknown { path, field } => data(
            "Unknown",
            vec![("path".into(), text(&path)), ("field".into(), text(&field))],
        ),
        Reason::Invalid { path, msg } => data(
            "Invalid",
            vec![("path".into(), text(&path)), ("msg".into(), text(&msg))],
        ),
    };
    data(
        "JsonError",
        vec![
            ("line".into(), Int(line as i128)),
            ("col".into(), Int(col as i128)),
            ("reason".into(), reason),
        ],
    )
}

fn schema(s: &str) -> S {
    serde_json::from_str(s).expect("the elaborator's schema")
}

/// A served tool's arguments object, decoded against its `$args` record into the parameters in
/// order (RFC 0062); `Err` is the `JsonError` value.
pub(crate) fn tool_args(sc: &S, text: &str) -> Result<Vec<Value>, Value> {
    let (j, pos) = json::parse_pos(text, 128).map_err(super::json::error)?;
    match (Cx { schema: sc }).decode(&sc["args"], &j, &pos, "$") {
        Ok(Data { fields, .. }) => Ok(fields.into_iter().map(|(_, v)| v).collect()),
        Ok(_) => unreachable!("`$args` is a record"),
        Err(f) => Err(error(text, f)),
    }
}

/// A served tool's result as json text, `None` for a non-finite float.
pub(crate) fn tool_result(sc: &S, v: &Value) -> Option<String> {
    let j = (Cx { schema: sc }).encode(&sc["ret"], v).ok()?;
    json::dump(&j, None).ok()
}

pub(super) static BINDS: &[(yan_std::registry::Id, Bind)] = &[
    (
        yan_std::registry::id::STD_JSON_PARSE,
        Bind::Fallible(|a| {
            let [Str(text), Str(sc)] = a else {
                return bad_args(yan_std::registry::id::STD_JSON_PARSE);
            };
            let sc = schema(sc);
            let (j, pos) = match json::parse_pos(text, 128) {
                Ok(x) => x,
                Err(e) => return Ok(Err(super::json::error(e))),
            };
            let cx = Cx { schema: &sc };
            Ok(cx
                .decode(&sc["root"], &j, &pos, "$")
                .map_err(|f| error(text, f)))
        }),
    ),
    (
        yan_std::registry::id::STD_JSON_DUMP,
        Bind::Plain(|a| {
            let [v, Str(sc)] = a else {
                return bad_args(yan_std::registry::id::STD_JSON_DUMP);
            };
            let sc = schema(sc);
            let cx = Cx { schema: &sc };
            match cx.encode(&sc["root"], v).map(|j| json::dump(&j, None)) {
                Ok(Ok(s)) => Ok(Str(s.into())),
                _ => panic("value"),
            }
        }),
    ),
];
