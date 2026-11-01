//! Yong IR to register bytecode.
//!
//! Each function gets one register per name it binds (its frame was a flat name map, so a name
//! keeps one slot across blocks) plus temporaries. Constructs the VM cannot run compile to an
//! `Unsupported` op, so they fail only when reached, as before.

use std::collections::HashSet;
use std::rc::Rc;

use yong_core::ast::*;

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
    JmpIfNot {
        cond: Opnd,
        to: u32,
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
    /// Panic with `kind` unless `src` is `true`.
    Check {
        src: Reg,
        kind: &'static str,
    },
    Panic(&'static str),
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
    Native(Rc<str>),
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
    codes: Vec<Option<Code>>,
    pending: Vec<(usize, &'m FnDef)>,
}

pub(crate) fn compile(m: &Module) -> Program {
    let mut g = Globals {
        fns: Map::default(),
        methods: Map::default(),
        nullary: ["None".to_owned()].into_iter().collect(),
        codes: Vec::new(),
        pending: Vec::new(),
    };
    for item in &m.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                let id = g.codes.len();
                g.codes.push(None);
                g.fns.insert(f.path.join("."), id);
                g.pending.push((id, f));
            }
            ItemKind::Impl(i) => {
                let ty = yong_core::type_text(&i.for_ty);
                for f in &i.fns {
                    let id = g.codes.len();
                    g.codes.push(None);
                    g.methods.insert(format!("{ty}.{}", f.path.join(".")), id);
                    g.pending.push((id, f));
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
            _ => {}
        }
    }
    // A later item of the same name replaces an earlier one; compile only the live ones.
    let live: HashSet<usize> = g.fns.values().chain(g.methods.values()).copied().collect();
    for (id, f) in std::mem::take(&mut g.pending) {
        if live.contains(&id) {
            let code = FnC::item(&mut g, f);
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
}

impl<'g, 'm> FnC<'g, 'm> {
    fn new(g: &'g mut Globals<'m>, params: &[Param], body: Option<&Block>, extra: &[Name]) -> Self {
        let mut c = FnC {
            g,
            code: Code::default(),
            regs: Map::default(),
            scopes: Vec::new(),
        };
        for p in params {
            let r = c.code.nregs;
            c.code.nregs += 1;
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

    fn item(g: &'g mut Globals<'m>, f: &FnDef) -> Code {
        let mut c = FnC::new(g, &f.params, f.body.as_ref(), &[]);
        for k in &f.contracts {
            if let Contract::Requires(e) = k {
                let t = c.temp();
                c.expr(e, t);
                c.emit(Op::Check {
                    src: t,
                    kind: "contract",
                });
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

    fn closure(&mut self, k: &Closure) -> usize {
        let names: Vec<Name> = k.captures.iter().map(|(_, n, _)| n.clone()).collect();
        let id = self.g.codes.len();
        self.g.codes.push(None);
        let mut c = FnC::new(self.g, &k.params, Some(&k.body), &names);
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
        self.code.ops.len() - 1
    }

    fn pc(&self) -> u32 {
        self.code.ops.len() as u32
    }

    fn patch(&mut self, at: usize, to: u32) {
        match &mut self.code.ops[at] {
            Op::Jmp(t)
            | Op::JmpIfNot { to: t, .. }
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
                Err(crate::Stop::Unsupported(m) | crate::Stop::Panic(m)) => Err(m),
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
            let end = self.pc();
            for at in pending {
                self.patch(at, end);
            }
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { pat, rhs, .. } => match pat {
                Pat::Bind(n) if !self.g.nullary.contains(n) => {
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
                let at = self.emit(Op::JmpIfNot { cond, to: 0 });
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
                self.emit(Op::Jmp(start));
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
            StmtKind::Ret(a) => match self.atom(a) {
                Ok(o) => {
                    self.emit(Op::Ret(o));
                }
                Err(m) => self.unsupported(m),
            },
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
            StmtKind::Par { blocks, .. } => blocks.iter().for_each(|b| self.block(b)),
            StmtKind::Assert(e) => {
                let t = self.temp();
                self.expr(e, t);
                self.emit(Op::Check {
                    src: t,
                    kind: "assert",
                });
            }
            StmtKind::Unreachable => {
                self.emit(Op::Panic("unreachable"));
            }
            StmtKind::Unsafe(b) => self.block(b),
            StmtKind::Drop(n) => {
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
            StmtKind::ComptimeFor { .. } | StmtKind::ComptimeIf { .. } => {
                self.unsupported("stage 0")
            }
            StmtKind::Asm { .. } => self.unsupported("inline assembly"),
            StmtKind::Reuse { .. } => self.unsupported("`reuse`"),
        }
    }

    /// Restore the `with` names between the top scope and scope `to`, exclusive.
    fn unwind(&mut self, to: usize) {
        let restores: Vec<(Reg, Reg)> = self.scopes[to + 1..]
            .iter()
            .rev()
            .filter_map(|s| match s {
                Scope::With { reg, saved } => Some((*reg, *saved)),
                _ => None,
            })
            .collect();
        for (dst, src) in restores {
            self.emit(Op::Restore { dst, src });
        }
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
        let at = self.emit(Op::Jmp(0));
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
            Rhs::Propagate { call, via } => {
                let via = match via {
                    None => Via::None,
                    Some(p) => {
                        let k = p.join(".");
                        match self.g.fns.get(&k) {
                            Some(f) => Via::Item(*f),
                            None => Via::Native(k.into()),
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
            name: name.into(),
            mode,
        });
        Some(self.code.sites.len() as u32 - 1)
    }

    fn call(&mut self, c: &Call, mode: Mode, dst: Reg) {
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
