//! Yan Core types as native slot shapes.

use super::*;

/// A Yan Core type as scalar slots.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Scalar(Ty),
    Str,
    /// A `bytes` value: a block holding a `Vec<u8>` (`yan_rt::bytes`).
    Bytes,
    /// A `bytearray`: the same block, changed in place.
    ByteArray,
    /// A `bigint`: a block holding its two's-complement bytes, little endian (`yan_rt::bigint`).
    BigInt,
    /// A `Path`: a `Str` block of normalized text, ordered by its parts.
    Path,
    /// A `WindowsPath`: a `Str` block of normalized text, equal, ordered and hashed case-folded.
    WinPath,
    /// A `char`: its code point in one word.
    Char,
    List(Box<Shape>),
    /// A `dict`: a block of entries in insertion order (`yan_rt::map`).
    Map(Box<Shape>, Box<Shape>),
    /// A `heap`: a list block kept as a min-heap (`yan_rt::pq`).
    Heap(Box<Shape>),
    /// A `deque`: a list block that also holds its `maxlen` (`yan_rt::dq`).
    Deque(Box<Shape>),
    Rec(String, Vec<(String, Shape)>),
    Tuple(Vec<Shape>),
    /// A tag slot, then each variant's fields in order.
    Sum(String, Vec<(String, Vec<(String, Shape)>)>),
    /// A closure: its code and its captures' block. `ret` is a `Result` for `fail E`.
    Fn(Vec<Shape>, Box<Shape>),
    /// A `Sender[T]` (true) or `Receiver[T]`: the channel both ends share.
    Chan(Box<Shape>, bool),
    /// An `Actor[S]`: the actor's index in `yan_rt::actor`, which the handle does not own.
    Actor(Box<Shape>),
    /// A `Ptr[T]` (6.6): its address, then its provenance (0 for none).
    Ptr,
    /// A `set`: a `yan_rt::map` block whose entries are keys with no value slots.
    Set(Box<Shape>),
}

impl Shape {
    pub(super) fn slots(&self, out: &mut Vec<Ty>) {
        match self {
            Shape::Scalar(t) => out.push(*t),
            Shape::Char => out.push(I64),
            Shape::Str
            | Shape::Bytes
            | Shape::ByteArray
            | Shape::BigInt
            | Shape::Path
            | Shape::WinPath
            | Shape::List(_)
            | Shape::Map(..)
            | Shape::Heap(_)
            | Shape::Deque(_)
            | Shape::Chan(..)
            | Shape::Actor(_)
            | Shape::Set(_) => out.push(Ty::Ptr),
            Shape::Fn(..) => out.extend([Ty::Ptr, Ty::Ptr]),
            Shape::Ptr => out.extend([I64, I64]),
            Shape::Rec(_, fs) => fs.iter().for_each(|(_, s)| s.slots(out)),
            Shape::Tuple(ss) => ss.iter().for_each(|s| s.slots(out)),
            Shape::Sum(_, vs) => {
                out.push(I64);
                vs.iter()
                    .flat_map(|(_, fs)| fs)
                    .for_each(|(_, s)| s.slots(out));
            }
        }
    }

    pub(super) fn tys(&self) -> Vec<Ty> {
        let mut v = Vec::new();
        self.slots(&mut v);
        v
    }

    pub(super) fn width(&self) -> usize {
        self.tys().len()
    }

    /// The fields of a record or tuple, by name, with their first slot.
    pub(super) fn fields(&self) -> Option<Vec<(String, usize, &Shape)>> {
        let named: Vec<(String, &Shape)> = match self {
            Shape::Rec(_, fs) => fs.iter().map(|(n, s)| (n.clone(), s)).collect(),
            Shape::Tuple(ss) => ss
                .iter()
                .enumerate()
                .map(|(i, s)| (format!("_{i}"), s))
                .collect(),
            _ => return None,
        };
        let mut at = 0;
        Some(
            named
                .into_iter()
                .map(|(n, s)| {
                    let r = (n, at, s);
                    at += s.width();
                    r
                })
                .collect(),
        )
    }

    /// A sum's variant `name`: its tag, first slot and fields.
    pub(super) fn variant(&self, name: &str) -> Option<(i128, usize, &Fields)> {
        let Shape::Sum(_, vs) = self else {
            return None;
        };
        let mut at = 1;
        for (tag, (n, fs)) in vs.iter().enumerate() {
            if n == name {
                return Some((tag as i128, at, fs));
            }
            at += fs.iter().map(|(_, s)| s.width()).sum::<usize>();
        }
        None
    }
}

pub(super) struct Types<'m> {
    pub(super) recs: HashMap<&'m str, &'m y::Record>,
    pub(super) sums: HashMap<&'m str, &'m y::Sum>,
}

