//! Drop elaboration (R770, 5.3 step 1): after this pass every owned, non-copy local is consumed
//! or dropped exactly once on every path.
//!
//! Owned locals are `let`/`var` bindings that do not alias another place (`Rhs::Proj` binds a
//! view) and `sink` parameters. Drops go at scope exit in reverse declaration order and before
//! every early exit. A value moved on only some paths is dropped on the branch edge where it is
//! still owned; if its type has a custom `Drop` or is `@linear`, an edge drop could run earlier
//! than 5.4 allows, so the pass writes a drop flag `var _dN: bool` instead (D2.3).
//!
//! A name bound by a `match` pattern or a non-copy projection views its source: moving it moves
//! the source (the rest of the source leaks).
//!
//! Not covered yet: overwriting a field or an `inout` place, and moving a field out of a local.

use std::collections::{HashMap, HashSet};
use std::mem;

use yan_check::own;
use yan_core::ast::*;
use yan_diag::Span;

use crate::Error;

type Live = HashMap<Name, bool>;
/// A point in the scope stack: locals of scope `.0` from index `.1` on, and every deeper scope.
type Pos = (usize, usize);

pub fn elaborate(m: &Module) -> Result<Module, Error> {
    let obs = Observable::new(m);
    let mut out = m.clone();
    for it in &mut out.items {
        match &mut it.kind {
            ItemKind::Fn(f) | ItemKind::Inst(Inst { def: f, .. }) => {
                if let Some(b) = &f.body {
                    f.body = Some(settle(&obs, &f.params, b)?);
                }
            }
            ItemKind::Impl(i) => {
                for f in &mut i.fns {
                    if let Some(b) = &f.body {
                        f.body = Some(settle(&obs, &f.params, b)?);
                    }
                }
            }
            ItemKind::Test(t) => t.body = settle(&obs, &[], &t.body)?,
            _ => {}
        }
    }
    Ok(out)
}

/// Run the pass over one body until the set of flagged locals stops growing.
fn settle(obs: &Observable, params: &[Param], body: &Block) -> Result<Block, Error> {
    let mut flags: HashMap<Name, Name> = HashMap::new();
    for _ in 0..8 {
        let mut e = Elab {
            obs,
            flags: flags.clone(),
            need: Vec::new(),
            tmp: 0,
            cx: Cx::default(),
        };
        let b = e.block(body, owned_params(params));
        if e.need.is_empty() {
            return Ok(b);
        }
        for n in e.need {
            let k = flags.len();
            flags.entry(n).or_insert(format!("_d{k}"));
        }
    }
    Err(Error::Unsupported("drop flags that do not settle".into()))
}

fn owned_params(params: &[Param]) -> Vec<(Name, Type)> {
    params
        .iter()
        .filter_map(|p| match p {
            Param::Value {
                conv: Conv::Sink,
                name,
                ty,
                ..
            } => Some((name.clone(), ty.clone())),
            _ => None,
        })
        .collect()
}

/// Types whose drop can be seen: custom `Drop`, `@linear`, extern handles, and anything holding one.
struct Observable(HashSet<String>);

