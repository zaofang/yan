//! Tree-walking interpreter over Yong IR text (M0).
//!
//! `par` runs its branches in order, which M0 allows. Contracts run at the `check` level for
//! `requires`; `ensures`, `decreases` and `example` are not evaluated yet.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::io::Write;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use num_bigint::BigInt;
use yong_core::ast::*;

mod native;
mod prelude;
mod regex;
#[cfg(test)]
mod std_parity;
#[cfg(test)]
mod std_selftest;

pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_PANIC: i32 = 101;

#[derive(Clone, Debug, PartialEq)]
enum Value {
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
    BigInt(BigInt),
    /// A record or a sum variant.
    Data {
        name: Name,
        fields: Vec<(Name, Value)>,
    },
    Cap(Name),
    Closure(Rc<(Closure, Frame)>),
}

/// FNV-1a: frames hash short names on every access, where SipHash dominated the profile.
#[derive(Default)]
struct Fnv(u64);

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

type Map<K, V> = HashMap<K, V, BuildHasherDefault<Fnv>>;

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
type Frame = Map<Name, Value>;

fn key(p: &Path) -> Cow<'_, str> {
    match p.as_slice() {
        [one] => Cow::Borrowed(one),
        _ => Cow::Owned(p.join(".")),
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

type R<T> = Result<T, Stop>;

enum Flow {
    Normal,
    Ret(Value),
    Fail(Value),
    Jump(Name, Vec<Value>),
    Break(Name),
    Continue(Name),
    Become(FnRef, Vec<Value>),
}

#[derive(Clone)]
enum FnRef {
    Item(Rc<FnDef>),
    Closure(Rc<(Closure, Frame)>),
}

struct Interp<'o> {
    fns: Map<Name, Rc<FnDef>>,
    /// `impl` methods by `<type>.<method>`; a call by method name dispatches on its first argument.
    methods: Map<Name, Rc<FnDef>>,
    nullary: HashSet<Name, BuildHasherDefault<Fnv>>,
    out: &'o mut dyn Write,
}

/// The Yong type a value's `impl` is keyed by, where the value tells it.
fn type_key(v: &Value) -> Option<String> {
    Some(match v {
        Value::Data { name, .. } => name.to_string(),
        Value::Int(_) => "i64".into(),
        Value::Float(_) => "f64".into(),
        Value::Bool(_) => "bool".into(),
        Value::Str(_) => "Str".into(),
        Value::Char(_) => "char".into(),
        _ => return None,
    })
}

fn panic<T>(kind: &str) -> R<T> {
    Err(Stop::Panic(kind.to_owned()))
}

fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(Stop::Unsupported(what.into()))
}

/// Run `fn main` of `m`, writing program output to `out`. Returns the exit status.
pub fn run(m: &Module, out: &mut dyn Write) -> R<i32> {
    let mut it = Interp {
        fns: Map::default(),
        methods: Map::default(),
        nullary: ["None".to_owned()].into_iter().collect(),
        out,
    };
    for item in &m.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                it.fns.insert(f.path.join("."), Rc::new(f.clone()));
            }
            ItemKind::Impl(i) => {
                let ty = yong_core::type_text(&i.for_ty);
                for f in &i.fns {
                    it.methods
                        .insert(format!("{ty}.{}", f.path.join(".")), Rc::new(f.clone()));
                }
            }
            ItemKind::Sum(s) => {
                it.nullary.extend(
                    s.variants
                        .iter()
                        .filter(|v| v.fields.is_none())
                        .map(|v| v.name.clone()),
                );
            }
            _ => {}
        }
    }
    let Some(main) = it.fns.get("main").cloned() else {
        return unsupported("no `fn main` in the module");
    };
    let args = main
        .params
        .iter()
        .map(|p| match p {
            Param::Cap { name, .. } => Ok(Value::Cap(name.clone())),
            Param::Value { .. } => unsupported("`main` with value parameters"),
        })
        .collect::<R<Vec<_>>>()?;
    let status = match it.call(FnRef::Item(main), args)? {
        (Ok(_), _) => 0,
        (Err(_), _) => EXIT_FAILURE,
    };
    Ok(status)
}