impl Types<'_> {
    pub(super) fn shape(&self, t: &y::Type) -> R<Shape> {
        self.shape_in(t, &mut Vec::new())
    }

    pub(super) fn shape_in(&self, t: &y::Type, stack: &mut Vec<String>) -> R<Shape> {
        match t {
            y::Type::Tuple(ts) => Ok(Shape::Tuple(
                ts.iter()
                    .map(|t| self.shape_in(t, stack))
                    .collect::<R<_>>()?,
            )),
            y::Type::Named(n) if n.path.len() == 1 => {
                let name = n.path[0].as_str();
                match (name, n.args.as_slice()) {
                    ("Str", []) => return Ok(Shape::Str),
                    ("Bytes", []) => return Ok(Shape::Bytes),
                    ("ByteArray", []) => return Ok(Shape::ByteArray),
                    ("bigint", []) => return Ok(Shape::BigInt),
                    ("Path", []) => return Ok(Shape::Path),
                    ("WindowsPath", []) => return Ok(Shape::WinPath),
                    // A capability handler is its index in `yan_rt` (`rand`, `fshandler`,
                    // `loghandler`), unless the program declares a type of that name.
                    ("Rand" | "Fs" | "Log" | "Env" | "Time", [])
                        if !self.recs.contains_key(name) && !self.sums.contains_key(name) =>
                    {
                        return Ok(Shape::Scalar(I64));
                    }
                    // The handler of `std.proc` is the dict `fake_proc` was given; 0 is the host.
                    ("Proc", [])
                        if !self.recs.contains_key(name) && !self.sums.contains_key(name) =>
                    {
                        let out = y::Type::Named(y::Named {
                            path: vec!["Output".into()],
                            args: Vec::new(),
                        });
                        return Ok(Shape::Map(
                            Box::new(Shape::Str),
                            Box::new(self.shape_in(&out, stack)?),
                        ));
                    }
                    // The scheduler `pct` builds is `yan_rt::sched`'s, not a value.
                    ("Sched", [])
                        if !self.recs.contains_key(name) && !self.sums.contains_key(name) =>
                    {
                        return Ok(Shape::Scalar(I64));
                    }
                    ("Ptr", [y::TArg::Type(_)]) if !self.recs.contains_key(name) => {
                        return Ok(Shape::Ptr);
                    }
                    ("char", []) => return Ok(Shape::Char),
                    ("Secret", [y::TArg::Type(e)]) if !self.recs.contains_key(name) => {
                        return Ok(Shape::Rec(
                            "Secret".into(),
                            vec![("value".into(), self.shape_in(e, stack)?)],
                        ));
                    }
                    // No lazy adapters natively: an `iter` from `std.itertools` is its list.
                    ("List" | "Iter", [y::TArg::Type(e)]) => {
                        return Ok(Shape::List(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("Map", [y::TArg::Type(k), y::TArg::Type(v)]) => {
                        return Ok(Shape::Map(
                            Box::new(self.shape_in(k, stack)?),
                            Box::new(self.shape_in(v, stack)?),
                        ));
                    }
                    ("Heap", [y::TArg::Type(e)]) => {
                        return Ok(Shape::Heap(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("Deque", [y::TArg::Type(e)]) => {
                        return Ok(Shape::Deque(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("SortedMap", [y::TArg::Type(k), y::TArg::Type(v)]) => {
                        return Ok(super::sorted::sorted_dict(
                            self.shape_in(k, stack)?,
                            self.shape_in(v, stack)?,
                        ));
                    }
                    ("SortedSet", [y::TArg::Type(t)]) => {
                        return Ok(super::sorted::sorted_set(self.shape_in(t, stack)?));
                    }
                    ("Option", [y::TArg::Type(t)]) => {
                        let t = self.shape_in(t, stack)?;
                        return Ok(Shape::Sum(
                            "Option".into(),
                            vec![
                                ("Some".into(), vec![("value".into(), t)]),
                                ("None".into(), vec![]),
                            ],
                        ));
                    }
                    ("Result", [y::TArg::Type(t), y::TArg::Type(e)]) => {
                        return Ok(result(self.shape_in(t, stack)?, self.shape_in(e, stack)?));
                    }
                    ("Sender" | "Receiver", [y::TArg::Type(e)]) => {
                        let sender = name == "Sender";
                        return Ok(Shape::Chan(Box::new(self.shape_in(e, stack)?), sender));
                    }
                    // The overflow policy is the actor's, at run time.
                    ("Actor" | "SendOnly", [y::TArg::Type(s), ..]) => {
                        return Ok(Shape::Actor(Box::new(self.shape_in(s, stack)?)));
                    }
                    ("Set", [y::TArg::Type(e)]) => {
                        return Ok(Shape::Set(Box::new(self.shape_in(e, stack)?)));
                    }
                    // A `Remote` is its handle: every node runs in this process (R417).
                    ("Remote", [y::TArg::Type(h)]) => return self.shape_in(h, stack),
                    // A `Group` is its index in `yan_rt`; its members are one-word handles.
                    ("Group", [y::TArg::Type(a)]) => {
                        if !matches!(self.shape_in(a, stack)?, Shape::Actor(_) | Shape::Scalar(_)) {
                            return unsupported(format!(
                                "type `{name}[..]` of members that are not handles"
                            ));
                        }
                        return Ok(Shape::Scalar(I64));
                    }
                    ("Undelivered", [y::TArg::Type(a), y::TArg::Type(r)]) => {
                        let (a, r) = (self.shape_in(a, stack)?, self.shape_in(r, stack)?);
                        return Ok(actor::undelivered(a, r));
                    }
                    (_, []) => {}
                    _ => return unsupported(format!("type `{name}[..]`")),
                }
                if let Some(t) = scalar_ty(name) {
                    return Ok(Shape::Scalar(t));
                }
                if stack.iter().any(|s| s == name) {
                    return unsupported(format!("recursive type `{name}`"));
                }
                stack.push(name.to_owned());
                let fields = |fs: &[y::Field], stack: &mut Vec<String>| {
                    fs.iter()
                        .map(|f| Ok((f.name.clone(), self.shape_in(&f.ty, stack)?)))
                        .collect::<R<Vec<_>>>()
                };
                let s = if let Some(r) = self.recs.get(name).filter(|r| r.tparams.is_empty()) {
                    Shape::Rec(name.to_owned(), fields(&r.fields, stack)?)
                } else if let Some(s) = self.sums.get(name).filter(|s| s.tparams.is_empty()) {
                    let vs = s
                        .variants
                        .iter()
                        .map(|v| {
                            let fs = v.fields.as_deref().unwrap_or(&[]);
                            Ok((v.name.clone(), fields(fs, stack)?))
                        })
                        .collect::<R<_>>()?;
                    Shape::Sum(name.to_owned(), vs)
                } else if STD_FAILURES.contains(&name) {
                    Shape::Rec(name.to_owned(), Vec::new())
                } else if name == "Duration" {
                    actor::duration()
                } else if name == "i128" {
                    time::wide("i128")
                } else if let Some(s) = proc::std_type(name) {
                    s
                } else if let Some(s) = civil::std_type(name) {
                    s
                } else if name == "Instant" {
                    time::shape(true)
                } else if name == "Stopped" {
                    actor::stopped()
                } else if let Some(s) = cluster::std_type(name) {
                    s
                } else {
                    return unsupported(format!("type `{name}`"));
                };
                stack.pop();
                Ok(s)
            }
            y::Type::Fn(ft) => {
                if ft.params.iter().any(|(c, _)| *c == y::Conv::Inout) {
                    return unsupported("a closure type with `inout` parameters");
                }
                let ps = ft
                    .params
                    .iter()
                    .map(|(_, t)| self.shape_in(t, stack))
                    .collect::<R<_>>()?;
                let ok = self.shape_in(&ft.ret, stack)?;
                let ret = match &ft.fail {
                    Some(e) => result(ok, self.shape_in(e, stack)?),
                    None => ok,
                };
                Ok(Shape::Fn(ps, Box::new(ret)))
            }
            // A union is a sum over its members, in the order written.
            y::Type::Union(ts) => Ok(Shape::Sum(
                "Union".into(),
                ts.iter()
                    .map(|t| {
                        let s = self.shape_in(t, stack)?;
                        Ok((yan_core::type_text(t), vec![("value".into(), s)]))
                    })
                    .collect::<R<_>>()?,
            )),
            _ => unsupported("a compound type"),
        }
    }
}

/// The fieldless failure types of section 9, which std declares.
/// Failures a `par` ranks below every other: a cancelled wait, and the `SessionClosed` a session
/// operation turns it into (R394, R634).
pub(super) const CANCEL_LIKE: &[&str] = &["Cancelled", "SessionClosed"];
pub(super) const STD_FAILURES: &[&str] = &[
    "Cancelled",
    "Timeout",
    "Draining",
    "Deadlock",
    "Full",
    "Unreachable",
    "Rejected",
    "SchemaMismatch",
];

pub(super) fn result(ok: Shape, err: Shape) -> Shape {
    Shape::Sum(
        "Result".into(),
        vec![
            ("Ok".into(), vec![("value".into(), ok)]),
            ("Err".into(), vec![("error".into(), err)]),
        ],
    )
}
