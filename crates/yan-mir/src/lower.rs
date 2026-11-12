//! Optimized Yan Core (`yan-opt`'s output, R771) to MIR.

use std::cell::RefCell;
use std::collections::HashMap;

mod actor;
mod call;
mod chars;
mod cluster;
mod collections;
mod consts;
mod contracts;
mod encoding;
mod entry;
mod iter;
mod math;
mod ordering;
mod path;
mod pattern;
mod shape;
mod show;
mod stmt;
mod unwind;
mod values;

pub use shape::Shape;
use shape::*;

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

fn zero(t: Ty) -> Operand {
    match t {
        Ty::Int(_) | Ty::Ptr => Operand::Int(0),
        Ty::F64 => Operand::F64(0.0),
        Ty::Bool => Operand::Bool(false),
        Ty::Unit => Operand::Unit,
    }
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
    let mut consts = Vec::new();
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
            y::ItemKind::Const(c) => {
                consts.push(c);
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
        thunks: HashMap::new(),
        costing,
        consts,
    });
    let main = mono.borrow_mut().instance(&types, main)?;
    let main = if mono.borrow().sigs[main as usize].fails {
        mono.borrow_mut().entry(main)
    } else {
        main
    };
    let mut prog = Program {
        funcs: Vec::new(),
        main,
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
            Todo::Thunk(k, s) => Fn::new(&mono, &types, &mut prog.strings, me).thunk(k, &s),
            Todo::Entry(main) => Fn::new(&mono, &types, &mut prog.strings, me).entry(main)?,
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
    /// The actor thunks made, by kind and shape (`actor.rs`).
    thunks: HashMap<(actor::Thunk, Shape), FuncId>,
    /// Some function has a `cost` bound, so every function counts steps and allocations.
    costing: bool,
    consts: Vec<&'m y::Const>,
}

#[derive(Clone)]
enum Todo<'m> {
    Def(usize),
    /// A closure body with its captures' names and types, read from the block in its first
    /// parameter.
    Closure(&'m y::Closure, Vec<(String, Shape)>),
    /// An actor thunk (`actor.rs`).
    Thunk(actor::Thunk, Shape),
    /// The entry that reports a failing `main`.
    Entry(FuncId),
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
        "shl" => Shl,
        "shr" => Shr,
        "wrapping_shl" => WrappingShl,
        "rotate_left" => RotateLeft,
        "rotate_right" => RotateRight,
        "count_ones" => CountOnes,
        "count_zeros" => CountZeros,
        "leading_zeros" => LeadingZeros,
        "trailing_zeros" => TrailingZeros,
        "bit_width" => BitWidth,
        "swap_bytes" => SwapBytes,
        "reverse_bits" => ReverseBits,
        "is_power_of_two" => IsPowerOfTwo,
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
    /// `ensures` predicates with their clause position, checked at every normal return (R882).
    ensures: Vec<(y::Expr, i128)>,
    /// The `inout` parameters' slots, returned with every result.
    inout: Vec<Local>,
    blocks: Vec<Block>,
    done: Vec<bool>,
    cur: BlockId,
    scopes: Vec<Scope>,
    /// The line of the statement being lowered.
    line: u32,
    /// The first local of each `par concurrent` branch enclosing the code being lowered,
    /// outermost first: a name whose locals start below `task_starts[d]` was bound outside the
    /// branch at depth `d + 1`.
    task_starts: Vec<Local>,
    /// Names of an enclosing frame whose `drop` in a branch waits for the exit that follows.
    deferred: Vec<String>,
    /// Each flagged local to its drop flag (5.3).
    flags: HashMap<String, String>,
    /// Whether the program counts steps and allocations for `cost` bounds (R610).
    costing: bool,
    /// The tokens of this function's `cost` counters, ended on every exit.
    cost_tokens: Vec<Operand>,
    /// The supervisor scope depth this function's `with supervisor` scopes end at (`actor.rs`).
    sup_depth: Option<Local>,
    /// Whether the codec rejected a value of the call through `Remote` being made (`cluster.rs`).
    mismatch: Option<Local>,
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
            ensures: Vec::new(),
            inout: Vec::new(),
            blocks: Vec::new(),
            done: Vec::new(),
            cur: 0,
            scopes: Vec::new(),
            line: 0,
            task_starts: Vec::new(),
            deferred: Vec::new(),
            flags: HashMap::new(),
            costing: mono.borrow().costing,
            cost_tokens: Vec::new(),
            sup_depth: None,
            mismatch: None,
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
        self.flags = yan_check::drop_flags(&f.params, body);
        self.body(&f.path.join("."), &names, &f.contracts, body, Vec::new())
    }

    fn closure(mut self, c: &'m y::Closure, caps: Vec<(String, Shape)>) -> R<Func> {
        let mut names = vec![""];
        for p in &c.params {
            let y::Param::Value { name, .. } = p else {
                return unsupported("a closure with a capability parameter");
            };
            names.push(name);
        }
        self.flags = yan_check::drop_flags(&c.params, &c.body);
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
        self.bind_consts()?;
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
                    self.check_clause(e, clause)?;
                    clause += 1;
                }
                y::Contract::Ensures(e) => {
                    self.ensures.push((e.clone(), clause));
                    clause += 1;
                }
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
        self.bind_olds();
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
        if self.task_starts.is_empty() {
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
        self.end_sups();
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
            (ScalarOp::Not, _) => ("not_bool", Ty::Bool),
            (ScalarOp::Ge, _) => ("ge_i64", Ty::Bool),
            (ScalarOp::Le, _) => ("le_i64", Ty::Bool),
            (ScalarOp::AddChecked, _) => ("add_checked_i64", I64),
            (ScalarOp::SubChecked, _) => ("sub_checked_i64", I64),
            (ScalarOp::MulChecked, _) => ("mul_checked_i64", I64),
            (ScalarOp::Add, _) => ("add_f64", Ty::F64),
            (ScalarOp::FloorDiv, _) => ("floordiv_i64", I64),
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
            y::Literal::Char(c) => (Operand::Int(i128::from(u32::from(*c))), Shape::Char),
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
                | Shape::Char
                | Shape::Str
                | Shape::Bytes
                | Shape::Path
                | Shape::List(_)
                | Shape::Map(..)
                | Shape::Heap(_)
                | Shape::Deque(_)
                | Shape::Chan(_)
                | Shape::Actor(_),
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