fn int_range(ty: &str) -> Option<(i128, i128)> {
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

fn literal(l: &Literal) -> R<Value> {
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
fn intrinsic(name: &str, args: &[Value]) -> Option<R<Value>> {
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

fn data(name: &str, field: &str, v: Value) -> Value {
    Value::Data {
        name: name.to_owned(),
        fields: vec![(field.to_owned(), v)],
    }
}

impl Interp<'_> {
    /// Call `f`. The outcome is the return value or the failure value, plus the final values of
    /// the parameters, for `inout` write-back.
    fn call(
        &mut self,
        mut f: FnRef,
        mut args: Vec<Value>,
    ) -> R<(Result<Value, Value>, Vec<Value>)> {
        loop {
            let cur = f.clone();
            let (params, body, mut frame) = match &cur {
                FnRef::Item(d) => (&d.params, d.body.as_ref(), Frame::default()),
                FnRef::Closure(c) => (&c.0.params, Some(&c.0.body), c.1.clone()),
            };
            let names: Vec<&Name> = params
                .iter()
                .map(|(Param::Value { name, .. } | Param::Cap { name, .. })| name)
                .collect();
            for (n, v) in names.iter().zip(args) {
                frame.insert((*n).clone(), v);
            }
            if let FnRef::Item(d) = &cur {
                for c in &d.contracts {
                    if let Contract::Requires(e) = c
                        && self.eval_expr(e, &mut frame)? != Value::Bool(true)
                    {
                        return panic("contract");
                    }
                }
            }
            let Some(body) = body else {
                return unsupported("call of a function without a body");
            };
            let flow = self.block(body, &mut frame)?;
            let finals = || {
                names
                    .iter()
                    .map(|n| frame.get(*n).cloned().unwrap_or(Value::Unit))
                    .collect()
            };
            match flow {
                Flow::Ret(v) => return Ok((Ok(v), finals())),
                Flow::Fail(e) => return Ok((Err(e), finals())),
                Flow::Become(g, a) => {
                    f = g;
                    args = a;
                }
                Flow::Normal => return unsupported("function body ended without `ret`"),
                Flow::Jump(k, _) => {
                    return unsupported(format!("jump to undeclared join point `{k}`"));
                }
                Flow::Break(l) | Flow::Continue(l) => {
                    return unsupported(format!("label `'{l}` outside its loop"));
                }
            }
        }
    }

    fn block(&mut self, b: &Block, frame: &mut Frame) -> R<Flow> {
        let mut jps: HashMap<&str, (&[Param], &Block)> = HashMap::new();
        for s in &b.stmts {
            if let StmtKind::Jp { name, params, body } = &s.kind {
                jps.insert(name, (params, body));
                continue;
            }
            let mut flow = self.stmt(s, frame)?;
            while let Flow::Jump(k, args) = &flow {
                let Some((params, body)) = jps.get(k.as_str()) else {
                    break;
                };
                for (p, v) in params.iter().zip(args) {
                    let (Param::Value { name, .. } | Param::Cap { name, .. }) = p;
                    frame.insert(name.clone(), v.clone());
                }
                flow = self.block(body, frame)?;
            }
            if !matches!(flow, Flow::Normal) {
                return Ok(flow);
            }
        }
        Ok(Flow::Normal)
    }

    fn atom(&self, a: &Atom, frame: &Frame) -> R<Value> {
        match a {
            Atom::Lit(l, _) => literal(l),
            Atom::Name(n, _) => self.lookup(n, frame),
        }
    }

    fn lookup(&self, n: &Name, frame: &Frame) -> R<Value> {
        match frame.get(n) {
            Some(v) => Ok(v.clone()),
            None if self.nullary.contains(n) => Ok(Value::Data {
                name: n.clone(),
                fields: Vec::new(),
            }),
            None => unsupported(format!("unbound name `{n}`")),
        }
    }

    fn truthy(&self, a: &Atom, frame: &Frame) -> R<bool> {
        match self.atom(a, frame)? {
            Value::Bool(b) => Ok(b),
            v => unsupported(format!("condition is not a bool: {v:?}")),
        }
    }

    fn stmt(&mut self, s: &Stmt, frame: &mut Frame) -> R<Flow> {
        Ok(match &s.kind {
            StmtKind::Let { pat, rhs, .. } => {
                let v = match self.rhs(rhs, frame)? {
                    Ok(v) => v,
                    Err(flow) => return Ok(flow),
                };
                if !self.bind(pat, &v, frame) {
                    return panic("pattern");
                }
                Flow::Normal
            }
            StmtKind::Assign { place, rhs, .. } => {
                let v = match self.rhs(rhs, frame)? {
                    Ok(v) => v,
                    Err(flow) => return Ok(flow),
                };
                self.assign(place, v, frame)?;
                Flow::Normal
            }
            StmtKind::Rhs(rhs) => match self.rhs(rhs, frame)? {
                Ok(_) => Flow::Normal,
                Err(flow) => flow,
            },
            StmtKind::If { cond, then, els } => {
                if self.truthy(cond, frame)? {
                    self.block(then, frame)?
                } else if let Some(e) = els {
                    self.block(e, frame)?
                } else {
                    Flow::Normal
                }
            }
            StmtKind::Match { scrutinee, arms } => {
                let v = self.atom(scrutinee, frame)?;
                for (p, b) in arms {
                    if self.matches(p, &v) {
                        self.bind(p, &v, frame);
                        return self.block(b, frame);
                    }
                }
                return panic("match");
            }
            StmtKind::Loop { label, body, .. } => loop {
                match self.block(body, frame)? {
                    Flow::Break(l) if l == *label => break Flow::Normal,
                    Flow::Continue(l) if l == *label => {}
                    Flow::Normal => {}
                    other => break other,
                }
            },
            StmtKind::Break { label, .. } => Flow::Break(label.clone()),
            StmtKind::Continue { label } => Flow::Continue(label.clone()),
            StmtKind::Jp { .. } => Flow::Normal,
            StmtKind::Jump { name, args } => Flow::Jump(
                name.clone(),
                args.iter().map(|a| self.atom(a, frame)).collect::<R<_>>()?,
            ),
            StmtKind::Ret(a) => Flow::Ret(self.atom(a, frame)?),
            StmtKind::Fail(a) => Flow::Fail(self.atom(a, frame)?),
            StmtKind::Become(c) => {
                let (f, args) = self.resolve_call(c, frame)?;
                match f {
                    Some(f) => Flow::Become(f, args),
                    None => return unsupported("`become` of a std function or intrinsic"),
                }
            }
            StmtKind::Par { blocks, .. } => {
                for b in blocks {
                    let flow = self.block(b, frame)?;
                    if !matches!(flow, Flow::Normal) {
                        return Ok(flow);
                    }
                }
                Flow::Normal
            }
            StmtKind::Assert(e) => {
                if self.eval_expr(e, frame)? != Value::Bool(true) {
                    return panic("assert");
                }
                Flow::Normal
            }
            StmtKind::Unreachable => return panic("unreachable"),
            StmtKind::Unsafe(b) => self.block(b, frame)?,
            StmtKind::Drop(n) => {
                frame.remove(n);
                Flow::Normal
            }
            StmtKind::Yield(_) => return unsupported("generators"),
            StmtKind::With {
                name,
                dyn_: false,
                handler,
                body,
            } => {
                let h = self.atom(handler, frame)?;
                let outer = frame.insert(name.clone(), h);
                let flow = self.block(body, frame)?;
                match outer {
                    Some(v) => frame.insert(name.clone(), v),
                    None => frame.remove(name),
                };
                flow
            }
            StmtKind::With { .. } => return unsupported("`with ... = dyn`"),
            StmtKind::ComptimeFor { .. } | StmtKind::ComptimeIf { .. } => {
                return unsupported("stage 0");
            }
            StmtKind::Asm { .. } => return unsupported("inline assembly"),
            StmtKind::Reuse { .. } => return unsupported("`reuse`"),
        })
    }

    fn eval_expr(&mut self, e: &Expr, frame: &mut Frame) -> R<Value> {
        match e {
            Expr::Atom(a) => self.atom(a, frame),
            Expr::Call(c) => match self.call_value(c, frame)? {
                Ok(v) => Ok(v),
                Err(_) => unsupported("a failing call in a contract or assertion"),
            },
        }
    }

    /// Evaluate `rhs`. `Err` carries a flow that leaves the statement: a propagated failure.
    fn rhs(&mut self, rhs: &Rhs, frame: &mut Frame) -> R<Result<Value, Flow>> {
        Ok(Ok(match rhs {
            Rhs::Atom(a) => self.atom(a, frame)?,
            Rhs::Proj(p) | Rhs::Copy(p) | Rhs::Move(p) => self.read(p, frame)?,
            Rhs::Call(c) => match self.call_value(c, frame)? {
                Ok(v) => v,
                Err(e) => return Ok(Err(Flow::Fail(e))),
            },
            Rhs::Propagate { call, via } => match self.call_value(call, frame)? {
                Ok(v) => v,
                Err(e) => {
                    let e = match via {
                        None => e,
                        Some(p) => match self.fns.get(key(p).as_ref()).cloned() {
                            Some(f) => match self.call(FnRef::Item(f), vec![e])?.0 {
                                Ok(v) | Err(v) => v,
                            },
                            None => match native::fallible(&key(p), &[e]) {
                                Some(Ok(v) | Err(v)) => v,
                                None => return unsupported(format!("`via {}`", p.join("."))),
                            },
                        },
                    };
                    return Ok(Err(Flow::Fail(e)));
                }
            },
            Rhs::Try(c) => match self.call_value(c, frame)? {
                Ok(v) => data("Ok", "value", v),
                Err(e) => data("Err", "error", e),
            },
            Rhs::Ctor(Ctor::Tuple(xs)) => {
                Value::Tuple(xs.iter().map(|a| self.atom(a, frame)).collect::<R<_>>()?)
            }
            Rhs::Ctor(Ctor::Named { path, fields, .. }) => Value::Data {
                name: path.last().expect("paths are non-empty").clone(),
                fields: fields
                    .iter()
                    .map(|(n, a)| Ok((n.clone(), self.atom(a, frame)?)))
                    .collect::<R<_>>()?,
            },
            Rhs::Closure(c) => {
                let captured = c
                    .captures
                    .iter()
                    .map(|(_, n, _)| Ok((n.clone(), self.lookup(n, frame)?)))
                    .collect::<R<Frame>>()?;
                Value::Closure(Rc::new(((**c).clone(), captured)))
            }
        }))
    }

    fn read(&self, p: &Place, frame: &Frame) -> R<Value> {
        let Some(mut v) = frame.get(&p.root) else {
            return self.lookup(&p.root, frame);
        };
        for proj in &p.projs {
            v = match (proj, v) {
                (Proj::Field(f, _), Value::Data { fields, .. }) => {
                    match fields.iter().find(|(n, _)| n == f) {
                        Some((_, v)) => v,
                        None => return unsupported(format!("no field `{f}`")),
                    }
                }
                (Proj::Field(f, _), Value::Tuple(xs)) => {
                    match f.strip_prefix('_').and_then(|i| i.parse::<usize>().ok()) {
                        Some(i) if i < xs.len() => &xs[i],
                        _ => return unsupported(format!("tuple field `{f}`")),
                    }
                }
                (Proj::Index(i), Value::List(xs)) => match self.atom(i, frame)? {
                    Value::Int(i) => match usize::try_from(i).ok().and_then(|i| xs.get(i)) {
                        Some(v) => v,
                        None => return panic("bounds"),
                    },
                    _ => return unsupported("non-integer index"),
                },
                (proj, v) => return unsupported(format!("projection {proj:?} on {v:?}")),
            };
        }
        Ok(v.clone())
    }

    fn assign(&self, p: &Place, v: Value, frame: &mut Frame) -> R<()> {
        let index = |a: &Atom| match self.atom(a, frame)? {
            Value::Int(i) => Ok(i),
            _ => unsupported("non-integer index"),
        };
        let idx: Vec<Option<i128>> = p
            .projs
            .iter()
            .map(|pr| {
                if let Proj::Index(a) = pr {
                    index(a).map(Some)
                } else {
                    Ok(None)
                }
            })
            .collect::<R<_>>()?;
        let Some(mut slot) = frame.get_mut(&p.root) else {
            frame.insert(p.root.clone(), v);
            return Ok(());
        };
        for (proj, i) in p.projs.iter().zip(idx) {
            slot = match (proj, slot) {
                (Proj::Field(f, _), Value::Data { fields, .. }) => {
                    match fields.iter_mut().find(|(n, _)| n == f) {
                        Some((_, s)) => s,
                        None => return unsupported(format!("no field `{f}`")),
                    }
                }
                (Proj::Index(_), Value::List(xs)) => {
                    match usize::try_from(i.expect("index computed"))
                        .ok()
                        .and_then(|i| xs.get_mut(i))
                    {
                        Some(s) => s,
                        None => return panic("bounds"),
                    }
                }
                _ => return unsupported("assignment through this projection"),
            };
        }
        *slot = v;
        Ok(())
    }

    /// `inout` arguments are moved out of the frame; the caller writes them back.
    fn resolve_call(&mut self, c: &Call, frame: &mut Frame) -> R<(Option<FnRef>, Vec<Value>)> {
        let args = c
            .args
            .iter()
            .map(|a| match a {
                Arg::Value(Conv::Inout, Atom::Name(n, _), _) if frame.contains_key(n) => {
                    Ok(frame.insert(n.clone(), Value::Unit).expect("present"))
                }
                Arg::Value(_, a, _) => self.atom(a, frame),
                Arg::Cap(n, _) => self.lookup(n, frame),
            })
            .collect::<R<Vec<_>>>()?;
        let name = key(&c.path);
        if let Some(f) = self.fns.get(name.as_ref()) {
            return Ok((Some(FnRef::Item(f.clone())), args));
        }
        if let Some(ty) = args.first().and_then(type_key)
            && let Some(f) = self.methods.get(&format!("{ty}.{name}"))
        {
            return Ok((Some(FnRef::Item(f.clone())), args));
        }
        if let (true, Some(Value::Closure(k))) = (c.path.len() == 1, frame.get(name.as_ref())) {
            return Ok((Some(FnRef::Closure(k.clone())), args));
        }
        Ok((None, args))
    }

    /// Call and write `inout` arguments back. `Err` is the callee's failure value.
    fn call_value(&mut self, c: &Call, frame: &mut Frame) -> R<Result<Value, Value>> {
        let (f, mut args) = self.resolve_call(c, frame)?;
        let name = key(&c.path);
        let (outcome, finals) = match f {
            Some(f) => self.call(f, args)?,
            None => {
                if let Some(r) = native::fallible(&name, &args) {
                    (r, args)
                } else {
                    let v = match intrinsic(&name, &args) {
                        Some(v) => v?,
                        None => self.std(&name, &mut args)?,
                    };
                    (Ok(v), args)
                }
            }
        };
        for (a, v) in c.args.iter().zip(finals) {
            if let Arg::Value(Conv::Inout, Atom::Name(n, _), _) = a {
                frame.insert(n.clone(), v);
            }
        }
        Ok(outcome)
    }

    /// The std functions used by M0 programs (assumption Y3).
    fn std(&mut self, name: &str, args: &mut [Value]) -> R<Value> {
        if let Some(r) = prelude::call(name, args) {
            return r;
        }
        if let Some(r) = native::call(name, args) {
            return r;
        }
        if let Some(r) = regex::call(name, args) {
            return r;
        }
        Ok(match (name, &mut *args) {
            ("std.io.print", [Value::Cap(_), Value::Str(s)]) => {
                writeln!(self.out, "{s}").map_err(|e| Stop::Unsupported(format!("stdout: {e}")))?;
                Value::Unit
            }
            ("std.list.new", []) => Value::List(Vec::new()),
            ("std.list.len", [Value::List(xs)]) => Value::Int(xs.len() as i128),
            ("std.list.push", [Value::List(xs), v]) => {
                xs.push(v.clone());
                Value::Unit
            }
            ("std.result.is_ok", [Value::Data { name, .. }]) => Value::Bool(name == "Ok"),
            _ => return unsupported(format!("call of `{name}`")),
        })
    }

    fn matches(&self, p: &Pat, v: &Value) -> bool {
        match (p, v) {
            (Pat::Wild, _) => true,
            (Pat::Bind(n), Value::Data { name, fields }) if self.nullary.contains(n) => {
                name == n && fields.is_empty()
            }
            (Pat::Bind(n), _) => !self.nullary.contains(n),
            (Pat::Lit(l), v) => literal(l).is_ok_and(|l| l == *v),
            (Pat::Range(lo, hi), Value::Int(x)) => match (literal(lo), literal(hi)) {
                (Ok(Value::Int(lo)), Ok(Value::Int(hi))) => (lo..=hi).contains(x),
                _ => false,
            },
            (Pat::Or(ps), v) => ps.iter().any(|p| self.matches(p, v)),
            (Pat::Tuple(ps), Value::Tuple(xs)) => {
                ps.len() == xs.len() && ps.iter().zip(xs).all(|(p, x)| self.matches(p, x))
            }
            (Pat::Ctor { path, fields: pf }, Value::Data { name, fields }) => {
                path.last() == Some(name)
                    && pf.as_ref().is_none_or(|(ps, _)| {
                        ps.iter()
                            .all(|(n, p)| fields.iter().any(|(m, x)| m == n && self.matches(p, x)))
                    })
            }
            (Pat::Slice { elems, rest }, Value::List(xs)) => {
                let fits = if rest.is_some() {
                    xs.len() >= elems.len()
                } else {
                    xs.len() == elems.len()
                };
                fits && elems.iter().zip(xs).all(|(p, x)| self.matches(p, x))
            }
            _ => false,
        }
    }

    /// Bind the names of `p`, which matched `v`. Returns false when `p` does not match.
    fn bind(&self, p: &Pat, v: &Value, frame: &mut Frame) -> bool {
        if !self.matches(p, v) {
            return false;
        }
        match (p, v) {
            (Pat::Bind(n), _) if !self.nullary.contains(n) => {
                frame.insert(n.clone(), v.clone());
            }
            (Pat::Tuple(ps), Value::Tuple(xs)) => {
                for (p, x) in ps.iter().zip(xs) {
                    self.bind(p, x, frame);
                }
            }
            (
                Pat::Ctor {
                    fields: Some((ps, _)),
                    ..
                },
                Value::Data { fields, .. },
            ) => {
                for (n, p) in ps {
                    if let Some((_, x)) = fields.iter().find(|(m, _)| m == n) {
                        self.bind(p, x, frame);
                    }
                }
            }
            (Pat::Or(ps), v) => {
                if let Some(p) = ps.iter().find(|p| self.matches(p, v)) {
                    self.bind(p, v, frame);
                }
            }
            (Pat::Slice { elems, rest }, Value::List(xs)) => {
                for (p, x) in elems.iter().zip(xs) {
                    self.bind(p, x, frame);
                }
                if let Some(Some(n)) = rest {
                    frame.insert(n.clone(), Value::List(xs[elems.len()..].to_vec()));
                }
            }
            _ => {}
        }
        true
    }
}

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
}
