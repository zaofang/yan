//! Yan IR to register bytecode.
//!
//! Each function gets one register per name it binds (its frame was a flat name map, so a name
//! keeps one slot across blocks) plus temporaries. Constructs the VM cannot run compile to an
//! `Unsupported` op, so they fail only when reached, as before.

use std::collections::HashSet;
use std::rc::Rc;

use yan_core::ast::*;
use yan_diag::Span;
use yan_std::registry::{self, Id};

use crate::{Fnv, Map, Value, int_range, literal};

pub(crate) type Reg = u32;

/// A register, or a constant when the top bit is set.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Opnd(u32);

const K: u32 = 1 << 31;

impl Opnd {
    pub(crate) fn konst(&self) -> Option<usize> {
        (self.0 & K != 0).then_some((self.0 & !K) as usize)
    }

    pub(crate) fn idx(&self) -> usize {
        (self.0 & !K) as usize
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum IntOp {
    Add,
    Sub,
    Mul,
    FloorDiv,
    Mod,
    Div,
    And,
    Or,
    Xor,
    Cmp(Cmp),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FloatOp {
    Add,
    Sub,
    Mul,
    Div,
    Cmp(Cmp),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum BoolOp {
    And,
    Or,
    Eq,
    Ne,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    /// Enter a cancel scope ending at the monotonic instant `src`, keeping the outer deadline in
    /// `saved`.
    DeadlineBegin {
        src: Opnd,
        saved: Reg,
    },
    DeadlineEnd {
        saved: Reg,
    },
    /// Open a `with supervisor` scope with the spec in `src` (9.6).
    SupBegin(Opnd),
    /// Close it, stopping its children.
    SupEnd,
    Move {
        dst: Reg,
        src: Opnd,
    },
    Read {
        dst: Reg,
        place: u32,
    },
    Write {
        place: u32,
        src: Reg,
    },
    /// Restore a `with` name: `dst = take(src)`.
    Restore {
        dst: Reg,
        src: Reg,
    },
    Clear(Reg),
    Jmp(u32),
    /// A loop's jump back to its start: one step (R487), unlike a jump to a join point.
    Loop(u32),
    /// Run the branches of `pars[idx]` as cooperative tasks, then continue at its join.
    Par(u32),
    /// A task's branch is done.
    TaskEnd,
    JmpIfNot {
        cond: Opnd,
        to: u32,
    },
    JmpIf {
        cond: Opnd,
        to: u32,
    },
    /// An integer compare into `dst` fused with the branch on it: jump when the result is `when`.
    CmpJmp {
        when: bool,
        c: Cmp,
        dst: Reg,
        a: Opnd,
        b: Opnd,
        to: u32,
        site: u32,
    },
    /// Record where a join point returns to.
    SetCont {
        reg: Reg,
        to: u32,
    },
    JmpCont(Reg),
    Ret(Opnd),
    Fail(Opnd),
    Call {
        site: u32,
        dst: Reg,
    },
    Become {
        site: u32,
    },
    /// An `<op>_<int>` intrinsic; `site` is the generic call for other argument values.
    Int {
        op: IntOp,
        ty: u8,
        dst: Reg,
        a: Opnd,
        b: Opnd,
        site: u32,
    },
    Float {
        op: FloatOp,
        dst: Reg,
        a: Opnd,
        b: Opnd,
        site: u32,
    },
    Bool {
        op: BoolOp,
        dst: Reg,
        a: Opnd,
        b: Opnd,
        site: u32,
    },
    Not {
        dst: Reg,
        a: Opnd,
        site: u32,
    },
    /// Start the `cost steps <= src` counter of this frame (R610), described by
    /// `code.contracts[info]`.
    CostBegin {
        src: Reg,
        info: u32,
        alloc: bool,
    },
    /// Panic with `kind` unless `src` is `true`.
    Check {
        src: Reg,
        info: u32,
    },
    /// An `assert` of `src`, described by `code.asserts[info]`.
    Assert {
        src: Reg,
        info: u32,
    },
    Panic(&'static str),
    /// `compile_error` reached at stage 0: panic with kind `compile_error` and `code.msgs[i]`.
    CompileError(u32),
    Unsupported(u32),
    /// Bind `pat` from `src` or jump to `to` when it does not match.
    Match {
        pat: u32,
        src: Reg,
        to: u32,
    },
    Tuple {
        dst: Reg,
        list: u32,
    },
    Data {
        dst: Reg,
        ctor: u32,
    },
    Closure {
        dst: Reg,
        clo: u32,
    },
}

/// Integer types the `Int` op knows, by `ty` index, as `int_range` gives them.
pub(crate) const INT_TYS: [&str; 10] = [
    "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "Addr",
];

pub(crate) const INT_RANGES: [(i128, i128); 10] = [
    (i8::MIN as i128, i8::MAX as i128),
    (i16::MIN as i128, i16::MAX as i128),
    (i32::MIN as i128, i32::MAX as i128),
    (i64::MIN as i128, i64::MAX as i128),
    (i128::MIN, i128::MAX),
    (0, u8::MAX as i128),
    (0, u16::MAX as i128),
    (0, u32::MAX as i128),
    (0, u64::MAX as i128),
    (0, u64::MAX as i128),
];

#[derive(Debug)]
pub(crate) enum Proj_ {
    Field(Rc<str>),
    Index(Opnd),
}

#[derive(Debug)]
pub(crate) struct PlaceC {
    pub root: Reg,
    pub projs: Vec<Proj_>,
    /// The source projections, for messages.
    pub src: Vec<Proj>,
}

#[derive(Debug)]
pub(crate) enum Mode {
    /// A failure leaves the function.
    Plain,
    Try,
    Propagate(Via),
    /// In a contract or assertion, where a failure is unsupported.
    Contract,
    Become,
}

#[derive(Debug)]
pub(crate) enum Via {
    None,
    Item(usize),
    Native(Option<Id>),
}

#[derive(Debug)]
pub(crate) enum Callee {
    Item(usize),
    /// Methods keyed by the type of the first argument, then a closure in `local`, then std.
    Dyn {
        methods: Vec<(Rc<str>, usize)>,
        local: Option<Reg>,
    },
    Native,
}

#[derive(Debug)]
pub(crate) enum ArgC {
    /// An `inout` argument: moved out, written back after the call.
    Inout(Reg),
    Val(Opnd),
}

#[derive(Debug)]
pub(crate) struct Site {
    pub callee: Callee,
    pub args: Vec<ArgC>,
    pub name: Rc<str>,
    /// The registry id of the std function `name`, resolved here so that bound intrinsics are
    /// not looked up by name at run time (R561).
    pub id: Option<Id>,
    pub mode: Mode,
}

#[derive(Debug)]
pub(crate) enum CPat {
    Wild,
    Lit(Option<Value>),
    Bind(Reg),
    Nullary(Name),
    Ctor {
        name: Name,
        fields: Option<Vec<(Name, CPat)>>,
    },
    Tuple(Vec<CPat>),
    Or(Vec<CPat>),
    Range(Option<(i128, i128)>),
    Slice {
        elems: Vec<CPat>,
        rest: Option<Option<Reg>>,
    },
}

#[derive(Debug)]
pub(crate) struct CtorC {
    pub name: Name,
    pub fields: Vec<(Name, Opnd)>,
}

#[derive(Debug)]
pub(crate) struct CloC {
    pub code: usize,
    pub captures: Vec<Opnd>,
}

#[derive(Debug, Default)]
pub(crate) struct Code {
    pub ops: Vec<Op>,
    pub consts: Vec<Value>,
    pub nregs: u32,
    pub nparams: u32,
    /// For a closure, the registers its captured values land in, in capture order.
    pub capture_regs: Vec<Reg>,
    pub places: Vec<PlaceC>,
    pub sites: Vec<Site>,
    pub pats: Vec<CPat>,
    pub lists: Vec<Vec<Opnd>>,
    pub ctors: Vec<CtorC>,
    pub clos: Vec<CloC>,
    pub msgs: Vec<String>,
    /// The item a panic trace names for this code.
    pub name: String,
    /// The statement each op came from, by pc.
    pub spans: Vec<Span>,
    pub asserts: Vec<AssertC>,
    pub contracts: Vec<ContractC>,
    /// The branch entry pcs and the join pc of each `par concurrent`.
    pub pars: Vec<(Vec<u32>, u32)>,
}

/// A checked `requires` clause: its item's identity hash, its position among the item's
/// `requires` and `ensures` clauses (R874), and the free variables the predicate reads (R875).
#[derive(Debug)]
pub(crate) struct ContractC {
    pub item: Option<[u8; 32]>,
    pub clause: u32,
    pub vars: Vec<(Name, Reg)>,
}

/// Where an `assert` is, and the operands of the comparison it tests, if any.
#[derive(Debug)]
pub(crate) struct AssertC {
    pub line: u32,
    pub sides: Vec<Opnd>,
    pub vars: Vec<(Name, Reg)>,
}

pub(crate) struct Program {
    pub codes: Vec<Rc<Code>>,
    pub fns: Map<Name, usize>,
}

struct Globals<'m> {
    fns: Map<Name, usize>,
    /// Method full names (`<type>.<name>`) to code index.
    methods: Map<Name, usize>,
    nullary: HashSet<Name, std::hash::BuildHasherDefault<Fnv>>,
    /// Module constants with literal or materialized (stage-0) values.
    consts: Map<Name, Value>,
    codes: Vec<Option<Code>>,
    pending: Vec<(usize, Name, &'m FnDef, Option<[u8; 32]>)>,
    hashes: Option<Vec<yan_core::hash::Hashed>>,
    types: Rc<crate::meta::Types>,
}

fn const_value(c: &ConstVal) -> Option<Value> {
    Some(match c {
        ConstVal::Lit(l) => literal(l).ok()?,
        ConstVal::Call(..) => return None,
        ConstVal::List(xs) => Value::List(xs.iter().map(const_value).collect::<Option<_>>()?),
        ConstVal::Tuple(xs) => Value::Tuple(xs.iter().map(const_value).collect::<Option<_>>()?),
        ConstVal::Path { path, fields } => Value::Data {
            name: path.join("."),
            fields: fields
                .iter()
                .flatten()
                .map(|(n, v)| Some((n.clone(), const_value(v)?)))
                .collect::<Option<_>>()?,
        },
    })
}

pub(crate) fn compile(m: &Module) -> Program {
    let mut g = Globals {
        fns: Map::default(),
        methods: Map::default(),
        nullary: ["None".to_owned()].into_iter().collect(),
        consts: Map::default(),
        codes: Vec::new(),
        pending: Vec::new(),
        hashes: m
            .items
            .iter()
            .any(|i| matches!(&i.kind, ItemKind::Fn(f) | ItemKind::Inst(Inst { def: f, .. }) if has_requires(f)) || matches!(&i.kind, ItemKind::Impl(i) if i.fns.iter().any(has_requires)))
            .then(|| yan_core::hash::hash_module(m)),
        types: Rc::new(crate::meta::Types::of(m)),
    };
    for (ix, item) in m.items.iter().enumerate() {
        let ih = g.hashes.as_ref().map(|h| *h[ix].identity.group.as_bytes());
        match &item.kind {
            // The interpreter erases types, so an instance runs as its generic function.
            ItemKind::Fn(f) | ItemKind::Inst(Inst { def: f, .. }) => {
                let id = g.codes.len();
                g.codes.push(None);
                g.fns.insert(f.path.join("."), id);
                g.pending.push((id, f.path.join("."), f, ih));
            }
            ItemKind::Impl(i) => {
                let ty = yan_core::type_text(&i.for_ty);
                for f in &i.fns {
                    let id = g.codes.len();
                    g.codes.push(None);
                    let name = format!("{ty}.{}", f.path.join("."));
                    g.methods.insert(name.clone(), id);
                    g.pending.push((id, name, f, ih));
                }
            }
            ItemKind::Sum(s) => {
                g.nullary.extend(
                    s.variants
                        .iter()
                        .filter(|v| v.fields.is_none())
                        .map(|v| v.name.clone()),
                );
            }
            ItemKind::Const(c) => {
                if let Some(v) = const_value(&c.value) {
                    g.consts.insert(c.path.join("."), v);
                }
            }
            _ => {}
        }
    }
    // A later item of the same name replaces an earlier one; compile only the live ones.
    let live: HashSet<usize> = g.fns.values().chain(g.methods.values()).copied().collect();
    for (id, name, f, ih) in std::mem::take(&mut g.pending) {
        if live.contains(&id) {
            let code = FnC::item(&mut g, name, f, ih);
            g.codes[id] = Some(code);
        }
    }
    Program {
        codes: g
            .codes
            .into_iter()
            .map(|c| Rc::new(c.unwrap_or_default()))
            .collect(),
        fns: g.fns,
    }
}

fn has_requires(f: &FnDef) -> bool {
    f.contracts
        .iter()
        .any(|k| matches!(k, Contract::Requires(_) | Contract::Ensures(_)))
}

/// The names a predicate reads, in first-use order.
fn free_vars(e: &Expr) -> Vec<Name> {
    let atoms: Vec<&Atom> = match e {
        Expr::Atom(a) => vec![a],
        Expr::Call(c) => c
            .args
            .iter()
            .filter_map(|a| match a {
                Arg::Value(_, a, _) => Some(a),
                Arg::Cap(..) => None,
            })
            .collect(),
    };
    let mut out: Vec<Name> = Vec::new();
    for a in atoms {
        if let Atom::Name(n, _) = a
            && !out.contains(n)
        {
            out.push(n.clone());
        }
    }
    out
}

fn param_name(p: &Param) -> &Name {
    let (Param::Value { name, .. } | Param::Cap { name, .. }) = p;
    name
}

/// The names a body binds, outside nested closures.
fn binds(
    b: &Block,
    nullary: &HashSet<Name, std::hash::BuildHasherDefault<Fnv>>,
    out: &mut Vec<Name>,
) {
    fn pat(p: &Pat, n: &HashSet<Name, std::hash::BuildHasherDefault<Fnv>>, out: &mut Vec<Name>) {
        match p {
            Pat::Bind(x) if !n.contains(x) => out.push(x.clone()),
            Pat::Ctor {
                fields: Some((fs, _)),
                ..
            } => fs.iter().for_each(|(_, p)| pat(p, n, out)),
            Pat::Tuple(ps) | Pat::Or(ps) => ps.iter().for_each(|p| pat(p, n, out)),
            Pat::Slice { elems, rest } => {
                elems.iter().for_each(|p| pat(p, n, out));
                if let Some(Some(x)) = rest {
                    out.push(x.clone());
                }
            }
            _ => {}
        }
    }
    fn call(c: &Call, out: &mut Vec<Name>) {
        for a in &c.args {
            if let Arg::Value(Conv::Inout, Atom::Name(n, _), _) = a {
                out.push(n.clone());
            }
        }
    }
    fn rhs(r: &Rhs, out: &mut Vec<Name>) {
        match r {
            Rhs::Call(c) | Rhs::Try(c) | Rhs::Propagate { call: c, .. } => call(c, out),
            _ => {}
        }
    }
    fn expr(e: &Expr, out: &mut Vec<Name>) {
        if let Expr::Call(c) = e {
            call(c, out);
        }
    }
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Let { pat: p, rhs: r, .. } => {
                rhs(r, out);
                pat(p, nullary, out);
            }
            StmtKind::Assign { place, rhs: r, .. } => {
                rhs(r, out);
                out.push(place.root.clone());
            }
            StmtKind::Rhs(r) => rhs(r, out),
            StmtKind::If { then, els, .. } => {
                binds(then, nullary, out);
                if let Some(e) = els {
                    binds(e, nullary, out);
                }
            }
            StmtKind::Match { arms, .. } => {
                for (p, b) in arms {
                    pat(p, nullary, out);
                    binds(b, nullary, out);
                }
            }
            StmtKind::Loop { body, .. } | StmtKind::Unsafe(body) => binds(body, nullary, out),
            StmtKind::Jp { params, body, .. } => {
                out.extend(params.iter().map(param_name).cloned());
                binds(body, nullary, out);
            }
            StmtKind::With { name, body, .. } => {
                out.push(name.clone());
                binds(body, nullary, out);
            }
            StmtKind::Deadline { body, .. } => binds(body, nullary, out),
            StmtKind::Par { blocks, .. } => blocks.iter().for_each(|b| binds(b, nullary, out)),
            StmtKind::Become(c) => call(c, out),
            StmtKind::Assert(e) => expr(e, out),
            _ => {}
        }
    }
}

enum Scope {
    Loop {
        label: Name,
        start: u32,
        breaks: Vec<usize>,
    },
    With {
        reg: Reg,
        saved: Reg,
    },
    Deadline {
        saved: Reg,
    },
    Sup,
    Block {
        jps: Vec<(Name, u32, Vec<Reg>)>,
        cont: Option<Reg>,
        /// `SetCont` ops to point past the current statement; `None` inside a join point body,
        /// which returns where the jump into it would have.
        sets: Option<Vec<usize>>,
    },
}

struct FnC<'g, 'm> {
    g: &'g mut Globals<'m>,
    code: Code,
    regs: Map<Name, Reg>,
    scopes: Vec<Scope>,
    /// The last pc taken as a jump target; ops before it cannot fuse with ops after.
    barrier: u32,
    /// The statement being compiled.
    at: Span,
    /// Booleans bound to a comparison of two atoms, for the message of an `assert` on them.
    cmps: Map<Name, [Atom; 2]>,
    /// `ensures` predicates with their clause position, checked at every `ret` (R882).
    ensures: Vec<(Expr, u32)>,
    item: Option<[u8; 32]>,
}

impl<'g, 'm> FnC<'g, 'm> {
    fn new(g: &'g mut Globals<'m>, params: &[Param], body: Option<&Block>, extra: &[Name]) -> Self {
        let mut c = FnC {
            g,
            code: Code::default(),
            regs: Map::default(),
            scopes: Vec::new(),
            barrier: 0,
            at: Span::default(),
            cmps: Map::default(),
            ensures: Vec::new(),
            item: None,
        };
        for p in params {
            let r = c.temp();
            // A repeated parameter name keeps its first slot.
            c.regs.entry(param_name(p).clone()).or_insert(r);
        }
        c.code.nparams = c.code.nregs;
        let mut names = extra.to_vec();
        if let Some(b) = body {
            binds(b, &c.g.nullary, &mut names);
        }
        for n in names {
            c.reg_of(&n);
        }
        c
    }

    fn item(g: &'g mut Globals<'m>, name: Name, f: &FnDef, item: Option<[u8; 32]>) -> Code {
        let mut c = FnC::new(g, &f.params, f.body.as_ref(), &[]);
        c.code.name = name;
        c.at = f.name_span;
        c.item = item;
        let mut clause = 0;
        for k in &f.contracts {
            match k {
                Contract::Requires(e) => {
                    let t = c.temp();
                    c.expr(e, t);
                    let info = c.code.contracts.len() as u32;
                    let vars = free_vars(e)
                        .into_iter()
                        .filter_map(|n| c.regs.get(&n).map(|r| (n, *r)))
                        .collect();
                    c.code.contracts.push(ContractC { item, clause, vars });
                    c.emit(Op::Check { src: t, info });
                    clause += 1;
                }
                Contract::Ensures(e) => {
                    c.bind_olds(e);
                    c.ensures.push((e.clone(), clause));
                    clause += 1;
                }
                Contract::Cost { steps, alloc } => {
                    // Both bounds are evaluated before either counter starts.
                    let mut begins = Vec::new();
                    for (e, alloc) in [(steps, false), (alloc, true)] {
                        let Some(e) = e else { continue };
                        let t = c.temp();
                        c.expr(e, t);
                        begins.push((t, alloc));
                    }
                    for (t, alloc) in begins {
                        let info = c.code.contracts.len() as u32;
                        c.code.contracts.push(ContractC {
                            item,
                            clause,
                            vars: Vec::new(),
                        });
                        c.emit(Op::CostBegin {
                            src: t,
                            info,
                            alloc,
                        });
                    }
                    clause += 1;
                }
                _ => {}
            }
        }
        match &f.body {
            Some(b) => {
                c.block(b);
                c.unsupported("function body ended without `ret`");
            }
            None => c.unsupported("call of a function without a body"),
        }
        c.code
    }

    /// Snapshot each parameter `p` that `e` reads as `old_p` (its entry value) before the body runs.
    fn bind_olds(&mut self, e: &Expr) {
        for n in free_vars(e) {
            if let Some(p) = n.strip_prefix("old_")
                && !self.regs.contains_key(&n)
                && let Some(src) = self.regs.get(p).copied()
            {
                let dst = self.reg_of(&n);
                self.emit(Op::Move {
                    dst,
                    src: Opnd(src),
                });
            }
        }
    }

    fn ret(&mut self, a: &Atom) {
        let o = match self.atom(a) {
            Ok(o) => o,
            Err(m) => return self.unsupported(m),
        };
        if self.ensures.is_empty() {
            self.emit(Op::Ret(o));
            return;
        }
        let out = self.reg_of(&"out".to_owned());
        self.emit(Op::Move { dst: out, src: o });
        for (e, clause) in self.ensures.clone() {
            let t = self.temp();
            self.expr(&e, t);
            let info = self.code.contracts.len() as u32;
            let vars = free_vars(&e)
                .into_iter()
                .filter_map(|n| self.regs.get(&n).map(|r| (n, *r)))
                .collect();
            self.code.contracts.push(ContractC {
                item: self.item,
                clause,
                vars,
            });
            self.emit(Op::Check { src: t, info });
        }
        self.emit(Op::Ret(Opnd(out)));
    }

    fn closure(&mut self, k: &Closure) -> usize {
        let names: Vec<Name> = k.captures.iter().map(|(_, n, _)| n.clone()).collect();
        let id = self.g.codes.len();
        self.g.codes.push(None);
        let mut c = FnC::new(self.g, &k.params, Some(&k.body), &names);
        c.code.name = format!("closure in {}", self.code.name);
        c.at = self.at;
        c.code.capture_regs = names.iter().map(|n| c.regs[n]).collect();
        c.block(&k.body);
        c.unsupported("function body ended without `ret`");
        let code = c.code;
        self.g.codes[id] = Some(code);
        id
    }

    fn reg_of(&mut self, n: &Name) -> Reg {
        if let Some(r) = self.regs.get(n) {
            return *r;
        }
        let r = self.temp();
        self.regs.insert(n.clone(), r);
        r
    }

    fn temp(&mut self) -> Reg {
        let r = self.code.nregs;
        self.code.nregs += 1;
        r
    }

    fn emit(&mut self, op: Op) -> usize {
        self.code.ops.push(op);
        self.code.spans.push(self.at);
        self.code.ops.len() - 1
    }

    /// The next pc, as a jump target.
    fn pc(&mut self) -> u32 {
        self.barrier = self.code.ops.len() as u32;
        self.barrier
    }

    /// Jump when `cond` is `when`. An integer compare into `cond` just before fuses with it.
    fn branch(&mut self, cond: Opnd, when: bool) -> usize {
        let n = self.code.ops.len();
        if n as u32 > self.barrier
            && let Some(Op::Int {
                op: IntOp::Cmp(c),
                dst,
                a,
                b,
                site,
                ..
            }) = self.code.ops.last().copied()
            && cond.konst().is_none()
            && cond.idx() == dst as usize
        {
            self.code.ops[n - 1] = Op::CmpJmp {
                when,
                c,
                dst,
                a,
                b,
                to: 0,
                site,
            };
            return n - 1;
        }
        self.emit(if when {
            Op::JmpIf { cond, to: 0 }
        } else {
            Op::JmpIfNot { cond, to: 0 }
        })
    }

    fn patch(&mut self, at: usize, to: u32) {
        match &mut self.code.ops[at] {
            Op::Jmp(t)
            | Op::Loop(t)
            | Op::JmpIfNot { to: t, .. }
            | Op::JmpIf { to: t, .. }
            | Op::CmpJmp { to: t, .. }
            | Op::SetCont { to: t, .. }
            | Op::Match { to: t, .. } => *t = to,
            _ => unreachable!("patched op has no target"),
        }
    }

    fn unsupported(&mut self, msg: impl Into<String>) {
        self.code.msgs.push(msg.into());
        let i = self.code.msgs.len() as u32 - 1;
        self.emit(Op::Unsupported(i));
    }

    fn konst(&mut self, v: Value) -> Opnd {
        self.code.consts.push(v);
        Opnd((self.code.consts.len() as u32 - 1) | K)
    }

    /// The operand for a name read, or the message for an unbound name.
    fn lookup(&mut self, n: &Name) -> Result<Opnd, String> {
        if let Some(r) = self.regs.get(n) {
            return Ok(Opnd(*r));
        }
        if let Some(v) = self.g.consts.get(n) {
            return Ok(self.konst(v.clone()));
        }
        if self.g.nullary.contains(n) {
            return Ok(self.konst(Value::Data {
                name: n.clone(),
                fields: Vec::new(),
            }));
        }
        Err(format!("unbound name `{n}`"))
    }

    fn atom(&mut self, a: &Atom) -> Result<Opnd, String> {
        match a {
            Atom::Lit(l, _) => match literal(l) {
                Ok(v) => Ok(self.konst(v)),
                Err(crate::Stop::Unsupported(m)) => Err(m),
                Err(crate::Stop::Panic(p)) => Err(p.message),
                Err(crate::Stop::Limit(m)) => Err(m.to_owned()),
            },
            Atom::Name(n, _) => self.lookup(n),
        }
    }

    fn atoms(&mut self, xs: &[Atom]) -> Result<Vec<Opnd>, String> {
        xs.iter().map(|a| self.atom(a)).collect()
    }

    /// Emit a move of `a` into `dst`, or the op that fails as reading it would.
    fn atom_into(&mut self, a: &Atom, dst: Reg) {
        match self.atom(a) {
            Ok(src) => {
                self.emit(Op::Move { dst, src });
            }
            Err(m) => self.unsupported(m),
        }
    }

    fn block(&mut self, b: &Block) {
        let has_jp = b
            .stmts
            .iter()
            .any(|s| matches!(s.kind, StmtKind::Jp { .. }));
        let cont = has_jp.then(|| self.temp());
        self.scopes.push(Scope::Block {
            jps: Vec::new(),
            cont,
            sets: None,
        });
        let top = self.scopes.len() - 1;
        for s in &b.stmts {
            if let StmtKind::Jp { name, params, body } = &s.kind {
                let over = self.emit(Op::Jmp(0));
                let start = self.pc();
                let ps = params.iter().map(|p| self.reg_of(param_name(p))).collect();
                let Scope::Block { jps, sets, .. } = &mut self.scopes[top] else {
                    unreachable!()
                };
                jps.push((name.clone(), start, ps));
                *sets = None;
                self.block(body);
                self.emit(Op::JmpCont(cont.expect("block has a join point")));
                let end = self.pc();
                self.patch(over, end);
                continue;
            }
            if let Scope::Block { sets, .. } = &mut self.scopes[top] {
                *sets = Some(Vec::new());
            }
            self.stmt(s);
            let Scope::Block { sets, .. } = &mut self.scopes[top] else {
                unreachable!()
            };
            let pending = sets.take().unwrap_or_default();
            if !pending.is_empty() {
                let end = self.pc();
                for at in pending {
                    self.patch(at, end);
                }
            }
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, s: &Stmt) {
        let outer = std::mem::replace(&mut self.at, s.span);
        self.stmt_ops(s);
        self.at = outer;
    }

    fn stmt_ops(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { pat, rhs, .. } => match pat {
                Pat::Bind(n) if !self.g.nullary.contains(n) => {
                    match comparison(rhs) {
                        Some(sides) => self.cmps.insert(n.clone(), sides),
                        None => self.cmps.remove(n),
                    };
                    let dst = self.reg_of(n);
                    self.rhs(rhs, dst);
                }
                _ => {
                    let t = self.temp();
                    self.rhs(rhs, t);
                    let p = self.pat(pat);
                    let at = self.emit(Op::Match {
                        pat: p,
                        src: t,
                        to: 0,
                    });
                    let over = self.emit(Op::Jmp(0));
                    let fail = self.pc();
                    self.patch(at, fail);
                    self.emit(Op::Panic("pattern"));
                    let end = self.pc();
                    self.patch(over, end);
                }
            },
            StmtKind::Assign { place, rhs, .. } => {
                if place.projs.is_empty() {
                    let dst = self.reg_of(&place.root);
                    self.rhs(rhs, dst);
                } else {
                    let t = self.temp();
                    self.rhs(rhs, t);
                    match self.place(place) {
                        Ok(p) => {
                            self.emit(Op::Write { place: p, src: t });
                        }
                        Err(m) => self.unsupported(m),
                    }
                }
            }
            StmtKind::Rhs(rhs) => {
                let t = self.temp();
                self.rhs(rhs, t);
            }
            StmtKind::If { cond, then, els } => {
                let cond = match self.atom(cond) {
                    Ok(c) => c,
                    Err(m) => return self.unsupported(m),
                };
                if els.is_none()
                    && let [only] = &then.stmts[..]
                    && let StmtKind::Break { label, .. } | StmtKind::Continue { label } = &only.kind
                    && self.leave_loop_if(cond, label, matches!(only.kind, StmtKind::Break { .. }))
                {
                    return;
                }
                let at = self.branch(cond, false);
                self.block(then);
                match els {
                    Some(e) => {
                        let over = self.emit(Op::Jmp(0));
                        let l = self.pc();
                        self.patch(at, l);
                        self.block(e);
                        let end = self.pc();
                        self.patch(over, end);
                    }
                    None => {
                        let end = self.pc();
                        self.patch(at, end);
                    }
                }
            }
            StmtKind::Match { scrutinee, arms } => {
                let src = self.temp();
                self.atom_into(scrutinee, src);
                let mut ends = Vec::new();
                for (p, b) in arms {
                    let p = self.pat(p);
                    let at = self.emit(Op::Match { pat: p, src, to: 0 });
                    self.block(b);
                    ends.push(self.emit(Op::Jmp(0)));
                    let next = self.pc();
                    self.patch(at, next);
                }
                self.emit(Op::Panic("match"));
                let end = self.pc();
                for at in ends {
                    self.patch(at, end);
                }
            }
            StmtKind::Loop { label, body, .. } => {
                let start = self.pc();
                self.scopes.push(Scope::Loop {
                    label: label.clone(),
                    start,
                    breaks: Vec::new(),
                });
                self.block(body);
                self.emit(Op::Loop(start));
                let Some(Scope::Loop { breaks, .. }) = self.scopes.pop() else {
                    unreachable!()
                };
                let end = self.pc();
                for at in breaks {
                    self.patch(at, end);
                }
            }
            StmtKind::Break { label, .. } => self.leave_loop(label, true),
            StmtKind::Continue { label } => self.leave_loop(label, false),
            StmtKind::Jp { .. } => {}
            StmtKind::Jump { name, args } => self.jump(name, args),
            StmtKind::Ret(a) => self.ret(a),
            StmtKind::Fail(a) => match self.atom(a) {
                Ok(o) => {
                    self.emit(Op::Fail(o));
                }
                Err(m) => self.unsupported(m),
            },
            StmtKind::Become(c) => {
                if let Some(site) = self.site(c, Mode::Become) {
                    self.emit(Op::Become { site });
                }
            }
            StmtKind::Par { blocks, concurrent } => self.par(blocks, *concurrent),
            StmtKind::Assert(e) => {
                let t = self.temp();
                self.expr(e, t);
                let sides = match e {
                    Expr::Atom(Atom::Name(n, _)) => self.cmps.get(n).cloned(),
                    _ => None,
                };
                let mut vars: Vec<(Name, Reg)> = Vec::new();
                for a in sides.iter().flatten() {
                    if let Atom::Name(n, _) = a
                        && !vars.iter().any(|(v, _)| v == n)
                        && let Some(r) = self.regs.get(n)
                    {
                        vars.push((n.clone(), *r));
                    }
                }
                let sides = sides
                    .and_then(|[a, b]| Some(vec![self.atom(&a).ok()?, self.atom(&b).ok()?]))
                    .unwrap_or_default();
                self.code.asserts.push(AssertC {
                    line: s.span.line,
                    sides,
                    vars,
                });
                let info = self.code.asserts.len() as u32 - 1;
                self.emit(Op::Assert { src: t, info });
            }
            StmtKind::Unreachable => {
                self.emit(Op::Panic("unreachable"));
            }
            StmtKind::Unsafe(b) => self.block(b),
            StmtKind::Drop(n) => {
                self.cmps.retain(|_, sides| {
                    !sides
                        .iter()
                        .any(|a| matches!(a, Atom::Name(m, _) if m == n))
                });
                if let Some(r) = self.regs.get(n) {
                    let r = *r;
                    self.emit(Op::Clear(r));
                }
            }
            StmtKind::Yield(_) => self.unsupported("generators"),
            StmtKind::With {
                name,
                dyn_: false,
                handler,
                body,
            } if name == "supervisor" => {
                let src = match self.atom(handler) {
                    Ok(o) => o,
                    Err(m) => return self.unsupported(m),
                };
                self.emit(Op::SupBegin(src));
                self.scopes.push(Scope::Sup);
                self.block(body);
                self.scopes.pop();
                self.emit(Op::SupEnd);
            }
            StmtKind::With {
                name,
                dyn_: false,
                handler,
                body,
            } => {
                let h = match self.atom(handler) {
                    Ok(h) => h,
                    Err(m) => return self.unsupported(m),
                };
                let reg = self.reg_of(name);
                let saved = self.temp();
                self.emit(Op::Restore {
                    dst: saved,
                    src: reg,
                });
                self.emit(Op::Move { dst: reg, src: h });
                self.scopes.push(Scope::With { reg, saved });
                self.block(body);
                self.scopes.pop();
                self.emit(Op::Restore {
                    dst: reg,
                    src: saved,
                });
            }
            StmtKind::With { .. } => self.unsupported("`with ... = dyn`"),
            StmtKind::Deadline { at, body } => {
                let src = match self.atom(at) {
                    Ok(o) => o,
                    Err(m) => return self.unsupported(m),
                };
                let saved = self.temp();
                self.emit(Op::DeadlineBegin { src, saved });
                self.scopes.push(Scope::Deadline { saved });
                self.block(body);
                self.scopes.pop();
                self.emit(Op::DeadlineEnd { saved });
            }
            StmtKind::ComptimeFor { .. } | StmtKind::ComptimeIf { .. } => {
                self.unsupported("stage 0")
            }
            StmtKind::Asm { .. } => self.unsupported("inline assembly"),
            StmtKind::Reuse { .. } => self.unsupported("`reuse`"),
        }
    }

    /// The branches of a `par` in source order. With `rand` in scope, branch `k` runs under
    /// `rand.split(k)`, so its stream depends on its task path only (RFC 0019 D5).
    fn par(&mut self, blocks: &[Block], concurrent: bool) {
        let Some(reg) = self.regs.get("rand").copied() else {
            if concurrent && blocks.len() > 1 {
                return self.par_tasks(blocks);
            }
            return blocks.iter().for_each(|b| self.block(b));
        };
        let saved = self.temp();
        self.emit(Op::Restore {
            dst: saved,
            src: reg,
        });
        self.scopes.push(Scope::With { reg, saved });
        for (k, b) in blocks.iter().enumerate() {
            let branch = self.konst(Value::Int(k as i128));
            self.code.sites.push(Site {
                callee: Callee::Native,
                args: vec![ArgC::Val(Opnd(saved)), ArgC::Val(branch)],
                id: registry::id("std.rand.split"),
                name: "std.rand.split".into(),
                mode: Mode::Plain,
            });
            let site = self.code.sites.len() as u32 - 1;
            self.emit(Op::Call { site, dst: reg });
            self.block(b);
        }
        self.scopes.pop();
        self.emit(Op::Restore {
            dst: reg,
            src: saved,
        });
    }

    /// The branches of a `par concurrent` as tasks over this frame. A name bound in more than one
    /// branch gets a register per branch, so interleaved branches do not share it.
    fn par_tasks(&mut self, blocks: &[Block]) {
        let bound: Vec<Vec<Name>> = blocks
            .iter()
            .map(|b| {
                let mut v = Vec::new();
                binds(b, &self.g.nullary, &mut v);
                v.sort();
                v.dedup();
                v
            })
            .collect();
        let at = self.code.ops.len();
        self.emit(Op::Par(0));
        let mut entries = Vec::new();
        for (k, b) in blocks.iter().enumerate() {
            entries.push(self.code.ops.len() as u32);
            let mut saved = Vec::new();
            for n in &bound[k] {
                let shared = bound
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != k && o.contains(n));
                if shared && let Some(old) = self.regs.get(n).copied() {
                    let fresh = self.temp();
                    self.regs.insert(n.clone(), fresh);
                    saved.push((n.clone(), old));
                }
            }
            self.block(b);
            self.emit(Op::TaskEnd);
            for (n, old) in saved {
                self.regs.insert(n, old);
            }
        }
        let join = self.code.ops.len() as u32;
        self.code.pars.push((entries, join));
        self.code.ops[at] = Op::Par(self.code.pars.len() as u32 - 1);
    }

    /// Restore the `with` names between the top scope and scope `to`, exclusive.
    fn unwind(&mut self, to: usize) {
        let restores: Vec<Op> = self.scopes[to + 1..]
            .iter()
            .rev()
            .filter_map(|s| match s {
                Scope::With { reg, saved } => Some(Op::Restore {
                    dst: *reg,
                    src: *saved,
                }),
                Scope::Deadline { saved } => Some(Op::DeadlineEnd { saved: *saved }),
                Scope::Sup => Some(Op::SupEnd),
                _ => None,
            })
            .collect();
        for op in restores {
            self.emit(op);
        }
    }

    /// `if cond { break 'label }` (or `continue`) as one branch, when no `with` is in between.
    fn leave_loop_if(&mut self, cond: Opnd, label: &Name, brk: bool) -> bool {
        let Some(i) = self
            .scopes
            .iter()
            .rposition(|s| matches!(s, Scope::Loop { label: l, .. } if l == label))
        else {
            return false;
        };
        if self.scopes[i + 1..]
            .iter()
            .any(|s| matches!(s, Scope::With { .. } | Scope::Deadline { .. } | Scope::Sup))
        {
            return false;
        }
        let at = self.branch(cond, true);
        let Scope::Loop { start, breaks, .. } = &mut self.scopes[i] else {
            unreachable!()
        };
        if brk {
            breaks.push(at);
        } else {
            let s = *start;
            self.patch(at, s);
        }
        true
    }

    fn leave_loop(&mut self, label: &Name, brk: bool) {
        let Some(i) = self
            .scopes
            .iter()
            .rposition(|s| matches!(s, Scope::Loop { label: l, .. } if l == label))
        else {
            return self.unsupported(format!("label `'{label}` outside its loop"));
        };
        self.unwind(i);
        let at = self.emit(if brk { Op::Jmp(0) } else { Op::Loop(0) });
        let Scope::Loop { start, breaks, .. } = &mut self.scopes[i] else {
            unreachable!()
        };
        if brk {
            breaks.push(at);
        } else {
            let s = *start;
            self.patch(at, s);
        }
    }

    fn jump(&mut self, name: &Name, args: &[Atom]) {
        let found = self
            .scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, s)| match s {
                Scope::Block { jps, cont, .. } => {
                    jps.iter().find(|(n, ..)| n == name).map(|(_, start, ps)| {
                        (i, *start, ps.clone(), cont.expect("block has a join point"))
                    })
                }
                _ => None,
            });
        let Some((i, start, params, cont)) = found else {
            return self.unsupported(format!("jump to undeclared join point `{name}`"));
        };
        let vals = match self.atoms(args) {
            Ok(v) => v,
            Err(m) => return self.unsupported(m),
        };
        let temps: Vec<Reg> = vals
            .into_iter()
            .map(|src| {
                let t = self.temp();
                self.emit(Op::Move { dst: t, src });
                t
            })
            .collect();
        self.unwind(i);
        let Scope::Block { sets, .. } = &self.scopes[i] else {
            unreachable!()
        };
        if sets.is_some() {
            let at = self.emit(Op::SetCont { reg: cont, to: 0 });
            if let Scope::Block { sets: Some(s), .. } = &mut self.scopes[i] {
                s.push(at);
            }
        }
        for (p, t) in params.into_iter().zip(temps) {
            self.emit(Op::Move {
                dst: p,
                src: Opnd(t),
            });
        }
        self.emit(Op::Jmp(start));
    }

