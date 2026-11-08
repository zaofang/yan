//! Optimized Yan Core (`yan-opt`'s output, R771) to MIR.

use std::cell::RefCell;
use std::collections::HashMap;

use yan_core::ast as y;
use yan_std::registry::{self, id};

use crate::*;

type R<T> = Result<T, String>;

fn unsupported<T>(what: impl std::fmt::Display) -> R<T> {
    Err(format!("{what} in native code"))
}

const I64: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

const DEQUE_IDS: &[registry::Id] = &[
    id::STD_PRELUDE_DEQUE,
    id::STD_COLLECTIONS_DEQUE_APPEND,
    id::STD_COLLECTIONS_DEQUE_APPENDLEFT,
    id::STD_COLLECTIONS_DEQUE_POP,
    id::STD_COLLECTIONS_DEQUE_POPLEFT,
    id::STD_COLLECTIONS_DEQUE_FIRST,
    id::STD_COLLECTIONS_DEQUE_LAST,
    id::STD_COLLECTIONS_DEQUE_EXTEND,
    id::STD_COLLECTIONS_DEQUE_ROTATE,
    id::STD_COLLECTIONS_DEQUE_CLEAR,
    id::STD_COLLECTIONS_DEQUE_MAXLEN,
    id::STD_COLLECTIONS_DEQUE_LEN,
    id::STD_COLLECTIONS_DEQUE_CONTAINS,
];

/// The kinds `RtFn::Select` takes, as `yan_rt::select` numbers them.
mod select_kind {
    pub const PERMUTATIONS: i64 = 0;
    pub const COMBINATIONS: i64 = 1;
    pub const WITH_REPLACEMENT: i64 = 2;
}

const ITERTOOLS_IDS: &[registry::Id] = &[
    id::STD_ITERTOOLS_PERMUTATIONS,
    id::STD_ITERTOOLS_COMBINATIONS,
    id::STD_ITERTOOLS_COMBINATIONS_WITH_REPLACEMENT,
    id::STD_ITERTOOLS_PRODUCT,
    id::STD_ITERTOOLS_PAIRWISE,
    id::STD_ITERTOOLS_ACCUMULATE,
];

pub(crate) type Fields = Vec<(String, Shape)>;

/// A Yan Core type as scalar slots.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Scalar(Ty),
    Str,
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
    /// A `Sender[T]` or `Receiver[T]`: the channel both ends share.
    Chan(Box<Shape>),
}

impl Shape {
    fn slots(&self, out: &mut Vec<Ty>) {
        match self {
            Shape::Scalar(t) => out.push(*t),
            Shape::Str
            | Shape::List(_)
            | Shape::Map(..)
            | Shape::Heap(_)
            | Shape::Deque(_)
            | Shape::Chan(_) => out.push(Ty::Ptr),
            Shape::Fn(..) => out.extend([Ty::Ptr, Ty::Ptr]),
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

    fn tys(&self) -> Vec<Ty> {
        let mut v = Vec::new();
        self.slots(&mut v);
        v
    }

    fn width(&self) -> usize {
        self.tys().len()
    }

    fn has_list(&self) -> bool {
        match self {
            Shape::List(_) => true,
            Shape::Scalar(_)
            | Shape::Str
            | Shape::Map(..)
            | Shape::Heap(_)
            | Shape::Deque(_)
            | Shape::Fn(..)
            | Shape::Chan(_) => false,
            Shape::Rec(_, fs) => fs.iter().any(|(_, s)| s.has_list()),
            Shape::Tuple(ss) => ss.iter().any(Shape::has_list),
            Shape::Sum(_, vs) => vs.iter().flat_map(|(_, fs)| fs).any(|(_, s)| s.has_list()),
        }
    }

    /// The fields of a record or tuple, by name, with their first slot.
    fn fields(&self) -> Option<Vec<(String, usize, &Shape)>> {
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
    fn variant(&self, name: &str) -> Option<(i128, usize, &Fields)> {
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

fn zero(t: Ty) -> Operand {
    match t {
        Ty::Int(_) | Ty::Ptr => Operand::Int(0),
        Ty::F64 => Operand::F64(0.0),
        Ty::Bool => Operand::Bool(false),
        Ty::Unit => Operand::Unit,
    }
}

struct Types<'m> {
    recs: HashMap<&'m str, &'m y::Record>,
    sums: HashMap<&'m str, &'m y::Sum>,
}

impl Types<'_> {
    fn shape(&self, t: &y::Type) -> R<Shape> {
        self.shape_in(t, &mut Vec::new())
    }

    fn shape_in(&self, t: &y::Type, stack: &mut Vec<String>) -> R<Shape> {
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
                        return Ok(Shape::Chan(Box::new(self.shape_in(e, stack)?)));
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
const CANCEL_LIKE: &[&str] = &["Cancelled", "SessionClosed"];
const STD_FAILURES: &[&str] = &["Cancelled", "Timeout", "Draining", "Deadlock"];

fn result(ok: Shape, err: Shape) -> Shape {
    Shape::Sum(
        "Result".into(),
        vec![
            ("Ok".into(), vec![("value".into(), ok)]),
            ("Err".into(), vec![("error".into(), err)]),
        ],
    )
}

#[derive(Clone)]
struct Sig {
    params: Vec<(y::Conv, Shape)>,
    /// The declared result.
    ok: Shape,
    /// What the function returns: `ok`, or `Result[ok, E]` for `fail E`.
    ret: Shape,
    fails: bool,
}

impl Sig {
    fn inout(&self) -> Vec<Ty> {
        self.params
            .iter()
            .filter(|(c, _)| *c == y::Conv::Inout)
            .flat_map(|(_, s)| s.tys())
            .collect()
    }
}

/// The default-target layout of `ty` against the records and sums in `items`, or what it uses that
/// native code cannot lay out yet.
pub fn layout_of<'a>(items: impl IntoIterator<Item = &'a y::ItemKind>, ty: &y::Type) -> R<Layout> {
    let mut types = Types {
        recs: HashMap::new(),
        sums: HashMap::new(),
    };
    for it in items {
        match it {
            y::ItemKind::Record(r) => {
                types.recs.insert(r.path.last().expect("named"), r);
            }
            y::ItemKind::Sum(s) => {
                types.sums.insert(s.path.last().expect("named"), s);
            }
            _ => {}
        }
    }
    types.shape(ty).map(|s| layout::layout(&s))
}

/// The MIR of `m`, or what it uses that native code cannot run yet.
pub fn lower(m: &y::Module) -> R<Program> {
    let mut types = Types {
        recs: HashMap::new(),
        sums: HashMap::new(),
    };
    let mut defs = Vec::new();
    let mut by_name = HashMap::new();
    let hashes = m
        .items
        .iter()
        .any(|i| match &i.kind {
            y::ItemKind::Fn(f) | y::ItemKind::Inst(y::Inst { def: f, .. }) => f
                .contracts
                .iter()
                .any(|k| matches!(k, y::Contract::Requires(_))),
            _ => false,
        })
        .then(|| yan_core::hash::hash_module(m));
    let mut item_hashes = Vec::new();
    for (ix, i) in m.items.iter().enumerate() {
        let (f, args) = match &i.kind {
            y::ItemKind::Fn(f) => (f, &[][..]),
            y::ItemKind::Inst(i) => (&i.def, &i.args[..]),
            y::ItemKind::Record(r) => {
                types.recs.insert(r.path.last().expect("named"), r);
                continue;
            }
            y::ItemKind::Sum(s) => {
                types.sums.insert(s.path.last().expect("named"), s);
                continue;
            }
            _ => return unsupported(format!("item at line {}", i.span.line)),
        };
        if f.body.is_none() {
            continue;
        }
        if let Some(t) = f.tparams.first() {
            return Err(format!(
                "internal error: type parameter `{}` of `{}` reached yan-mir",
                t.name,
                f.path.join(".")
            ));
        }
        by_name.insert(key(&f.path, args), defs.len());
        item_hashes.push(
            hashes
                .as_ref()
                .map(|h| h[ix].identity.group.to_hex().to_string()),
        );
        defs.push(f);
    }
    let main = *by_name.get("main").ok_or("no `fn main` in the module")?;
    let costing = defs.iter().any(|f| {
        f.contracts
            .iter()
            .any(|k| matches!(k, y::Contract::Cost { .. }))
    });
    let mono = RefCell::new(Mono {
        defs,
        item_hashes,
        by_name,
        insts: HashMap::new(),
        sigs: Vec::new(),
        todo: Vec::new(),
        costing,
    });
    let main = mono.borrow_mut().instance(&types, main)?;
    let mut prog = Program {
        funcs: Vec::new(),
        main,
        main_fails: mono.borrow().sigs[main as usize].fails,
        strings: Vec::new(),
    };
    // Instances are lowered in id order; lowering one can add more.
    while prog.funcs.len() < mono.borrow().todo.len() {
        let me = prog.funcs.len();
        let todo = mono.borrow().todo[me].clone();
        let func = match todo {
            Todo::Def(def) => {
                let f: &y::FnDef = mono.borrow().defs[def];
                let item = mono.borrow().item_hashes[def].clone();
                Fn::new(&mono, &types, &mut prog.strings, me).func(f, item)?
            }
            Todo::Closure(c, caps) => {
                Fn::new(&mono, &types, &mut prog.strings, me).closure(c, caps)?
            }
        };
        prog.funcs.push(func);
    }
    crate::co::run(&mut prog);
    Ok(prog)
}

/// How a call names a definition: its path, then an instance's arguments (R772).
fn key(path: &[String], args: &[y::TArg]) -> String {
    let mut k = path.join(".");
    if !args.is_empty() {
        let args: Vec<String> = args
            .iter()
            .map(|a| match a {
                y::TArg::Type(t) => yan_core::type_text(t),
                y::TArg::Nat(n) => n.clone(),
            })
            .collect();
        k += &format!("[{}]", args.join(", "));
    }
    k
}

/// The worklist that lowers each reached definition once, in order. `yan-opt` has already
/// replaced generic definitions by instances.
struct Mono<'m> {
    defs: Vec<&'m y::FnDef>,
    /// The identity hash of each definition's item, in hex, when a contract needs it (R882).
    item_hashes: Vec<Option<String>>,
    by_name: HashMap<String, usize>,
    insts: HashMap<usize, FuncId>,
    sigs: Vec<Sig>,
    /// What each instance lowers, in id order.
    todo: Vec<Todo<'m>>,
    /// Some function has a `cost` bound, so every function counts steps and allocations.
    costing: bool,
}

#[derive(Clone)]
enum Todo<'m> {
    Def(usize),
    /// A closure body with its captures' names and types, read from the block in its first
    /// parameter.
    Closure(&'m y::Closure, Vec<(String, Shape)>),
}

impl Mono<'_> {
    fn instance(&mut self, types: &Types, def: usize) -> R<FuncId> {
        if let Some(&id) = self.insts.get(&def) {
            return Ok(id);
        }
        let f = self.defs[def];
        let ok = types.shape(&f.ret)?;
        let sig = Sig {
            params: params(f)?
                .into_iter()
                .map(|(_, c, t)| Ok((c, types.shape(t)?)))
                .collect::<R<_>>()?,
            ret: match &f.fail {
                Some(e) => result(ok.clone(), types.shape(e)?),
                None => ok.clone(),
            },
            ok,
            fails: f.fail.is_some(),
        };
        let id = self.sigs.len() as FuncId;
        self.sigs.push(sig);
        self.todo.push(Todo::Def(def));
        self.insts.insert(def, id);
        Ok(id)
    }
}

fn params(f: &y::FnDef) -> R<Vec<(&str, y::Conv, &y::Type)>> {
    f.params
        .iter()
        .filter_map(|p| match p {
            y::Param::Value { comptime: true, .. } => Some(unsupported("a comptime parameter")),
            y::Param::Value { name, ty, conv, .. } => Some(Ok((name.as_str(), *conv, ty))),
            y::Param::Cap { .. } => None,
        })
        .collect()
}

fn scalar_ty(name: &str) -> Option<Ty> {
    let int = |bits, signed| Some(Ty::Int(IntTy { bits, signed }));
    match name {
        "i8" => int(8, true),
        "i16" => int(16, true),
        "i32" => int(32, true),
        "i64" => int(64, true),
        "u8" => int(8, false),
        "u16" => int(16, false),
        "u32" => int(32, false),
        "u64" | "Addr" => int(64, false),
        "f64" => Some(Ty::F64),
        "bool" => Some(Ty::Bool),
        "Unit" => Some(Ty::Unit),
        _ => None,
    }
}

/// `<op>_<ty>` as an op and its operand type.
fn scalar(path: &str) -> Option<(ScalarOp, Option<Ty>)> {
    use ScalarOp::*;
    if path == "exclusive_check" {
        return Some((Exclusive, None));
    }
    let (op, ty) = path.rsplit_once('_')?;
    let op = match op {
        "add_checked" => AddChecked,
        "sub_checked" => SubChecked,
        "mul_checked" => MulChecked,
        "floordiv" => FloorDiv,
        "mod" => Mod,
        "div" => Div,
        "and" => And,
        "or" => Or,
        "xor" => Xor,
        "not" => Not,
        "add" => Add,
        "sub" => Sub,
        "mul" => Mul,
        "eq" => Eq,
        "ne" => Ne,
        "lt" => Lt,
        "le" => Le,
        "gt" => Gt,
        "ge" => Ge,
        _ => return None,
    };
    Some((op, Some(scalar_ty(ty)?)))
}

fn parse_int(s: &str) -> R<i128> {
    let t = s.replace('_', "");
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.as_str()),
    };
    let (radix, digits) = match t.get(..2) {
        Some("0x") => (16, &t[2..]),
        Some("0o") => (8, &t[2..]),
        Some("0b") => (2, &t[2..]),
        _ => (10, t),
    };
    let v = i128::from_str_radix(digits, radix).map_err(|_| format!("integer literal `{s}`"))?;
    Ok(if neg { -v } else { v })
}

/// A part of a match's scrutinee: its first slot and its shape.
#[derive(Clone)]
struct Occ {
    at: usize,
    shape: Shape,
}

/// A row of a match's clause matrix: one pattern per column, and the names bound so far.
#[derive(Clone)]
struct Row<'p> {
    pats: Vec<&'p y::Pat>,
    arm: usize,
    binds: Vec<(&'p str, Occ)>,
}

/// A block that reaches a match arm, and the slots each name binds there.
type Leaf = (BlockId, Vec<(String, Vec<Operand>, Shape)>);

static WILD: y::Pat = y::Pat::Wild;

/// Split or-patterns into rows and turn bindings into wildcards.
fn normalize<'p>(rows: Vec<Row<'p>>, cols: &[Occ]) -> Vec<Row<'p>> {
    fn expand<'p>(mut row: Row<'p>, cols: &[Occ], out: &mut Vec<Row<'p>>) {
        for (i, c) in cols.iter().enumerate() {
            match row.pats[i] {
                y::Pat::Or(ps) => {
                    for p in ps {
                        let mut r = row.clone();
                        r.pats[i] = p;
                        expand(r, cols, out);
                    }
                    return;
                }
                y::Pat::Bind(n) if c.shape.variant(n).is_none() => {
                    row.binds.push((n, c.clone()));
                    row.pats[i] = &WILD;
                }
                _ => {}
            }
        }
        out.push(row);
    }
    let mut out = Vec::new();
    for r in rows {
        expand(r, cols, &mut out);
    }
    out
}

/// The variant a pattern names; a bare name is a fieldless variant when the sum has one.
fn ctor_name<'p>(p: &'p y::Pat, s: &Shape) -> Option<&'p str> {
    match p {
        y::Pat::Ctor { path, .. } => path.last().map(String::as_str),
        y::Pat::Bind(n) if s.variant(n).is_some() => Some(n),
        _ => None,
    }
}

/// The patterns for the fields `names` of a record, tuple or variant pattern.
fn sub_pats<'p>(p: &'p y::Pat, names: &[&str]) -> R<Vec<&'p y::Pat>> {
    match p {
        y::Pat::Wild | y::Pat::Bind(_) | y::Pat::Ctor { fields: None, .. } => {
            Ok(vec![&WILD; names.len()])
        }
        y::Pat::Tuple(ps) if ps.len() == names.len() => Ok(ps.iter().collect()),
        y::Pat::Ctor {
            fields: Some((fs, _)),
            ..
        } => Ok(names
            .iter()
            .map(|n| fs.iter().find(|(f, _)| f == n).map_or(&WILD, |(_, p)| p))
            .collect()),
        _ => unsupported("this pattern"),
    }
}

