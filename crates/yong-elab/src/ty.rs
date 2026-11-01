//! Xin types during elaboration and their Yong spelling.

use yong_core::ast::{self as y, Named, TArg};
use yong_names::{Surface, type_name};

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Int,
    Float,
    /// Arbitrary precision; not `Copy`.
    BigInt,
    Bool,
    Str,
    Char,
    /// A sized scalar other than `int` and `float`, by its Yong name (`i32`, `u8`, `f32`).
    Fixed(&'static str),
    Unit,
    Never,
    List(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Set(Box<Ty>),
    /// A std.collections type by its Xin name (see [`COLLECTIONS`]), with its arguments.
    Coll(&'static str, Vec<Ty>),
    Tuple(Vec<Ty>),
    Option(Box<Ty>),
    Result(Box<Ty>, Box<Ty>),
    /// An anonymous union of nominal error types, only after `fail` (8.1).
    Union(Vec<Ty>),
    /// A record, sum or newtype of this module.
    Data(String),
    /// A type parameter of the enclosing function, or `Self` in a trait.
    Param(String),
    /// `Callable[[params], ret]`; `par` when prefixed `par` (R436).
    Fn(bool, Vec<Ty>, Box<Ty>),
    /// A `core.ffi` type in an ffi module (12.3, 12.5): `Ptr[T]`, `CStr`, `View[T]` and so on.
    Foreign(&'static str, Vec<Ty>),
    /// Produced after an error was reported; compatible with every type.
    Err,
}

/// Each std.collections type, by its Xin name; its Yong name is in the projection table.
pub const COLLECTIONS: [&str; 17] = [
    "deque",
    "iter",
    "heap",
    "sorted_dict",
    "sorted_set",
    "bytes",
    "bytearray",
    "Path",
    "WindowsPath",
    "Duration",
    "Instant",
    "MonoInstant",
    "PlainDate",
    "PlainTime",
    "PlainDateTime",
    "Period",
    "TimeZone",
];

/// `name` as a std.collections type, with its `&'static` spelling and arity.
pub fn collection(name: &str) -> Option<(&'static str, usize)> {
    let n = COLLECTIONS.iter().find(|n| **n == name)?;
    let row = yong_names::type_row(n, Surface::Xin).expect("in the projection table");
    Some((n, row.arity))
}

fn named(name: &str, args: Vec<y::Type>) -> y::Type {
    y::Type::Named(Named {
        path: vec![name.to_owned()],
        args: args.into_iter().map(TArg::Type).collect(),
    })
}

impl Ty {
    pub fn core(&self) -> y::Type {
        match self {
            Ty::Int => named("i64", vec![]),
            Ty::Float => named("f64", vec![]),
            Ty::BigInt => named("bigint", vec![]),
            Ty::Bool => named("bool", vec![]),
            Ty::Str => named("Str", vec![]),
            Ty::Char => named("char", vec![]),
            Ty::Fixed(n) => named(n, vec![]),
            Ty::Unit => named("Unit", vec![]),
            Ty::Never | Ty::Err => named("Never", vec![]),
            Ty::List(t) => named("List", vec![t.core()]),
            Ty::Dict(k, v) => named("Map", vec![k.core(), v.core()]),
            Ty::Set(t) => named("Set", vec![t.core()]),
            Ty::Coll(n, ts) => {
                let y = type_name(n, Surface::Xin, Surface::Yong).expect("a collection");
                named(y, ts.iter().map(Ty::core).collect())
            }
            Ty::Tuple(ts) => y::Type::Tuple(ts.iter().map(Ty::core).collect()),
            Ty::Option(t) => named("Option", vec![t.core()]),
            Ty::Result(t, e) => named("Result", vec![t.core(), e.core()]),
            Ty::Union(ts) => y::Type::Union(ts.iter().map(Ty::core).collect()),
            Ty::Data(n) | Ty::Param(n) => named(n, vec![]),
            Ty::Foreign(n, ts) => named(n, ts.iter().map(Ty::core).collect()),
            Ty::Fn(_, ps, r) => y::Type::Fn(y::FnType {
                params: ps.iter().map(|t| (y::Conv::Let, t.core())).collect(),
                ret: Box::new(r.core()),
                uses: None,
                fail: None,
            }),
        }
    }

    pub fn targ(&self) -> TArg {
        TArg::Type(self.core())
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Ty::Int
            | Ty::Float
            | Ty::Bool
            | Ty::Char
            | Ty::Fixed(_)
            | Ty::Unit
            | Ty::Never
            | Ty::Err => true,
            Ty::Tuple(ts) => ts.iter().all(Ty::is_copy),
            _ => false,
        }
    }

    /// `self` can stand where `want` is expected.
    pub fn fits(&self, want: &Ty) -> bool {
        match (self, want) {
            (Ty::Err | Ty::Never, _) | (_, Ty::Err) => true,
            // Checked when the call's type arguments are inferred.
            (_, Ty::Param(_)) => true,
            (Ty::List(a), Ty::List(b))
            | (Ty::Set(a), Ty::Set(b))
            | (Ty::Option(a), Ty::Option(b)) => a.fits(b),
            (Ty::Dict(a, b), Ty::Dict(c, d)) | (Ty::Result(a, b), Ty::Result(c, d)) => {
                a.fits(c) && b.fits(d)
            }
            (Ty::Tuple(a), Ty::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.fits(y))
            }
            (Ty::Coll(m, a), Ty::Coll(n, b)) => {
                m == n && a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.fits(y))
            }
            (Ty::Fn(p, a, r), Ty::Fn(q, b, s)) => {
                (*p || !q)
                    && a.len() == b.len()
                    && a.iter().zip(b).all(|(x, y)| x.fits(y))
                    && r.fits(s)
            }
            // A list passes as a view of its elements (12.5).
            (Ty::List(a), Ty::Foreign("View" | "MutView", b)) => b.len() == 1 && a.fits(&b[0]),
            (a, Ty::Option(b)) => a.fits(b),
            (Ty::Union(xs), Ty::Union(ys)) => xs.iter().all(|x| ys.contains(x)),
            (a, Ty::Union(ms)) => ms.contains(a),
            (a, b) => a == b,
        }
    }

    /// The suffix of the Y4 intrinsic for this scalar, if it has one.
    pub fn scalar(&self) -> Option<&'static str> {
        match self {
            Ty::Int => Some("i64"),
            Ty::Float => Some("f64"),
            Ty::BigInt => Some("bigint"),
            Ty::Bool => Some("bool"),
            Ty::Fixed(n) => Some(n),
            _ => None,
        }
    }

    /// The std module that declares this type's methods (R475).
    pub fn module(&self) -> Option<&'static str> {
        match self {
            Ty::Str | Ty::Char => Some("text"),
            Ty::List(_) => Some("list"),
            Ty::Dict(..) => Some("dict"),
            Ty::Set(_) => Some("set"),
            Ty::Coll("deque", _) => Some("collections.deque"),
            Ty::Coll("heap", _) => Some("collections.heap"),
            Ty::Coll("sorted_dict", _) => Some("collections.sorted_dict"),
            Ty::Coll("iter", _) => Some("iter.ops"),
            Ty::Coll("bytes", _) => Some("encoding"),
            Ty::Coll("bytearray", _) => Some("encoding.bytearray"),
            Ty::Coll("Path", _) => Some("path"),
            Ty::Coll("WindowsPath", _) => Some("path.windows"),
            Ty::Coll("Duration", _) => Some("time.duration"),
            Ty::Coll("Instant", _) => Some("time.instant"),
            Ty::Coll("MonoInstant", _) => Some("time.mono"),
            Ty::Coll("PlainDate", _) => Some("time.date"),
            Ty::Coll("PlainTime", _) => Some("time.plain_time"),
            Ty::Coll("PlainDateTime", _) => Some("time.plain_date_time"),
            Ty::Coll("Period", _) => Some("time.period"),
            Ty::Coll("TimeZone", _) => Some("time.zone"),
            Ty::Coll(_, _) => Some("collections.sorted_set"),
            Ty::Int | Ty::Float | Ty::BigInt | Ty::Bool | Ty::Fixed(_) => Some("num"),
            Ty::Option(_) | Ty::Result(..) => Some("result"),
            _ => None,
        }
    }

    /// Binds the type parameters in `self` to the matching parts of `got`.
    pub fn unify(&self, got: &Ty, map: &mut Vec<(String, Ty)>) {
        match (self, got) {
            (_, Ty::Err | Ty::Never) => {}
            (Ty::Param(n), t) => {
                if !map.iter().any(|(m, _)| m == n) {
                    map.push((n.clone(), t.clone()));
                }
            }
            (Ty::List(a), Ty::List(b))
            | (Ty::Set(a), Ty::Set(b))
            | (Ty::Option(a), Ty::Option(b)) => a.unify(b, map),
            (Ty::Dict(a, b), Ty::Dict(c, d)) | (Ty::Result(a, b), Ty::Result(c, d)) => {
                a.unify(c, map);
                b.unify(d, map);
            }
            (Ty::Tuple(a), Ty::Tuple(b)) => a.iter().zip(b).for_each(|(x, y)| x.unify(y, map)),
            (Ty::Coll(m, a), Ty::Coll(n, b)) if m == n => {
                a.iter().zip(b).for_each(|(x, y)| x.unify(y, map))
            }
            (Ty::Fn(_, a, r), Ty::Fn(_, b, s)) => {
                a.iter().zip(b).for_each(|(x, y)| x.unify(y, map));
                r.unify(s, map);
            }
            (Ty::Option(a), b) => a.unify(b, map),
            _ => {}
        }
    }

    pub fn mentions(&self, p: &str) -> bool {
        match self {
            Ty::Param(n) => n == p,
            Ty::List(t) | Ty::Set(t) | Ty::Option(t) => t.mentions(p),
            Ty::Dict(a, b) | Ty::Result(a, b) => a.mentions(p) || b.mentions(p),
            Ty::Tuple(ts) | Ty::Coll(_, ts) => ts.iter().any(|t| t.mentions(p)),
            Ty::Fn(_, ps, r) => r.mentions(p) || ps.iter().any(|t| t.mentions(p)),
            _ => false,
        }
    }

    pub fn subst(&self, map: &[(String, Ty)]) -> Ty {
        let s = |t: &Ty| Box::new(t.subst(map));
        match self {
            Ty::Param(n) => map
                .iter()
                .find(|(m, _)| m == n)
                .map_or_else(|| self.clone(), |(_, t)| t.clone()),
            Ty::List(t) => Ty::List(s(t)),
            Ty::Set(t) => Ty::Set(s(t)),
            Ty::Option(t) => Ty::Option(s(t)),
            Ty::Dict(k, v) => Ty::Dict(s(k), s(v)),
            Ty::Result(t, e) => Ty::Result(s(t), s(e)),
            Ty::Tuple(ts) => Ty::Tuple(ts.iter().map(|t| t.subst(map)).collect()),
            Ty::Coll(n, ts) => Ty::Coll(n, ts.iter().map(|t| t.subst(map)).collect()),
            Ty::Fn(p, ps, r) => Ty::Fn(*p, ps.iter().map(|t| t.subst(map)).collect(), s(r)),
            t => t.clone(),
        }
    }

    pub fn elem(&self) -> Option<&Ty> {
        match self {
            Ty::List(t) | Ty::Set(t) | Ty::Option(t) => Some(t),
            Ty::Coll(n, ts) if *n != "sorted_dict" => ts.first(),
            _ => None,
        }
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Float => write!(f, "float"),
            Ty::BigInt => write!(f, "bigint"),
            Ty::Bool => write!(f, "bool"),
            Ty::Str => write!(f, "str"),
            Ty::Char => write!(f, "char"),
            Ty::Fixed(n) => {
                let (signed, bits) = n.split_at(1);
                let base = match signed {
                    "i" => "int",
                    "u" => "uint",
                    _ => "float",
                };
                write!(f, "{base}{bits}")
            }
            Ty::Unit => write!(f, "None"),
            Ty::Never => write!(f, "Never"),
            Ty::Err => write!(f, "?"),
            Ty::List(t) => write!(f, "list[{t}]"),
            Ty::Dict(k, v) => write!(f, "dict[{k}, {v}]"),
            Ty::Set(t) => write!(f, "set[{t}]"),
            Ty::Foreign(n, ts) | Ty::Coll(n, ts) if ts.is_empty() => write!(f, "{n}"),
            Ty::Tuple(ts) | Ty::Coll(_, ts) | Ty::Foreign(_, ts) => {
                let name = match self {
                    Ty::Coll(n, _) | Ty::Foreign(n, _) => n,
                    _ => "tuple",
                };
                write!(f, "{name}[")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, "]")
            }
            Ty::Option(t) => write!(f, "{t} | None"),
            Ty::Result(t, e) => write!(f, "Result[{t}, {e}]"),
            Ty::Union(ts) => {
                let ts: Vec<String> = ts.iter().map(Ty::to_string).collect();
                write!(f, "{}", ts.join(" | "))
            }
            Ty::Data(n) | Ty::Param(n) => write!(f, "{n}"),
            Ty::Fn(p, ps, r) => {
                let ps: Vec<String> = ps.iter().map(Ty::to_string).collect();
                let par = if *p { "par " } else { "" };
                write!(f, "{par}Callable[[{}], {r}]", ps.join(", "))
            }
        }
    }
}

/// Xin's sized scalar names the elaborator lowers, and their Yong spelling (16.3).
pub fn fixed(name: &str) -> Option<&'static str> {
    const LOWERED: [&str; 9] = [
        "int8", "int16", "int32", "int128", "uint8", "uint16", "uint32", "uint64", "float32",
    ];
    LOWERED
        .contains(&name)
        .then(|| type_name(name, Surface::Xin, Surface::Yong))
        .flatten()
}
