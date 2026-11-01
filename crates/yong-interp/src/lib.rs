//! Interpreter over Yong IR text: `compile` lowers each function to register bytecode, `vm` runs it.
//!
//! `par` runs its branches in order, which M0 allows. Contracts run at the `check` level for
//! `requires`; `ensures`, `decreases` and `example` are not evaluated yet.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::io::Write;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use num_bigint::BigInt;
use yong_core::ast::*;

mod bind;
mod collections;
mod compile;
mod iter;
mod keyed;
mod math;
mod native;
mod prelude;
mod regex;
#[cfg(test)]
mod std_parity;
#[cfg(test)]
mod std_selftest;
mod vm;

pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_PANIC: i32 = 101;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Value {
    #[default]
    Unit,
    Bool(bool),
    Int(i128),
    Float(f64),
    Char(char),
    Str(Rc<str>),
    Tuple(Vec<Value>),
    List(Vec<Value>),
    /// Insertion-ordered; removal keeps the order of the rest (std.dict).
    Dict(IndexMap<Key, Value, BuildHasherDefault<Fnv>>),
    Set(IndexSet<Key, BuildHasherDefault<Fnv>>),
    /// A deque and its `maxlen`.
    Deque(VecDeque<Value>, Option<usize>),
    /// A 4-ary min-heap in array order.
    Heap(Vec<Value>),
    /// Sorted by key.
    SortedMap(Vec<(Value, Value)>),
    SortedSet(Vec<Value>),
    BigInt(BigInt),
    /// A record or a sum variant.
    Data {
        name: Name,
        fields: Vec<(Name, Value)>,
    },
    Cap(Name),
    Closure(Rc<vm::Clo>),
    Iter(iter::It),
}

/// FNV-1a: frames hash short names on every access, where SipHash dominated the profile.
#[derive(Default)]
pub(crate) struct Fnv(u64);

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            self.0
        };
        for b in bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
        self.0 = h;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type Map<K, V> = HashMap<K, V, BuildHasherDefault<Fnv>>;

/// A dict key or set element. Only hashable values reach here; the checker rejects the rest.
#[derive(Clone, Debug, PartialEq)]
struct Key(Value);

impl Eq for Key {}

impl Hash for Key {
    fn hash<H: Hasher>(&self, h: &mut H) {
        fn go<H: Hasher>(v: &Value, h: &mut H) {
            std::mem::discriminant(v).hash(h);
            match v {
                Value::Bool(b) => b.hash(h),
                Value::Int(i) => i.hash(h),
                Value::Float(f) => (if *f == 0.0 { 0.0f64 } else { *f }).to_bits().hash(h),
                Value::Char(c) => c.hash(h),
                Value::Str(s) => s.hash(h),
                Value::BigInt(b) => b.hash(h),
                Value::Tuple(xs) | Value::List(xs) => xs.iter().for_each(|x| go(x, h)),
                Value::Data { name, fields } => {
                    name.hash(h);
                    fields.iter().for_each(|(_, x)| go(x, h));
                }
                _ => {}
            }
        }
        go(&self.0, h)
    }
}
/// Why a run stopped early.
#[derive(Debug)]
pub enum Stop {
    /// A panic with its kind (8.3).
    Panic(String),
    /// A construct this interpreter does not run yet: a tool failure, not a program result.
    Unsupported(String),
}

pub(crate) type R<T> = Result<T, Stop>;

pub(crate) struct Interp<'o> {
    out: &'o mut dyn Write,
}

pub(crate) fn panic<T>(kind: &str) -> R<T> {
    Err(Stop::Panic(kind.to_owned()))
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(Stop::Unsupported(what.into()))
}

/// Run `fn main` of `m`, writing program output to `out`. Returns the exit status.
pub fn run(m: &Module, out: &mut dyn Write) -> R<i32> {
    let program = compile::compile(m);
    let Some(main) = program.fns.get("main").copied() else {
        return unsupported("no `fn main` in the module");
    };
    let def = m
        .items
        .iter()
        .rev()
        .find_map(|i| match &i.kind {
            ItemKind::Fn(f) if f.path.join(".") == "main" => Some(f),
            _ => None,
        })
        .expect("main is a fn item");
    let args = def
        .params
        .iter()
        .map(|p| match p {
            Param::Cap { name, .. } => Ok(Value::Cap(name.clone())),
            Param::Value { .. } => unsupported("`main` with value parameters"),
        })
        .collect::<R<Vec<_>>>()?;
    let status = match vm::Vm::new(program, Interp { out }).call(main, args)?.0 {
        Ok(_) => 0,
        Err(_) => EXIT_FAILURE,
    };
    Ok(status)
}

pub(crate) fn int_range(ty: &str) -> Option<(i128, i128)> {
    Some(match ty {
        "i8" => (i8::MIN.into(), i8::MAX.into()),
        "i16" => (i16::MIN.into(), i16::MAX.into()),
        "i32" => (i32::MIN.into(), i32::MAX.into()),
        "i64" => (i64::MIN.into(), i64::MAX.into()),
        "i128" => (i128::MIN, i128::MAX),
        "u8" => (0, u8::MAX.into()),
        "u16" => (0, u16::MAX.into()),
        "u32" => (0, u32::MAX.into()),
        "u64" => (0, u64::MAX.into()),
        "Addr" => (0, u64::MAX.into()),
        _ => return None,
    })
}