/// Whether `p` succeeds exactly when the test `h` does (`Some(true)`), never does
/// (`Some(false)`), or neither is known (`None`, also for a wildcard).
fn same_test(h: &y::Pat, p: &y::Pat) -> Option<bool> {
    use y::Literal as L;
    match (h, p) {
        (y::Pat::Lit(a), y::Pat::Lit(b)) => match (a, b) {
            (L::Int(x), L::Int(y)) => Some(parse_int(x).ok()? == parse_int(y).ok()?),
            (L::Str(x), L::Str(y)) => Some(x == y),
            (L::Bool(x), L::Bool(y)) => Some(x == y),
            _ => None,
        },
        (y::Pat::Range(..), y::Pat::Range(..)) if h == p => Some(true),
        _ => None,
    }
}

#[derive(Clone)]
struct Var {
    locals: Vec<Local>,
    shape: Shape,
}

/// Where a place's slots are.
enum At {
    Reg(Vec<Local>),
    /// Slots from `word` on in the block at `ptr`.
    Mem(Operand, u32),
}

enum Scope {
    Loop {
        label: String,
        head: BlockId,
        exit: BlockId,
    },
    /// A block that declares join points. A jump from its statement `at` sets `cont` to `at`,
    /// and the end of a join point's body resumes after that statement.
    Jps {
        jps: Vec<(String, BlockId, Vec<Local>)>,
        cont: Local,
        at: Option<i128>,
    },
}

struct Fn<'a, 'm> {
    mono: &'a RefCell<Mono<'m>>,
    types: &'a Types<'m>,
    strings: &'a mut Vec<String>,
    me: usize,
    locals: Vec<Ty>,
    names: HashMap<String, Var>,
    /// The sides of each comparison result bound by `let`, for `assert` panics (R796).
    cmps: HashMap<String, [y::Atom; 2]>,
    /// The hex identity hash of the function's item, for `contract` panics.
    item: Option<String>,
    /// The `inout` parameters' slots, returned with every result.
    inout: Vec<Local>,
    blocks: Vec<Block>,
    done: Vec<bool>,
    cur: BlockId,
    scopes: Vec<Scope>,
    /// The line of the statement being lowered.
    line: u32,
    /// How many `par concurrent` branches enclose the code being lowered.
    tasks: u32,
    /// Whether the program counts steps and allocations for `cost` bounds (R610).
    costing: bool,
    /// The tokens of this function's `cost` counters, ended on every exit.
    cost_tokens: Vec<Operand>,
}

impl<'a, 'm> Fn<'a, 'm> {
    fn new(
        mono: &'a RefCell<Mono<'m>>,
        types: &'a Types<'m>,
        strings: &'a mut Vec<String>,
        me: usize,
    ) -> Self {
        Fn {
            mono,
            types,
            strings,
            me,
            locals: Vec::new(),
            names: HashMap::new(),
            cmps: HashMap::new(),
            item: None,
            inout: Vec::new(),
            blocks: Vec::new(),
            done: Vec::new(),
            cur: 0,
            scopes: Vec::new(),
            line: 0,
            tasks: 0,
            costing: mono.borrow().costing,
            cost_tokens: Vec::new(),
        }
    }

    fn sig(&self, f: FuncId) -> Sig {
        self.mono.borrow().sigs[f as usize].clone()
    }

    fn shape(&self, t: &y::Type) -> R<Shape> {
        self.types.shape(t)
    }

    /// The instance a call names, if it names a function of this module.
    fn callee(&self, c: &y::Call) -> R<Option<FuncId>> {
        let Some(&def) = self.mono.borrow().by_name.get(&key(&c.path, &c.targs)) else {
            return Ok(None);
        };
        self.mono.borrow_mut().instance(self.types, def).map(Some)
    }

    fn func(mut self, f: &'m y::FnDef, item: Option<String>) -> R<Func> {
        self.line = f.name_span.line;
        self.item = item;
        let names: Vec<&str> = params(f)?.into_iter().map(|(n, ..)| n).collect();
        let body = f.body.as_ref().expect("filtered on body");
        self.body(&f.path.join("."), &names, &f.contracts, body, Vec::new())
    }

    fn closure(self, c: &'m y::Closure, caps: Vec<(String, Shape)>) -> R<Func> {
        let mut names = vec![""];
        for p in &c.params {
            let y::Param::Value { name, .. } = p else {
                return unsupported("a closure with a capability parameter");
            };
            names.push(name);
        }
        self.body("closure", &names, &[], &c.body, caps)
    }

    /// With `caps`, the first parameter is the captures' block.
    fn body(
        mut self,
        name: &str,
        param_names: &[&str],
        contracts: &[y::Contract],
        body: &'m y::Block,
        caps: Vec<(String, Shape)>,
    ) -> R<Func> {
        let sig = self.sig(self.me as FuncId);
        let mut env = None;
        for (name, (conv, shape)) in param_names.iter().zip(&sig.params) {
            let v = self.var(shape.clone());
            if *conv == y::Conv::Inout {
                self.inout.extend(&v.locals);
            }
            if name.is_empty() {
                env = Some(Operand::Local(v.locals[0]));
            } else {
                self.names.insert((*name).to_owned(), v);
            }
        }
        let n_params = self.locals.len() as u32;
        let mut ret = sig.ret.tys();
        ret.extend(sig.inout());
        self.cur = self.block_new();
        if let Some(env) = env {
            let mut word = 0;
            for (n, shape) in caps {
                let v = self.read(&At::Mem(env.clone(), word), &shape);
                word += v.len() as u32;
                let var = self.var(shape);
                self.copy_to(&var.locals.clone(), v);
                self.names.insert(n, var);
            }
        }
        let mut clause = 0;
        for c in contracts {
            match c {
                y::Contract::Requires(e) => {
                    let t = self.expr(e)?;
                    let mut fields = vec![("clause".to_owned(), Operand::Int(clause))];
                    if let Some(h) = self.item.clone() {
                        fields.push(("item".to_owned(), self.lit(&h)));
                    }
                    for n in free_names(e) {
                        if let Some(v) =
                            self.scalar_field(&y::Atom::Name(n.clone(), Default::default()))
                        {
                            fields.push((n, v));
                        }
                    }
                    let fields = fields.into_iter().map(|(n, v)| (self.lit(&n), v)).collect();
                    self.check_fields(t, "contract", fields);
                    clause += 1;
                }
                y::Contract::Ensures(_) => clause += 1,
                y::Contract::Cost { steps, alloc } => {
                    // Both bounds are evaluated before either counter starts.
                    let mut begins = Vec::new();
                    for (r, e) in [(0, steps), (1, alloc)] {
                        if let Some(e) = e {
                            begins.push((r, self.expr_of(e, I64)?));
                        }
                    }
                    for (r, b) in begins {
                        let t = self.rt(
                            RtFn::CostBegin,
                            I64,
                            vec![Operand::Int(r), b, Operand::Int(clause)],
                        );
                        self.cost_tokens.push(t);
                    }
                    clause += 1;
                }
                _ => {}
            }
        }
        self.block(body)?;
        self.finish(Term::Panic("unreachable"));
        Ok(Func {
            name: name.to_owned(),
            params: n_params,
            ret,
            locals: self.locals,
            blocks: self.blocks,
        })
    }

    fn local(&mut self, t: Ty) -> Local {
        self.locals.push(t);
        self.locals.len() as Local - 1
    }

    fn var(&mut self, shape: Shape) -> Var {
        let locals = shape.tys().into_iter().map(|t| self.local(t)).collect();
        Var { locals, shape }
    }

    fn block_new(&mut self) -> BlockId {
        self.blocks.push(Block {
            insts: Vec::new(),
            lines: Vec::new(),
            term: Term::Panic("unreachable"),
        });
        self.done.push(false);
        self.blocks.len() as BlockId - 1
    }

    fn emit(&mut self, i: Inst) {
        // A call is a step and a fresh or grown block an `alloc` (R487, R925).
        let step = self.costing && matches!(i, Inst::Call { .. } | Inst::CallIndirect { .. });
        let alloc = self.costing && matches!(&i, Inst::Rt { f, .. } if f.allocates());
        if step {
            self.count(0);
        }
        self.push(i);
        if alloc {
            self.count(1);
        }
    }

    fn push(&mut self, i: Inst) {
        let b = &mut self.blocks[self.cur as usize];
        b.insts.push(i);
        b.lines.push(self.line);
    }

    /// One more of `resource` (0 steps, 1 alloc) against the running `cost` bounds.
    fn count(&mut self, resource: i128) {
        self.push(Inst::Rt {
            dst: None,
            f: RtFn::CostCount,
            args: vec![Operand::Int(resource), Operand::Int(1)],
        });
    }

    /// A loop back edge is a step (R487).
    fn back_edge(&mut self) {
        if self.costing {
            self.count(0);
        }
    }

    /// End this function's `cost` counters before it leaves.
    fn end_costs(&mut self) {
        if let Some(t) = self.cost_tokens.first().cloned() {
            self.push(Inst::Rt {
                dst: None,
                f: RtFn::CostEnd,
                args: vec![t],
            });
        }
    }

    /// End the current block with `t` unless it already ended.
    fn finish(&mut self, t: Term) {
        let c = self.cur as usize;
        if !self.done[c] {
            self.blocks[c].term = t;
            self.done[c] = true;
        }
    }

    /// End the current block with `t` and continue in a new, unreachable one.
    fn end(&mut self, t: Term) {
        self.finish(t);
        self.cur = self.block_new();
    }

    fn goto(&mut self, b: BlockId) {
        self.finish(Term::Jump(b));
        self.cur = b;
    }

    fn branch(&mut self, cond: Operand) -> (BlockId, BlockId) {
        let (t, e) = (self.block_new(), self.block_new());
        self.finish(Term::Branch {
            cond,
            then: t,
            els: e,
        });
        (t, e)
    }

    /// `cond` or a panic of `kind` carrying `fields`.
    fn check_fields(&mut self, cond: Operand, kind: &'static str, fields: Vec<(Operand, Operand)>) {
        let (ok, bad) = self.branch(cond);
        self.cur = bad;
        self.finish(Term::PanicFields { kind, fields });
        self.cur = ok;
    }

    /// An integer, float or bool atom's value; other types are left out of a panic (R796).
    fn scalar_field(&mut self, a: &y::Atom) -> Option<Operand> {
        match self.atom_any(a).ok()? {
            (v, Shape::Scalar(Ty::Int(_) | Ty::F64 | Ty::Bool)) if v.len() == 1 => {
                v.into_iter().next()
            }
            _ => None,
        }
    }

    fn copy_to(&mut self, dst: &[Local], src: Vec<Operand>) {
        for (d, s) in dst.iter().zip(src) {
            self.emit(Inst::Copy { dst: *d, src: s });
        }
    }

    fn rt(&mut self, f: RtFn, ty: Ty, args: Vec<Operand>) -> Operand {
        let d = self.local(ty);
        self.emit(Inst::Rt {
            dst: Some(d),
            f,
            args,
        });
        Operand::Local(d)
    }

