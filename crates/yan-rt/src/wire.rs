//! The canonical binary `Wire` codec (17.4): a schema-driven encoding of values of the types
//! that `@derive(Wire)` accepts. The elaborator builds a [`Schemas`] from a type; std decoders and
//! the actor transport call [`encode`] and [`decode`] with it.

use std::collections::HashMap;

/// A type as `derive(Wire)` sees it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Schema {
    Unit,
    Bool,
    Int,
    Float,
    Char,
    Str,
    Bytes,
    List(Box<Schema>),
    Set(Box<Schema>),
    Dict(Box<Schema>, Box<Schema>),
    Option(Box<Schema>),
    Tuple(Vec<Schema>),
    /// A record or sum by name, resolved in [`Schemas::defs`] (so types may be recursive).
    Named(String),
}

/// The fields of a record, or the variants of a sum, in declaration order.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Def {
    Record(Vec<(String, Schema)>),
    Sum(Vec<(String, Vec<(String, Schema)>)>),
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Schemas {
    pub root: String,
    pub defs: HashMap<String, Def>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    Char(char),
    Str(String),
    Bytes(Vec<u8>),
    /// A list or a set (a set is encoded in the order given).
    List(Vec<Val>),
    Dict(Vec<(Val, Val)>),
    Option(Option<Box<Val>>),
    Tuple(Vec<Val>),
    Record(Vec<Val>),
    /// The variant's index in declaration order, and its fields.
    Variant(usize, Vec<Val>),
}

/// Decoding limits (17.4): nesting depth, total input size, and the length of any collection
/// or string.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub depth: usize,
    pub bytes: usize,
    pub len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            depth: 128,
            bytes: 16 << 20,
            len: 1 << 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WireError {
    /// The value does not have the schema's shape.
    Shape(&'static str),
    Eof,
    Trailing,
    Depth,
    Size,
    Utf8,
    /// A variant index or option tag out of range.
    Tag(u64),
    UnknownType(String),
}

fn put_varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push(n as u8 | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

fn zigzag(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

fn unzigzag(n: u64) -> i64 {
    (n >> 1) as i64 ^ -((n & 1) as i64)
}

/// NaN has one encoding, so equal values give equal bytes (7).
fn canonical(x: f64) -> f64 {
    if x.is_nan() { f64::NAN } else { x }
}

pub fn encode(s: &Schemas, v: &Val) -> Result<Vec<u8>, WireError> {
    let mut out = Vec::new();
    enc(s, &Schema::Named(s.root.clone()), v, &mut out)?;
    Ok(out)
}

fn def<'a>(s: &'a Schemas, n: &str) -> Result<&'a Def, WireError> {
    s.defs
        .get(n)
        .ok_or_else(|| WireError::UnknownType(n.to_owned()))
}

fn enc(s: &Schemas, t: &Schema, v: &Val, out: &mut Vec<u8>) -> Result<(), WireError> {
    let bad = WireError::Shape("value does not match its schema");
    match (t, v) {
        (Schema::Unit, Val::Unit) => {}
        (Schema::Bool, Val::Bool(b)) => out.push(u8::from(*b)),
        (Schema::Int, Val::Int(n)) => put_varint(out, zigzag(*n)),
        (Schema::Float, Val::Float(x)) => out.extend(canonical(*x).to_bits().to_le_bytes()),
        (Schema::Char, Val::Char(c)) => put_varint(out, u64::from(*c)),
        (Schema::Str, Val::Str(x)) => {
            put_varint(out, x.len() as u64);
            out.extend(x.as_bytes());
        }
        (Schema::Bytes, Val::Bytes(x)) => {
            put_varint(out, x.len() as u64);
            out.extend(x);
        }
        (Schema::List(e) | Schema::Set(e), Val::List(xs)) => {
            put_varint(out, xs.len() as u64);
            for x in xs {
                enc(s, e, x, out)?;
            }
        }
        (Schema::Dict(k, e), Val::Dict(xs)) => {
            put_varint(out, xs.len() as u64);
            for (a, b) in xs {
                enc(s, k, a, out)?;
                enc(s, e, b, out)?;
            }
        }
        (Schema::Option(_), Val::Option(None)) => out.push(0),
        (Schema::Option(e), Val::Option(Some(x))) => {
            out.push(1);
            enc(s, e, x, out)?;
        }
        (Schema::Tuple(ts), Val::Tuple(xs)) if ts.len() == xs.len() => {
            for (t, x) in ts.iter().zip(xs) {
                enc(s, t, x, out)?;
            }
        }
        (Schema::Named(n), Val::Record(xs)) => {
            let Def::Record(fs) = def(s, n)? else {
                return Err(bad);
            };
            if fs.len() != xs.len() {
                return Err(bad);
            }
            for ((_, t), x) in fs.iter().zip(xs) {
                enc(s, t, x, out)?;
            }
        }
        (Schema::Named(n), Val::Variant(i, xs)) => {
            let Def::Sum(vs) = def(s, n)? else {
                return Err(bad);
            };
            let (_, fs) = vs.get(*i).ok_or(bad.clone())?;
            if fs.len() != xs.len() {
                return Err(bad);
            }
            put_varint(out, *i as u64);
            for ((_, t), x) in fs.iter().zip(xs) {
                enc(s, t, x, out)?;
            }
        }
        _ => return Err(bad),
    }
    Ok(())
}

pub fn decode(s: &Schemas, bytes: &[u8], lim: Limits) -> Result<Val, WireError> {
    if bytes.len() > lim.bytes {
        return Err(WireError::Size);
    }
    let mut r = Reader { b: bytes, at: 0 };
    let v = dec(s, &Schema::Named(s.root.clone()), &mut r, lim, 0)?;
    if r.at != bytes.len() {
        return Err(WireError::Trailing);
    }
    Ok(v)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self.at.checked_add(n).ok_or(WireError::Eof)?;
        let r = self.b.get(self.at..end).ok_or(WireError::Eof)?;
        self.at = end;
        Ok(r)
    }

    fn varint(&mut self) -> Result<u64, WireError> {
        let mut n = 0u64;
        for shift in (0..70).step_by(7) {
            let b = self.take(1)?[0];
            if shift == 63 && b > 1 {
                return Err(WireError::Size);
            }
            n |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(n);
            }
        }
        Err(WireError::Size)
    }

    fn len(&mut self, lim: Limits) -> Result<usize, WireError> {
        let n = self.varint()?;
        if n > lim.len as u64 {
            return Err(WireError::Size);
        }
        Ok(n as usize)
    }
}