fn parse_int(s: &str) -> R<i128> {
    let t: Cow<str> = if s.contains('_') {
        Cow::Owned(s.replace('_', ""))
    } else {
        Cow::Borrowed(s)
    };
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.as_ref()),
    };
    let (radix, digits) = match t.get(..2) {
        Some("0x") => (16, &t[2..]),
        Some("0o") => (8, &t[2..]),
        Some("0b") => (2, &t[2..]),
        _ => (10, t),
    };
    match i128::from_str_radix(digits, radix) {
        Ok(v) if neg => Ok(-v),
        Ok(v) => Ok(v),
        Err(_) => unsupported(format!("integer literal `{s}` beyond i128")),
    }
}

pub(crate) fn literal(l: &Literal) -> R<Value> {
    Ok(match l {
        Literal::Int(s) => Value::Int(parse_int(s)?),
        Literal::Float(s) => Value::Float(
            s.replace('_', "")
                .parse()
                .map_err(|_| Stop::Unsupported(format!("float `{s}`")))?,
        ),
        Literal::Str(s) => Value::Str(s.as_str().into()),
        Literal::Char(c) => Value::Char(*c),
        Literal::Bool(b) => Value::Bool(*b),
        Literal::Unit => Value::Unit,
    })
}

fn checked(v: Option<i128>, ty: &str) -> R<Value> {
    let (lo, hi) = int_range(ty).expect("caller checked the type");
    match v {
        Some(v) if (lo..=hi).contains(&v) => Ok(Value::Int(v)),
        _ => panic("overflow"),
    }
}

/// `<op>_<scalar>` intrinsics (assumption Y4). `None` when `name` is not an intrinsic.
pub(crate) fn intrinsic(name: &str, args: &[Value]) -> Option<R<Value>> {
    if name == "exclusive_check" {
        return Some(match args {
            [a, b] if a == b => panic("exclusivity"),
            _ => Ok(Value::Unit),
        });
    }
    let (op, ty) = name.rsplit_once('_')?;
    if let [Value::BigInt(a), Value::BigInt(b)] = args {
        return native::bigint_op(op, a, b);
    }
    Some(match (args, int_range(ty)) {
        ([Value::Int(a), Value::Int(b)], Some(_)) => {
            let (a, b) = (*a, *b);
            match op {
                "add_checked" => checked(a.checked_add(b), ty),
                "sub_checked" => checked(a.checked_sub(b), ty),
                "mul_checked" => checked(a.checked_mul(b), ty),
                "floordiv" | "mod" if b == 0 => panic("div_zero"),
                "floordiv" => checked(
                    Some(a.div_euclid(b) - i128::from(a.rem_euclid(b) != 0 && b < 0)),
                    ty,
                ),
                "mod" => {
                    let r = a.rem_euclid(b);
                    checked(Some(if r != 0 && b < 0 { r + b } else { r }), ty)
                }
                "div" => Ok(Value::Float(a as f64 / b as f64)),
                "and" => Ok(Value::Int(a & b)),
                "or" => Ok(Value::Int(a | b)),
                "xor" => Ok(Value::Int(a ^ b)),
                "eq" => Ok(Value::Bool(a == b)),
                "ne" => Ok(Value::Bool(a != b)),
                "lt" => Ok(Value::Bool(a < b)),
                "le" => Ok(Value::Bool(a <= b)),
                "gt" => Ok(Value::Bool(a > b)),
                "ge" => Ok(Value::Bool(a >= b)),
                _ => return None,
            }
        }
        ([Value::Float(a), Value::Float(b)], None) if ty.starts_with('f') => {
            let (a, b) = (*a, *b);
            Ok(match op {
                "add" => Value::Float(a + b),
                "sub" => Value::Float(a - b),
                "mul" => Value::Float(a * b),
                "div" => Value::Float(a / b),
                "eq" => Value::Bool(a == b),
                "ne" => Value::Bool(a != b),
                "lt" => Value::Bool(a < b),
                "le" => Value::Bool(a <= b),
                "gt" => Value::Bool(a > b),
                "ge" => Value::Bool(a >= b),
                _ => return None,
            })
        }
        ([Value::Bool(a), Value::Bool(b)], None) if ty == "bool" => Ok(Value::Bool(match op {
            "and" => *a && *b,
            "or" => *a || *b,
            "eq" => a == b,
            "ne" => a != b,
            _ => return None,
        })),
        ([Value::Bool(a)], None) if ty == "bool" && op == "not" => Ok(Value::Bool(!a)),
        _ => return None,
    })
}

pub(crate) fn data(name: &str, field: &str, v: Value) -> Value {
    Value::Data {
        name: name.to_owned(),
        fields: vec![(field.to_owned(), v)],
    }
}