    /// In a task, tell the scheduler that the exit that follows fails with `ev` of shape `e`,
    /// and whether that is `Cancelled` (R394).
    fn exit_kind(&mut self, e: &Shape, ev: &[Operand]) {
        if self.tasks == 0 {
            return;
        }
        let kind = match e {
            Shape::Rec(n, fs) if CANCEL_LIKE.contains(&n.as_str()) && fs.is_empty() => {
                Operand::Int(2)
            }
            Shape::Sum(_, _) if CANCEL_LIKE.iter().any(|n| e.variant(n).is_some()) => {
                let k = self.local(I64);
                self.emit(Inst::Copy {
                    dst: k,
                    src: Operand::Int(1),
                });
                for n in CANCEL_LIKE {
                    let Some((tag, _, _)) = e.variant(n) else {
                        continue;
                    };
                    let is =
                        self.scalar_op(ScalarOp::Eq, I64, vec![ev[0].clone(), Operand::Int(tag)]);
                    let (c, o) = self.branch(is);
                    let join = self.block_new();
                    self.cur = c;
                    self.emit(Inst::Copy {
                        dst: k,
                        src: Operand::Int(2),
                    });
                    self.goto(join);
                    self.cur = o;
                    self.goto(join);
                    self.cur = join;
                }
                Operand::Local(k)
            }
            _ => Operand::Int(1),
        };
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ParExitKind,
            args: vec![kind],
        });
    }

    /// Return `v` with the final `inout` slots.
    fn ret(&mut self, mut v: Vec<Operand>) {
        v.extend(self.inout.iter().map(|l| Operand::Local(*l)));
        self.end_costs();
        self.end(Term::Return(v));
    }

    /// The slots of sum `s` holding variant `name` with fields `v`.
    fn variant_value(s: &Shape, name: &str, v: Vec<Operand>) -> Vec<Operand> {
        let (tag, at, _) = s.variant(name).expect("a variant of the sum");
        let mut out: Vec<Operand> = s.tys().into_iter().map(zero).collect();
        out[0] = Operand::Int(tag);
        let w = v.len();
        out.splice(at..at + w, v);
        out
    }

    fn lit(&mut self, s: &str) -> Operand {
        self.strings.push(s.to_owned());
        Operand::Str(self.strings.len() as u32 - 1)
    }

    fn scalar_op(&mut self, op: ScalarOp, ty: Ty, args: Vec<Operand>) -> Operand {
        let (name, out) = match (op, ty) {
            (ScalarOp::Eq, _) => ("eq_i64", Ty::Bool),
            (ScalarOp::Ne, _) => ("ne_i64", Ty::Bool),
            (ScalarOp::Ge, _) => ("ge_i64", Ty::Bool),
            (ScalarOp::Le, _) => ("le_i64", Ty::Bool),
            (ScalarOp::AddChecked, _) => ("add_checked_i64", I64),
            (ScalarOp::SubChecked, _) => ("sub_checked_i64", I64),
            (ScalarOp::MulChecked, _) => ("mul_checked_i64", I64),
            (ScalarOp::Add, _) => ("add_f64", Ty::F64),
            _ => unreachable!("only used for these"),
        };
        let d = self.local(out);
        self.emit(Inst::Scalar {
            dst: Some(d),
            id: registry::id(name).expect("scalar ops are registered"),
            op,
            ty,
            args,
        });
        Operand::Local(d)
    }

    fn name(&self, n: &str) -> R<&Var> {
        self.names
            .get(n)
            .ok_or_else(|| format!("unbound name `{n}`"))
    }

    fn ops(v: &Var) -> Vec<Operand> {
        v.locals.iter().map(|l| Operand::Local(*l)).collect()
    }

    fn literal(&mut self, l: &y::Literal) -> R<(Operand, Shape)> {
        Ok(match l {
            y::Literal::Int(s) => (Operand::Int(parse_int(s)?), Shape::Scalar(I64)),
            y::Literal::Float(s) => (
                Operand::F64(
                    s.replace('_', "")
                        .parse()
                        .map_err(|_| format!("float `{s}`"))?,
                ),
                Shape::Scalar(Ty::F64),
            ),
            y::Literal::Bool(b) => (Operand::Bool(*b), Shape::Scalar(Ty::Bool)),
            y::Literal::Unit => (Operand::Unit, Shape::Scalar(Ty::Unit)),
            y::Literal::Str(s) => (self.lit(s), Shape::Str),
            y::Literal::Char(_) => return unsupported("a char"),
        })
    }

    /// An atom whose type is its own: a local or a literal.
    fn atom_any(&mut self, a: &y::Atom) -> R<(Vec<Operand>, Shape)> {
        match a {
            y::Atom::Name(n, _) => {
                let v = self.name(n)?;
                Ok((Self::ops(v), v.shape.clone()))
            }
            y::Atom::Lit(l, _) => {
                let (o, s) = self.literal(l)?;
                Ok((vec![o], s))
            }
        }
    }

    /// An atom of type `want`: also a fieldless variant of a sum.
    fn atom(&mut self, a: &y::Atom, want: &Shape) -> R<Vec<Operand>> {
        if let y::Atom::Name(n, _) = a
            && !self.names.contains_key(n)
            && let Some((tag, ..)) = want.variant(n)
        {
            let mut v: Vec<Operand> = want.tys().into_iter().map(zero).collect();
            v[0] = Operand::Int(tag);
            return Ok(v);
        }
        if let (y::Atom::Lit(y::Literal::Int(s), _), Shape::Scalar(Ty::F64)) = (a, want) {
            return Ok(vec![Operand::F64(parse_int(s)? as f64)]);
        }
        Ok(self.atom_any(a)?.0)
    }

    fn scalar1(&mut self, a: &y::Atom) -> R<Operand> {
        let (mut v, s) = self.atom_any(a)?;
        match (s, v.len()) {
            (
                Shape::Scalar(_)
                | Shape::Str
                | Shape::List(_)
                | Shape::Map(..)
                | Shape::Heap(_)
                | Shape::Deque(_)
                | Shape::Chan(_),
                1,
            ) => Ok(v.remove(0)),
            _ => unsupported("an aggregate operand"),
        }
    }

    /// A predicate's value.
    fn expr(&mut self, e: &y::Expr) -> R<Operand> {
        self.expr_of(e, Ty::Bool)
    }

    fn expr_of(&mut self, e: &y::Expr, ty: Ty) -> R<Operand> {
        match e {
            y::Expr::Atom(a) => self.scalar1(a),
            y::Expr::Call(c) => {
                let t = self.local(ty);
                self.call(
                    c,
                    Some(Var {
                        locals: vec![t],
                        shape: Shape::Scalar(ty),
                    }),
                )?;
                Ok(Operand::Local(t))
            }
        }
    }

    fn value_args(c: &y::Call) -> R<Vec<(y::Conv, &y::Atom)>> {
        c.args
            .iter()
            .filter_map(|a| match a {
                y::Arg::Value(conv, a, _) => Some(Ok((*conv, a))),
                y::Arg::Cap(..) => None,
            })
            .collect()
    }

    fn targ(&self, c: &y::Call) -> R<Option<Shape>> {
        match c.targs.as_slice() {
            [y::TArg::Type(t)] => Ok(Some(self.shape(t)?)),
            _ => Ok(None),
        }
    }

    fn user_args(&mut self, func: FuncId, c: &y::Call) -> R<(Vec<Operand>, Vec<Local>)> {
        let args = Self::value_args(c)?;
        let sig = &self.sig(func);
        if sig.params.len() != args.len() {
            return unsupported(format!(
                "call of `{}` with these arguments",
                c.path.join(".")
            ));
        }
        let params: Vec<(y::Conv, Shape)> = sig.params.clone();
        let mut ops = Vec::new();
        let mut back = Vec::new();
        for ((conv, a), (_, shape)) in args.into_iter().zip(&params) {
            if conv == y::Conv::Inout {
                let y::Atom::Name(n, _) = a else {
                    return unsupported("an `inout` literal");
                };
                back.extend(self.name(n)?.locals.clone());
            }
            ops.extend(self.atom(a, shape)?);
        }
        Ok((ops, back))
    }

    fn call(&mut self, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        if let Some(func) = self.callee(c)? {
            let (args, back) = self.user_args(func, c)?;
            let mut d = match dst {
                Some(v) => v.locals,
                None => self.var(self.sig(func).ret.clone()).locals,
            };
            d.extend(back);
            self.emit(Inst::Call { dst: d, func, args });
            return Ok(());
        }
        if let (
            [n],
            Some(Var {
                shape: Shape::Fn(ps, ret),
                locals,
            }),
        ) = (
            c.path.as_slice(),
            self.names.get(&c.path.join(".")).cloned(),
        ) {
            let args = Self::value_args(c)?;
            if args.len() != ps.len() {
                return unsupported(format!("call of `{n}` with these arguments"));
            }
            let mut ops = vec![Operand::Local(locals[1])];
            let mut tys = vec![Ty::Ptr];
            for ((_, a), s) in args.into_iter().zip(&ps) {
                ops.extend(self.atom(a, s)?);
                tys.extend(s.tys());
            }
            let d = match dst {
                Some(v) => v.locals,
                None => self.var((*ret).clone()).locals,
            };
            self.emit(Inst::CallIndirect {
                dst: d,
                code: Operand::Local(locals[0]),
                args: ops,
                params: tys,
                ret: ret.tys(),
            });
            return Ok(());
        }
        let Some(id) = registry::id(&name) else {
            return unsupported(format!("call of `{name}`"));
        };
        let args = Self::value_args(c)?;
        let dst1 = dst.as_ref().and_then(|v| v.locals.first().copied());
        let ct = match id {
            id::STD_CT_CT_SELECT => Some(ScalarOp::CtSelect),
            id::STD_CT_CT_EQ => Some(ScalarOp::CtEq),
            id::STD_CT_CT_LT => Some(ScalarOp::CtLt),
            _ => None,
        };
        if let Some(op) = ct {
            let args = args
                .iter()
                .map(|(_, a)| self.scalar1(a))
                .collect::<R<Vec<_>>>()?;
            self.emit(Inst::Scalar {
                dst: dst1,
                id,
                op,
                ty: I64,
                args,
            });
            return Ok(());
        }
        if let Some((op, ty)) = scalar(&name) {
            let args = args
                .iter()
                .map(|(_, a)| self.scalar1(a))
                .collect::<R<Vec<_>>>()?;
            let ty = match ty {
                Some(t) => t,
                None => match args.first() {
                    Some(Operand::Local(l)) => self.locals[*l as usize],
                    _ => I64,
                },
            };
            self.emit(Inst::Scalar {
                dst: dst1,
                id,
                op,
                ty,
                args,
            });
            return Ok(());
        }
        let set = |me: &mut Self, v: Operand| {
            if let Some(d) = dst1 {
                me.emit(Inst::Copy { dst: d, src: v });
            }
        };
        if [
            id::STD_DICT_NEW,
            id::STD_DICT_INSERT,
            id::STD_DICT_INDEX,
            id::STD_DICT_GET,
            id::STD_DICT_CONTAINS_KEY,
            id::STD_DICT_LEN,
            id::STD_DICT_REMOVE,
            id::STD_DICT_SETDEFAULT,
            id::STD_DICT_ITEMS,
            id::STD_DICT_KEYS,
            id::STD_DICT_VALUES,
        ]
        .contains(&id)
        {
            return self.dict_call(id, c, dst);
        }
        if DEQUE_IDS.contains(&id) {
            return self.deque_call(id, c, dst);
        }
        if ITERTOOLS_IDS.contains(&id) {
            return self.itertools_call(id, c, dst);
        }
        if [
            id::STD_PRELUDE_HEAP,
            id::STD_COLLECTIONS_HEAP_PUSH,
            id::STD_COLLECTIONS_HEAP_POP,
            id::STD_COLLECTIONS_HEAP_PEEK,
            id::STD_COLLECTIONS_HEAP_PUSHPOP,
            id::STD_COLLECTIONS_HEAP_INTO_SORTED,
            id::STD_COLLECTIONS_HEAP_LEN,
        ]
        .contains(&id)
        {
            return self.heap_call(id, c, dst);
        }
        match (id, args.as_slice()) {
            (id::STD_IO_PRINT, [(_, a)]) => {
                let s = self.scalar1(a)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::Print,
                    args: vec![s],
                });
            }
            (id::STD_FS_READ, [(_, path)]) => {
                let Some(dst) = dst else {
                    return unsupported("`fs.read` without a result");
                };
                let Some((_, _, efs)) = dst.shape.variant("Err") else {
                    return unsupported("`fs.read` without its failure");
                };
                if !matches!(&efs[0].1, Shape::Rec(n, fs) if n == "FsError" && fs.len() == 2) {
                    return unsupported("`fs.read` with another failure type");
                }
                let path = self.scalar1(path)?;
                let p = self.rt(RtFn::FsRead, Ty::Ptr, vec![path.clone()]);
                let failed = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (e, o) = self.branch(failed);
                let join = self.block_new();
                self.cur = e;
                let empty = self.lit("");
                let at = self.rt(RtFn::Concat, Ty::Ptr, vec![path, empty]);
                let reason = self.rt(RtFn::FsReason, Ty::Ptr, vec![]);
                let err = Self::variant_value(&dst.shape, "Err", vec![at, reason]);
                self.copy_to(&dst.locals, err);
                self.goto(join);
                self.cur = o;
                let ok = Self::variant_value(&dst.shape, "Ok", vec![p]);
                self.copy_to(&dst.locals, ok);
                self.goto(join);
            }
            (id::STD_IO_READ_ALL, []) => {
                let r = self.rt(RtFn::ReadAll, Ty::Ptr, vec![]);
                set(self, r);
            }
            (id::STD_IO_READ_LINE, []) => {
                let Some(dst) = dst else {
                    return unsupported("`read_line` without a result");
                };
                let p = self.rt(RtFn::ReadLine, Ty::Ptr, vec![]);
                let eof = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (e, o) = self.branch(eof);
                let join = self.block_new();
                self.cur = e;
                let none = Self::variant_value(&dst.shape, "None", Vec::new());
                self.copy_to(&dst.locals, none);
                self.goto(join);
                self.cur = o;
                let some = Self::variant_value(&dst.shape, "Some", vec![p]);
                self.copy_to(&dst.locals, some);
                self.goto(join);
                self.cur = join;
            }
            (id::STD_TEXT_CONCAT, [(_, a), (_, b)]) => {
                let (a, b) = (self.scalar1(a)?, self.scalar1(b)?);
                let r = self.rt(RtFn::Concat, Ty::Ptr, vec![a, b]);
                set(self, r);
            }
            (id::STD_FMT_SHOW, [(_, a)]) => {
                let (v, s) = match self.targ(c)? {
                    Some(s) => (self.atom(a, &s)?, s),
                    None => self.atom_any(a)?,
                };
                let r = self.show(&v, &s, false)?;
                set(self, r);
            }
            (id::STD_JSON_DUMP, [(_, a), (_, schema)]) => {
                let Some(shape) = self.targ(c)? else {
                    return unsupported("`json.dump` without its type");
                };
                let v = self.atom(a, &shape)?;
                let schema = self.scalar1(schema)?;
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v);
                let r = self.rt(RtFn::JsonDump, Ty::Ptr, vec![buf.clone(), schema]);
                self.free(RtFn::EnvFree, buf);
                set(self, r);
            }
            (id::STD_CHAN_CHANNEL, [(_, cap)]) => {
                let (Some(e), Some(dst)) = (self.targ(c)?, dst) else {
                    return unsupported("`Channel` without its type or result");
                };
                let cap = self.scalar1(cap)?;
                let words = Operand::Int(e.width() as i128);
                let ch = self.rt(RtFn::ChanNew, Ty::Ptr, vec![cap, words]);
                self.copy_to(&dst.locals, vec![ch.clone(), ch]);
            }
            (id::STD_CHAN_SENDER_SEND, [(_, tx), (_, a)]) => {
                let (Some(e), Some(dst)) = (self.targ(c)?, dst) else {
                    return unsupported("`send` without its type or result");
                };
                let tx = self.scalar1(tx)?;
                let v = self.atom(a, &e)?;
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v);
                let st = self.blocking(RtFn::ChanSend, vec![tx, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                self.chan_result(&dst, st, |_| Vec::new())?;
            }
            (id::STD_CHAN_RECEIVER_RECV, [(_, rx)]) => {
                let (Some(e), Some(dst)) = (self.targ(c)?, dst) else {
                    return unsupported("`recv` without its type or result");
                };
                let rx = self.scalar1(rx)?;
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(e.width() as i128)]);
                let st = self.blocking(RtFn::ChanRecv, vec![rx, out.clone()]);
                self.chan_result(&dst, st, |me| me.read(&At::Mem(out.clone(), 0), &e))?;
                self.free(RtFn::EnvFree, out);
            }
            (id::STD_JSON_PARSE, [(_, a), (_, schema)]) => {
                let Some(dst) = dst else {
                    return unsupported("`json.parse` without a result");
                };
                let (src, schema) = (self.scalar1(a)?, self.scalar1(schema)?);
                let buf = self.rt(
                    RtFn::Alloc,
                    Ty::Ptr,
                    vec![Operand::Int(dst.locals.len() as i128)],
                );
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::JsonParse,
                    args: vec![buf.clone(), src, schema],
                });
                let v = self.read(&At::Mem(buf.clone(), 0), &dst.shape);
                self.copy_to(&dst.locals, v);
                self.free(RtFn::EnvFree, buf);
            }
            (id::STD_CSV_DUMP, [(_, rows), (_, delim), (_, schema)]) => {
                let (rows, delim, schema) = (
                    self.scalar1(rows)?,
                    self.scalar1(delim)?,
                    self.scalar1(schema)?,
                );
                let r = self.rt(RtFn::CsvDump, Ty::Ptr, vec![rows, delim, schema]);
                set(self, r);
            }
            (id::STD_CSV_PARSE, [(_, a), (_, delim), (_, schema)]) => {
                let Some(dst) = dst else {
                    return unsupported("`csv.parse` without a result");
                };
                let (src, delim, schema) = (
                    self.scalar1(a)?,
                    self.scalar1(delim)?,
                    self.scalar1(schema)?,
                );
                let buf = self.rt(
                    RtFn::Alloc,
                    Ty::Ptr,
                    vec![Operand::Int(dst.locals.len() as i128)],
                );
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::CsvParse,
                    args: vec![buf.clone(), src, delim, schema],
                });
                let v = self.read(&At::Mem(buf.clone(), 0), &dst.shape);
                self.copy_to(&dst.locals, v);
                self.free(RtFn::EnvFree, buf);
            }
            (id::STD_TEXT_CHAR_COUNT, [(_, a)]) => {
                let s = self.scalar1(a)?;
                let r = self.rt(RtFn::CharCount, I64, vec![s]);
                set(self, r);
            }
            (id::STD_TEXT_BYTE_LEN, [(_, a)]) => {
                let s = self.scalar1(a)?;
                let r = self.rt(RtFn::ByteLen, I64, vec![s]);
                set(self, r);
            }
            (
                id::STD_TEXT_SLICE_CLAMPED | id::STD_LIST_SLICE_CLAMPED,
                [(_, a), (_, b), (_, c), (_, d)],
            ) => {
                let (mut v, s) = self.atom_any(a)?;
                let mut args = vec![v.remove(0)];
                let opt = Shape::Sum(
                    "Option".into(),
                    vec![
                        ("Some".into(), vec![("value".into(), Shape::Scalar(I64))]),
                        ("None".into(), vec![]),
                    ],
                );
                for bound in [b, c, d] {
                    args.extend(self.atom(bound, &opt)?);
                }
                let r = match &s {
                    Shape::Str if id == id::STD_TEXT_SLICE_CLAMPED => {
                        self.rt(RtFn::StrSlice, Ty::Ptr, args)
                    }
                    Shape::List(e) if id == id::STD_LIST_SLICE_CLAMPED => {
                        let l = self.rt(RtFn::ListSlice, Ty::Ptr, args);
                        if Self::has_heap(e) {
                            let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                            self.each(n, |me, i| {
                                let at =
                                    At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i]), 0);
                                let ev = me.read(&at, e);
                                let c = me.clone_value(ev, e)?;
                                me.write(&at, c);
                                Ok(())
                            })?;
                        }
                        l
                    }
                    _ => return unsupported("a slice of this type"),
                };
                set(self, r);
            }
            (id::STD_TEXT_INT_OF, [(_, a)]) => {
                let s = self.scalar1(a)?;
                let r = self.rt(RtFn::IntOf, I64, vec![s]);
                set(self, r);
            }
            (id::STD_LIST_NEW, []) => {
                let Some(e) = self.targ(c)? else {
                    return unsupported("`std.list.new` without its element type");
                };
                let r = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                set(self, r);
            }
            (id::STD_LIST_APPEND, [(_, y::Atom::Name(l, _)), (_, x)]) => {
                let list = self.name(l)?.clone();
                let Shape::List(e) = &list.shape else {
                    return unsupported(format!("call of `{name}`"));
                };
                let v = self.atom(x, e)?;
                let at = self.rt(RtFn::ListPush, Ty::Ptr, Self::ops(&list));
                self.write(&At::Mem(at, 0), v);
            }
            (id::STD_PRELUDE_LIST | id::STD_ITER_OPS_COLLECT, [(conv, a)]) => {
                let (_, s) = self.atom_any(a)?;
                if !matches!(s, Shape::List(_)) {
                    return unsupported("`list` of a non-list");
                }
                let v = self.owned(conv, a, &s)?;
                set(self, v[0].clone());
            }
            // The cursor of an `iter` from `std.itertools` is the list block's `limit` slot.
            (id::STD_ITER_OPS_DONE, [(_, a)]) => {
                let l = self.scalar1(a)?;
                let pos = self.rt(RtFn::DqMaxlen, I64, vec![l.clone()]);
                let n = self.rt(RtFn::ListLen, I64, vec![l]);
                let done = self.scalar_op(ScalarOp::Ge, I64, vec![pos, n]);
                set(self, done);
            }
            (id::STD_ITER_OPS_CURRENT, [(_, a)]) => {
                let (v, s) = self.atom_any(a)?;
                let Shape::List(e) = s else {
                    return unsupported("`current` of this iterator");
                };
                let l = v[0].clone();
                let pos = self.rt(RtFn::DqMaxlen, I64, vec![l.clone()]);
                let x = self.element(&l, pos.clone(), &e)?;
                let next = self.scalar_op(ScalarOp::AddChecked, I64, vec![pos, Operand::Int(1)]);
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqLimit,
                    args: vec![l, next],
                });
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), x);
                }
            }
            (id::STD_LIST_LEN, [(_, a)]) => {
                let l = self.scalar1(a)?;
                let r = self.rt(RtFn::ListLen, I64, vec![l]);
                set(self, r);
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }

    fn concat(&mut self, parts: Vec<Operand>) -> Operand {
        let mut it = parts.into_iter();
        let first = it.next().expect("at least one part");
        it.fold(first, |acc, p| self.concat2(acc, p))
    }

    /// `a` and `b` are fresh blocks or literals, so the result replaces both.
    fn concat2(&mut self, a: Operand, b: Operand) -> Operand {
        let r = self.rt(RtFn::Concat, Ty::Ptr, vec![a.clone(), b.clone()]);
        self.free(RtFn::StrFree, a);
        self.free(RtFn::StrFree, b);
        r
    }

    /// CPython's `str` of a value, or its `repr` inside a container (SQ20).
    fn show(&mut self, v: &[Operand], s: &Shape, nested: bool) -> R<Operand> {
        Ok(match s {
            Shape::Scalar(Ty::Int(t)) => {
                let f = if t.signed {
                    RtFn::ShowI64
                } else {
                    RtFn::ShowU64
                };
                self.rt(f, Ty::Ptr, vec![v[0].clone()])
            }
            Shape::Scalar(Ty::F64) => self.rt(RtFn::ShowF64, Ty::Ptr, vec![v[0].clone()]),
            Shape::Scalar(Ty::Bool) => self.rt(RtFn::ShowBool, Ty::Ptr, vec![v[0].clone()]),
            Shape::Scalar(Ty::Unit) => self.lit("None"),
            Shape::Scalar(Ty::Ptr) => unreachable!("not a Yan Core type"),
            Shape::Fn(..) => self.lit("<function>"),
            Shape::Chan(_) => return unsupported("`show` of a channel end"),
            Shape::Str if nested => self.rt(RtFn::Repr, Ty::Ptr, vec![v[0].clone()]),
            Shape::Str => {
                let empty = self.lit("");
                self.rt(RtFn::Concat, Ty::Ptr, vec![v[0].clone(), empty])
            }
            Shape::Rec(name, _) if name == "JsonError" => {
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v.to_vec());
                let r = self.rt(RtFn::JsonErrorShow, Ty::Ptr, vec![buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                r
            }
            Shape::Rec(name, _) if name == "CsvError" => {
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v.to_vec());
                let r = self.rt(RtFn::CsvErrorShow, Ty::Ptr, vec![buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                r
            }
            Shape::Rec(name, fs) => self.show_fields(name, v, fs)?,
            Shape::Tuple(ss) => {
                let mut parts = vec![self.lit("(")];
                let mut at = 0;
                for (i, e) in ss.iter().enumerate() {
                    if i > 0 {
                        parts.push(self.lit(", "));
                    }
                    let w = e.width();
                    parts.push(self.show(&v[at..at + w], e, true)?);
                    at += w;
                }
                parts.push(self.lit(if ss.len() == 1 { ",)" } else { ")" }));
                self.concat(parts)
            }
            Shape::Sum(name, vs) => {
                let option = name == "Option";
                let out = self.local(Ty::Ptr);
                let join = self.block_new();
                for (n, fs) in vs {
                    let (tag, at, _) = s.variant(n).expect("its own variant");
                    let w: usize = fs.iter().map(|(_, s)| s.width()).sum();
                    let is =
                        self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(tag)]);
                    let (t, e) = self.branch(is);
                    self.cur = t;
                    // `Option` shows as its value or `None`.
                    let r = match (option, fs.first()) {
                        (true, Some((_, inner))) => self.show(&v[at..at + w], inner, nested)?,
                        (true, None) => self.lit("None"),
                        _ => self.show_fields(n, &v[at..at + w], fs)?,
                    };
                    self.emit(Inst::Copy { dst: out, src: r });
                    self.finish(Term::Jump(join));
                    self.cur = e;
                }
                self.finish(Term::Panic("unreachable"));
                self.cur = join;
                Operand::Local(out)
            }
            Shape::Map(k, e) => self.show_map(v[0].clone(), k, e)?,
            Shape::Heap(e) => {
                let mut schema = String::new();
                Self::key_schema(e, &mut schema)?;
                let schema = self.lit(&schema);
                let l = self.rt(RtFn::ListCopy, Ty::Ptr, v.to_vec());
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::HeapSort,
                    args: vec![l.clone(), schema],
                });
                let inner = self.show(std::slice::from_ref(&l), &Shape::List(e.clone()), nested)?;
                self.free(RtFn::ListFree, l);
                let open = self.lit("heap(");
                let head = self.concat2(open, inner);
                let close = self.lit(")");
                self.concat2(head, close)
            }
            Shape::Deque(e) => {
                let l = self.rt(RtFn::ListCopy, Ty::Ptr, v.to_vec());
                let inner = self.show(std::slice::from_ref(&l), &Shape::List(e.clone()), nested)?;
                self.free(RtFn::ListFree, l);
                let open = self.lit("deque(");
                let head = self.concat2(open, inner);
                let suffix = self.rt(RtFn::DqSuffix, Ty::Ptr, v.to_vec());
                let head = self.concat2(head, suffix);
                let close = self.lit(")");
                self.concat2(head, close)
            }
            Shape::List(e) => {
                let list = v[0].clone();
                let out = self.local(Ty::Ptr);
                let open = self.lit("[");
                self.emit(Inst::Copy {
                    dst: out,
                    src: open,
                });
                let n = self.rt(RtFn::ListLen, I64, vec![list.clone()]);
                let i = self.local(I64);
                self.emit(Inst::Copy {
                    dst: i,
                    src: Operand::Int(0),
                });
                let head = self.block_new();
                self.goto(head);
                let done = self.scalar_op(ScalarOp::Ge, I64, vec![Operand::Local(i), n]);
                let (exit, body) = self.branch(done);
                self.cur = body;
                let first =
                    self.scalar_op(ScalarOp::Eq, I64, vec![Operand::Local(i), Operand::Int(0)]);
                let (skip, sep) = self.branch(first);
                self.cur = sep;
                let comma = self.lit(", ");
                let r = self.concat2(Operand::Local(out), comma);
                self.emit(Inst::Copy { dst: out, src: r });
                self.goto(skip);
                let at = self.rt(RtFn::ListAt, Ty::Ptr, vec![list, Operand::Local(i)]);
                let ev = self.read(&At::Mem(at, 0), e);
                let es = self.show(&ev, e, true)?;
                let r = self.concat2(Operand::Local(out), es);
                self.emit(Inst::Copy { dst: out, src: r });
                let next = self.scalar_op(
                    ScalarOp::AddChecked,
                    I64,
                    vec![Operand::Local(i), Operand::Int(1)],
                );
                self.emit(Inst::Copy { dst: i, src: next });
                self.finish(Term::Jump(head));
                self.cur = exit;
                let close = self.lit("]");
                let r = self.concat2(Operand::Local(out), close);
                self.emit(Inst::Copy { dst: out, src: r });
                Operand::Local(out)
            }
        })
    }

    fn show_map(&mut self, m: Operand, k: &Shape, e: &Shape) -> R<Operand> {
        let out = self.local(Ty::Ptr);
        let open = self.lit("{");
        self.emit(Inst::Copy {
            dst: out,
            src: open,
        });
        let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
        self.each(n, |me, i| {
            let first = me.scalar_op(ScalarOp::Eq, I64, vec![i.clone(), Operand::Int(0)]);
            let (skip, sep) = me.branch(first);
            me.cur = sep;
            let comma = me.lit(", ");
            let r = me.concat2(Operand::Local(out), comma);
            me.emit(Inst::Copy { dst: out, src: r });
            me.goto(skip);
            let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
            let kv = me.read(&At::Mem(at.clone(), 0), k);
            let ks = me.show(&kv, k, true)?;
            let colon = me.lit(": ");
            let ev = me.read(&At::Mem(at, k.width() as u32), e);
            let es = me.show(&ev, e, true)?;
            let r = me.concat(vec![Operand::Local(out), ks, colon, es]);
            me.emit(Inst::Copy { dst: out, src: r });
            Ok(())
        })?;
        let close = self.lit("}");
        Ok(self.concat2(Operand::Local(out), close))
    }

    /// `i` from 0 below `n`.
    fn each(&mut self, n: Operand, mut body: impl FnMut(&mut Self, Operand) -> R<()>) -> R<()> {
        let i = self.local(I64);
        self.emit(Inst::Copy {
            dst: i,
            src: Operand::Int(0),
        });
        let head = self.block_new();
        self.goto(head);
        let done = self.scalar_op(ScalarOp::Ge, I64, vec![Operand::Local(i), n]);
        let (exit, b) = self.branch(done);
        self.cur = b;
        body(self, Operand::Local(i))?;
        let next = self.scalar_op(
            ScalarOp::AddChecked,
            I64,
            vec![Operand::Local(i), Operand::Int(1)],
        );
        self.emit(Inst::Copy { dst: i, src: next });
        self.finish(Term::Jump(head));
        self.cur = exit;
        Ok(())
    }

    /// A copy of `v` that owns its own blocks.
    fn clone_value(&mut self, v: Vec<Operand>, s: &Shape) -> R<Vec<Operand>> {
        if !Self::has_heap(s) {
            return Ok(v);
        }
        Ok(match s {
            Shape::Scalar(_) => v,
            Shape::Str => {
                let empty = self.lit("");
                vec![self.rt(RtFn::Concat, Ty::Ptr, vec![v[0].clone(), empty])]
            }
            Shape::List(e) | Shape::Heap(e) | Shape::Deque(e) => {
                let l = self.rt(RtFn::ListCopy, Ty::Ptr, v);
                if Self::has_heap(e) {
                    let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                    self.each(n, |me, i| {
                        let at = At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i]), 0);
                        let ev = me.read(&at, e);
                        let c = me.clone_value(ev, e)?;
                        me.write(&at, c);
                        Ok(())
                    })?;
                }
                vec![l]
            }
            Shape::Map(k, e) => {
                let m = self.rt(RtFn::MapCopy, Ty::Ptr, v);
                let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
                self.each(n, |me, i| {
                    let p = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
                    for (at, s) in [(0, k), (k.width() as u32, e)] {
                        if Self::has_heap(s) {
                            let at = At::Mem(p.clone(), at);
                            let x = me.read(&at, s);
                            let c = me.clone_value(x, s)?;
                            me.write(&at, c);
                        }
                    }
                    Ok(())
                })?;
                vec![m]
            }
            Shape::Rec(..) | Shape::Tuple(_) => {
                let mut out = Vec::with_capacity(v.len());
                for (_, at, fs) in s.fields().expect("an aggregate") {
                    out.extend(self.clone_value(v[at..at + fs.width()].to_vec(), fs)?);
                }
                out
            }
            Shape::Sum(_, vs) => {
                let out = self.var(s.clone());
                self.copy_to(&out.locals, v.clone());
                let Operand::Local(tag) = v[0] else {
                    return unsupported("a copy of a constant sum");
                };
                let join = self.block_new();
                let arms: Vec<BlockId> = vs.iter().map(|_| self.block_new()).collect();
                self.finish(Term::Switch {
                    on: tag,
                    targets: arms.clone(),
                });
                let mut at = 1;
                for (arm, (_, fs)) in arms.into_iter().zip(vs) {
                    self.cur = arm;
                    for (_, fs) in fs {
                        let w = fs.width();
                        let c = self.clone_value(v[at..at + w].to_vec(), fs)?;
                        self.copy_to(&out.locals[at..at + w], c);
                        at += w;
                    }
                    self.finish(Term::Jump(join));
                }
                self.cur = join;
                Self::ops(&out)
            }
            Shape::Fn(..) => return unsupported("a copy of a closure"),
            Shape::Chan(_) => return unsupported("a copy of a channel end"),
        })
    }

    /// How `yan_rt::map` encodes a key of shape `s`.
    fn key_schema(s: &Shape, out: &mut String) -> R<()> {
        match s {
            Shape::Scalar(Ty::Int(t)) => {
                out.push(if t.signed { 'i' } else { 'u' });
                out.push_str(&t.bits.to_string());
            }
            Shape::Scalar(Ty::F64) => out.push('f'),
            Shape::Scalar(Ty::Bool) => out.push('b'),
            Shape::Scalar(Ty::Unit) => out.push('n'),
            Shape::Str => out.push('s'),
            Shape::Tuple(ss) => {
                out.push('(');
                for s in ss {
                    Self::key_schema(s, out)?;
                }
                out.push(')');
            }
            Shape::Rec(name, fs) => {
                out.push('{');
                out.push_str(name);
                out.push('|');
                for (_, s) in fs {
                    Self::key_schema(s, out)?;
                }
                out.push('}');
            }
            Shape::Sum(_, vs) => {
                out.push('<');
                for (n, fs) in vs {
                    out.push_str(n);
                    out.push('|');
                    for (_, s) in fs {
                        Self::key_schema(s, out)?;
                    }
                    out.push(';');
                }
                out.push('>');
            }
            _ => return unsupported("a dict key of this type"),
        }
        Ok(())
    }

    /// `key`'s slots in a fresh block, freed with `EnvFree`.
    fn key_buf(&mut self, key: Vec<Operand>) -> Operand {
        let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(key.len() as i128)]);
        self.write(&At::Mem(buf.clone(), 0), key);
        buf
    }

    /// An argument a `take` parameter owns: a borrowed one is copied.
    fn owned(&mut self, conv: &y::Conv, a: &y::Atom, s: &Shape) -> R<Vec<Operand>> {
        let v = self.atom(a, s)?;
        if matches!(conv, y::Conv::Sink) || matches!(a, y::Atom::Lit(..)) {
            return Ok(v);
        }
        self.clone_value(v, s)
    }

    fn map_arg(&mut self, a: &y::Atom) -> R<(Operand, Shape, Shape)> {
        let (mut v, s) = self.atom_any(a)?;
        let Shape::Map(k, e) = s else {
            return unsupported("a `std.dict` call on a non-dict");
        };
        Ok((v.remove(0), *k, *e))
    }

    fn deque_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        if id == id::STD_PRELUDE_DEQUE {
            let [y::TArg::Type(t)] = c.targs.as_slice() else {
                return unsupported("`deque` without its element type");
            };
            let e = self.shape(t)?;
            let (src, limit) = match args.as_slice() {
                [] => (None, None),
                [a] => (Some(*a), None),
                [a, m] => (Some(*a), Some(m.1)),
                _ => return unsupported("`deque` with extra arguments"),
            };
            let l = match src {
                Some((conv, a)) => {
                    let (_, s) = self.atom_any(a)?;
                    if !matches!(s, Shape::List(_)) {
                        return unsupported("`deque` of a non-list");
                    }
                    self.owned(&conv, a, &s)?.remove(0)
                }
                None => self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                ),
            };
            let l = match limit {
                Some(m) => {
                    let m = self.scalar1(m)?;
                    let d = self.rt(
                        RtFn::ListNew,
                        Ty::Ptr,
                        vec![Operand::Int(e.width() as i128)],
                    );
                    self.emit(Inst::Rt {
                        dst: None,
                        f: RtFn::DqLimit,
                        args: vec![d.clone(), m],
                    });
                    let out = self.rt(RtFn::DqExtend, Ty::Ptr, vec![d.clone(), l]);
                    self.glue(&Shape::List(Box::new(e)), &[out]);
                    d
                }
                None => l,
            };
            set(self, vec![l]);
            return Ok(());
        }
        let Some((_, h)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (mut v, s) = self.atom_any(h)?;
        let Shape::Deque(e) = s else {
            return unsupported("a `deque` call on a non-deque");
        };
        let l = v.remove(0);
        let w = e.width() as i128;
        match (id, &args[1..]) {
            (id::STD_COLLECTIONS_DEQUE_LEN, []) => {
                let r = self.rt(RtFn::ListLen, I64, vec![l]);
                set(self, vec![r]);
            }
            (
                id::STD_COLLECTIONS_DEQUE_APPEND | id::STD_COLLECTIONS_DEQUE_APPENDLEFT,
                [(xc, x)],
            ) => {
                let front = i128::from(id == id::STD_COLLECTIONS_DEQUE_APPENDLEFT);
                let x = self.owned(xc, x, &e)?;
                let xb = self.key_buf(x);
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let evicted = self.rt(
                    RtFn::DqPush,
                    I64,
                    vec![l, Operand::Int(front), xb.clone(), out.clone()],
                );
                if Self::has_heap(&e) {
                    let (drop_it, join) = (self.block_new(), self.block_new());
                    let none = self.scalar_op(ScalarOp::Eq, I64, vec![evicted, Operand::Int(0)]);
                    let (skip, evict) = self.branch(none);
                    self.cur = skip;
                    self.goto(join);
                    self.cur = evict;
                    self.goto(drop_it);
                    self.cur = drop_it;
                    let old = self.read(&At::Mem(out.clone(), 0), &e);
                    self.glue(&e, &old);
                    self.goto(join);
                }
                self.free(RtFn::EnvFree, xb);
                self.free(RtFn::EnvFree, out);
            }
            (id::STD_COLLECTIONS_DEQUE_POP | id::STD_COLLECTIONS_DEQUE_POPLEFT, []) => {
                let front = i128::from(id == id::STD_COLLECTIONS_DEQUE_POPLEFT);
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let ok = self.rt(RtFn::DqPop, I64, vec![l, Operand::Int(front), out.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![ok, Operand::Int(0)]);
                let (bad, good) = self.branch(empty);
                self.cur = bad;
                self.finish(Term::Panic("bounds"));
                self.cur = good;
                let v = self.read(&At::Mem(out.clone(), 0), &e);
                self.free(RtFn::EnvFree, out);
                set(self, v);
            }
            (id::STD_COLLECTIONS_DEQUE_FIRST | id::STD_COLLECTIONS_DEQUE_LAST, []) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![n.clone(), Operand::Int(0)]);
                let (none, some) = self.branch(empty);
                let join = self.block_new();
                self.cur = none;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = some;
                let i = if id == id::STD_COLLECTIONS_DEQUE_FIRST {
                    Operand::Int(0)
                } else {
                    self.scalar_op(ScalarOp::AddChecked, I64, vec![n, Operand::Int(-1)])
                };
                let p = self.rt(RtFn::ListAt, Ty::Ptr, vec![l, i]);
                let top = self.read(&At::Mem(p, 0), &e);
                let top = self.clone_value(top, &e)?;
                set(self, Self::variant_value(&ds, "Some", top));
                self.goto(join);
            }
            (id::STD_COLLECTIONS_DEQUE_EXTEND, [(xc, x)]) => {
                let (_, xs) = self.atom_any(x)?;
                if !matches!(xs, Shape::List(_)) {
                    return unsupported("`deque.extend` of a non-list");
                }
                let src = self.owned(xc, x, &xs)?.remove(0);
                let out = self.rt(RtFn::DqExtend, Ty::Ptr, vec![l, src]);
                self.glue(&Shape::List(e), &[out]);
            }
            (id::STD_COLLECTIONS_DEQUE_ROTATE, [(_, n)]) => {
                let n = self.scalar1(n)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqRotate,
                    args: vec![l, n],
                });
            }
            (id::STD_COLLECTIONS_DEQUE_CLEAR, []) => {
                let out = self.rt(RtFn::DqTake, Ty::Ptr, vec![l]);
                self.glue(&Shape::List(e), &[out]);
            }
            (id::STD_COLLECTIONS_DEQUE_MAXLEN, []) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let m = self.rt(RtFn::DqMaxlen, I64, vec![l]);
                let none = self.scalar_op(ScalarOp::Le, I64, vec![m.clone(), Operand::Int(-1)]);
                let (n, sm) = self.branch(none);
                let join = self.block_new();
                self.cur = n;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = sm;
                set(self, Self::variant_value(&ds, "Some", vec![m]));
                self.goto(join);
            }
            (id::STD_COLLECTIONS_DEQUE_CONTAINS, [(_, x)]) => {
                let x = self.atom(x, &e)?;
                let xb = self.key_buf(x);
                let mut schema = String::new();
                Self::key_schema(&e, &mut schema)?;
                let schema = self.lit(&schema);
                let r = self.rt(RtFn::DqContains, I64, vec![l, schema, xb.clone()]);
                self.free(RtFn::EnvFree, xb);
                let r = self.scalar_op(ScalarOp::Ne, I64, vec![r, Operand::Int(0)]);
                set(self, vec![r]);
            }
            _ => return unsupported(format!("call of `{}`", c.path.join("."))),
        }
        Ok(())
    }

    /// `std.itertools`, eagerly: the result is the list its `iter` would yield.
    fn itertools_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let Some((_, xs_atom)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (xv, xs) = self.atom_any(xs_atom)?;
        let Shape::List(e) = xs else {
            return unsupported("`std.itertools` on a non-list");
        };
        let xs = xv[0].clone();
        let n = self.rt(RtFn::ListLen, I64, vec![xs.clone()]);
        let out = match id {
            id::STD_ITERTOOLS_PERMUTATIONS
            | id::STD_ITERTOOLS_COMBINATIONS
            | id::STD_ITERTOOLS_COMBINATIONS_WITH_REPLACEMENT => {
                let kind = match id {
                    id::STD_ITERTOOLS_PERMUTATIONS => select_kind::PERMUTATIONS,
                    id::STD_ITERTOOLS_COMBINATIONS => select_kind::COMBINATIONS,
                    _ => select_kind::WITH_REPLACEMENT,
                };
                let Some((_, r)) = args.get(1) else {
                    return unsupported("`std.itertools` without `r`");
                };
                let r = if id == id::STD_ITERTOOLS_PERMUTATIONS {
                    self.int_or(r, n.clone())?
                } else {
                    self.scalar1(r)?
                };
                self.selections(kind, &xs, &e, n, r)?
            }
            id::STD_ITERTOOLS_PRODUCT => {
                let Some((_, b)) = args.get(1) else {
                    return unsupported("`product` without a second list");
                };
                let (bv, bs) = self.atom_any(b)?;
                let Shape::List(f) = bs else {
                    return unsupported("`product` of a non-list");
                };
                let bl = bv[0].clone();
                let m = self.rt(RtFn::ListLen, I64, vec![bl.clone()]);
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int((e.width() + f.width()) as i128)],
                );
                self.each(n, |me, i| {
                    me.each(m.clone(), |me, j| {
                        let mut pair = me.element(&xs, i.clone(), &e)?;
                        pair.extend(me.element(&bl, j, &f)?);
                        let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                        me.write(&At::Mem(slot, 0), pair);
                        Ok(())
                    })
                })?;
                out
            }
            id::STD_ITERTOOLS_PAIRWISE => {
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int((2 * e.width()) as i128)],
                );
                let pairs = self.scalar_op(ScalarOp::SubChecked, I64, vec![n, Operand::Int(1)]);
                self.each(pairs, |me, i| {
                    let mut pair = me.element(&xs, i.clone(), &e)?;
                    let next = me.scalar_op(ScalarOp::AddChecked, I64, vec![i, Operand::Int(1)]);
                    pair.extend(me.element(&xs, next, &e)?);
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), pair);
                    Ok(())
                })?;
                out
            }
            _ => {
                let (Some((_, f)), Some((_, init))) = (args.get(1), args.get(2)) else {
                    return unsupported("`accumulate` without its defaults");
                };
                self.accumulate(&xs, n, &e, f, init)?
            }
        };
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::DqLimit,
            args: vec![out.clone(), Operand::Int(0)],
        });
        if let Some(d) = &dst {
            self.copy_to(&d.locals.clone(), vec![out]);
        }
        Ok(())
    }

    /// `accumulate(xs, f, initial)`: the running values, `f` or `+` combining them.
    fn accumulate(
        &mut self,
        xs: &Operand,
        n: Operand,
        e: &Shape,
        f: &y::Atom,
        init: &y::Atom,
    ) -> R<Operand> {
        let option = |t: Shape| {
            Shape::Sum(
                "Option".into(),
                vec![
                    ("Some".into(), vec![("value".into(), t)]),
                    ("None".into(), vec![]),
                ],
            )
        };
        let fn_shape = Shape::Fn(vec![e.clone(), e.clone()], Box::new(e.clone()));
        let f = self.atom(f, &option(fn_shape))?;
        let init = self.atom(init, &option(e.clone()))?;
        let add = match e {
            Shape::Scalar(t) if *t == I64 => Some(ScalarOp::AddChecked),
            Shape::Scalar(Ty::F64) => Some(ScalarOp::Add),
            Shape::Scalar(Ty::Int(_)) => return unsupported("`accumulate` of this integer type"),
            _ => None,
        };
        let acc = self.var(e.clone());
        let started = self.local(I64);
        let out = self.rt(
            RtFn::ListNew,
            Ty::Ptr,
            vec![Operand::Int(e.width() as i128)],
        );
        let push = |me: &mut Self, v: Vec<Operand>| -> R<()> {
            let v = me.clone_value(v, e)?;
            let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
            me.write(&At::Mem(slot, 0), v);
            Ok(())
        };
        let has_init = self.scalar_op(ScalarOp::Eq, I64, vec![init[0].clone(), Operand::Int(0)]);
        let (yes, no) = self.branch(has_init);
        let join = self.block_new();
        self.cur = yes;
        let v = self.clone_value(init[1..].to_vec(), e)?;
        self.copy_to(&acc.locals, v);
        self.emit(Inst::Copy {
            dst: started,
            src: Operand::Int(1),
        });
        push(self, Self::ops(&acc))?;
        self.goto(join);
        self.cur = no;
        self.emit(Inst::Copy {
            dst: started,
            src: Operand::Int(0),
        });
        self.goto(join);
        self.each(n, |me, i| {
            let x = me.element(xs, i, e)?;
            let first = me.scalar_op(
                ScalarOp::Eq,
                I64,
                vec![Operand::Local(started), Operand::Int(0)],
            );
            let (first, rest) = me.branch(first);
            let next = me.block_new();
            me.cur = first;
            me.copy_to(&acc.locals, x.clone());
            me.emit(Inst::Copy {
                dst: started,
                src: Operand::Int(1),
            });
            me.goto(next);
            me.cur = rest;
            let cur = Self::ops(&acc);
            let has_f = me.scalar_op(ScalarOp::Eq, I64, vec![f[0].clone(), Operand::Int(0)]);
            let (call, plain) = me.branch(has_f);
            let joined = me.block_new();
            let r = me.var(e.clone());
            me.cur = call;
            let mut ops = vec![f[2].clone()];
            ops.extend(cur.clone());
            ops.extend(x.clone());
            let mut params = vec![Ty::Ptr];
            params.extend(e.tys());
            params.extend(e.tys());
            me.emit(Inst::CallIndirect {
                dst: r.locals.clone(),
                code: f[1].clone(),
                args: ops,
                params,
                ret: e.tys(),
            });
            me.goto(joined);
            me.cur = plain;
            match add {
                Some(op) => {
                    let ty = if op == ScalarOp::Add { Ty::F64 } else { I64 };
                    let sum = me.scalar_op(op, ty, vec![cur[0].clone(), x[0].clone()]);
                    me.copy_to(&r.locals, vec![sum]);
                    me.goto(joined);
                }
                None => {
                    me.finish(Term::Panic("value"));
                    me.cur = joined;
                }
            }
            me.glue(e, &cur);
            me.glue(e, &x);
            me.copy_to(&acc.locals, Self::ops(&r));
            me.goto(next);
            push(me, Self::ops(&acc))
        })?;
        if Self::has_heap(e) {
            let any = self.scalar_op(
                ScalarOp::Eq,
                I64,
                vec![Operand::Local(started), Operand::Int(1)],
            );
            let (yes, no) = self.branch(any);
            let done = self.block_new();
            self.cur = yes;
            self.glue(e, &Self::ops(&acc));
            self.goto(done);
            self.cur = no;
            self.goto(done);
        }
        Ok(out)
    }

    /// Element `i` of the list block `l`, cloned.
    fn element(&mut self, l: &Operand, i: Operand, e: &Shape) -> R<Vec<Operand>> {
        let at = At::Mem(self.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i]), 0);
        let v = self.read(&at, e);
        self.clone_value(v, e)
    }

    /// The payload of an `int | None` argument, or `none` for the `None`.
    fn int_or(&mut self, a: &y::Atom, none: Operand) -> R<Operand> {
        let want = Shape::Sum(
            "Union".into(),
            vec![
                ("int".into(), vec![("value".into(), Shape::Scalar(I64))]),
                ("None".into(), vec![]),
            ],
        );
        let v = self.atom(a, &want)?;
        let r = self.local(I64);
        let some = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
        let (yes, no) = self.branch(some);
        let join = self.block_new();
        self.cur = yes;
        self.emit(Inst::Copy {
            dst: r,
            src: v[1].clone(),
        });
        self.goto(join);
        self.cur = no;
        self.emit(Inst::Copy { dst: r, src: none });
        self.goto(join);
        Ok(Operand::Local(r))
    }

    /// The list of `r`-element lists of `xs` that `kind` selects.
    fn selections(
        &mut self,
        kind: i64,
        xs: &Operand,
        e: &Shape,
        n: Operand,
        r: Operand,
    ) -> R<Operand> {
        let sel = self.rt(
            RtFn::Select,
            Ty::Ptr,
            vec![Operand::Int(kind as i128), n, r.clone()],
        );
        let count = {
            let at = At::Mem(
                self.rt(RtFn::ListAt, Ty::Ptr, vec![sel.clone(), Operand::Int(0)]),
                0,
            );
            self.read(&at, &Shape::Scalar(I64)).remove(0)
        };
        let out = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(1)]);
        self.each(count, |me, k| {
            let row = me.scalar_op(ScalarOp::MulChecked, I64, vec![k, r.clone()]);
            let row = me.scalar_op(ScalarOp::AddChecked, I64, vec![row, Operand::Int(1)]);
            let inner = me.rt(
                RtFn::ListNew,
                Ty::Ptr,
                vec![Operand::Int(e.width() as i128)],
            );
            me.each(r.clone(), |me, j| {
                let at = me.scalar_op(ScalarOp::AddChecked, I64, vec![row.clone(), j]);
                let p = me.rt(RtFn::ListAt, Ty::Ptr, vec![sel.clone(), at]);
                let idx = me.read(&At::Mem(p, 0), &Shape::Scalar(I64)).remove(0);
                let x = me.element(xs, idx, e)?;
                let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![inner.clone()]);
                me.write(&At::Mem(slot, 0), x);
                Ok(())
            })?;
            let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
            me.write(&At::Mem(slot, 0), vec![inner]);
            Ok(())
        })?;
        self.free(RtFn::ListFree, sel);
        Ok(out)
    }

    fn heap_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let hrt = |me: &mut Self, f: RtFn, args: Vec<Operand>| {
            me.emit(Inst::Rt { dst: None, f, args });
        };
        if id == id::STD_PRELUDE_HEAP {
            let [y::TArg::Type(t)] = c.targs.as_slice() else {
                return unsupported("`heap` without its element type");
            };
            let e = self.shape(t)?;
            let mut schema = String::new();
            Self::key_schema(&e, &mut schema)?;
            let l = match args.as_slice() {
                [] => self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                ),
                [(conv, a)] => {
                    let (_, s) = self.atom_any(a)?;
                    if !matches!(s, Shape::List(_)) {
                        return unsupported("`heap` of a non-list");
                    }
                    let l = self.owned(conv, a, &s)?.remove(0);
                    let schema = self.lit(&schema);
                    hrt(self, RtFn::HeapFix, vec![l.clone(), schema]);
                    l
                }
                _ => return unsupported("`heap` with extra arguments"),
            };
            set(self, vec![l]);
            return Ok(());
        }
        let Some((_, h)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (mut v, s) = self.atom_any(h)?;
        let Shape::Heap(e) = s else {
            return unsupported("a `heap` call on a non-heap");
        };
        let l = v.remove(0);
        let mut schema = String::new();
        Self::key_schema(&e, &mut schema)?;
        let w = e.width() as i128;
        match (id, &args[1..]) {
            (id::STD_COLLECTIONS_HEAP_LEN, []) => {
                let r = self.rt(RtFn::ListLen, I64, vec![l]);
                set(self, vec![r]);
            }
            (id::STD_COLLECTIONS_HEAP_PUSH, [(xc, x)]) => {
                let x = self.owned(xc, x, &e)?;
                let at = self.rt(RtFn::ListPush, Ty::Ptr, vec![l.clone()]);
                self.write(&At::Mem(at, 0), x);
                let schema = self.lit(&schema);
                hrt(self, RtFn::HeapUp, vec![l, schema]);
            }
            (id::STD_COLLECTIONS_HEAP_POP, []) => {
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let schema = self.lit(&schema);
                let ok = self.rt(RtFn::HeapPop, I64, vec![l, schema, out.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![ok, Operand::Int(0)]);
                let (bad, good) = self.branch(empty);
                self.cur = bad;
                self.finish(Term::Panic("bounds"));
                self.cur = good;
                let v = self.read(&At::Mem(out.clone(), 0), &e);
                self.free(RtFn::EnvFree, out);
                set(self, v);
            }
            (id::STD_COLLECTIONS_HEAP_PEEK, []) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                let empty = self.scalar_op(ScalarOp::Eq, I64, vec![n, Operand::Int(0)]);
                let (none, some) = self.branch(empty);
                let join = self.block_new();
                self.cur = none;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = some;
                let p = self.rt(RtFn::ListAt, Ty::Ptr, vec![l, Operand::Int(0)]);
                let top = self.read(&At::Mem(p, 0), &e);
                let top = self.clone_value(top, &e)?;
                set(self, Self::variant_value(&ds, "Some", top));
                self.goto(join);
            }
            (id::STD_COLLECTIONS_HEAP_PUSHPOP, [(xc, x)]) => {
                let x = self.owned(xc, x, &e)?;
                let xb = self.key_buf(x);
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w)]);
                let schema = self.lit(&schema);
                hrt(
                    self,
                    RtFn::HeapPushPop,
                    vec![l, schema, xb.clone(), out.clone()],
                );
                let v = self.read(&At::Mem(out.clone(), 0), &e);
                self.free(RtFn::EnvFree, xb);
                self.free(RtFn::EnvFree, out);
                set(self, v);
            }
            (id::STD_COLLECTIONS_HEAP_INTO_SORTED, []) => {
                let (conv, h) = args[0];
                let l = self.owned(&conv, h, &Shape::Heap(e.clone()))?.remove(0);
                let schema = self.lit(&schema);
                hrt(self, RtFn::HeapSort, vec![l.clone(), schema]);
                set(self, vec![l]);
            }
            _ => return unsupported(format!("call of `{}`", c.path.join("."))),
        }
        Ok(())
    }

    fn dict_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        if id == id::STD_DICT_NEW {
            let [y::TArg::Type(k), y::TArg::Type(e)] = c.targs.as_slice() else {
                return unsupported("`std.dict.new` without its key and value types");
            };
            let (k, e) = (self.shape(k)?, self.shape(e)?);
            let mut schema = String::new();
            Self::key_schema(&k, &mut schema)?;
            let schema = self.lit(&schema);
            let r = self.rt(
                RtFn::MapNew,
                Ty::Ptr,
                vec![
                    Operand::Int(k.width() as i128),
                    Operand::Int(e.width() as i128),
                    schema,
                ],
            );
            set(self, vec![r]);
            return Ok(());
        }
        let Some((_, m)) = args.first() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (m, k, e) = self.map_arg(m)?;
        let kw = k.width() as u32;
        match (id, &args[1..]) {
            (id::STD_DICT_LEN, []) => {
                let r = self.rt(RtFn::MapLen, I64, vec![m]);
                set(self, vec![r]);
            }
            (id::STD_DICT_CONTAINS_KEY, [(_, key)]) => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let r = self.scalar_op(ScalarOp::Ne, I64, vec![p, Operand::Int(0)]);
                set(self, vec![r]);
            }
            (id::STD_DICT_INDEX, [(_, key)]) => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let p = self.rt(RtFn::MapIndex, Ty::Ptr, vec![m, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let v = self.read(&At::Mem(p, 0), &e);
                let v = self.clone_value(v, &e)?;
                set(self, v);
            }
            (id::STD_DICT_GET, [(_, key)]) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (none, some) = self.branch(missing);
                let join = self.block_new();
                self.cur = none;
                set(self, Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = some;
                let v = self.read(&At::Mem(p, 0), &e);
                let v = self.clone_value(v, &e)?;
                set(self, Self::variant_value(&ds, "Some", v));
                self.goto(join);
            }
            (id::STD_DICT_INSERT | id::STD_DICT_SETDEFAULT, [(kc, key), (vc, val)]) => {
                let key = self.owned(kc, key, &k)?;
                let val = self.owned(vc, val, &e)?;
                let buf = self.key_buf(key.clone());
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m.clone(), buf.clone()]);
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (new, old) = self.branch(missing);
                let join = self.block_new();
                self.cur = new;
                let q = self.rt(RtFn::MapPush, Ty::Ptr, vec![m, buf.clone()]);
                self.write(&At::Mem(q.clone(), 0), val.clone());
                if id == id::STD_DICT_SETDEFAULT {
                    let v = self.read(&At::Mem(q, 0), &e);
                    let v = self.clone_value(v, &e)?;
                    set(self, v);
                }
                self.goto(join);
                self.cur = old;
                self.glue(&k, &key);
                if id == id::STD_DICT_SETDEFAULT {
                    self.glue(&e, &val);
                    let v = self.read(&At::Mem(p, 0), &e);
                    let v = self.clone_value(v, &e)?;
                    set(self, v);
                } else {
                    let prev = self.read(&At::Mem(p.clone(), 0), &e);
                    self.glue(&e, &prev);
                    self.write(&At::Mem(p, 0), val);
                }
                self.goto(join);
                self.free(RtFn::EnvFree, buf);
            }
            (id::STD_DICT_REMOVE, [(_, key)]) => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let out = self.rt(
                    RtFn::Alloc,
                    Ty::Ptr,
                    vec![Operand::Int(i128::from(kw) + e.width() as i128)],
                );
                let r = self.rt(RtFn::MapRemove, I64, vec![m, buf.clone(), out.clone()]);
                self.free(RtFn::EnvFree, buf);
                let ds = dst.as_ref().map(|d| d.shape.clone());
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![r, Operand::Int(0)]);
                let (none, some) = self.branch(missing);
                let join = self.block_new();
                self.cur = none;
                if let Some(ds) = &ds {
                    set(self, Self::variant_value(ds, "None", vec![]));
                }
                self.goto(join);
                self.cur = some;
                let kv = self.read(&At::Mem(out.clone(), 0), &k);
                self.glue(&k, &kv);
                let v = self.read(&At::Mem(out.clone(), kw), &e);
                match &ds {
                    Some(ds) => set(self, Self::variant_value(ds, "Some", v)),
                    None => self.glue(&e, &v),
                }
                self.goto(join);
                self.free(RtFn::EnvFree, out);
            }
            (id::STD_DICT_ITEMS | id::STD_DICT_KEYS | id::STD_DICT_VALUES, []) => {
                let (take_k, take_v) = match id {
                    id::STD_DICT_ITEMS => (true, true),
                    id::STD_DICT_KEYS => (true, false),
                    _ => (false, true),
                };
                let words = if take_k { k.width() } else { 0 } + if take_v { e.width() } else { 0 };
                let l = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(words as i128)]);
                let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
                self.each(n, |me, i| {
                    let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
                    let mut x = Vec::new();
                    if take_k {
                        let kv = me.read(&At::Mem(at.clone(), 0), &k);
                        x.extend(me.clone_value(kv, &k)?);
                    }
                    if take_v {
                        let ev = me.read(&At::Mem(at, kw), &e);
                        x.extend(me.clone_value(ev, &e)?);
                    }
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![l.clone()]);
                    me.write(&At::Mem(slot, 0), x);
                    Ok(())
                })?;
                set(self, vec![l]);
            }
            _ => return unsupported(format!("call of `{}`", c.path.join("."))),
        }
        Ok(())
    }

    fn show_fields(&mut self, name: &str, v: &[Operand], fs: &[(String, Shape)]) -> R<Operand> {
        let mut parts = vec![self.lit(&format!("{name}("))];
        let mut at = 0;
        for (i, (n, s)) in fs.iter().enumerate() {
            let sep = if i > 0 { ", " } else { "" };
            parts.push(self.lit(&format!("{sep}{n}=")));
            let w = s.width();
            parts.push(self.show(&v[at..at + w], s, true)?);
            at += w;
        }
        parts.push(self.lit(")"));
        Ok(self.concat(parts))
    }

    fn place(&mut self, p: &y::Place) -> R<(At, Shape)> {
        let root = self.name(&p.root)?.clone();
        let mut at = At::Reg(root.locals);
        let mut shape = root.shape;
        for proj in &p.projs {
            match proj {
                y::Proj::Field(f, _) => {
                    let Some((_, start, s)) = shape
                        .fields()
                        .and_then(|fs| fs.into_iter().find(|(n, ..)| n == f))
                    else {
                        return unsupported(format!("field `{f}`"));
                    };
                    let w = s.width();
                    let s = s.clone();
                    at = match at {
                        At::Reg(ls) => At::Reg(ls[start..start + w].to_vec()),
                        At::Mem(p, word) => At::Mem(p, word + start as u32),
                    };
                    shape = s;
                }
                y::Proj::Index(i) => {
                    let (Shape::List(e) | Shape::Deque(e)) = shape.clone() else {
                        return unsupported("indexing a non-list");
                    };
                    let list = self.read(&at, &shape).remove(0);
                    let i = self.scalar1(i)?;
                    at = At::Mem(self.rt(RtFn::ListAt, Ty::Ptr, vec![list, i]), 0);
                    shape = *e;
                }
            }
        }
        Ok((at, shape))
    }

    fn has_map(s: &Shape) -> bool {
        match s {
            Shape::Map(..) => true,
            Shape::Scalar(_) | Shape::Str | Shape::Fn(..) | Shape::Chan(_) => false,
            Shape::List(e) | Shape::Heap(e) | Shape::Deque(e) => Self::has_map(e),
            Shape::Rec(_, fs) => fs.iter().any(|(_, s)| Self::has_map(s)),
            Shape::Tuple(ss) => ss.iter().any(Self::has_map),
            Shape::Sum(_, vs) => vs
                .iter()
                .flat_map(|(_, fs)| fs)
                .any(|(_, s)| Self::has_map(s)),
        }
    }

    fn has_heap(s: &Shape) -> bool {
        match s {
            Shape::Scalar(_) => false,
            Shape::Str
            | Shape::List(_)
            | Shape::Map(..)
            | Shape::Heap(_)
            | Shape::Deque(_)
            | Shape::Fn(..)
            | Shape::Chan(_) => true,
            Shape::Rec(_, fs) => fs.iter().any(|(_, s)| Self::has_heap(s)),
            Shape::Tuple(ss) => ss.iter().any(Self::has_heap),
            Shape::Sum(_, vs) => vs
                .iter()
                .flat_map(|(_, fs)| fs)
                .any(|(_, s)| Self::has_heap(s)),
        }
    }

    /// Drop glue (5.4): free the blocks of the value `v` of shape `s`, fields in declaration
    /// order. A closure's captures block is freed, not the values it captured.
    fn glue(&mut self, s: &Shape, v: &[Operand]) {
        if !Self::has_heap(s) {
            return;
        }
        match s {
            Shape::Scalar(_) => {}
            Shape::Fn(..) => self.free(RtFn::EnvFree, v[1].clone()),
            Shape::Str => self.free(RtFn::StrFree, v[0].clone()),
            Shape::Chan(_) => self.free(RtFn::ChanDrop, v[0].clone()),
            Shape::List(e) | Shape::Heap(e) | Shape::Deque(e) => {
                if Self::has_heap(e) {
                    let list = v[0].clone();
                    let n = self.rt(RtFn::ListLen, I64, vec![list.clone()]);
                    let i = self.local(I64);
                    self.emit(Inst::Copy {
                        dst: i,
                        src: Operand::Int(0),
                    });
                    let head = self.block_new();
                    self.goto(head);
                    let done = self.scalar_op(ScalarOp::Ge, I64, vec![Operand::Local(i), n]);
                    let (exit, body) = self.branch(done);
                    self.cur = body;
                    let at = self.rt(RtFn::ListAt, Ty::Ptr, vec![list, Operand::Local(i)]);
                    let ev = self.read(&At::Mem(at, 0), e);
                    self.glue(e, &ev);
                    let next = self.scalar_op(
                        ScalarOp::AddChecked,
                        I64,
                        vec![Operand::Local(i), Operand::Int(1)],
                    );
                    self.emit(Inst::Copy { dst: i, src: next });
                    self.finish(Term::Jump(head));
                    self.cur = exit;
                }
                self.free(RtFn::ListFree, v[0].clone());
            }
            Shape::Map(k, e) => {
                let m = v[0].clone();
                let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
                self.each(n, |me, i| {
                    let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
                    let kv = me.read(&At::Mem(at.clone(), 0), k);
                    me.glue(k, &kv);
                    let ev = me.read(&At::Mem(at, k.width() as u32), e);
                    me.glue(e, &ev);
                    Ok(())
                })
                .expect("glue does not fail");
                self.free(RtFn::MapFree, m);
            }
            Shape::Rec(..) | Shape::Tuple(_) => {
                for (_, at, fs) in s.fields().expect("an aggregate") {
                    self.glue(fs, &v[at..at + fs.width()]);
                }
            }
            Shape::Sum(_, vs) => {
                let Operand::Local(tag) = v[0] else {
                    unreachable!("a sum's tag is a local")
                };
                let join = self.block_new();
                let arms: Vec<BlockId> = vs.iter().map(|_| self.block_new()).collect();
                self.finish(Term::Switch {
                    on: tag,
                    targets: arms.clone(),
                });
                let mut at = 1;
                for (arm, (_, fs)) in arms.into_iter().zip(vs) {
                    self.cur = arm;
                    for (_, fs) in fs {
                        let w = fs.width();
                        self.glue(fs, &v[at..at + w]);
                        at += w;
                    }
                    self.finish(Term::Jump(join));
                }
                self.cur = join;
            }
        }
    }

    /// A channel operation; one that parks suspends its task and runs again when the task
    /// resumes. It can park inside a task, or anywhere in a coroutine variant (`co`).
    /// Its status: 0, or 2 when the task is cancelled (9.4).
    fn blocking(&mut self, f: RtFn, mut args: Vec<Operand>) -> Operand {
        args.push(Operand::Int(i128::from(self.tasks > 0)));
        let op = self.block_new();
        self.goto(op);
        let st = self.rt(f, I64, args);
        let parked = self.scalar_op(ScalarOp::Eq, I64, vec![st.clone(), Operand::Int(1)]);
        let (suspend, on) = self.branch(parked);
        self.cur = suspend;
        self.finish(Term::Suspend { resume: op });
        self.cur = on;
        st
    }

    /// `dst` of a channel operation with status `st`: `Ok` of what `ok` reads, or
    /// `Err(Cancelled)`.
    fn chan_result(
        &mut self,
        dst: &Var,
        st: Operand,
        ok: impl FnOnce(&mut Self) -> Vec<Operand>,
    ) -> R<()> {
        let Some((_, _, efs)) = dst.shape.variant("Err") else {
            return unsupported("a channel operation without its failure");
        };
        let e = &efs[0].1;
        let err = match e {
            Shape::Rec(n, fs) if n == "Cancelled" && fs.is_empty() => Vec::new(),
            Shape::Sum(_, _) if e.variant("Cancelled").is_some() => {
                Self::variant_value(e, "Cancelled", Vec::new())
            }
            _ => return unsupported("a channel failure without `Cancelled`"),
        };
        let err = Self::variant_value(&dst.shape, "Err", err);
        let cancelled = self.scalar_op(ScalarOp::Eq, I64, vec![st, Operand::Int(2)]);
        let (c, o) = self.branch(cancelled);
        let join = self.block_new();
        self.cur = c;
        self.copy_to(&dst.locals, err);
        self.goto(join);
        self.cur = o;
        let v = ok(self);
        let v = Self::variant_value(&dst.shape, "Ok", v);
        self.copy_to(&dst.locals, v);
        self.goto(join);
        Ok(())
    }

    fn free(&mut self, f: RtFn, block: Operand) {
        self.emit(Inst::Rt {
            dst: None,
            f,
            args: vec![block],
        });
    }

    fn read(&mut self, at: &At, s: &Shape) -> Vec<Operand> {
        match at {
            At::Reg(ls) => ls.iter().map(|l| Operand::Local(*l)).collect(),
            At::Mem(p, word) => s
                .tys()
                .into_iter()
                .enumerate()
                .map(|(i, t)| {
                    let d = self.local(t);
                    self.emit(Inst::Load {
                        dst: d,
                        ptr: p.clone(),
                        word: word + i as u32,
                    });
                    Operand::Local(d)
                })
                .collect(),
        }
    }

    fn write(&mut self, at: &At, v: Vec<Operand>) {
        match at {
            At::Reg(ls) => self.copy_to(&ls.clone(), v),
            At::Mem(p, word) => {
                for (i, src) in v.into_iter().enumerate() {
                    self.emit(Inst::Store {
                        ptr: p.clone(),
                        word: word + i as u32,
                        src,
                    });
                }
            }
        }
    }

    /// The value of `r`, of type `want`.
    fn value(&mut self, r: &'m y::Rhs, want: &Shape) -> R<Vec<Operand>> {
        match r {
            y::Rhs::Atom(a) => self.atom(a, want),
            y::Rhs::Call(c) => {
                let v = self.var(want.clone());
                let ops = Self::ops(&v);
                self.call(c, Some(v))?;
                Ok(ops)
            }
            y::Rhs::Proj(p) | y::Rhs::Move(p) => {
                let (at, s) = self.place(p)?;
                Ok(self.read(&at, &s))
            }
            y::Rhs::Copy(p) => {
                let (at, s) = self.place(p)?;
                let v = self.read(&at, &s);
                match &s {
                    s if Self::has_map(s) => self.clone_value(v, s),
                    Shape::List(e) | Shape::Deque(e) if !e.has_list() => {
                        Ok(vec![self.rt(RtFn::ListCopy, Ty::Ptr, v)])
                    }
                    s if s.has_list() => unsupported("a copy of nested lists"),
                    _ => Ok(v),
                }
            }
            y::Rhs::Ctor(c) => self.ctor(c, want),
            y::Rhs::Try(c) => {
                let v = self.var(want.clone());
                let ops = Self::ops(&v);
                self.call(c, Some(v))?;
                Ok(ops)
            }
            y::Rhs::Propagate {
                call,
                via: None,
                drops,
            } => self.propagate(call, drops, want),
            y::Rhs::Propagate { .. } => unsupported("`?` through a conversion"),
            y::Rhs::Closure(c) => self.make_closure(c, want),
        }
    }

    /// `c?`: its `Ok` value, or return its error from this function.
    fn drop_name(&mut self, n: &str) {
        if let Some(v) = self.names.get(n).cloned() {
            let ops: Vec<Operand> = v.locals.iter().map(|l| Operand::Local(*l)).collect();
            self.glue(&v.shape, &ops);
        }
    }

    fn propagate(&mut self, c: &y::Call, drops: &[y::Name], want: &Shape) -> R<Vec<Operand>> {
        let mine = self.sig(self.me as FuncId).ret.clone();
        let theirs = match self.callee(c)? {
            Some(func) => self.sig(func).ret.clone(),
            // `fs.read` fails with the `FsError` the caller propagates; its lowering checks that.
            None if registry::id(&c.path.join(".")) == Some(id::STD_FS_READ) => {
                let Some((_, _, mine_e)) = mine.variant("Err") else {
                    return unsupported("`?` outside a failing function");
                };
                result(Shape::Str, mine_e[0].1.clone())
            }
            None => return unsupported(format!("`?` on `{}`", c.path.join("."))),
        };
        let (Some((_, ok_at, _)), Some((_, err_at, efs)), Some((_, _, mine_e))) = (
            theirs.variant("Ok"),
            theirs.variant("Err"),
            mine.variant("Err"),
        ) else {
            return unsupported("`?` outside a failing function");
        };
        if efs[0].1 != mine_e[0].1 {
            return unsupported("`?` across error types");
        }
        let e = efs[0].1.clone();
        let r = self.var(theirs);
        let v = Self::ops(&r);
        self.call(c, Some(r))?;
        let is_ok = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
        let (ok, err) = self.branch(is_ok);
        self.cur = err;
        let ev = v[err_at..err_at + e.width()].to_vec();
        for n in drops {
            self.drop_name(n);
        }
        self.exit_kind(&e, &ev);
        self.ret(Self::variant_value(&mine, "Err", ev));
        self.cur = ok;
        Ok(v[ok_at..ok_at + want.width()].to_vec())
    }

    /// Lift `c` to its own function; its value is that code and a block of the captures.
    fn make_closure(&mut self, c: &'m y::Closure, want: &Shape) -> R<Vec<Operand>> {
        let mut caps = Vec::new();
        let mut vals = Vec::new();
        for (conv, n, _) in &c.captures {
            if *conv == y::Conv::Inout {
                return unsupported("an `inout` capture");
            }
            let v = self.name(n)?.clone();
            vals.extend(Self::ops(&v));
            caps.push((n.clone(), v.shape));
        }
        let Shape::Fn(ps, ret) = want else {
            return unsupported("a closure of this type");
        };
        let mut params = vec![(y::Conv::Let, Shape::Scalar(Ty::Ptr))];
        params.extend(ps.iter().map(|s| (y::Conv::Let, s.clone())));
        let ok = match ret.variant("Err").filter(|_| c.fail.is_some()) {
            Some(_) => match &**ret {
                Shape::Sum(_, vs) => vs[0].1[0].1.clone(),
                _ => unreachable!("a Result"),
            },
            None => (**ret).clone(),
        };
        let id = {
            let mut m = self.mono.borrow_mut();
            let id = m.sigs.len() as FuncId;
            m.sigs.push(Sig {
                params,
                ok,
                ret: (**ret).clone(),
                fails: c.fail.is_some(),
            });
            m.todo.push(Todo::Closure(c, caps));
            id
        };
        if self.costing && !vals.is_empty() {
            self.count(1);
        }
        let block = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(vals.len() as i128)]);
        self.write(&At::Mem(block.clone(), 0), vals);
        Ok(vec![Operand::Func(id), block])
    }

    fn ctor(&mut self, c: &y::Ctor, want: &Shape) -> R<Vec<Operand>> {
        match (c, want) {
            (y::Ctor::Tuple(xs), Shape::Tuple(ss)) if xs.len() == ss.len() => {
                let mut v = Vec::new();
                for (x, s) in xs.iter().zip(ss) {
                    v.extend(self.atom(x, s)?);
                }
                Ok(v)
            }
            (y::Ctor::Named { path, fields, .. }, _) => {
                let name = path.last().expect("named");
                let (tag, start, fs): (Option<i128>, usize, Vec<(String, Shape)>) = match want {
                    Shape::Rec(_, fs) => (None, 0, fs.clone()),
                    Shape::Sum(..) => match want.variant(name) {
                        Some((t, at, fs)) => (Some(t), at, fs.to_vec()),
                        None => return unsupported(format!("constructor `{name}`")),
                    },
                    _ => return unsupported(format!("constructor `{name}`")),
                };
                let mut v: Vec<Operand> = want.tys().into_iter().map(zero).collect();
                if let Some(t) = tag {
                    v[0] = Operand::Int(t);
                }
                let mut at = start;
                for (n, s) in &fs {
                    let Some((_, x)) = fields.iter().find(|(f, _)| f == n) else {
                        return unsupported(format!("constructor `{name}` without field `{n}`"));
                    };
                    let xv = self.atom(x, s)?;
                    let w = xv.len();
                    v.splice(at..at + w, xv);
                    at += w;
                }
                Ok(v)
            }
            _ => unsupported("a constructor"),
        }
    }

    /// Test `v` against `p`, binding names, and continue in `fail` when it does not match.
    fn pat(&mut self, p: &y::Pat, v: &[Operand], s: &Shape, fail: BlockId) -> R<()> {
        let test = |me: &mut Self, ok: Operand| {
            let next = me.block_new();
            me.finish(Term::Branch {
                cond: ok,
                then: next,
                els: fail,
            });
            me.cur = next;
        };
        match (p, s) {
            (y::Pat::Wild, _) => {}
            (y::Pat::Bind(n), _) => {
                let var = self.var(s.clone());
                self.copy_to(&var.locals.clone(), v.to_vec());
                self.names.insert(n.clone(), var);
            }
            (y::Pat::Lit(l), _) => {
                let (o, _) = self.literal(l)?;
                let ok = match s {
                    Shape::Str => self.rt(RtFn::StrEq, Ty::Bool, vec![v[0].clone(), o]),
                    Shape::Scalar(t @ (Ty::Int(_) | Ty::Bool)) => {
                        let d = self.local(Ty::Bool);
                        self.emit(Inst::Scalar {
                            dst: Some(d),
                            id: registry::id("eq_i64").expect("registered"),
                            op: ScalarOp::Eq,
                            ty: *t,
                            args: vec![v[0].clone(), o],
                        });
                        Operand::Local(d)
                    }
                    _ => return unsupported("this literal pattern"),
                };
                test(self, ok);
            }
            (y::Pat::Range(lo, hi), Shape::Scalar(t @ Ty::Int(_))) => {
                let (lo, _) = self.literal(lo)?;
                let (hi, _) = self.literal(hi)?;
                for (op, bound) in [(ScalarOp::Ge, lo), (ScalarOp::Le, hi)] {
                    let ok = self.scalar_op(op, *t, vec![v[0].clone(), bound]);
                    test(self, ok);
                }
            }
            (y::Pat::Tuple(ps), Shape::Tuple(ss)) if ps.len() == ss.len() => {
                let mut at = 0;
                for (p, e) in ps.iter().zip(ss) {
                    let w = e.width();
                    self.pat(p, &v[at..at + w], e, fail)?;
                    at += w;
                }
            }
            (y::Pat::Ctor { path, fields }, Shape::Rec(..) | Shape::Sum(..)) => {
                let name = path.last().expect("named");
                let (start, fs) = match s {
                    Shape::Sum(..) => {
                        let Some((tag, at, fs)) = s.variant(name) else {
                            return unsupported(format!("pattern `{name}`"));
                        };
                        let ok = self.scalar_op(
                            ScalarOp::Eq,
                            I64,
                            vec![v[0].clone(), Operand::Int(tag)],
                        );
                        test(self, ok);
                        (at, fs.to_vec())
                    }
                    Shape::Rec(_, fs) => (0, fs.clone()),
                    _ => unreachable!("matched"),
                };
                for (n, sub) in fields.iter().flat_map(|(fs, _)| fs) {
                    let mut at = start;
                    let Some((_, fsh)) = fs.iter().find(|(f, sh)| {
                        if f == n {
                            return true;
                        }
                        at += sh.width();
                        false
                    }) else {
                        return unsupported(format!("field `{n}`"));
                    };
                    let w = fsh.width();
                    self.pat(sub, &v[at..at + w], fsh, fail)?;
                }
            }
            (y::Pat::Or(ps), _) => {
                let ok = self.block_new();
                for p in ps {
                    let next = self.block_new();
                    let before = self.names.len();
                    self.pat(p, v, s, next)?;
                    if self.names.len() != before {
                        return unsupported("an or-pattern that binds");
                    }
                    self.finish(Term::Jump(ok));
                    self.cur = next;
                }
                self.finish(Term::Jump(fail));
                self.cur = ok;
            }
            _ => return unsupported("this pattern"),
        }
        Ok(())
    }

    /// Lower the clause matrix `rows` over `cols` to a decision tree (Maranget, "Compiling
    /// pattern matching to good decision trees", ML 2008), which tests each part of the
    /// scrutinee at most once on any path. A leaf copies its row's bindings into the arm's
    /// variables, and every arm body is lowered once.
    fn tree(
        &mut self,
        v: &[Operand],
        mut cols: Vec<Occ>,
        rows: Vec<Row<'m>>,
        leaves: &mut [Vec<Leaf>],
    ) -> R<()> {
        let mut rows = normalize(rows, &cols);
        let Some(first) = rows.first() else {
            self.finish(Term::Panic("unreachable"));
            return Ok(());
        };
        let refutable = |p: &y::Pat| !matches!(p, y::Pat::Wild);
        // The needed column of the first row with the longest refutable prefix (heuristic p).
        let Some(col) = (0..cols.len())
            .filter(|&c| refutable(first.pats[c]))
            .max_by_key(|&c| {
                let n = rows.iter().take_while(|r| refutable(r.pats[c])).count();
                (n, std::cmp::Reverse(c))
            })
        else {
            let first = rows.swap_remove(0);
            return self.leaf(v, &first, leaves);
        };
        let occ = cols[col].clone();
        match &occ.shape {
            Shape::Tuple(_) | Shape::Rec(..) => {
                let fs = occ.shape.fields().expect("a record or tuple");
                let names: Vec<&str> = fs.iter().map(|(n, ..)| n.as_str()).collect();
                cols.remove(col);
                cols.extend(fs.iter().map(|(_, at, s)| Occ {
                    at: occ.at + at,
                    shape: (*s).clone(),
                }));
                for r in &mut rows {
                    let h = r.pats.remove(col);
                    r.pats.extend(sub_pats(h, &names)?);
                }
                self.tree(v, cols, rows, leaves)
            }
            Shape::Sum(_, vs) => {
                cols.remove(col);
                let heads: Vec<&y::Pat> = rows.iter_mut().map(|r| r.pats.remove(col)).collect();
                let mut named = Vec::new();
                for h in &heads {
                    match ctor_name(h, &occ.shape) {
                        Some(n) => named.push(n),
                        None if matches!(h, y::Pat::Wild) => {}
                        None => return unsupported("this pattern"),
                    }
                }
                let on = match &v[occ.at] {
                    Operand::Local(l) => *l,
                    o => {
                        let l = self.local(I64);
                        self.emit(Inst::Copy {
                            dst: l,
                            src: o.clone(),
                        });
                        l
                    }
                };
                let mut subs = Vec::new();
                let mut default = None;
                let mut targets = Vec::new();
                for (name, fs) in vs {
                    if !named.contains(&name.as_str()) {
                        let b = *default.get_or_insert_with(|| {
                            let b = self.block_new();
                            let rows = rows
                                .iter()
                                .zip(&heads)
                                .filter(|(_, h)| matches!(h, y::Pat::Wild))
                                .map(|(r, _)| r.clone())
                                .collect();
                            subs.push((b, cols.clone(), rows));
                            b
                        });
                        targets.push(b);
                        continue;
                    }
                    let (_, mut at, _) = occ.shape.variant(name).expect("a variant");
                    let mut c = cols.clone();
                    for (_, s) in fs {
                        c.push(Occ {
                            at: occ.at + at,
                            shape: s.clone(),
                        });
                        at += s.width();
                    }
                    let names: Vec<&str> = fs.iter().map(|(n, _)| n.as_str()).collect();
                    let mut sub = Vec::new();
                    for (r, h) in rows.iter().zip(&heads) {
                        if matches!(h, y::Pat::Wild) || ctor_name(h, &occ.shape) == Some(name) {
                            let mut r = r.clone();
                            r.pats.extend(sub_pats(h, &names)?);
                            sub.push(r);
                        }
                    }
                    let b = self.block_new();
                    subs.push((b, c, sub));
                    targets.push(b);
                }
                self.finish(Term::Switch { on, targets });
                for (b, c, sub) in subs {
                    self.cur = b;
                    self.tree(v, c, sub, leaves)?;
                }
                Ok(())
            }
            s => {
                let h = first.pats[col];
                let w = s.width();
                let no = self.block_new();
                self.pat(h, &v[occ.at..occ.at + w], s, no)?;
                let is_bool = matches!(s, Shape::Scalar(Ty::Bool));
                let (mut yes_rows, mut no_rows) = (Vec::new(), Vec::new());
                for r in &rows {
                    let same = same_test(h, r.pats[col]);
                    if same != Some(false) {
                        let mut r = r.clone();
                        if same == Some(true) {
                            r.pats[col] = &WILD;
                        }
                        yes_rows.push(r);
                    }
                    if same != Some(true) {
                        let mut r = r.clone();
                        if is_bool && same == Some(false) {
                            r.pats[col] = &WILD;
                        }
                        no_rows.push(r);
                    }
                }
                self.tree(v, cols.clone(), yes_rows, leaves)?;
                self.cur = no;
                self.tree(v, cols, no_rows, leaves)
            }
        }
    }

    /// Leave the current block open for `row`'s arm, with the slots each name binds.
    fn leaf(&mut self, v: &[Operand], row: &Row, leaves: &mut [Vec<Leaf>]) -> R<()> {
        let binds = row
            .binds
            .iter()
            .map(|(n, o)| {
                let src = v[o.at..o.at + o.shape.width()].to_vec();
                ((*n).to_owned(), src, o.shape.clone())
            })
            .collect();
        leaves[row.arm].push((self.cur, binds));
        Ok(())
    }

    /// Lower `body` after its leaves: in the leaf's own block when there is one, else in a
    /// block they all jump to.
    fn arm(&mut self, leaves: Vec<Leaf>, body: &'m y::Block, join: BlockId) -> R<()> {
        let outer = self.names.clone();
        let Some((first, names)) = leaves.first() else {
            return Ok(());
        };
        let vars: Vec<(String, Var)> = names
            .iter()
            .map(|(n, _, s)| (n.clone(), self.var(s.clone())))
            .collect();
        let entry = if leaves.len() == 1 {
            *first
        } else {
            self.block_new()
        };
        for (b, binds) in &leaves {
            self.cur = *b;
            for (n, src, _) in binds {
                let Some((_, var)) = vars.iter().find(|(m, _)| m == n) else {
                    return unsupported("an or-pattern whose alternatives bind different names");
                };
                let dst = var.locals.clone();
                self.copy_to(&dst, src.clone());
            }
            if *b != entry {
                self.finish(Term::Jump(entry));
            }
        }
        self.cur = entry;
        self.names.extend(vars);
        self.block(body)?;
        self.finish(Term::Jump(join));
        self.names = outer;
        Ok(())
    }

    /// Names bound in `b` go out of scope at its end.
    fn block(&mut self, b: &'m y::Block) -> R<()> {
        let outer = self.names.clone();
        let r = self.block_in(b);
        self.names = outer;
        r
    }

    fn block_in(&mut self, b: &'m y::Block) -> R<()> {
        let decls: Vec<_> = b
            .stmts
            .iter()
            .filter_map(|s| match &s.kind {
                y::StmtKind::Jp { name, params, body } => Some((name, params, body)),
                _ => None,
            })
            .collect();
        if decls.is_empty() {
            return b.stmts.iter().try_for_each(|s| self.stmt(s));
        }
        let cont = self.local(I64);
        let mut jps = Vec::new();
        let mut jp_vars = Vec::new();
        for (name, params, _) in &decls {
            let mut ls = Vec::new();
            let mut vs = Vec::new();
            for p in params.iter() {
                let y::Param::Value { name, ty, .. } = p else {
                    return unsupported("a capability join-point parameter");
                };
                let v = self.var(self.shape(ty)?);
                ls.extend(&v.locals);
                vs.push((name.clone(), v));
            }
            jps.push(((*name).clone(), self.block_new(), ls));
            jp_vars.push(vs);
        }
        self.scopes.push(Scope::Jps {
            jps: jps.clone(),
            cont,
            at: None,
        });
        let mut resume = Vec::new();
        // A join point's body sees the names declared before it, and its parameters.
        let mut seen = Vec::new();
        for s in &b.stmts {
            if matches!(s.kind, y::StmtKind::Jp { .. }) {
                seen.push(self.names.clone());
                continue;
            }
            if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
                *at = Some(resume.len() as i128);
            }
            self.stmt(s)?;
            let next = self.block_new();
            self.goto(next);
            resume.push(next);
        }
        let after = self.cur;
        if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
            *at = None;
        }
        let jps_in = decls.iter().zip(&jps).zip(jp_vars).zip(seen);
        for ((((_, _, body), (_, start, _)), vs), names) in jps_in {
            self.cur = *start;
            self.names = names;
            self.names.extend(vs);
            self.block(body)?;
            self.finish(Term::Switch {
                on: cont,
                targets: resume.clone(),
            });
        }
        self.scopes.pop();
        self.cur = after;
        Ok(())
    }

    fn jump(&mut self, name: &str, args: &[y::Atom]) -> R<()> {
        let found = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Jps { jps, cont, at } => jps
                .iter()
                .find(|(n, ..)| n == name)
                .map(|(_, b, ps)| (*b, ps.clone(), *cont, *at)),
            Scope::Loop { .. } => None,
        });
        let Some((target, params, cont, at)) = found else {
            return unsupported(format!("jump to `{name}`"));
        };
        let mut vals = Vec::new();
        for a in args {
            vals.extend(self.atom_any(a)?.0);
        }
        // Through temporaries, so `jump k(b, a)` reads before it writes.
        let temps: Vec<Local> = vals
            .into_iter()
            .zip(&params)
            .map(|(src, p)| {
                let t = self.local(self.locals[*p as usize]);
                self.emit(Inst::Copy { dst: t, src });
                t
            })
            .collect();
        for (p, t) in params.iter().zip(temps) {
            self.emit(Inst::Copy {
                dst: *p,
                src: Operand::Local(t),
            });
        }
        if let Some(at) = at {
            self.emit(Inst::Copy {
                dst: cont,
                src: Operand::Int(at),
            });
        }
        self.end(Term::Jump(target));
        Ok(())
    }

    fn leave(&mut self, label: &str, brk: bool) -> R<()> {
        let to = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Loop {
                label: l,
                head,
                exit,
            } if l == label => Some(if brk { *exit } else { *head }),
            _ => None,
        });
        match to {
            Some(b) => {
                if !brk {
                    self.back_edge();
                }
                self.end(Term::Jump(b));
                Ok(())
            }
            None => unsupported(format!("loop label `{label}`")),
        }
    }

    fn stmt(&mut self, s: &'m y::Stmt) -> R<()> {
        use y::StmtKind as K;
        self.line = s.span.line;
        match &s.kind {
            K::Let { pat, ty, rhs, .. } => {
                let shape = self.shape(ty)?;
                let v = self.value(rhs, &shape)?;
                match pat {
                    y::Pat::Bind(n) => {
                        match comparison(rhs) {
                            Some(sides) => self.cmps.insert(n.clone(), sides),
                            None => self.cmps.remove(n),
                        };
                        let var = self.var(shape);
                        self.copy_to(&var.locals.clone(), v);
                        self.names.insert(n.clone(), var);
                    }
                    y::Pat::Wild => {}
                    p => {
                        let fail = self.block_new();
                        self.pat(p, &v, &shape, fail)?;
                        let ok = self.cur;
                        self.cur = fail;
                        self.finish(Term::Panic("unreachable"));
                        self.cur = ok;
                    }
                }
                Ok(())
            }
            K::Assign { place, rhs, .. } => {
                let (at, shape) = self.place(place)?;
                let v = self.value(rhs, &shape)?;
                self.write(&at, v);
                Ok(())
            }
            K::If { cond, then, els } => {
                let cond = self.scalar1(cond)?;
                let (t, e) = self.branch(cond);
                let join = self.block_new();
                self.cur = t;
                self.block(then)?;
                self.goto(join);
                self.cur = e;
                if let Some(els) = els {
                    self.block(els)?;
                }
                self.goto(join);
                Ok(())
            }
            K::Match { scrutinee, arms } => {
                let (v, shape) = self.atom_any(scrutinee)?;
                let rows = arms
                    .iter()
                    .enumerate()
                    .map(|(arm, (p, _))| Row {
                        pats: vec![p],
                        arm,
                        binds: Vec::new(),
                    })
                    .collect();
                let mut leaves = vec![Vec::new(); arms.len()];
                self.tree(&v, vec![Occ { at: 0, shape }], rows, &mut leaves)?;
                let join = self.block_new();
                for ((_, body), l) in arms.iter().zip(leaves) {
                    self.arm(l, body, join)?;
                }
                self.cur = join;
                Ok(())
            }
            K::Loop { label, body, .. } => {
                let (head, exit) = (self.block_new(), self.block_new());
                self.goto(head);
                self.scopes.push(Scope::Loop {
                    label: label.clone(),
                    head,
                    exit,
                });
                self.block(body)?;
                self.scopes.pop();
                self.back_edge();
                self.goto(head);
                self.cur = exit;
                Ok(())
            }
            K::Break { value: Some(_), .. } => unsupported("`break` with a value"),
            K::Break { label, value: None } => self.leave(label, true),
            K::Continue { label } => self.leave(label, false),
            K::Jp { .. } => unreachable!("handled by `block`"),
            K::Jump { name, args } => self.jump(name, args),
            K::Ret(a) => {
                let sig = &self.sig(self.me as FuncId);
                let (ok, ret, fails) = (sig.ok.clone(), sig.ret.clone(), sig.fails);
                let v = self.atom(a, &ok)?;
                let v = if fails {
                    Self::variant_value(&ret, "Ok", v)
                } else {
                    v
                };
                self.ret(v);
                Ok(())
            }
            K::Fail(a) => {
                let ret = self.sig(self.me as FuncId).ret.clone();
                let Some((_, _, fs)) = ret.variant("Err") else {
                    return unsupported("`fail` outside a failing function");
                };
                let e = fs[0].1.clone();
                let v = self.atom(a, &e)?;
                self.exit_kind(&e, &v);
                self.ret(Self::variant_value(&ret, "Err", v));
                Ok(())
            }
            K::Become(c) => {
                let name = c.path.join(".");
                let Some(func) = self.callee(c)? else {
                    return unsupported(format!("`become` of `{name}`"));
                };
                if !self.inout.is_empty() || !self.sig(func).inout().is_empty() {
                    return unsupported("`become` with `inout` parameters");
                }
                if self.sig(func).ret != self.sig(self.me as FuncId).ret {
                    return unsupported("`become` across failure types");
                }
                let (args, _) = self.user_args(func, c)?;
                if !self.cost_tokens.is_empty() {
                    // The callee's work counts against this frame's bounds, so the frame stays.
                    let dst: Vec<Local> = self
                        .sig(func)
                        .ret
                        .tys()
                        .into_iter()
                        .map(|t| self.local(t))
                        .collect();
                    self.emit(Inst::Call {
                        dst: dst.clone(),
                        func,
                        args,
                    });
                    self.ret(dst.into_iter().map(Operand::Local).collect());
                    return Ok(());
                }
                if self.costing {
                    self.count(0);
                }
                self.end(Term::TailCall { func, args });
                Ok(())
            }
            // Names bound in a `par` child are read after the block.
            // Branches as tasks that switch at blocking channel operations (RFC 0043).
            K::Par {
                concurrent: true,
                blocks,
            } if blocks.len() > 1 => {
                let tasks: Vec<BlockId> = blocks.iter().map(|_| self.block_new()).collect();
                let join = self.block_new();
                self.finish(Term::Par {
                    tasks: tasks.clone(),
                    join,
                });
                self.tasks += 1;
                for (b, t) in blocks.iter().zip(tasks) {
                    self.cur = t;
                    self.block_in(b)?;
                    self.finish(Term::TaskEnd);
                }
                self.tasks -= 1;
                self.cur = join;
                Ok(())
            }
            K::Par { blocks, .. } => blocks.iter().try_for_each(|b| self.block_in(b)),
            K::Assert(e) => {
                let sides = match e {
                    y::Expr::Atom(y::Atom::Name(n, _)) => self.cmps.get(n).cloned(),
                    _ => None,
                };
                let t = self.expr(e)?;
                let mut fields = Vec::new();
                for (name, a) in [("left", 0), ("right", 1)]
                    .into_iter()
                    .filter_map(|(n, i)| sides.as_ref().map(|s| (n, s[i].clone())))
                {
                    if let Some(v) = self.scalar_field(&a) {
                        fields.push((name.to_owned(), v));
                    }
                }
                let mut seen = Vec::new();
                for a in sides.iter().flatten() {
                    if let y::Atom::Name(n, _) = a
                        && !seen.contains(n)
                    {
                        seen.push(n.clone());
                        if let Some(v) = self.scalar_field(a) {
                            fields.push((n.clone(), v));
                        }
                    }
                }
                let fields = fields.into_iter().map(|(n, v)| (self.lit(&n), v)).collect();
                self.check_fields(t, "assert", fields);
                Ok(())
            }
            K::Unreachable => {
                self.end(Term::Panic("unreachable"));
                Ok(())
            }
            K::Unsafe(b) => self.block(b),
            K::Drop(n) => {
                self.drop_name(n);
                Ok(())
            }
            K::Rhs(y::Rhs::Call(c)) => self.call(c, None),
            K::Rhs(r @ y::Rhs::Propagate { .. }) => {
                self.value(r, &Shape::Scalar(Ty::Unit))?;
                Ok(())
            }
            K::Rhs(_) => Ok(()),
            K::Yield(_) => unsupported("generators"),
            K::With { .. } => unsupported("`with`"),
            K::Deadline { .. } => unsupported("`deadline`"),
            K::ComptimeFor { .. } | K::ComptimeIf { .. } => unsupported("a comptime statement"),
            K::Asm { .. } => unsupported("`asm`"),
            K::Reuse { .. } => unsupported("`reuse`"),
        }
    }
}

