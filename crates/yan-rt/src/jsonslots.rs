//! `json.parse[T]` and `json.dump[T]` over native slots (17.3, R874 to R877): the same schema
//! and rules as `yan-interp/src/jsonwire.rs`, on the slot layout of `yan-mir`'s shapes. A value
//! is its slots in order; a record is its fields, a sum is a tag then every variant's fields,
//! `Option` is a sum of `Some` (tag 0) and `None`, a tuple is its parts.

use crate::json::{self, Json, Pos, Reason};

/// The tag key of a sum's object, `@wire(tag: ...)` or `tag` (R886).
fn tag(def: &Json) -> &str {
    match get(def, "tag") {
        Some(Json::Str(s)) => s,
        _ => "tag",
    }
}

/// Whether the type has `@wire(unknown: "ignore")` (R886).
fn ignores(def: &Json) -> bool {
    matches!(get(def, "unk"), Some(Json::Bool(true)))
}

/// A slot before its blocks exist, so a failed decode leaks nothing.
#[derive(Debug, PartialEq)]
pub enum Slot {
    Word(u64),
    Str(String),
    /// A list whose elements take `words` slots each.
    List(usize, Vec<Slot>),
    /// A dict with keys encoded by `schema` (`crate::map`), as key and value slots in order.
    Map {
        schema: String,
        kw: usize,
        vw: usize,
        entries: Vec<(Vec<Slot>, Vec<Slot>)>,
    },
}

type Fail = (usize, Reason);
type D = Result<(), Fail>;

pub(crate) fn get<'a>(o: &'a Json, k: &str) -> Option<&'a Json> {
    match o {
        Json::Object(fs) => fs.iter().find(|(n, _)| n == k).map(|(_, v)| v),
        _ => None,
    }
}

pub(crate) fn items(j: &Json) -> &[Json] {
    match j {
        Json::Array(xs) => xs,
        _ => &[],
    }
}

pub(crate) fn text(j: Option<&Json>) -> &str {
    match j {
        Some(Json::Str(s)) => s,
        _ => "",
    }
}

/// The key or tag value a field or variant has on the wire.
pub(crate) fn wire(d: &Json) -> &str {
    match get(d, "w") {
        Some(Json::Str(s)) => s,
        _ => text(get(d, "n")),
    }
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

fn expected(t: &Json) -> String {
    match t {
        Json::Str(s) => s.clone(),
        Json::Object(o) => match o.first() {
            Some((k, Json::Str(n))) if k == "named" => n.clone(),
            Some((k, _)) => match k.as_str() {
                "opt" | "list" | "tuple" => "array".into(),
                "dict" => "object".into(),
                k => k.into(),
            },
            None => String::new(),
        },
        _ => String::new(),
    }
}

fn kid(p: &Pos, i: usize) -> &Pos {
    p.kids.get(i).unwrap_or(p)
}

struct Cx<'a> {
    defs: &'a Json,
}

