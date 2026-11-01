//! Jian types during elaboration and their Yong spelling.

use yong_core::ast::{self as y, Named, TArg};

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
    /// A std.collections type by its Jian name (see [`COLLECTIONS`]), with its arguments.
    Coll(&'static str, Vec<Ty>),
    Tuple(Vec<Ty>),
    Option(Box<Ty>),
    Result(Box<Ty>, Box<Ty>),
    /// A record, sum or newtype of this module.
    Data(String),
    /// A type parameter of the enclosing function, or `Self` in a trait.
    Param(String),
    /// Produced after an error was reported; compatible with every type.
    Err,
}

/// Each std.collections type: its Jian name and its Yong name.
pub const COLLECTIONS: [(&str, &str); 4] = [
    ("deque", "Deque"),
    ("heap", "Heap"),
    ("sorted_dict", "SortedMap"),
    ("sorted_set", "SortedSet"),
];

/// `name` as a std.collections type, with its `&'static` spelling and arity.
pub fn collection(name: &str) -> Option<(&'static str, usize)> {
    COLLECTIONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(n, _)| (*n, if *n == "sorted_dict" { 2 } else { 1 }))
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
                let y = COLLECTIONS
                    .iter()
                    .find(|(j, _)| j == n)
                    .expect("a collection")
                    .1;
                named(y, ts.iter().map(Ty::core).collect())
            }
            Ty::Tuple(ts) => y::Type::Tuple(ts.iter().map(Ty::core).collect()),
            Ty::Option(t) => named("Option", vec![t.core()]),
            Ty::Result(t, e) => named("Result", vec![t.core(), e.core()]),
            Ty::Data(n) | Ty::Param(n) => named(n, vec![]),
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
            (a, Ty::Option(b)) => a.fits(b),
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
            Ty::Tuple(ts) | Ty::Coll(_, ts) => {
                let name = match self {
                    Ty::Coll(n, _) => n,
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
            Ty::Data(n) | Ty::Param(n) => write!(f, "{n}"),
        }
    }
}

/// Jian's sized scalar names and their Yong spelling.
pub fn fixed(name: &str) -> Option<&'static str> {
    Some(match name {
        "int8" => "i8",
        "int16" => "i16",
        "int32" => "i32",
        "uint8" => "u8",
        "uint16" => "u16",
        "uint32" => "u32",
        "uint64" => "u64",
        "float32" => "f32",
        _ => return None,
    })
}