/// The names a contract predicate reads, in first-use order.
fn free_names(e: &y::Expr) -> Vec<String> {
    let atoms: Vec<&y::Atom> = match e {
        y::Expr::Atom(a) => vec![a],
        y::Expr::Call(c) => c
            .args
            .iter()
            .filter_map(|a| match a {
                y::Arg::Value(_, a, _) => Some(a),
                y::Arg::Cap(..) => None,
            })
            .collect(),
    };
    let mut out: Vec<String> = Vec::new();
    for a in atoms {
        if let y::Atom::Name(n, _) = a
            && !out.contains(n)
        {
            out.push(n.clone());
        }
    }
    out
}

/// The two sides of a comparison call like `gt_i64(let n, let m)`.
fn comparison(rhs: &y::Rhs) -> Option<[y::Atom; 2]> {
    let y::Rhs::Call(c) = rhs else { return None };
    let f = c.path.last()?;
    let cmp = ["eq", "ne", "lt", "le", "gt", "ge"]
        .iter()
        .any(|p| f == p || f.strip_prefix(p).is_some_and(|r| r.starts_with('_')));
    match c.args.as_slice() {
        [y::Arg::Value(_, a, _), y::Arg::Value(_, b, _)] if cmp => Some([a.clone(), b.clone()]),
        _ => None,
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn lowers_join_points_and_reports_the_rest() {
        let src = "module t\nfn main() -> Unit {\n var n: i64 = 0\n\
                   jp k(let m: i64) { n = add_checked_i64(let n, let m) }\n\
                   loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n\
                   if big { jump k(10) }\n }\n assert eq_i64(let n, let 13)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty());
        let p = lower(&m.unwrap()).expect("lowers");
        let f = &p.funcs[p.main as usize];
        assert!(
            f.blocks
                .iter()
                .any(|b| matches!(b.term, Term::Switch { .. }))
        );
        let (m, _) = yan_core::parse(
            "t.yir",
            "module t\nsum T { Leaf, Node { pub l: T } }\nfn main() -> Unit {\n let t: T = Leaf\n ret ()\n}\n",
        );
        assert!(
            lower(&m.unwrap())
                .unwrap_err()
                .contains("recursive type `T`")
        );
    }

    #[test]
    fn assert_panics_carry_sides_and_free_variables() {
        let src = "module t\nfn main() -> Unit {\n let n: i64 = 3\n let m: i64 = 4\n let c: bool = gt_i64(let n, let m)\n assert c\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty());
        let p = lower(&m.unwrap()).expect("lowers");
        let f = &p.funcs[p.main as usize];
        let fields = f
            .blocks
            .iter()
            .find_map(|b| match &b.term {
                Term::PanicFields {
                    kind: "assert",
                    fields,
                } => Some(fields.clone()),
                _ => None,
            })
            .expect("an assert panic with fields");
        let names: Vec<_> = fields
            .iter()
            .map(|(n, _)| match n {
                Operand::Str(i) => p.strings[*i as usize].as_str(),
                _ => panic!("a name"),
            })
            .collect();
        assert_eq!(names, ["left", "right", "n", "m"]);
    }

    #[test]
    fn contract_panics_carry_clause_item_and_free_variables() {
        let src = "module t\nfn f(let a: i64, let b: i64) -> i64 requires gt_i64(let a, let b) {\n ret a\n}\n\
                   fn main() -> Unit {\n let r: i64 = f(let 2, let 1)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = lower(&m.unwrap()).expect("lowers");
        let fields = p
            .funcs
            .iter()
            .flat_map(|f| &f.blocks)
            .find_map(|b| match &b.term {
                Term::PanicFields {
                    kind: "contract",
                    fields,
                } => Some(fields.clone()),
                _ => None,
            })
            .expect("a contract panic with fields");
        let names: Vec<_> = fields
            .iter()
            .map(|(n, _)| match n {
                Operand::Str(i) => p.strings[*i as usize].as_str(),
                _ => panic!("a name"),
            })
            .collect();
        assert_eq!(names, ["clause", "item", "a", "b"]);
        assert_eq!(fields[0].1, Operand::Int(0));
    }

    #[test]
    fn rejects_a_type_parameter() {
        let src = "module t\nfn id[T: Type](let x: T) -> T {\n ret x\n}\nfn main() -> Unit {\n ret ()\n}\n";
        let (m, _) = yan_core::parse("t.yir", src);
        let Err(e) = lower(&m.unwrap()) else {
            panic!("lowered a generic fn")
        };
        assert!(e.starts_with("internal error: type parameter `T`"), "{e}");
    }

    #[test]
    fn splits_sums_into_a_tag_and_variant_slots() {
        let (m, d) = yan_core::parse(
            "t.yir",
            "module t\nsum S { A { pub x: i64 }, B { pub y: f64, pub z: bool }, C }\n\
             fn main() -> Unit {\n let s: S = B { y: 1.0, z: true }\n ret ()\n}\n",
        );
        assert!(d.is_empty(), "{d:?}");
        let p = lower(&m.unwrap()).expect("lowers");
        let f = &p.funcs[p.main as usize];
        assert_eq!(f.locals[..4], [I64, I64, Ty::F64, Ty::Bool]);
    }

    /// The tag of `o` is read by one switch, and `Neg { k: 0 }` falls through to the row
    /// below it without testing the tag again.
    #[test]
    fn lowers_match_to_one_switch_per_tag() {
        let (m, d) = yan_core::parse(
            "t.yir",
            "module t\nsum Op { Add, Sub, Neg { k: i64 }, Lit { v: i64 } }\n\
             fn f(let o: Op) -> i64 {\n jp k(let a: i64) { ret a }\n match o {\n\
             Add => { jump k(1) }\n Neg { k: 0 } => { jump k(2) }\n\
             Neg { k: n } | Lit { v: n } => { jump k(n) }\n _ => { jump k(7) }\n }\n}\n\
             fn main() -> Unit {\n let a: i64 = f(let Sub)\n ret ()\n}\n",
        );
        assert!(d.is_empty(), "{d:?}");
        let p = lower(&m.unwrap()).expect("lowers");
        let f = p.funcs.iter().find(|f| f.name.ends_with('f')).expect("f");
        let switches = f
            .blocks
            .iter()
            .filter(|b| matches!(&b.term, Term::Switch { targets, .. } if targets.len() == 4))
            .count();
        let tests = f
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| {
                matches!(
                    i,
                    Inst::Scalar {
                        op: ScalarOp::Eq,
                        ..
                    }
                )
            })
            .count();
        assert_eq!((switches, tests), (1, 1));
    }
}