impl Cx<'_> {
    fn def(&self, n: &str) -> &Json {
        get(self.defs, n).unwrap_or(&Json::Null)
    }

    /// The slots of a value of type `t`.
    fn width(&self, t: &Json) -> usize {
        match t {
            Json::Str(_) => 1,
            Json::Object(o) => match o.first() {
                Some((k, v)) => match k.as_str() {
                    "opt" => 1 + self.width(v),
                    "tuple" => items(v).iter().map(|t| self.width(t)).sum(),
                    "named" => {
                        let def = self.def(text(Some(v)));
                        if let Some(rec) = get(def, "rec") {
                            self.fields_width(rec)
                        } else {
                            let vs = get(def, "sum").map_or(&[][..], items);
                            1 + vs
                                .iter()
                                .map(|v| self.fields_width(get(v, "fs").unwrap_or(&Json::Null)))
                                .sum::<usize>()
                        }
                    }
                    _ => 1,
                },
                None => 0,
            },
            _ => 0,
        }
    }

    /// The `crate::map` key schema of type `t`, if it can be a native dict key.
    fn key_schema(&self, t: &Json, out: &mut String) -> Option<()> {
        let fields = |me: &Self, fs: &Json, out: &mut String| {
            items(fs)
                .iter()
                .try_for_each(|f| me.key_schema(get(f, "t")?, out))
        };
        match t {
            Json::Str(n) => out.push_str(match n.as_str() {
                "bool" => "b",
                "int" => "i64",
                "float" => "f",
                "str" => "s",
                _ => return None,
            }),
            Json::Object(o) => {
                let (k, v) = o.first()?;
                match k.as_str() {
                    "opt" => {
                        out.push_str("<Some|");
                        self.key_schema(v, out)?;
                        out.push_str(";None|;>");
                    }
                    "tuple" => {
                        out.push('(');
                        for t in items(v) {
                            self.key_schema(t, out)?;
                        }
                        out.push(')');
                    }
                    "named" => {
                        let n = text(Some(v));
                        let def = self.def(n);
                        if let Some(rec) = get(def, "rec") {
                            out.push('{');
                            out.push_str(n);
                            out.push('|');
                            fields(self, rec, out)?;
                            out.push('}');
                        } else {
                            out.push('<');
                            for v in get(def, "sum").map_or(&[][..], items) {
                                out.push_str(text(get(v, "n")));
                                out.push('|');
                                fields(self, get(v, "fs").unwrap_or(&Json::Null), out)?;
                                out.push(';');
                            }
                            out.push('>');
                        }
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
        Some(())
    }

    fn fields_width(&self, fs: &Json) -> usize {
        items(fs)
            .iter()
            .map(|f| self.width(get(f, "t").unwrap_or(&Json::Null)))
            .sum()
    }

    fn mismatch(&self, path: &str, t: &Json, j: &Json, p: &Pos) -> Fail {
        (
            p.at,
            Reason::Mismatch {
                path: path.into(),
                expected: expected(t),
                found: kind(j).into(),
            },
        )
    }

    fn decode(&self, t: &Json, j: &Json, p: &Pos, path: &str, out: &mut Vec<Slot>) -> D {
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
        match t {
            Json::Str(n) => match (n.as_str(), j) {
                ("bool", Json::Bool(b)) => out.push(Slot::Word(u64::from(*b))),
                ("int", Json::Int(i)) => out.push(Slot::Word(*i as u64)),
                ("float", Json::Float(f)) => out.push(Slot::Word(f.to_bits())),
                ("float", Json::Int(i)) if i.unsigned_abs() <= 1 << 53 => {
                    out.push(Slot::Word((*i as f64).to_bits()));
                }
                ("float", Json::Int(_)) => return Err((p.at, Reason::Range)),
                ("str", Json::Str(s)) => out.push(Slot::Str(s.clone())),
                ("char", Json::Str(s)) if s.chars().count() == 1 => {
                    out.push(Slot::Word(s.chars().next().map_or(0, u64::from)));
                }
                ("char", Json::Str(_)) => return Err(invalid("expected one character")),
                _ => return Err(bad()),
            },
            Json::Object(o) => {
                let (k, v) = o.first().expect("a schema");
                match (k.as_str(), v, j) {
                    ("opt", _, Json::Null) => {
                        out.push(Slot::Word(1));
                        out.extend((0..self.width(v)).map(|_| Slot::Word(0)));
                    }
                    ("opt", t, _) => {
                        out.push(Slot::Word(0));
                        self.decode(t, j, p, path, out)?;
                    }
                    ("list", t, Json::Array(xs)) => {
                        let mut elems = Vec::new();
                        for (i, x) in xs.iter().enumerate() {
                            self.decode(t, x, kid(p, i), &format!("{path}[{i}]"), &mut elems)?;
                        }
                        out.push(Slot::List(self.width(t), elems));
                    }
                    ("dict", kv, _) => {
                        let [kt, vt] = items(kv) else {
                            return Err(bad());
                        };
                        let mut schema = String::new();
                        if self.key_schema(kt, &mut schema).is_none() {
                            return Err(invalid("a key type native code cannot hash yet"));
                        }
                        let mut entries: Vec<(Vec<Slot>, Vec<Slot>)> = Vec::new();
                        match (kt, j) {
                            (Json::Str(k), Json::Object(fs)) if k == "str" => {
                                for (i, (name, x)) in fs.iter().enumerate() {
                                    let mut v = Vec::new();
                                    self.decode(
                                        vt,
                                        x,
                                        kid(p, i),
                                        &format!("{path}.{name}"),
                                        &mut v,
                                    )?;
                                    let k = vec![Slot::Str(name.clone())];
                                    match entries.iter_mut().find(|(e, _)| *e == k) {
                                        Some(e) => e.1 = v,
                                        None => entries.push((k, v)),
                                    }
                                }
                            }
                            (Json::Str(k), _) if k == "str" => return Err(bad()),
                            (_, Json::Array(xs)) => {
                                for (i, x) in xs.iter().enumerate() {
                                    let here = format!("{path}[{i}]");
                                    let q = kid(p, i);
                                    let pair = match x {
                                        Json::Array(pair) if pair.len() == 2 => pair,
                                        _ => {
                                            return Err((
                                                q.at,
                                                Reason::Mismatch {
                                                    path: here,
                                                    expected: "array".into(),
                                                    found: kind(x).into(),
                                                },
                                            ));
                                        }
                                    };
                                    let (mut k, mut v) = (Vec::new(), Vec::new());
                                    self.decode(
                                        kt,
                                        &pair[0],
                                        kid(q, 0),
                                        &format!("{here}[0]"),
                                        &mut k,
                                    )?;
                                    self.decode(
                                        vt,
                                        &pair[1],
                                        kid(q, 1),
                                        &format!("{here}[1]"),
                                        &mut v,
                                    )?;
                                    if entries.iter().any(|(e, _)| *e == k) {
                                        return Err((
                                            q.at,
                                            Reason::Invalid {
                                                path: here,
                                                msg: "duplicate key".into(),
                                            },
                                        ));
                                    }
                                    entries.push((k, v));
                                }
                            }
                            _ => return Err(bad()),
                        }
                        out.push(Slot::Map {
                            schema,
                            kw: self.width(kt),
                            vw: self.width(vt),
                            entries,
                        });
                    }
                    ("tuple", ts, Json::Array(xs)) => {
                        let ts = items(ts);
                        if ts.len() != xs.len() {
                            return Err(bad());
                        }
                        for (i, (t, x)) in ts.iter().zip(xs).enumerate() {
                            self.decode(t, x, kid(p, i), &format!("{path}[{i}]"), out)?;
                        }
                    }
                    ("named", Json::Str(n), _) => self.named(n, t, j, p, path, out)?,
                    _ => return Err(bad()),
                }
            }
            _ => return Err(bad()),
        }
        Ok(())
    }

    fn named(&self, n: &str, t: &Json, j: &Json, p: &Pos, path: &str, out: &mut Vec<Slot>) -> D {
        let Json::Object(fs) = j else {
            return Err(self.mismatch(path, t, j, p));
        };
        let def = self.def(n);
        if let Some(rec) = get(def, "rec") {
            return self.fields(rec, fs, p, path, None, ignores(def), out);
        }
        let vs = get(def, "sum").map_or(&[][..], items);
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
        let Some(chosen) = vs.iter().position(|v| wire(v) == tag.as_str()) else {
            return Err((
                kid(p, i).at,
                Reason::Mismatch {
                    path: tag_path,
                    expected: names(),
                    found: tag.clone(),
                },
            ));
        };
        out.push(Slot::Word(chosen as u64));
        for (k, v) in vs.iter().enumerate() {
            let decl = get(v, "fs").unwrap_or(&Json::Null);
            if k == chosen {
                self.fields(decl, fs, p, path, Some(i), ignores(def), out)?;
            } else {
                out.extend((0..self.fields_width(decl)).map(|_| Slot::Word(0)));
            }
        }
        Ok(())
    }

    /// The members of `fs` against the declared fields `decl`; `skip` is the tag member.
    #[allow(clippy::too_many_arguments)]
    fn fields(
        &self,
        decl: &Json,
        fs: &[(String, Json)],
        p: &Pos,
        path: &str,
        skip: Option<usize>,
        ignore: bool,
        out: &mut Vec<Slot>,
    ) -> D {
        let decl = items(decl);
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
        for f in decl {
            let key = wire(f);
            let t = get(f, "t").unwrap_or(&Json::Null);
            let here = format!("{path}.{key}");
            match fs.iter().position(|(k, _)| k == key) {
                Some(i) => self.decode(t, &fs[i].1, kid(p, i), &here, out)?,
                None if get(f, "d").is_some() => {
                    let d = get(f, "d").unwrap_or(&Json::Null);
                    let at = Pos {
                        at: p.at,
                        kids: vec![],
                    };
                    self.decode(t, d, &at, &here, out)?;
                }
                None if get(t, "opt").is_some() => {
                    out.push(Slot::Word(1));
                    out.extend((0..self.width(t) - 1).map(|_| Slot::Word(0)));
                }
                None => {
                    return Err((
                        p.at,
                        Reason::Missing {
                            path: path.into(),
                            field: key.into(),
                        },
                    ));
                }
            }
        }
        Ok(())
    }

    /// Reads a value of type `t` from `w`, which is exactly its slots.
    ///
    /// # Safety
    /// The `Str` and `List` slots in `w` point to live blocks.
    unsafe fn encode(&self, t: &Json, w: &[u64]) -> Result<Json, ()> {
        Ok(match t {
            Json::Str(n) => match n.as_str() {
                "bool" => Json::Bool(w[0] & 0xff != 0),
                "int" => Json::Int(w[0] as i64),
                "float" if f64::from_bits(w[0]).is_finite() => Json::Float(f64::from_bits(w[0])),
                // SAFETY: the caller's contract.
                "str" => Json::Str(unsafe { &*(w[0] as *const String) }.clone()),
                "char" => Json::Str(char::from_u32(w[0] as u32).ok_or(())?.to_string()),
                _ => return Err(()),
            },
            Json::Object(o) => {
                let (k, t) = o.first().ok_or(())?;
                match k.as_str() {
                    "opt" if w[0] == 1 => Json::Null,
                    // SAFETY: forwarded.
                    "opt" => unsafe { self.encode(t, &w[1..])? },
                    "list" => {
                        // SAFETY: the caller's contract.
                        let l = unsafe { &*(w[0] as *const crate::abi::List) };
                        let n = self.width(t);
                        let xs = if n == 0 {
                            vec![]
                        } else {
                            l.data.chunks(n).collect()
                        };
                        Json::Array(
                            xs.into_iter()
                                // SAFETY: elements are values of `t`.
                                .map(|x| unsafe { self.encode(t, x) })
                                .collect::<Result<_, _>>()?,
                        )
                    }
                    "dict" => {
                        let [kt, vt] = items(t) else {
                            return Err(());
                        };
                        // SAFETY: the caller's contract.
                        let m = unsafe { &*(w[0] as *const crate::map::Map) };
                        let (kw, vw) = (self.width(kt), self.width(vt));
                        let entries = m.entries().map(|e| {
                            // SAFETY: entries are a key of `kt` then a value of `vt`.
                            unsafe {
                                Ok((
                                    self.encode(kt, &e[..kw])?,
                                    self.encode(vt, &e[kw..kw + vw])?,
                                ))
                            }
                        });
                        if *kt == Json::Str("str".into()) {
                            Json::Object(
                                entries
                                    .map(|e| match e? {
                                        (Json::Str(k), v) => Ok((k, v)),
                                        _ => Err(()),
                                    })
                                    .collect::<Result<_, _>>()?,
                            )
                        } else {
                            Json::Array(
                                entries
                                    .map(|e| e.map(|(k, v)| Json::Array(vec![k, v])))
                                    .collect::<Result<_, _>>()?,
                            )
                        }
                    }
                    "tuple" => {
                        let mut at = 0;
                        let mut out = Vec::new();
                        for t in items(t) {
                            let n = self.width(t);
                            // SAFETY: forwarded.
                            out.push(unsafe { self.encode(t, &w[at..at + n])? });
                            at += n;
                        }
                        Json::Array(out)
                    }
                    "named" => {
                        let def = self.def(text(Some(t)));
                        let mut out = Vec::new();
                        let (decl, mut at) = if let Some(rec) = get(def, "rec") {
                            (rec, 0)
                        } else {
                            let vs = get(def, "sum").map_or(&[][..], items);
                            let chosen = w[0] as usize;
                            let mut at = 1;
                            for v in &vs[..chosen] {
                                at += self.fields_width(get(v, "fs").unwrap_or(&Json::Null));
                            }
                            let v = vs.get(chosen).ok_or(())?;
                            out.push((tag(def).to_owned(), Json::Str(wire(v).into())));
                            (get(v, "fs").unwrap_or(&Json::Null), at)
                        };
                        for f in items(decl) {
                            let t = get(f, "t").unwrap_or(&Json::Null);
                            let n = self.width(t);
                            // SAFETY: forwarded.
                            out.push((wire(f).to_owned(), unsafe {
                                self.encode(t, &w[at..at + n])?
                            }));
                            at += n;
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

pub(crate) fn word(x: usize) -> Slot {
    Slot::Word(x as u64)
}

/// A `JsonError`: line, column, then the `JsonReason` as a tag and every variant's fields.
fn error(src: &str, at: usize, reason: Reason) -> Vec<Slot> {
    let (line, col) = json::line_col(src, at);
    let mut r: Vec<Slot> = Vec::new();
    let null = || Slot::Word(0);
    let (tag, pick): (usize, Vec<Slot>) = match reason {
        Reason::Syntax(m) => (0, vec![Slot::Str(m)]),
        Reason::Depth => (1, vec![]),
        Reason::Duplicate(k) => (2, vec![Slot::Str(k)]),
        Reason::Range => (3, vec![]),
        Reason::Mismatch {
            path,
            expected,
            found,
        } => (
            4,
            vec![Slot::Str(path), Slot::Str(expected), Slot::Str(found)],
        ),
        Reason::Missing { path, field } => (5, vec![Slot::Str(path), Slot::Str(field)]),
        Reason::Unknown { path, field } => (6, vec![Slot::Str(path), Slot::Str(field)]),
        Reason::Invalid { path, msg } => (7, vec![Slot::Str(path), Slot::Str(msg)]),
    };
    // Field counts of Syntax, Depth, Duplicate, Range, Mismatch, Missing, Unknown, Invalid, the
    // variants of the std `JsonReason`.
    let mut pick = pick.into_iter();
    r.push(word(line));
    r.push(word(col));
    r.push(word(tag));
    for (i, n) in [1, 0, 1, 0, 3, 2, 2, 2].into_iter().enumerate() {
        for _ in 0..n {
            r.push(if i == tag {
                pick.next().unwrap_or_else(null)
            } else {
                null()
            });
        }
    }
    r
}

/// The slots of `Result[T, JsonError]` for parsing `src` against `schema`.
pub fn parse(src: &str, schema: &str) -> Vec<Slot> {
    let sc = json::parse(schema, 128).expect("the elaborator's schema");
    let cx = Cx {
        defs: get(&sc, "defs").unwrap_or(&Json::Null),
    };
    let root = get(&sc, "root").unwrap_or(&Json::Null);
    let ok_width = cx.width(root);
    let failed = |slots: Vec<Slot>| {
        let mut out = vec![Slot::Word(1)];
        out.extend((0..ok_width).map(|_| Slot::Word(0)));
        out.extend(slots);
        out
    };
    let (j, pos) = match json::parse_pos(src, 128) {
        Ok(x) => x,
        Err(e) => return failed(syntax_error(&e)),
    };
    let mut value = vec![Slot::Word(0)];
    match cx.decode(root, &j, &pos, "$", &mut value) {
        Ok(()) => {
            // Ok is tag 0; the Err slots stay zero.
            value.extend((0..14).map(|_| Slot::Word(0)));
            value
        }
        Err((at, reason)) => failed(error(src, at, reason)),
    }
}

fn syntax_error(e: &json::JsonError) -> Vec<Slot> {
    let mut slots = error("", 0, e.reason.clone());
    slots[0] = word(e.line);
    slots[1] = word(e.col);
    slots
}

/// The json text of the value in `w` against `schema`, or `None` when it has no json form
/// (a non-finite float).
///
/// # Safety
/// `w` holds the slots of the schema's root type; its `Str` and `List` slots point to live blocks.
pub unsafe fn dump(w: &[u64], schema: &str) -> Option<String> {
    let sc = json::parse(schema, 128).expect("the elaborator's schema");
    let cx = Cx {
        defs: get(&sc, "defs").unwrap_or(&Json::Null),
    };
    // SAFETY: the caller's contract.
    let j = unsafe { cx.encode(get(&sc, "root")?, w) }.ok()?;
    json::dump(&j, None).ok()
}

/// The slot count of the schema's root type.
pub fn root_width(schema: &str) -> usize {
    let sc = json::parse(schema, 128).expect("the elaborator's schema");
    let cx = Cx {
        defs: get(&sc, "defs").unwrap_or(&Json::Null),
    };
    cx.width(get(&sc, "root").unwrap_or(&Json::Null))
}

/// `line L, col C: reason` (R573) for the 14 slots of a `JsonError`.
///
/// # Safety
/// The `Str` slots of the chosen variant point to live blocks.
pub unsafe fn show_error(w: &[u64]) -> String {
    // SAFETY: the caller's contract.
    let st = |i: usize| unsafe { &*(w[i] as *const String) };
    let reason = match w[2] {
        0 => st(3).clone(),
        1 => "nesting exceeds max_depth".into(),
        2 => format!("duplicate key {:?}", st(4)),
        4 => format!("{}: expected {}, found {}", st(5), st(6), st(7)),
        5 => format!("{}: missing field {:?}", st(8), st(9)),
        6 => format!("{}: unknown field {:?}", st(10), st(11)),
        7 => format!("{}: {}", st(12), st(13)),
        _ => "number out of range".into(),
    };
    format!("line {}, col {}: {reason}", w[0] as i64, w[1] as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"{"root":{"named":"P"},"defs":{"P":{"rec":[{"n":"a","t":"int"},{"n":"b","t":{"opt":"str"}}]}}}"#;

    #[test]
    fn parse_lays_out_a_result() {
        let r = parse(r#"{"a":7}"#, SCHEMA);
        // Ok tag, a, b's tag and payload, then 14 zero Err slots
        assert_eq!(r.len(), 1 + 3 + 14);
        assert!(matches!(r[0], Slot::Word(0)));
        assert!(matches!(r[1], Slot::Word(7)));
        assert!(matches!(r[2], Slot::Word(1)));
    }

    #[test]
    fn parse_failure_is_an_err_with_a_position() {
        let r = parse(r#"{"a":"x"}"#, SCHEMA);
        assert!(matches!(r[0], Slot::Word(1)));
        assert!(matches!(r[4], Slot::Word(1)), "line");
        assert!(matches!(r[5], Slot::Word(6)), "col");
    }

    #[test]
    fn dump_reads_the_slots() {
        let name = Box::into_raw(Box::new("hi".to_owned()));
        let w = [5, 0, name as u64];
        // SAFETY: `w` holds a `P` and `name` is live.
        let out = unsafe { dump(&w, SCHEMA) };
        // SAFETY: reclaimed once.
        drop(unsafe { Box::from_raw(name) });
        assert_eq!(out.as_deref(), Some(r#"{"a":5,"b":"hi"}"#));
    }

    #[test]
    fn a_sum_reads_its_tag_key_and_the_type_can_ignore_unknown_members() {
        let schema = r#"{"root":{"named":"S"},"defs":{"S":{"sum":[{"n":"A","w":"a","fs":[{"n":"x","t":"int"}]}],"tag":"kind","unk":true}}}"#;
        let r = parse(r#"{"kind":"a","x":1,"y":2}"#, schema);
        assert!(matches!(r[0], Slot::Word(0)), "{}", r.len());
        let r = parse(r#"{"tag":"a","x":1}"#, schema);
        assert!(matches!(r[0], Slot::Word(1)));
    }
}