/// The std functions used by M0 programs.
static BUILTINS: &[(yong_std::registry::Id, bind::Bind)] = {
    use bind::{Bind, bad_args};
    use yong_std::registry::id::*;
    &[
        (
            STD_IO_PRINT,
            Bind::Cap(|it, a| match a {
                [Value::Cap(_), Value::Str(s)] => match writeln!(it.out, "{s}") {
                    Ok(()) => Ok(Value::Unit),
                    Err(e) => unsupported(format!("stdout: {e}")),
                },
                _ => bad_args(STD_IO_PRINT),
            }),
        ),
        (STD_LIST_NEW, Bind::Plain(|_| Ok(Value::List(Vec::new())))),
        (
            STD_LIST_LEN,
            Bind::Plain(|a| match a {
                [Value::List(xs)] => Ok(Value::Int(xs.len() as i128)),
                _ => bad_args(STD_LIST_LEN),
            }),
        ),
        (
            STD_LIST_PUSH,
            Bind::Plain(|a| match a {
                [Value::List(xs), v] => {
                    xs.push(std::mem::take(v));
                    Ok(Value::Unit)
                }
                _ => bad_args(STD_LIST_PUSH),
            }),
        ),
        (
            STD_RESULT_IS_OK,
            Bind::Plain(|a| match a {
                [Value::Data { name, .. }] => Ok(Value::Bool(name == "Ok")),
                _ => bad_args(STD_RESULT_IS_OK),
            }),
        ),
    ]
};

#[cfg(test)]
mod selftest {
    use super::*;

    fn run_src(body: &str) -> (i32, String) {
        let src = format!("module t\n{body}");
        let (m, d) = yong_core::parse("t.yong", &src);
        assert!(
            d.is_empty(),
            "{:?}",
            d.iter().map(|d| d.render_line(None)).collect::<Vec<_>>()
        );
        let mut out = Vec::new();
        let status = match run(&m.unwrap(), &mut out) {
            Ok(s) => s,
            Err(Stop::Panic(_)) => EXIT_PANIC,
            Err(Stop::Unsupported(w)) => panic!("unsupported: {w}"),
        };
        (status, String::from_utf8(out).unwrap())
    }

    #[test]
    fn inout_writes_back_and_closures_capture() {
        let (s, _) = run_src(
            "fn bump(inout n: i64) -> Unit {\n n = add_checked_i64(let n, let 1)\n ret ()\n}\n\
             fn main() -> Unit {\n var a: i64 = 1\n bump(inout a)\n let b: i64 = 10\n\
             let f: Fn(let i64) -> i64 = fn [let b](let x: i64) -> i64 {\n let r: i64 = add_checked_i64(let x, let b)\n ret r\n }\n\
             let r: i64 = f(let a)\n assert eq_i64(let r, let 12)\n ret ()\n}\n",
        );
        assert_eq!(s, 0);
    }

    #[test]
    fn failure_out_of_main_exits_1() {
        let (s, _) = run_src(
            "record E { c: i64 }\nfn main() -> Unit fail E {\n let e: E = E { c: 1 }\n fail e\n}\n",
        );
        assert_eq!(s, EXIT_FAILURE);
    }

    #[test]
    fn floor_semantics() {
        let i = |op, a, b| match intrinsic(op, &[Value::Int(a), Value::Int(b)]) {
            Some(Ok(Value::Int(v))) => v,
            other => panic!("{other:?}"),
        };
        assert_eq!(i("floordiv_i64", -7, 2), -4);
        assert_eq!(i("floordiv_i64", 7, -2), -4);
        assert_eq!(i("floordiv_i64", -7, -2), 3);
        assert_eq!(i("mod_i64", -7, 3), 2);
        assert_eq!(i("mod_i64", 7, -3), -2);
        assert!(matches!(
            intrinsic(
                "floordiv_i64",
                &[Value::Int(i64::MIN.into()), Value::Int(-1)]
            ),
            Some(Err(_))
        ));
        assert!(matches!(
            intrinsic("add_checked_u8", &[Value::Int(255), Value::Int(1)]),
            Some(Err(_))
        ));
    }

    #[test]
    fn loops_and_labels() {
        let (s, out) = run_src(
            "fn main(cap io: Io) -> Unit {\n var i: i64 = 0\n loop 'l {\n i = add_checked_i64(let i, let 1)\n\
             let done: bool = ge_i64(let i, let 3)\n if done { break 'l }\n std.io.print(cap io, let \"x\")\n }\n ret ()\n}\n",
        );
        assert_eq!((s, out.as_str()), (0, "x\nx\n"));
    }

    #[test]
    fn join_point_returns_past_the_jumping_statement() {
        let (s, out) = run_src(
            "fn main(cap io: Io) -> Unit {\n var n: i64 = 0\n\
             jp k(let m: i64) { n = add_checked_i64(let n, let m) }\n\
             loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n\
             if big { jump k(10) }\n }\n\
             assert eq_i64(let n, let 13)\n std.io.print(cap io, let \"done\")\n ret ()\n}\n",
        );
        assert_eq!((s, out.as_str()), (0, "done\n"));
    }
}