    fn expr(&mut self, e: &Expr, dst: Reg) {
        match e {
            Expr::Atom(a) => self.atom_into(a, dst),
            Expr::Call(c) => self.call(c, Mode::Contract, dst),
        }
    }

    fn rhs(&mut self, r: &Rhs, dst: Reg) {
        match r {
            Rhs::Atom(a) => self.atom_into(a, dst),
            Rhs::Proj(p) | Rhs::Copy(p) | Rhs::Move(p) => {
                if p.projs.is_empty() || !self.regs.contains_key(&p.root) {
                    match self.lookup(&p.root) {
                        Ok(src) => {
                            self.emit(Op::Move { dst, src });
                        }
                        Err(m) => self.unsupported(m),
                    }
                    return;
                }
                match self.place(p) {
                    Ok(place) => {
                        self.emit(Op::Read { dst, place });
                    }
                    Err(m) => self.unsupported(m),
                }
            }
            Rhs::Call(c) => self.call(c, Mode::Plain, dst),
            Rhs::Try(c) => self.call(c, Mode::Try, dst),
            Rhs::Propagate { call, via, .. } => {
                let via = match via {
                    None => Via::None,
                    Some(p) => {
                        let k = p.join(".");
                        match self.g.fns.get(&k) {
                            Some(f) => Via::Item(*f),
                            None => Via::Native(registry::id(&k)),
                        }
                    }
                };
                self.call(call, Mode::Propagate(via), dst);
            }
            Rhs::Ctor(Ctor::Tuple(xs)) => match self.atoms(xs) {
                Ok(xs) => {
                    self.code.lists.push(xs);
                    let list = self.code.lists.len() as u32 - 1;
                    self.emit(Op::Tuple { dst, list });
                }
                Err(m) => self.unsupported(m),
            },
            Rhs::Ctor(Ctor::Named { path, fields, .. }) => {
                let fs: Result<Vec<_>, String> = fields
                    .iter()
                    .map(|(n, a)| Ok((n.clone(), self.atom(a)?)))
                    .collect();
                match fs {
                    Ok(fields) => {
                        self.code.ctors.push(CtorC {
                            name: path.last().expect("paths are non-empty").clone(),
                            fields,
                        });
                        let ctor = self.code.ctors.len() as u32 - 1;
                        self.emit(Op::Data { dst, ctor });
                    }
                    Err(m) => self.unsupported(m),
                }
            }
            Rhs::Closure(k) => {
                let caps: Result<Vec<_>, String> =
                    k.captures.iter().map(|(_, n, _)| self.lookup(n)).collect();
                match caps {
                    Ok(captures) => {
                        let code = self.closure(k);
                        self.code.clos.push(CloC { code, captures });
                        let clo = self.code.clos.len() as u32 - 1;
                        self.emit(Op::Closure { dst, clo });
                    }
                    Err(m) => self.unsupported(m),
                }
            }
        }
    }