impl Observable {
    fn new(m: &Module) -> Self {
        let last = |p: &Path| p.last().cloned().unwrap_or_default();
        let mut set = HashSet::new();
        for it in &m.items {
            if it.attrs.iter().any(|a| a.name == "linear") {
                match &it.kind {
                    ItemKind::Record(r) => drop(set.insert(last(&r.path))),
                    ItemKind::Sum(s) => drop(set.insert(last(&s.path))),
                    _ => {}
                }
            }
            match &it.kind {
                ItemKind::Impl(i) if i.trait_.path.last().is_some_and(|n| n == "Drop") => {
                    if let Type::Named(n) = &i.for_ty {
                        set.insert(last(&n.path));
                    }
                }
                ItemKind::Extern(x) => {
                    for xi in &x.items {
                        if let XItem::Type { name, .. } = xi {
                            set.insert(name.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        loop {
            let before = set.len();
            for it in &m.items {
                let (name, tys): (String, Vec<&Type>) = match &it.kind {
                    ItemKind::Record(r) => {
                        (last(&r.path), r.fields.iter().map(|f| &f.ty).collect())
                    }
                    ItemKind::Sum(s) => (
                        last(&s.path),
                        s.variants
                            .iter()
                            .flat_map(|v| v.fields.iter().flatten().map(|f| &f.ty))
                            .collect(),
                    ),
                    ItemKind::Newtype(n) => (last(&n.path), vec![&n.ty]),
                    _ => continue,
                };
                if tys.iter().any(|t| mentions(t, &set)) {
                    set.insert(name);
                }
            }
            if set.len() == before {
                return Observable(set);
            }
        }
    }

    fn of(&self, t: &Type) -> bool {
        mentions(t, &self.0)
    }
}

fn mentions(t: &Type, set: &HashSet<String>) -> bool {
    match t {
        Type::Named(n) => {
            n.path.last().is_some_and(|b| set.contains(b))
                || n.args
                    .iter()
                    .any(|a| matches!(a, TArg::Type(t) if mentions(t, set)))
        }
        Type::Tuple(ts) | Type::Union(ts) => ts.iter().any(|t| mentions(t, set)),
        Type::Refined { ty, .. } => mentions(ty, set),
        Type::Dyn(_) => true,
        Type::Fn(_) => false,
    }
}

struct Local {
    name: Name,
    ty: Type,
}

struct Loop {
    label: Name,
    pos: Pos,
    entry: Live,
    brk: Option<Live>,
}

struct Jp {
    pos: Pos,
    def: Live,
}

#[derive(Default)]
struct Cx {
    scopes: Vec<Vec<Local>>,
    live: Live,
    loops: Vec<Loop>,
    jps: HashMap<Name, Jp>,
    /// Names bound by `match` patterns, to the scrutinee local they view.
    views: HashMap<Name, Name>,
}

#[derive(Clone, Copy, PartialEq)]
enum Flow {
    Normal,
    Diverged,
}

struct Branch {
    stmts: Vec<Stmt>,
    flow: Flow,
    live: Live,
}

struct Elab<'a> {
    obs: &'a Observable,
    /// Local to the name of its drop flag.
    flags: HashMap<Name, Name>,
    /// Locals found to need a flag this run.
    need: Vec<Name>,
    tmp: usize,
    cx: Cx,
}

fn stmt(kind: StmtKind, span: Span) -> Stmt {
    Stmt { kind, span }
}

fn bool_lit(v: bool, span: Span) -> Rhs {
    Rhs::Atom(Atom::Lit(Literal::Bool(v), span))
}

fn place(root: &str, span: Span) -> Place {
    Place {
        root: root.to_owned(),
        root_span: span,
        projs: Vec::new(),
    }
}

impl Elab<'_> {
    fn flag_on(&mut self, x: &Name) {
        if !self.flags.contains_key(x) && !self.need.contains(x) {
            self.need.push(x.clone());
        }
    }

    fn ty_of(&self, x: &str) -> Option<&Type> {
        self.cx
            .scopes
            .iter()
            .rev()
            .flatten()
            .rev()
            .find(|l| l.name == x)
            .map(|l| &l.ty)
    }

    /// A new scope holding `owned` (bound by the caller) and the statements of `b`.
    fn block(&mut self, b: &Block, owned: Vec<(Name, Type)>) -> Block {
        self.cx.scopes.push(Vec::new());
        let at = b.stmts.first().map(|s| s.span).unwrap_or_default();
        let mut out = Vec::new();
        for (n, ty) in &owned {
            self.declare(n, ty, at, &mut out);
        }
        let mut flow = Flow::Normal;
        for s in &b.stmts {
            if flow == Flow::Diverged {
                break;
            }
            flow = self.stmt(s, &mut out);
        }
        let end = b.stmts.last().map(|s| s.span).unwrap_or_default();
        if flow == Flow::Normal {
            let top = self.cx.scopes.len() - 1;
            self.exit_drops((top, 0), &[], end, &mut out);
        }
        for l in self.cx.scopes.pop().unwrap_or_default() {
            self.cx.live.remove(&l.name);
        }
        Block { stmts: out }
    }

    fn flow_of(&self, b: &Block) -> Flow {
        match b.stmts.last().map(|s| &s.kind) {
            Some(
                StmtKind::Ret(_)
                | StmtKind::Fail(_)
                | StmtKind::Become(_)
                | StmtKind::Jump { .. }
                | StmtKind::Break { .. }
                | StmtKind::Continue { .. }
                | StmtKind::Unreachable,
            ) => Flow::Diverged,
            _ => Flow::Normal,
        }
    }

    fn declare(&mut self, n: &Name, ty: &Type, at: Span, out: &mut Vec<Stmt>) {
        if own::is_copy(ty) {
            return;
        }
        self.cx.scopes.last_mut().expect("in a scope").push(Local {
            name: n.clone(),
            ty: ty.clone(),
        });
        self.cx.live.insert(n.clone(), true);
        if let Some(f) = self.flags.get(n) {
            out.push(stmt(
                StmtKind::Let {
                    mutable: true,
                    pat: Pat::Bind(f.clone()),
                    pat_span: at,
                    ty: Type::Named(Named {
                        path: vec!["bool".into()],
                        args: Vec::new(),
                    }),
                    rhs: bool_lit(true, at),
                    rhs_span: at,
                },
                at,
            ));
        }
    }

    fn declare_pat(&mut self, p: &Pat, ty: &Type, at: Span, out: &mut Vec<Stmt>) {
        match (p, ty) {
            (Pat::Bind(n), _) => self.declare(n, ty, at, out),
            (Pat::Tuple(ps), Type::Tuple(ts)) if ps.len() == ts.len() => {
                for (p, t) in ps.iter().zip(ts) {
                    self.declare_pat(p, t, at, out);
                }
            }
            _ => {}
        }
    }

    /// `drop x`, or `if _dN { drop x }` for a flagged local; nothing when `x` was moved.
    fn drop_stmt(&self, x: &Name, at: Span, out: &mut Vec<Stmt>) {
        let drop = stmt(StmtKind::Drop(x.clone()), at);
        match self.flags.get(x) {
            Some(f) => out.push(stmt(
                StmtKind::If {
                    cond: Atom::Name(f.clone(), at),
                    then: Block { stmts: vec![drop] },
                    els: None,
                },
                at,
            )),
            None if self.cx.live.get(x) == Some(&true) => out.push(drop),
            None => {}
        }
    }

    /// Drops for everything declared at or after `pos`, innermost and latest first, except `skip`.
    fn exit_drops(&self, pos: Pos, skip: &[Name], at: Span, out: &mut Vec<Stmt>) {
        for (si, scope) in self.cx.scopes.iter().enumerate().rev() {
            if si < pos.0 {
                break;
            }
            let from = if si == pos.0 { pos.1 } else { 0 };
            for l in scope[from..].iter().rev() {
                if !skip.contains(&l.name) {
                    self.drop_stmt(&l.name, at, out);
                }
            }
        }
    }

    /// What a function exit drops, in order: the `?` drop list. A flagged local is listed too and
    /// is dropped under its flag.
    fn exit_names(&self, skip: &[Name]) -> Vec<Name> {
        let mut out = Vec::new();
        for scope in self.cx.scopes.iter().rev() {
            for l in scope.iter().rev() {
                let owned =
                    self.flags.contains_key(&l.name) || self.cx.live.get(&l.name) == Some(&true);
                if owned && !skip.contains(&l.name) {
                    out.push(l.name.clone());
                }
            }
        }
        out
    }

    /// What `s` consumes, with a name bound by a `match` pattern standing for the scrutinee it
    /// views: moving the payload out moves the scrutinee.
    fn consumed(&self, s: &StmtKind) -> Vec<Name> {
        own::consumed(s)
            .into_iter()
            .map(|n| self.cx.views.get(n).unwrap_or(n).clone())
            .collect()
    }

    /// Mark the owned locals `s` consumes and return the flagged ones.
    fn consume(&mut self, s: &StmtKind) -> Vec<Name> {
        let mut flagged = Vec::new();
        for n in &self.consumed(s) {
            if let Some(v) = self.cx.live.get_mut(n) {
                *v = false;
                if self.flags.contains_key(n) {
                    flagged.push(n.clone());
                }
            }
        }
        flagged
    }

    fn set_flag(&self, x: &Name, v: bool, at: Span, out: &mut Vec<Stmt>) {
        let f = &self.flags[x];
        out.push(stmt(
            StmtKind::Assign {
                place: place(f, at),
                rhs: bool_lit(v, at),
                rhs_span: at,
            },
            at,
        ));
    }

    fn clear_flags(&self, flagged: Vec<Name>, at: Span, out: &mut Vec<Stmt>) {
        for x in flagged {
            self.set_flag(&x, false, at, out);
        }
    }

    /// Make the current state of the locals in `target` equal to it: drop what the target says
    /// is gone, flag what the target says is still owned.
    fn reconcile(&mut self, target: &Live, at: Span, out: &mut Vec<Stmt>) {
        let mut names: Vec<&Name> = target.keys().collect();
        names.sort();
        for x in names {
            if self.flags.contains_key(x) {
                continue;
            }
            match (target[x], self.cx.live.get(x).copied()) {
                (false, Some(true)) => {
                    if self.ty_of(x).is_some_and(|t| self.obs.of(t)) {
                        self.flag_on(x);
                    } else {
                        self.drop_stmt(x, at, out);
                        self.cx.live.insert(x.clone(), false);
                    }
                }
                (true, Some(false)) => self.flag_on(x),
                _ => {}
            }
        }
    }

    fn restrict(&self, within: &Live) -> Live {
        within
            .keys()
            .filter_map(|k| self.cx.live.get(k).map(|v| (k.clone(), *v)))
            .collect()
    }

    fn closure(&mut self, r: &mut Rhs) {
        if let Rhs::Closure(c) = r {
            let saved = mem::take(&mut self.cx);
            c.body = self.block(&c.body.clone(), owned_params(&c.params));
            self.cx = saved;
        }
    }

    fn run_branches(&mut self, blocks: Vec<Option<&Block>>, pre: &Live) -> Vec<Branch> {
        blocks
            .into_iter()
            .map(|b| {
                self.cx.live = pre.clone();
                match b {
                    Some(b) => {
                        let nb = self.block(b, Vec::new());
                        Branch {
                            flow: self.flow_of(&nb),
                            stmts: nb.stmts,
                            live: self.restrict(pre),
                        }
                    }
                    None => Branch {
                        stmts: Vec::new(),
                        flow: Flow::Normal,
                        live: pre.clone(),
                    },
                }
            })
            .collect()
    }

    /// Join branch end states: a local gone on some paths is dropped at the end of the others.
    fn join(&mut self, bs: &mut [Branch], pre: &Live, at: Span) -> Flow {
        if bs.iter().all(|b| b.flow == Flow::Diverged) {
            self.cx.live = pre.clone();
            return Flow::Diverged;
        }
        let mut names: Vec<&Name> = pre.keys().collect();
        names.sort();
        let mut target = Live::new();
        for x in names {
            let ends: Vec<bool> = bs
                .iter()
                .filter(|b| b.flow == Flow::Normal)
                .map(|b| b.live[x])
                .collect();
            target.insert(x.clone(), ends.iter().all(|v| *v));
        }
        for b in bs.iter_mut().filter(|b| b.flow == Flow::Normal) {
            self.cx.live = mem::take(&mut b.live);
            self.reconcile(&target, at, &mut b.stmts);
        }
        self.cx.live = target;
        Flow::Normal
    }

    fn stmt(&mut self, s: &Stmt, out: &mut Vec<Stmt>) -> Flow {
        let at = s.span;
        let mut new = s.clone();
        let skip = self.consumed(&s.kind);
        match &mut new.kind {
            StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) => {
                self.closure(rhs);
                if let Rhs::Propagate { drops, .. } = rhs {
                    *drops = self.exit_names(&skip);
                }
            }
            _ => {}
        }
        match &s.kind {
            StmtKind::Drop(_) | StmtKind::Reuse { .. } => {
                // checked Yan Core has neither (D2.1); `check` reports it first
                out.push(new);
                Flow::Normal
            }
            StmtKind::Let { pat, ty, rhs, .. } => {
                let flagged = self.consume(&s.kind);
                out.push(new);
                self.clear_flags(flagged, at, out);
                if let (Rhs::Proj(p), Pat::Bind(x)) = (rhs, pat)
                    && !own::is_copy(ty)
                {
                    let root = self.cx.views.get(&p.root).unwrap_or(&p.root).clone();
                    self.cx.views.insert(x.clone(), root);
                } else if let Pat::Bind(x) = pat {
                    self.cx.views.remove(x);
                }
                if !matches!(rhs, Rhs::Proj(_)) {
                    self.declare_pat(pat, ty, at, out);
                }
                Flow::Normal
            }
            StmtKind::Assign { place: p, rhs, .. } => {
                let flagged = self.consume(&s.kind);
                let x = &p.root;
                let ty = self.ty_of(x).cloned();
                match ty {
                    Some(ty) if p.projs.is_empty() && self.cx.live.contains_key(x) => {
                        // The old value goes after the right-hand side is evaluated.
                        if matches!(rhs, Rhs::Atom(Atom::Lit(..))) {
                            self.drop_stmt(x, at, out);
                            out.push(new);
                        } else {
                            self.tmp += 1;
                            let t = format!("_o{}", self.tmp);
                            let StmtKind::Assign {
                                rhs: r, rhs_span, ..
                            } = new.kind
                            else {
                                unreachable!("matched as an assignment")
                            };
                            out.push(stmt(
                                StmtKind::Let {
                                    mutable: false,
                                    pat: Pat::Bind(t.clone()),
                                    pat_span: at,
                                    ty,
                                    rhs: r,
                                    rhs_span,
                                },
                                at,
                            ));
                            self.drop_stmt(x, at, out);
                            out.push(stmt(
                                StmtKind::Assign {
                                    place: p.clone(),
                                    rhs: Rhs::Move(place(&t, at)),
                                    rhs_span,
                                },
                                at,
                            ));
                        }
                        self.cx.live.insert(x.clone(), true);
                        if self.flags.contains_key(x) {
                            self.set_flag(x, true, at, out);
                        }
                    }
                    _ => out.push(new),
                }
                self.clear_flags(flagged, at, out);
                Flow::Normal
            }
            StmtKind::Rhs(_) | StmtKind::Assert(_) | StmtKind::Asm { .. } => {
                let flagged = self.consume(&s.kind);
                out.push(new);
                self.clear_flags(flagged, at, out);
                Flow::Normal
            }
            StmtKind::If { cond, then, els } | StmtKind::ComptimeIf { cond, then, els } => {
                let pre = self.cx.live.clone();
                let mut bs = self.run_branches(vec![Some(then), els.as_ref()], &pre);
                let flow = self.join(&mut bs, &pre, at);
                let (e, t) = (bs.pop().expect("two"), bs.pop().expect("two"));
                let els =
                    (els.is_some() || !e.stmts.is_empty()).then_some(Block { stmts: e.stmts });
                let (then, els) = (Block { stmts: t.stmts }, els);
                let comptime = matches!(s.kind, StmtKind::ComptimeIf { .. });
                let cond = cond.clone();
                out.push(stmt(
                    if comptime {
                        StmtKind::ComptimeIf { cond, then, els }
                    } else {
                        StmtKind::If { cond, then, els }
                    },
                    at,
                ));
                flow
            }
            StmtKind::Match { scrutinee, arms } => {
                let pre = self.cx.live.clone();
                let mut bound = Vec::new();
                if let Atom::Name(x, _) = scrutinee {
                    let root = self.cx.views.get(x).unwrap_or(x).clone();
                    if pre.contains_key(&root) {
                        for (p, _) in arms {
                            own::pat_binders(p, &mut bound);
                        }
                        for n in &bound {
                            self.cx.views.insert(n.clone(), root.clone());
                        }
                    }
                }
                let mut bs = self.run_branches(arms.iter().map(|(_, b)| Some(b)).collect(), &pre);
                for n in &bound {
                    self.cx.views.remove(n);
                }
                let flow = self.join(&mut bs, &pre, at);
                let arms = arms
                    .iter()
                    .zip(bs)
                    .map(|((p, _), b)| (p.clone(), Block { stmts: b.stmts }))
                    .collect();
                out.push(stmt(
                    StmtKind::Match {
                        scrutinee: scrutinee.clone(),
                        arms,
                    },
                    at,
                ));
                flow
            }
            StmtKind::Loop {
                label,
                invariants,
                decreases,
                body,
            } => {
                let entry = self.cx.live.clone();
                self.cx.loops.push(Loop {
                    label: label.clone(),
                    pos: (self.cx.scopes.len(), 0),
                    entry: entry.clone(),
                    brk: None,
                });
                let mut nb = self.block(body, Vec::new());
                if self.flow_of(&nb) == Flow::Normal {
                    self.cx.live = self.restrict(&entry);
                    self.reconcile(&entry, at, &mut nb.stmts);
                }
                let lp = self.cx.loops.pop().expect("pushed above");
                out.push(stmt(
                    StmtKind::Loop {
                        label: label.clone(),
                        invariants: invariants.clone(),
                        decreases: decreases.clone(),
                        body: nb,
                    },
                    at,
                ));
                match lp.brk {
                    Some(l) => {
                        self.cx.live = l;
                        Flow::Normal
                    }
                    None => {
                        self.cx.live = entry;
                        Flow::Diverged
                    }
                }
            }
            StmtKind::ComptimeFor {
                name,
                ty,
                iter,
                body,
            } => {
                let entry = self.cx.live.clone();
                let mut nb = self.block(body, Vec::new());
                if self.flow_of(&nb) == Flow::Normal {
                    self.cx.live = self.restrict(&entry);
                    self.reconcile(&entry, at, &mut nb.stmts);
                }
                self.cx.live = entry;
                out.push(stmt(
                    StmtKind::ComptimeFor {
                        name: name.clone(),
                        ty: ty.clone(),
                        iter: iter.clone(),
                        body: nb,
                    },
                    at,
                ));
                Flow::Normal
            }
            StmtKind::Break { label, .. } | StmtKind::Continue { label } => {
                let skip = self.consumed(&s.kind);
                let Some(i) = self
                    .cx
                    .loops
                    .iter()
                    .rposition(|l| l.label == *label)
                    .or(self.cx.loops.len().checked_sub(1))
                else {
                    out.push(new);
                    return Flow::Diverged;
                };
                let (pos, entry) = (self.cx.loops[i].pos, self.cx.loops[i].entry.clone());
                self.exit_drops(pos, &skip, at, out);
                self.consume(&s.kind);
                self.cx.live = self.restrict(&entry);
                if matches!(s.kind, StmtKind::Break { .. }) {
                    match self.cx.loops[i].brk.clone() {
                        None => self.cx.loops[i].brk = Some(self.cx.live.clone()),
                        Some(t) => self.reconcile(&t, at, out),
                    }
                } else {
                    self.reconcile(&entry, at, out);
                }
                out.push(new);
                Flow::Diverged
            }
            StmtKind::Jp { name, params, body } => {
                let pos = (
                    self.cx.scopes.len() - 1,
                    self.cx.scopes.last().map_or(0, Vec::len),
                );
                let saved = self.cx.live.clone();
                self.cx.jps.insert(
                    name.clone(),
                    Jp {
                        pos,
                        def: saved.clone(),
                    },
                );
                let owned = owned_params(params);
                let nb = self.block(body, owned);
                self.cx.live = saved;
                out.push(stmt(
                    StmtKind::Jp {
                        name: name.clone(),
                        params: params.clone(),
                        body: nb,
                    },
                    at,
                ));
                Flow::Normal
            }
            StmtKind::Jump { name, .. } => {
                let skip = self.consumed(&s.kind);
                let (pos, def) = match self.cx.jps.get(name) {
                    Some(j) => (j.pos, j.def.clone()),
                    None => ((0, 0), Live::new()),
                };
                self.exit_drops(pos, &skip, at, out);
                let flagged = self.consume(&s.kind);
                self.reconcile(&def, at, out);
                self.clear_flags(flagged, at, out);
                out.push(new);
                Flow::Diverged
            }
            StmtKind::Ret(_) | StmtKind::Fail(_) | StmtKind::Become(_) => {
                let skip = self.consumed(&s.kind);
                self.exit_drops((0, 0), &skip, at, out);
                self.consume(&s.kind);
                out.push(new);
                Flow::Diverged
            }
            StmtKind::Unreachable => {
                out.push(new);
                Flow::Diverged
            }
            StmtKind::Yield(_) => {
                let flagged = self.consume(&s.kind);
                out.push(new);
                self.clear_flags(flagged, at, out);
                Flow::Normal
            }
            StmtKind::With {
                handler,
                name,
                dyn_,
                body,
            } => {
                let flagged = self.consume(&s.kind);
                let nb = self.block(body, Vec::new());
                let flow = self.flow_of(&nb);
                out.push(stmt(
                    StmtKind::With {
                        name: name.clone(),
                        dyn_: *dyn_,
                        handler: handler.clone(),
                        body: nb,
                    },
                    at,
                ));
                self.clear_flags(flagged, at, out);
                flow
            }
            StmtKind::Deadline { at: when, body } => {
                let nb = self.block(body, Vec::new());
                let flow = self.flow_of(&nb);
                out.push(stmt(
                    StmtKind::Deadline {
                        at: when.clone(),
                        body: nb,
                    },
                    at,
                ));
                flow
            }
            StmtKind::Unsafe(body) => {
                let nb = self.block(body, Vec::new());
                let flow = self.flow_of(&nb);
                out.push(stmt(StmtKind::Unsafe(nb), at));
                flow
            }
            StmtKind::Par { concurrent, blocks } => {
                let blocks = blocks.iter().map(|b| self.block(b, Vec::new())).collect();
                out.push(stmt(
                    StmtKind::Par {
                        concurrent: *concurrent,
                        blocks,
                    },
                    at,
                ));
                Flow::Normal
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elab(src: &str) -> String {
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let out = elaborate(&m.expect("parses")).expect("elaborates");
        let diags = yan_check::check_optimized("t.yir", &out);
        assert!(diags.is_empty(), "{diags:?}\n{}", yan_core::print(&out));
        yan_core::print(&out)
    }

    fn has(text: &str, lines: &[&str]) {
        let got: Vec<&str> = text.lines().map(str::trim).collect();
        let mut at = 0;
        for l in lines {
            let Some(i) = got[at..].iter().position(|g| g == l) else {
                panic!("`{l}` missing or out of order in\n{text}")
            };
            at += i + 1;
        }
    }

    #[test]
    fn scope_exit_in_reverse_order_and_before_ret() {
        let t = elab(
            "module t\nfn f(let c: bool) -> Str {\n var s: Str = \"a\"\n var u: Str = \"b\"\n\
             if c {\n  let k: Str = std.text.concat(let s, let u)\n  ret k\n }\n ret s\n}\n",
        );
        has(
            &t,
            &[
                "let k: Str = std.text.concat(let s, let u)",
                "drop u",
                "drop s",
                "ret k",
            ],
        );
        has(&t, &["}", "drop u", "ret s"]);
    }

    #[test]
    fn assignment_drops_the_old_value_after_the_right_hand_side() {
        let t = elab(
            "module t\nfn f(let a: Str) -> Str {\n var s: Str = \"a\"\n\
             s = std.text.concat(let s, let a)\n ret s\n}\n",
        );
        has(
            &t,
            &[
                "let _o1: Str = std.text.concat(let s, let a)",
                "drop s",
                "s = move _o1",
                "ret s",
            ],
        );
    }

    #[test]
    fn a_conditional_move_drops_on_the_branch_edge() {
        let t = elab(
            "module t\nfn g(sink s: Str, let c: bool) -> Unit {\n if c {\n  let t: Str = move s\n  ret ()\n }\n ret ()\n}\n\
             fn h(sink s: Str, let c: bool) -> Unit {\n if c {\n  let t: Str = move s\n  let n: i64 = 1\n }\n ret ()\n}\n",
        );
        has(
            &t,
            &[
                "let t: Str = move s",
                "drop t",
                "ret ()",
                "}",
                "drop s",
                "ret ()",
            ],
        );
        assert!(!t.contains("_d0"), "{t}");
    }

    #[test]
    fn a_type_with_a_custom_drop_gets_a_flag() {
        let t = elab(
            "module t\nrecord Guard { n: i64 }\nimpl Drop for Guard {\n fn drop(inout self: Guard) -> Unit {\n  ret ()\n }\n}\n\
             fn take(sink g: Guard) -> Unit {\n ret ()\n}\n\
             fn f(sink g: Guard, let c: bool) -> Unit {\n if c {\n  let u: Unit = take(sink g)\n }\n ret ()\n}\n",
        );
        has(
            &t,
            &[
                "var _d0: bool = true",
                "let u: Unit = take(sink g)",
                "_d0 = false",
                "if _d0 {",
                "drop g",
                "}",
                "ret ()",
            ],
        );
    }

    #[test]
    fn propagation_lists_the_owned_locals_for_its_failing_edge() {
        let t = elab(
            "module t\nrecord E { n: i64 }\nfn g(let a: Str) -> i64 fail E {\n ret 1\n}\n\
             fn f(let c: bool) -> i64 fail E {\n let s: Str = \"a\"\n let u: Str = \"b\"\n\
             let x: i64 = g(let s)?\n ret x\n}\n",
        );
        has(&t, &["let x: i64 = g(let s)? drop u, s"]);
        has(&t, &["drop u", "drop s", "ret x"]);
    }

    #[test]
    fn a_result_payload_bound_by_match_is_not_dropped_twice() {
        let t = elab(
            "module t\nrecord Bad { pub code: i64 }\nfn mk(let x: i64) -> Str fail Bad {\n let s: Str = \"a\"\n ret s\n}\n\
             fn f() -> Str {\n let r: Result[Str, Bad] = try mk(let 1)\n\
             match r {\n Ok { value: v } => {\n  ret v\n }\n Err { error: e } => {\n  let z: Str = \"e\"\n  ret z\n }\n }\n}\n",
        );
        has(&t, &["Ok { value: v } => {", "ret v"]);
        has(
            &t,
            &[
                "Err { error: e } => {",
                "let z: Str = \"e\"",
                "drop r",
                "ret z",
            ],
        );
        assert_eq!(t.matches("drop r").count(), 1, "{t}");
    }

    #[test]
    fn a_jump_out_of_a_result_match_clears_the_scrutinee_flag() {
        let t = elab(
            "module t\nrecord Bad { pub code: i64 }\nfn mk(let x: i64) -> Str fail Bad {\n let s: Str = \"a\"\n ret s\n}\n\
             fn f() -> Unit {\n let r: Result[Str, Bad] = try mk(let 1)\n\
             jp k(let v: Str) {\n  let a: Str = move v\n  ret ()\n }\n\
             match r {\n Ok { value: o } => {\n  jump k(o)\n }\n Err { error: e } => {\n  let z: Str = \"e\"\n  jump k(z)\n }\n }\n}\n",
        );
        has(&t, &["Ok { value: o } => {", "_d0 = false", "jump k(o)"]);
    }

    #[test]
    fn break_and_continue_drop_the_loop_body_locals() {
        let t = elab(
            "module t\nfn f(let c: bool) -> Unit {\n loop 'a {\n  let s: Str = \"x\"\n  if c {\n   break 'a\n  }\n  continue 'a\n }\n ret ()\n}\n",
        );
        has(&t, &["drop s", "break 'a"]);
        has(&t, &["drop s", "continue 'a"]);
    }

    #[test]
    fn the_check_rejects_a_missing_and_a_repeated_drop() {
        let bad = |src: &str| {
            let (m, d) = yan_core::parse("t.yir", src);
            assert!(d.is_empty(), "{d:?}");
            yan_check::check_optimized("t.yir", &m.expect("parses"))
        };
        let leak = bad("module t\nfn f() -> Unit {\n let s: Str = \"a\"\n ret ()\n}\n");
        assert_eq!(leak.len(), 1, "{leak:?}");
        let twice =
            bad("module t\nfn f() -> Unit {\n let s: Str = \"a\"\n drop s\n drop s\n ret ()\n}\n");
        assert_eq!(twice.len(), 1, "{twice:?}");
        assert!(
            bad("module t\nfn f() -> Unit {\n let s: Str = \"a\"\n drop s\n ret ()\n}\n")
                .is_empty()
        );
    }

    #[test]
    fn the_check_takes_each_field_of_a_destructured_tuple_once() {
        let bad = |body: &str| {
            let src = format!(
                "module t\nfn f(sink p: Tuple(Str, Str)) -> Unit {{\n let a: Str = p._0\n let b: Str = p._1\n{body} ret ()\n}}\n"
            );
            let (m, d) = yan_core::parse("t.yir", &src);
            assert!(d.is_empty(), "{d:?}");
            yan_check::check_optimized("t.yir", &m.expect("parses"))
        };
        assert!(bad(" var x: Str = move a\n var y: Str = move b\n drop y\n drop x\n").is_empty());
        let twice = bad(" var x: Str = move a\n var y: Str = move a\n drop y\n drop x\n");
        assert_eq!(twice.len(), 1, "{twice:?}");
        let whole = bad(" var x: Str = move a\n drop p\n drop x\n");
        assert_eq!(whole.len(), 1, "{whole:?}");
    }
}
