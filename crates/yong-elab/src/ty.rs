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
    Tuple(Vec<Ty>),
    Option(Box<Ty>),
    Result(Box<Ty>, Box<Ty>),
    /// A record, sum or newtype of this module.
    Data(String),
    /// Produced after an error was reported; compatible with every type.
    Err,
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
            Ty::Tuple(ts) => y::Type::Tuple(ts.iter().map(Ty::core).collect()),
            Ty::Option(t) => named("Option", vec![t.core()]),
            Ty::Result(t, e) => named("Result", vec![t.core(), e.core()]),
            Ty::Data(n) => named(n, vec![]),
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
            (Ty::List(a), Ty::List(b))
            | (Ty::Set(a), Ty::Set(b))
            | (Ty::Option(a), Ty::Option(b)) => a.fits(b),
            (Ty::Dict(a, b), Ty::Dict(c, d)) | (Ty::Result(a, b), Ty::Result(c, d)) => {
                a.fits(c) && b.fits(d)
            }
            (Ty::Tuple(a), Ty::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.fits(y))
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
            Ty::Int | Ty::Float | Ty::BigInt | Ty::Bool | Ty::Fixed(_) => Some("num"),
            Ty::Option(_) | Ty::Result(..) => Some("result"),
            _ => None,
        }
    }

    pub fn elem(&self) -> Option<&Ty> {
        match self {
            Ty::List(t) | Ty::Set(t) | Ty::Option(t) => Some(t),
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
            Ty::Tuple(ts) => {
                write!(f, "tuple[")?;
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
            Ty::Data(n) => write!(f, "{n}"),
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