    fn place(&mut self, p: &Place) -> Result<u32, String> {
        let root = self.reg_of(&p.root);
        let projs = p
            .projs
            .iter()
            .map(|pr| {
                Ok(match pr {
                    Proj::Field(f, _) => Proj_::Field(f.as_str().into()),
                    Proj::Index(a) => Proj_::Index(self.atom(a)?),
                })
            })
            .collect::<Result<_, String>>()?;
        self.code.places.push(PlaceC {
            root,
            projs,
            src: p.projs.clone(),
        });
        Ok(self.code.places.len() as u32 - 1)
    }

    fn pat(&mut self, p: &Pat) -> u32 {
        let c = self.cpat(p);
        self.code.pats.push(c);
        self.code.pats.len() as u32 - 1
    }

    fn cpat(&mut self, p: &Pat) -> CPat {
        match p {
            Pat::Wild => CPat::Wild,
            Pat::Lit(l) => CPat::Lit(literal(l).ok()),
            Pat::Bind(n) if self.g.nullary.contains(n) => CPat::Nullary(n.clone()),
            Pat::Bind(n) => CPat::Bind(self.reg_of(n)),
            Pat::Ctor { path, fields } => CPat::Ctor {
                name: path.last().expect("paths are non-empty").clone(),
                fields: fields
                    .as_ref()
                    .map(|(fs, _)| fs.iter().map(|(n, p)| (n.clone(), self.cpat(p))).collect()),
            },
            Pat::Tuple(ps) => CPat::Tuple(ps.iter().map(|p| self.cpat(p)).collect()),
            Pat::Or(ps) => CPat::Or(ps.iter().map(|p| self.cpat(p)).collect()),
            Pat::Range(lo, hi) => CPat::Range(match (literal(lo), literal(hi)) {
                (Ok(Value::Int(lo)), Ok(Value::Int(hi))) => Some((lo, hi)),
                _ => None,
            }),
            Pat::Slice { elems, rest } => CPat::Slice {
                elems: elems.iter().map(|p| self.cpat(p)).collect(),
                rest: rest.as_ref().map(|r| r.as_ref().map(|n| self.reg_of(n))),
            },
        }
    }