fn dec(
    s: &Schemas,
    t: &Schema,
    r: &mut Reader,
    lim: Limits,
    depth: usize,
) -> Result<Val, WireError> {
    if depth > lim.depth {
        return Err(WireError::Depth);
    }
    let d = depth + 1;
    Ok(match t {
        Schema::Unit => Val::Unit,
        Schema::Bool => match r.take(1)?[0] {
            0 => Val::Bool(false),
            1 => Val::Bool(true),
            b => return Err(WireError::Tag(u64::from(b))),
        },
        Schema::Int => Val::Int(unzigzag(r.varint()?)),
        Schema::Float => {
            let b: [u8; 8] = r.take(8)?.try_into().expect("8 bytes");
            Val::Float(f64::from_bits(u64::from_le_bytes(b)))
        }
        Schema::Char => {
            let n = r.varint()?;
            Val::Char(
                u32::try_from(n)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or(WireError::Tag(n))?,
            )
        }
        Schema::Str => {
            let n = r.len(lim)?;
            let x = std::str::from_utf8(r.take(n)?).map_err(|_| WireError::Utf8)?;
            Val::Str(x.to_owned())
        }
        Schema::Bytes => {
            let n = r.len(lim)?;
            Val::Bytes(r.take(n)?.to_vec())
        }
        Schema::List(e) | Schema::Set(e) => {
            let n = r.len(lim)?;
            let mut xs = Vec::new();
            for _ in 0..n {
                xs.push(dec(s, e, r, lim, d)?);
            }
            Val::List(xs)
        }
        Schema::Dict(k, e) => {
            let n = r.len(lim)?;
            let mut xs = Vec::new();
            for _ in 0..n {
                xs.push((dec(s, k, r, lim, d)?, dec(s, e, r, lim, d)?));
            }
            Val::Dict(xs)
        }
        Schema::Option(e) => match r.take(1)?[0] {
            0 => Val::Option(None),
            1 => Val::Option(Some(Box::new(dec(s, e, r, lim, d)?))),
            b => return Err(WireError::Tag(u64::from(b))),
        },
        Schema::Tuple(ts) => Val::Tuple(
            ts.iter()
                .map(|t| dec(s, t, r, lim, d))
                .collect::<Result<_, _>>()?,
        ),
        Schema::Named(n) => match def(s, n)? {
            Def::Record(fs) => Val::Record(
                fs.iter()
                    .map(|(_, t)| dec(s, t, r, lim, d))
                    .collect::<Result<_, _>>()?,
            ),
            Def::Sum(vs) => {
                let i = r.varint()?;
                let (_, fs) = usize::try_from(i)
                    .ok()
                    .and_then(|i| vs.get(i))
                    .ok_or(WireError::Tag(i))?;
                let xs = fs
                    .iter()
                    .map(|(_, t)| dec(s, t, r, lim, d))
                    .collect::<Result<_, _>>()?;
                Val::Variant(i as usize, xs)
            }
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes() -> Schemas {
        let point = Def::Record(vec![("x".into(), Schema::Int), ("y".into(), Schema::Float)]);
        let shape = Def::Sum(vec![
            (
                "Dot".into(),
                vec![("at".into(), Schema::Named("Point".into()))],
            ),
            (
                "Poly".into(),
                vec![(
                    "pts".into(),
                    Schema::List(Box::new(Schema::Named("Point".into()))),
                )],
            ),
            (
                "Tree".into(),
                vec![(
                    "kids".into(),
                    Schema::List(Box::new(Schema::Named("Shape".into()))),
                )],
            ),
            ("Empty".into(), vec![]),
        ]);
        Schemas {
            root: "Shape".into(),
            defs: HashMap::from([("Point".into(), point), ("Shape".into(), shape)]),
        }
    }

    fn point(x: i64, y: f64) -> Val {
        Val::Record(vec![Val::Int(x), Val::Float(y)])
    }

    #[test]
    fn sum_round_trip() {
        let s = shapes();
        for v in [
            Val::Variant(0, vec![point(-3, 1.5)]),
            Val::Variant(
                1,
                vec![Val::List(vec![point(i64::MIN, 0.0), point(i64::MAX, -0.0)])],
            ),
            Val::Variant(3, vec![]),
            Val::Variant(2, vec![Val::List(vec![Val::Variant(3, vec![])])]),
        ] {
            let b = encode(&s, &v).unwrap();
            assert_eq!(decode(&s, &b, Limits::default()).unwrap(), v);
        }
    }

    #[test]
    fn record_round_trip() {
        let s = Schemas {
            root: "Point".into(),
            ..shapes()
        };
        let b = encode(&s, &point(300, 2.5)).unwrap();
        assert_eq!(decode(&s, &b, Limits::default()).unwrap(), point(300, 2.5));
    }

    #[test]
    fn containers_round_trip() {
        let t = Schema::Tuple(vec![
            Schema::Option(Box::new(Schema::Str)),
            Schema::Dict(Box::new(Schema::Str), Box::new(Schema::Bytes)),
            Schema::Char,
            Schema::Bool,
            Schema::Unit,
        ]);
        let s = Schemas {
            root: "R".into(),
            defs: HashMap::from([("R".into(), Def::Record(vec![("t".into(), t)]))]),
        };
        let v = Val::Record(vec![Val::Tuple(vec![
            Val::Option(Some(Box::new(Val::Str("héllo".into())))),
            Val::Dict(vec![(Val::Str("k".into()), Val::Bytes(vec![0, 255]))]),
            Val::Char('語'),
            Val::Bool(true),
            Val::Unit,
        ])]);
        let b = encode(&s, &v).unwrap();
        assert_eq!(decode(&s, &b, Limits::default()).unwrap(), v);
    }

    #[test]
    fn nan_is_canonical() {
        let s = Schemas {
            root: "P".into(),
            defs: HashMap::from([("P".into(), Def::Record(vec![("f".into(), Schema::Float)]))]),
        };
        let a = encode(&s, &Val::Record(vec![Val::Float(f64::NAN)])).unwrap();
        let b = encode(&s, &Val::Record(vec![Val::Float(-f64::NAN)])).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_bad_input() {
        let s = shapes();
        let ok = encode(&s, &Val::Variant(0, vec![point(1, 1.0)])).unwrap();
        let lim = Limits::default();
        assert_eq!(decode(&s, &ok[..ok.len() - 1], lim), Err(WireError::Eof));
        assert_eq!(
            decode(&s, &[&ok[..], &[0]].concat(), lim),
            Err(WireError::Trailing)
        );
        assert_eq!(decode(&s, &[9], lim), Err(WireError::Tag(9)));
        assert_eq!(
            decode(&s, &ok, Limits { bytes: 2, ..lim }),
            Err(WireError::Size)
        );
        assert_eq!(
            encode(&s, &Val::Variant(7, vec![])),
            Err(WireError::Shape("value does not match its schema"))
        );
    }

    #[test]
    fn limits() {
        let s = shapes();
        let mut v = Val::Variant(3, vec![]);
        for _ in 0..10 {
            v = Val::Variant(2, vec![Val::List(vec![v])]);
        }
        let b = encode(&s, &v).unwrap();
        let shallow = Limits {
            depth: 5,
            ..Limits::default()
        };
        assert_eq!(decode(&s, &b, shallow), Err(WireError::Depth));
        assert!(decode(&s, &b, Limits::default()).is_ok());
        // a list claiming 2^40 items is refused before anything is allocated
        let mut huge = vec![2u8, 0x80, 0x80, 0x80, 0x80, 0x80, 0x20];
        assert_eq!(decode(&s, &huge, Limits::default()), Err(WireError::Size));
        huge.truncate(1);
        assert_eq!(decode(&s, &huge, Limits::default()), Err(WireError::Eof));
    }

    #[test]
    fn varint_bounds() {
        let s = Schemas {
            root: "P".into(),
            defs: HashMap::from([("P".into(), Def::Record(vec![("n".into(), Schema::Int)]))]),
        };
        let eleven = [0x80u8; 10].iter().chain(&[1]).copied().collect::<Vec<_>>();
        assert_eq!(decode(&s, &eleven, Limits::default()), Err(WireError::Size));
    }
}