    /// Add a call site, or emit the op that fails as evaluating its arguments would.
    fn site(&mut self, c: &Call, mode: Mode) -> Option<u32> {
        let mut args = Vec::with_capacity(c.args.len());
        for a in &c.args {
            let r = match a {
                Arg::Value(Conv::Inout, Atom::Name(n, _), _) => Ok(ArgC::Inout(self.reg_of(n))),
                Arg::Value(_, a, _) => self.atom(a).map(ArgC::Val),
                Arg::Cap(n, _) => self.lookup(n).map(ArgC::Val),
            };
            match r {
                Ok(a) => args.push(a),
                Err(m) => {
                    self.unsupported(m);
                    return None;
                }
            }
        }
        let name = c.path.join(".");
        let callee = if let Some(f) = self.g.fns.get(&name) {
            Callee::Item(*f)
        } else {
            let suffix = format!(".{name}");
            let methods: Vec<(Rc<str>, usize)> = self
                .g
                .methods
                .iter()
                .filter_map(|(k, f)| k.strip_suffix(&suffix).map(|ty| (ty.into(), *f)))
                .collect();
            let local = match c.path.as_slice() {
                [one] => self.regs.get(one).copied(),
                _ => None,
            };
            if methods.is_empty() && local.is_none() {
                Callee::Native
            } else {
                Callee::Dyn { methods, local }
            }
        };
        self.code.sites.push(Site {
            callee,
            args,
            id: registry::id(&name),
            name: name.into(),
            mode,
        });
        Some(self.code.sites.len() as u32 - 1)
    }

    fn call(&mut self, c: &Call, mode: Mode, dst: Reg) {
        if c.path == ["std", "meta", "type_of"]
            && let [TArg::Type(t)] = c.targs.as_slice()
        {
            let src = self.konst(crate::meta::handle(t.clone(), &self.g.types));
            self.emit(Op::Move { dst, src });
            return;
        }
        if c.path == ["std", "meta", "compile_error"]
            && let [Arg::Value(_, Atom::Lit(Literal::Str(msg), _), _)] = c.args.as_slice()
        {
            self.code.msgs.push(msg.clone());
            let i = self.code.msgs.len() as u32 - 1;
            self.emit(Op::CompileError(i));
            return;
        }
        let typed = matches!(mode, Mode::Plain | Mode::Propagate(_) | Mode::Contract);
        let Some(site) = self.site(c, mode) else {
            return;
        };
        let s = &self.code.sites[site as usize];
        if typed
            && matches!(s.callee, Callee::Native)
            && let Some(op) = intrinsic_op(&s.name, &s.args, dst, site)
        {
            self.emit(op);
            return;
        }
        self.emit(Op::Call { site, dst });
    }
}

/// The typed op for an `<op>_<scalar>` intrinsic call (assumption Y4), when there is one.
fn intrinsic_op(name: &str, args: &[ArgC], dst: Reg, site: u32) -> Option<Op> {
    let (op, ty) = name.rsplit_once('_')?;
    let vals: Vec<Opnd> = args
        .iter()
        .map(|a| match a {
            ArgC::Val(o) => Some(*o),
            ArgC::Inout(_) => None,
        })
        .collect::<Option<_>>()?;
    let cmp = |op: &str| {
        Some(match op {
            "eq" => Cmp::Eq,
            "ne" => Cmp::Ne,
            "lt" => Cmp::Lt,
            "le" => Cmp::Le,
            "gt" => Cmp::Gt,
            "ge" => Cmp::Ge,
            _ => return None,
        })
    };
    if let [a] = vals[..] {
        return (ty == "bool" && op == "not").then_some(Op::Not { dst, a, site });
    }
    let [a, b] = vals[..] else {
        return None;
    };
    if int_range(ty).is_some() {
        let ty = INT_TYS.iter().position(|t| *t == ty)? as u8;
        let op = match op {
            "add_checked" => IntOp::Add,
            "sub_checked" => IntOp::Sub,
            "mul_checked" => IntOp::Mul,
            "floordiv" => IntOp::FloorDiv,
            "mod" => IntOp::Mod,
            "div" => IntOp::Div,
            "and" => IntOp::And,
            "or" => IntOp::Or,
            "xor" => IntOp::Xor,
            op => IntOp::Cmp(cmp(op)?),
        };
        return Some(Op::Int {
            op,
            ty,
            dst,
            a,
            b,
            site,
        });
    }
    if ty.starts_with('f') {
        let op = match op {
            "add" => FloatOp::Add,
            "sub" => FloatOp::Sub,
            "mul" => FloatOp::Mul,
            "div" => FloatOp::Div,
            op => FloatOp::Cmp(cmp(op)?),
        };
        return Some(Op::Float {
            op,
            dst,
            a,
            b,
            site,
        });
    }
    if ty == "bool" {
        let op = match op {
            "and" => BoolOp::And,
            "or" => BoolOp::Or,
            "eq" => BoolOp::Eq,
            "ne" => BoolOp::Ne,
            _ => return None,
        };
        return Some(Op::Bool {
            op,
            dst,
            a,
            b,
            site,
        });
    }
    None
}

/// The two sides of `rhs` when it is a comparison call on atoms.
fn comparison(rhs: &Rhs) -> Option<[Atom; 2]> {
    let Rhs::Call(c) = rhs else { return None };
    let f = c.path.last()?;
    let cmp = ["eq", "ne", "lt", "le", "gt", "ge"]
        .iter()
        .any(|p| f == p || f.strip_prefix(p).is_some_and(|r| r.starts_with('_')));
    match c.args.as_slice() {
        [Arg::Value(_, a, _), Arg::Value(_, b, _)] if cmp => Some([a.clone(), b.clone()]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_ranges_follow_int_range() {
        for (ty, r) in INT_TYS.iter().zip(INT_RANGES) {
            assert_eq!(int_range(ty), Some(r), "{ty}");
        }
    }
}
