//! Drop elaboration (R770, 5.3 step 1): after this pass every owned, non-copy local is consumed
//! or dropped exactly once on every path.
//!
//! Ownership comes from the shared owned fact (RFC 0054, [`crate::owned`]): a local is dropped
//! where it is must-owned. Where control flow meets, a local some incoming path owns and the
//! meeting point does not must-own is dropped on that path's edge.
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
use crate::owned::{Facts, Key, addr, owned_params, pat_decls};
use yan_flow::Binding;
/// A point in the scope stack: locals of scope `.0` from index `.1` on, and every deeper scope.
type Pos = (usize, usize);

pub fn elaborate(m: &Module) -> Result<Module, Error> {
    let obs = Observable::new(m);
    let copy = own::copy_fields(m);
    let mut out = m.clone();
    for it in &mut out.items {
        match &mut it.kind {
            ItemKind::Fn(f) | ItemKind::Inst(Inst { def: f, .. }) => {
                if let Some(b) = &f.body {
                    f.body = Some(settle(&obs, &copy, &f.params, b)?);
                }
            }
            ItemKind::Impl(i) => {
                for f in &mut i.fns {
                    if let Some(b) = &f.body {
                        f.body = Some(settle(&obs, &copy, &f.params, b)?);
                    }
                }
            }
            ItemKind::Test(t) => t.body = settle(&obs, &copy, &[], &t.body)?,
            _ => {}
        }
    }
    Ok(out)
}

/// Run the pass over one body until the set of flagged locals stops growing.
fn settle(
    obs: &Observable,
    copy: &own::CopyFields,
    params: &[Param],
    body: &Block,
) -> Result<Block, Error> {
    let body = &crate::joins::split(body);
    let facts = Facts::new(params, body, copy);
    let mut flags: HashMap<Name, Name> = HashMap::new();
    for _ in 0..8 {
        let mut e = Elab {
            obs,
            copy,
            facts: &facts,
            flags: flags.clone(),
            need: Vec::new(),
            tmp: 0,
            resumed: HashMap::new(),
            ends: HashSet::new(),
            unsupported: None,
            cx: Cx::default(),
        };
        let b = e.block(body, owned_params(params));
        if let Some(what) = e.unsupported {
            return Err(Error::Unsupported(what.into()));
        }
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
    id: Binding,
}

struct Loop {
    label: Name,
    pos: Pos,
    key: usize,
    broken: bool,
}

struct Jp {
    pos: Pos,
    key: usize,
    /// Address of the body block, and whether it falls through (R764).
    body: usize,
    falls: bool,
    /// The statement of the declaring block that its jumps come from, once one has.
    cont: Option<usize>,
    hoisted: bool,
    /// Join points of the same block its body jumps to.
    calls: Vec<Name>,
}

/// The statement being walked in a scope of the stack.
#[derive(Clone, Copy)]
struct Top {
    stmt: usize,
    is_jp: bool,
    last: bool,
}

#[derive(Default)]
struct Cx {
    scopes: Vec<Vec<Local>>,
    loops: Vec<Loop>,
    jps: HashMap<Name, Jp>,
    /// Per scope, the statement of its block being walked.
    tops: Vec<Top>,
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
    end: Key,
}

struct Elab<'a> {
    obs: &'a Observable,
    copy: &'a own::CopyFields,
    facts: &'a Facts,
    /// Local to the name of its drop flag.
    flags: HashMap<Name, Name>,
    /// Locals found to need a flag this run.
    need: Vec<Name>,
    tmp: usize,
    /// Blocks whose end is reached.
    ends: HashSet<usize>,
    /// Statements some jump inside them left for a falling-through join point of their block,
    /// with the names of those join points.
    resumed: HashMap<usize, Vec<Name>>,
    /// Set when the pass would silently drop code (R764 flow it does not model).
    unsupported: Option<&'static str>,
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

fn rhs_mut(k: &mut StmtKind) -> Option<&mut Rhs> {
    match k {
        StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) => Some(rhs),
        _ => None,
    }
}

impl Elab<'_> {
    fn flag_on(&mut self, x: &Name) {
        if !self.flags.contains_key(x) && !self.need.contains(x) {
            self.need.push(x.clone());
        }
    }

    fn local(&self, x: &str) -> Option<&Local> {
        self.cx
            .scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|l| l.name == x)
    }

    fn owned(&self, k: Key, x: &str) -> bool {
        self.local(x).is_some_and(|l| self.facts.must(k, l.id))
    }

    fn top(&self) -> Pos {
        (self.cx.scopes.len(), 0)
    }

    /// A new scope holding `owned` (bound by the caller) and the statements of `b`.
    fn block(&mut self, b: &Block, owned: Vec<(Name, Type)>) -> Block {
        self.cx.scopes.push(Vec::new());
        let at = b.stmts.first().map(|s| s.span).unwrap_or_default();
        let mut out = Vec::new();
        for (n, ty) in &owned {
            self.declare(n, ty, addr(b), at, &mut out);
        }
        let mut flow = Flow::Normal;
        let top = self.cx.scopes.len() - 1;
        self.cx.tops.truncate(top);
        self.cx.tops.push(Top {
            stmt: 0,
            is_jp: false,
            last: false,
        });
        for (i, s) in b.stmts.iter().enumerate() {
            if flow == Flow::Diverged {
                break;
            }
            self.cx.tops[top] = Top {
                stmt: addr(s),
                is_jp: matches!(s.kind, StmtKind::Jp { .. }),
                last: i + 1 == b.stmts.len(),
            };
            flow = self.stmt(s, &mut out);
            if let Some(jps) = self.resumed.remove(&addr(s)) {
                // R764: what follows `s` runs after the bodies of the join points it jumped to.
                flow = Flow::Normal;
                self.resume(&jps, s, &mut out);
            }
        }
        let end = b.stmts.last().map(|s| s.span).unwrap_or_default();
        let k = addr(b);
        if flow == Flow::Normal {
            self.exit_drops((top, 0), &[], end, Key::End(k), &mut out);
            self.ends.insert(k);
        }
        self.cx.scopes.pop();
        self.cx.tops.truncate(top);
        Block { stmts: out }
    }

    /// The bodies of `jps` end where `after` begins: what they own and it does not is dropped
    /// at their end.
    fn resume(&mut self, jps: &[Name], after: &Stmt, out: &mut [Stmt]) {
        let pos = self.here();
        for name in jps {
            let Some(body) = self.cx.jps.get(name).map(|j| j.body) else {
                continue;
            };
            let mut tail = Vec::new();
            self.edge(
                Key::End(body),
                Key::After(addr(after)),
                pos,
                after.span,
                &mut tail,
            );
            for st in out.iter_mut() {
                if let StmtKind::Jp { name: n, body, .. } = &mut st.kind
                    && n == name
                {
                    body.stmts.append(&mut tail);
                    break;
                }
            }
        }
    }

    /// A `par` branch: what its top-level join points bind is visible after the `par`, so their
    /// bodies declare into the enclosing scope.
    fn hoisted_block(&mut self, b: &Block) -> Block {
        let mut out = Vec::new();
        for s in &b.stmts {
            match &s.kind {
                StmtKind::Jp { name, params, body } => {
                    self.cx.jps.insert(
                        name.clone(),
                        Jp {
                            pos: self.here(),
                            key: addr(s),
                            body: addr(body),
                            falls: false,
                            cont: None,
                            hoisted: true,
                            calls: Vec::new(),
                        },
                    );
                    let mut nb = Vec::new();
                    for (n, ty) in owned_params(params) {
                        self.declare(&n, &ty, addr(body), s.span, &mut nb);
                    }
                    for s in &body.stmts {
                        self.stmt(s, &mut nb);
                    }
                    // What the body binds is not yet bound where the branch jumps to it, so
                    // those jumps drop only what the branch bound after the join point.
                    let here = self.here();
                    if let Some(jp) = self.cx.jps.get_mut(name) {
                        jp.pos = here;
                    }
                    out.push(stmt(
                        StmtKind::Jp {
                            name: name.clone(),
                            params: params.to_vec(),
                            body: Block { stmts: nb },
                        },
                        s.span,
                    ));
                }
                _ => {
                    self.stmt(s, &mut out);
                }
            }
        }
        Block { stmts: out }
    }

    fn here(&self) -> Pos {
        (
            self.cx.scopes.len() - 1,
            self.cx.scopes.last().map_or(0, Vec::len),
        )
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

    fn declare(&mut self, n: &Name, ty: &Type, site: usize, at: Span, out: &mut Vec<Stmt>) {
        if own::is_copy(ty) {
            return;
        }
        self.cx.scopes.last_mut().expect("in a scope").push(Local {
            name: n.clone(),
            ty: ty.clone(),
            id: self.facts.binding(site, n),
        });
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

    /// `drop x`, or `if _dN { drop x }` for a flagged local; nothing when `x` is not owned at `k`.
    fn drop_stmt(&self, x: &Name, at: Span, k: Key, out: &mut Vec<Stmt>) {
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
            None if self.owned(k, x) => out.push(drop),
            None => {}
        }
    }

    /// Drops for everything declared at or after `pos`, innermost and latest first, except `skip`.
    fn exit_drops(&self, pos: Pos, skip: &[Name], at: Span, k: Key, out: &mut Vec<Stmt>) {
        for (si, scope) in self.cx.scopes.iter().enumerate().rev() {
            if si < pos.0 {
                break;
            }
            let from = if si == pos.0 { pos.1 } else { 0 };
            for l in scope[from..].iter().rev() {
                if !skip.contains(&l.name) {
                    self.drop_stmt(&l.name, at, k, out);
                }
            }
        }
    }

    /// What a function exit drops, in order: the `?` drop list. A flagged local is listed too and
    /// is dropped under its flag.
    fn exit_names(&self, skip: &[Name], k: Key) -> Vec<Name> {
        let mut out = Vec::new();
        for scope in self.cx.scopes.iter().rev() {
            for l in scope.iter().rev() {
                let owned = self.flags.contains_key(&l.name) || self.facts.must(k, l.id);
                if owned && !skip.contains(&l.name) {
                    out.push(l.name.clone());
                }
            }
        }
        out
    }

    /// Control goes from `from` to `to`, where the locals declared before `pos` are in scope. A
    /// local `from` owns and `to` does not must-own is dropped on this edge, or gets a flag when
    /// its drop is observable (5.4): the drop must then wait for the scope's end.
    fn edge(&mut self, from: Key, to: Key, pos: Pos, at: Span, out: &mut Vec<Stmt>) {
        let mut seen = HashSet::new();
        let mut visible: Vec<(Name, Binding, bool)> = Vec::new();
        for (si, scope) in self.cx.scopes.iter().enumerate().rev() {
            if si > pos.0 {
                continue;
            }
            let upto = if si == pos.0 { pos.1 } else { scope.len() };
            for l in scope[..upto.min(scope.len())].iter().rev() {
                if seen.insert(l.name.clone()) {
                    visible.push((l.name.clone(), l.id, self.obs.of(&l.ty)));
                }
            }
        }
        visible.sort_by(|a, b| a.0.cmp(&b.0));
        for (x, id, obs) in visible {
            if self.flags.contains_key(&x) {
                continue;
            }
            if self.facts.must(to, id) {
                continue;
            }
            if self.facts.must(from, id) {
                if obs {
                    self.flag_on(&x);
                } else {
                    out.push(stmt(StmtKind::Drop(x), at));
                }
            } else if obs && self.facts.may(to, id) {
                self.flag_on(&x);
            }
        }
    }

    /// What `s` consumes, with a name bound by a `match` pattern standing for the scrutinee it
    /// views: moving the payload out moves the scrutinee.
    fn consumed(&self, s: &StmtKind) -> Vec<Name> {
        own::consumed(s)
            .into_iter()
            .map(|n| self.cx.views.get(n).unwrap_or(n).clone())
            .collect()
    }

    /// The flagged locals `s` consumes.
    fn consume(&self, s: &StmtKind) -> Vec<Name> {
        self.consumed(s)
            .into_iter()
            .filter(|n| self.flags.contains_key(n) && self.local(n).is_some())
            .collect()
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

    /// Elaborates the body of a closure in `orig` into its copy `r`.
    fn closure(&mut self, r: &mut Rhs, orig: &Rhs) {
        if let (Rhs::Closure(c), Rhs::Closure(o)) = (r, orig) {
            let saved = mem::take(&mut self.cx);
            c.body = self.block(&o.body, owned_params(&o.params));
            self.cx = saved;
        }
    }

    fn run_branches(&mut self, blocks: Vec<Option<&Block>>, fall: Key) -> Vec<Branch> {
        blocks
            .into_iter()
            .map(|b| match b {
                Some(b) => {
                    let nb = self.block(b, Vec::new());
                    Branch {
                        flow: self.flow_of(&nb),
                        stmts: nb.stmts,
                        end: Key::End(addr(b)),
                    }
                }
                None => Branch {
                    stmts: Vec::new(),
                    flow: Flow::Normal,
                    end: fall,
                },
            })
            .collect()
    }

    /// The branches meet at `to`.
    fn join(&mut self, bs: &mut [Branch], to: Key, at: Span) -> Flow {
        if bs.iter().all(|b| b.flow == Flow::Diverged) {
            return Flow::Diverged;
        }
        for b in bs.iter_mut().filter(|b| b.flow == Flow::Normal) {
            self.edge(b.end, to, self.top(), at, &mut b.stmts);
        }
        Flow::Normal
    }

    fn stmt(&mut self, s: &Stmt, out: &mut Vec<Stmt>) -> Flow {
        let at = s.span;
        let key = addr(s);
        let mid = Key::Mid(key);
        let mut new = s.clone();
        let skip = self.consumed(&s.kind);
        if let (Some(rhs), Some(orig)) = (rhs_mut(&mut new.kind), crate::owned::rhs_of(&s.kind)) {
            self.closure(rhs, orig);
            if let Rhs::Propagate { drops, .. } = rhs {
                *drops = self.exit_names(&skip, mid);
            }
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
                    let mut ds = Vec::new();
                    pat_decls(pat, ty, &mut ds);
                    for (n, t) in ds {
                        self.declare(n, t, key, at, out);
                    }
                }
                Flow::Normal
            }
            StmtKind::Assign { place: p, rhs, .. } => {
                let flagged = self.consume(&s.kind);
                let x = &p.root;
                match self.local(x).map(|l| l.ty.clone()) {
                    Some(ty) if p.projs.is_empty() => {
                        // The old value goes after the right-hand side is evaluated.
                        if matches!(rhs, Rhs::Atom(Atom::Lit(..))) {
                            self.drop_stmt(x, at, mid, out);
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
                            self.drop_stmt(x, at, mid, out);
                            out.push(stmt(
                                StmtKind::Assign {
                                    place: p.clone(),
                                    rhs: Rhs::Move(place(&t, at)),
                                    rhs_span,
                                },
                                at,
                            ));
                        }
                        if self.flags.contains_key(x) {
                            self.set_flag(x, true, at, out);
                        }
                    }
                    _ => out.push(new),
                }
                self.clear_flags(flagged, at, out);
                Flow::Normal
            }
            StmtKind::Rhs(_) | StmtKind::Assert(_) | StmtKind::Asm { .. } | StmtKind::Yield(_) => {
                let flagged = self.consume(&s.kind);
                out.push(new);
                self.clear_flags(flagged, at, out);
                Flow::Normal
            }
            StmtKind::If { cond, then, els } | StmtKind::ComptimeIf { cond, then, els } => {
                let mut bs = self.run_branches(vec![Some(then), els.as_ref()], Key::Fall(key));
                let flow = self.join(&mut bs, Key::Join(key), at);
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
                let mut bound = Vec::new();
                if let Atom::Name(x, _) = scrutinee {
                    let root = self.cx.views.get(x).unwrap_or(x).clone();
                    if self.local(&root).is_some() {
                        for (p, _) in arms {
                            own::view_binders(p, self.copy, &mut bound);
                        }
                        for n in &bound {
                            self.cx.views.insert(n.clone(), root.clone());
                        }
                    }
                }
                let mut bs =
                    self.run_branches(arms.iter().map(|(_, b)| Some(b)).collect(), Key::Fall(key));
                for n in &bound {
                    self.cx.views.remove(n);
                }
                let flow = self.join(&mut bs, Key::Join(key), at);
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
                self.enter_loop(key);
                self.cx.loops.push(Loop {
                    label: label.clone(),
                    pos: self.top(),
                    key,
                    broken: false,
                });
                let mut nb = self.block(body, Vec::new());
                if self.flow_of(&nb) == Flow::Normal {
                    self.edge(
                        Key::End(addr(body)),
                        Key::Head(key),
                        self.top(),
                        at,
                        &mut nb.stmts,
                    );
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
                if lp.broken {
                    Flow::Normal
                } else {
                    Flow::Diverged
                }
            }
            StmtKind::ComptimeFor {
                name,
                ty,
                iter,
                body,
            } => {
                self.enter_loop(key);
                let mut nb = self.block(body, Vec::new());
                if self.flow_of(&nb) == Flow::Normal {
                    self.edge(
                        Key::End(addr(body)),
                        Key::Head(key),
                        self.top(),
                        at,
                        &mut nb.stmts,
                    );
                }
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
                let (pos, lk) = (self.cx.loops[i].pos, self.cx.loops[i].key);
                self.exit_drops(pos, &skip, at, mid, out);
                let to = if matches!(s.kind, StmtKind::Break { .. }) {
                    self.cx.loops[i].broken = true;
                    Key::Join(lk)
                } else {
                    Key::Head(lk)
                };
                self.edge(mid, to, pos, at, out);
                out.push(new);
                Flow::Diverged
            }
            StmtKind::Jp { name, params, body } => {
                self.cx.jps.insert(
                    name.clone(),
                    Jp {
                        pos: self.here(),
                        key,
                        body: addr(body),
                        falls: false,
                        cont: None,
                        hoisted: false,
                        calls: Vec::new(),
                    },
                );
                let nb = self.block(body, owned_params(params));
                if let Some(jp) = self.cx.jps.get_mut(name) {
                    jp.falls = self.ends.contains(&addr(body));
                }
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
                self.resume_point(name);
                let jp = self.cx.jps.get(name).map(|j| (j.pos, j.key));
                self.exit_drops(jp.map_or((0, 0), |j| j.0), &skip, at, mid, out);
                let flagged = self.consume(&s.kind);
                if let Some((pos, jk)) = jp {
                    self.edge(mid, Key::Join(jk), pos, at, out);
                }
                self.clear_flags(flagged, at, out);
                out.push(new);
                Flow::Diverged
            }
            StmtKind::Ret(_) | StmtKind::Fail(_) | StmtKind::Become(_) => {
                self.exit_drops((0, 0), &skip, at, mid, out);
                out.push(new);
                Flow::Diverged
            }
            StmtKind::Unreachable => {
                out.push(new);
                Flow::Diverged
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
                let blocks = blocks.iter().map(|b| self.hoisted_block(b)).collect();
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

    /// A jump to a join point, directly or from the body of a sibling, whose body falls through:
    /// the statement of the declaring block that holds the jump is where control resumes. The
    /// jump of a sibling body is accounted for where its own join point is jumped to. Only one
    /// such statement is modelled per join point (`joins` copies the others), and locals the
    /// block declared after the join point are dead there.
    fn resume_point(&mut self, name: &Name) {
        let Some(jp) = self.cx.jps.get(name).filter(|j| !j.hoisted) else {
            return;
        };
        let (si, seen) = jp.pos;
        let top = self.cx.tops[si];
        if top.is_jp {
            let from = self
                .cx
                .jps
                .iter_mut()
                .find(|(_, j)| j.key == top.stmt && !j.hoisted);
            if let Some((_, from)) = from
                && !from.calls.contains(name)
            {
                from.calls.push(name.clone());
            }
            return;
        }
        let mut reached = vec![name.clone()];
        let mut i = 0;
        while i < reached.len() {
            let calls = self.cx.jps.get(&reached[i]).map(|j| j.calls.clone());
            for c in calls.into_iter().flatten() {
                if !reached.contains(&c) {
                    reached.push(c);
                }
            }
            i += 1;
        }
        let late = self.cx.scopes[si].len() > seen;
        for n in reached {
            let Some(jp) = self.cx.jps.get(&n).filter(|j| j.falls && !j.hoisted) else {
                continue;
            };
            let unsupported = if jp.cont.is_some_and(|c| c != top.stmt) {
                Some("a falling-through join point jumped to from two statements")
            } else if late && !top.last {
                Some("owned locals declared between a falling-through join point and its jump")
            } else {
                None
            };
            if unsupported.is_some() {
                self.unsupported = unsupported;
                return;
            }
            let names = self.resumed.entry(top.stmt).or_default();
            if !names.contains(&n) {
                names.push(n.clone());
            }
            if let Some(jp) = self.cx.jps.get_mut(&n) {
                jp.cont = Some(top.stmt);
            }
        }
    }

    /// A local owned on entry to a loop but not on every way back to its head is dropped where
    /// the loop body loses it, which the entry edge cannot know: it gets a flag.
    fn enter_loop(&mut self, key: usize) {
        let lost: Vec<Name> = self
            .cx
            .scopes
            .iter()
            .flatten()
            .filter(|l| {
                self.facts.must(Key::Mid(key), l.id) && !self.facts.must(Key::Head(key), l.id)
            })
            .map(|l| l.name.clone())
            .collect();
        for x in lost {
            self.flag_on(&x);
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
    fn a_jump_that_owns_a_plain_value_drops_it_on_the_edge() {
        let t = elab(
            "module t\nrecord Bad { pub code: i64 }\nfn mk(let x: i64) -> Str fail Bad {\n let s: Str = \"a\"\n ret s\n}\n\
             fn f() -> Unit {\n let r: Result[Str, Bad] = try mk(let 1)\n\
             jp k(let v: Str) {\n  let a: Str = move v\n  ret ()\n }\n\
             match r {\n Ok { value: o } => {\n  jump k(o)\n }\n Err { error: e } => {\n  let z: Str = \"e\"\n  jump k(z)\n }\n }\n}\n",
        );
        assert!(!t.contains("_d0"), "{t}");
        has(&t, &["Err { error: e } => {", "drop r", "jump k(z)"]);
    }

    #[test]
    fn a_jump_that_owns_a_custom_drop_value_leaves_it_to_a_flag() {
        let t = elab(
            "module t\nrecord Guard { n: i64 }\nimpl Drop for Guard {\n fn drop(inout self: Guard) -> Unit {\n  ret ()\n }\n}\n\
             fn take(sink g: Guard) -> Unit {\n ret ()\n}\n\
             fn f(sink g: Guard, let c: bool) -> Unit {\n jp k() {\n  ret ()\n }\n\
             if c {\n  let u: Unit = take(sink g)\n  jump k()\n }\n jump k()\n}\n",
        );
        has(
            &t,
            &[
                "var _d0: bool = true",
                "jp k() {",
                "if _d0 {",
                "drop g",
                "ret ()",
            ],
        );
        has(
            &t,
            &["let u: Unit = take(sink g)", "_d0 = false", "jump k()"],
        );
    }

    #[test]
    fn code_after_a_jump_to_a_falling_through_join_point_is_kept() {
        let t = elab(
            "module t\nfn f(let c: bool) -> Unit {\n let s: Str = \"a\"\n jp k() {\n  let w: Str = \"c\"\n }\n\
             if c { jump k() } else { jump k() }\n let x: Str = \"d\"\n ret ()\n}\n",
        );
        has(&t, &["let w: Str = \"c\"", "drop w", "}"]);
        has(&t, &["let x: Str = \"d\"", "drop x", "drop s", "ret ()"]);
        assert_eq!(t.matches("drop s").count(), 1, "{t}");
    }

    #[test]
    fn a_falling_through_body_that_consumes_an_owned_local_leaves_it_gone_after() {
        let t = elab(
            "module t\nfn take(sink p: Str) -> Unit {\n ret ()\n}\n\
             fn f(let c: bool) -> Unit {\n let s: Str = \"a\"\n jp k() {\n  let u: Unit = take(sink s)\n }\n\
             if c { jump k() } else { jump k() }\n let x: Str = \"d\"\n ret ()\n}\n",
        );
        assert_eq!(t.matches("drop s").count(), 0, "{t}");
        assert_eq!(t.matches("drop x").count(), 1, "{t}");
    }

    #[test]
    fn owned_locals_between_a_falling_through_join_point_and_code_after_it_live_on() {
        let t = elab(
            "module t\nfn f(let c: bool) -> Unit {\n jp k() {\n  let w: Str = \"c\"\n }\n\
             let y: Str = \"b\"\n if c { jump k() } else { jump k() }\n let x: Str = \"d\"\n ret ()\n}\n",
        );
        has(&t, &["let x: Str = \"d\"", "drop x", "drop y", "ret ()"]);
        assert_eq!(t.matches("drop y").count(), 1, "{t}");
        assert_eq!(t.matches("drop w").count(), 1, "{t}");
    }

    #[test]
    fn a_join_point_jumped_to_from_two_statements_is_copied_per_statement() {
        let t = elab(
            "module t\nfn f(let c: bool) -> Unit {\n let s: Str = \"a\"\n jp k() {\n  let w: Str = \"c\"\n }\n\
             if c { jump k() } else { jump k() }\n let y: Str = \"b\"\n if c { jump k() } else { jump k() }\n let x: Str = \"d\"\n ret ()\n}\n",
        );
        assert_eq!(t.matches("drop w").count(), 2, "{t}");
        assert_eq!(t.matches("drop s").count(), 1, "{t}");
        assert_eq!(t.matches("drop y").count(), 1, "{t}");
        assert_eq!(t.matches("drop x").count(), 1, "{t}");
    }

    #[test]
    fn a_falling_through_join_point_jumped_to_from_a_sibling_resumes_after_the_sibling_jump() {
        let t = elab(
            "module t\nfn f(let c: bool) -> Unit {\n let s: Str = \"a\"\n jp k() {\n  let w: Str = \"c\"\n }\n\
             jp m() {\n  jump k()\n }\n if c { jump m() } else { jump k() }\n let x: Str = \"d\"\n ret ()\n}\n",
        );
        has(&t, &["let w: Str = \"c\"", "drop w", "}"]);
        assert_eq!(t.matches("drop s").count(), 1, "{t}");
        assert_eq!(t.matches("drop x").count(), 1, "{t}");
    }

    #[test]
    fn the_check_verifies_code_after_a_jump_to_a_falling_through_join_point() {
        let src = "module t\nfn f(let c: bool) -> Unit {\n let s: Str = \"a\"\n jp k() {\n  let w: Str = \"c\"\n  drop w\n }\n\
                   if c { jump k() } else { jump k() }\n let x: Str = \"d\"\n drop s\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let diags = yan_check::check_optimized("t.yir", &m.expect("parses"));
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn a_join_point_that_falls_through_drops_what_its_block_declared_before_it() {
        let t = elab(
            "module t\nfn f(let c: bool) -> Unit {\n if c {\n  let s: Str = \"a\"\n\
             jp k() {\n   let u: Str = \"b\"\n   jp m() {\n    let w: Str = \"c\"\n   }\n   jump m()\n  }\n\
             let x: Str = \"d\"\n  jump k()\n }\n ret ()\n}\n",
        );
        has(
            &t,
            &[
                "let w: Str = \"c\"",
                "drop w",
                "}",
                "jump m()",
                "drop u",
                "}",
            ],
        );
        has(
            &t,
            &[
                "let x: Str = \"d\"",
                "drop x",
                "jump k()",
                "drop s",
                "}",
                "ret ()",
            ],
        );
        assert_eq!(t.matches("drop s").count(), 1, "{t}");
        assert_eq!(t.matches("drop x").count(), 1, "{t}");
    }

    #[test]
    fn a_par_branch_else_binding_lives_to_the_end_of_the_scope() {
        let t = elab(
            "module t\nrecord Bad { pub code: i64 }\nfn mk(let x: i64) -> Str fail Bad {\n let s: Str = \"a\"\n ret s\n}\n\
             fn f(cap io: Io) -> Unit {\n par {\n  {\n   let _t: Result[Str, Bad] = try mk(let 1)\n   jp k(sink v: Str) {\n    let r: Str = move v\n   }\n   match _t {\n    Ok { value: o } => {\n     jump k(o)\n    }\n    Err { error: _ } => {\n     let z: Str = \"e\"\n     jump k(z)\n    }\n   }\n  }\n }\n std.io.print(cap io, let r)\n ret ()\n}\n",
        );
        has(&t, &["std.io.print(cap io, let r)", "drop r", "ret ()"]);
    }

    #[test]
    fn a_copy_field_bound_by_a_pattern_passes_to_a_jump_without_moving_the_scrutinee() {
        let t = elab(
            "module t\nsum Sh { A { x: i64 }, B { x: i64, y: i64 } }\nfn f(let c: bool, let a: i64) -> i64 {\n let s: Sh = B { x: a, y: a }\n var r: i64 = 0\n match s {\n  A { x: p } => {\n   r = p\n  }\n  B { x: q, y: w } => {\n   jp k(let v: i64) {\n    r = v\n   }\n   if c {\n    jump k(w)\n   } else {\n    jump k(q)\n   }\n  }\n }\n ret r\n}\n",
        );
        assert_eq!(t.matches("drop s").count(), 1, "{t}");
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

    #[test]
    fn the_check_takes_each_field_a_record_pattern_binds_once() {
        let bad = |body: &str| {
            let src = format!(
                "module t\nrecord P {{ pub a: Str, pub b: Str }}\nfn f(sink p: P) -> Unit {{\n match p {{\n  P {{ a: x, b: y }} => {{\n{body}  }}\n }}\n ret ()\n}}\n"
            );
            let (m, d) = yan_core::parse("t.yir", &src);
            assert!(d.is_empty(), "{d:?}");
            yan_check::check_optimized("t.yir", &m.expect("parses"))
        };
        assert!(
            bad("   var u: Str = move x\n   var v: Str = move y\n   drop v\n   drop u\n")
                .is_empty()
        );
        let twice = bad("   var u: Str = move x\n   var v: Str = move x\n   drop v\n   drop u\n");
        assert_eq!(twice.len(), 1, "{twice:?}");
    }

    #[test]
    fn the_check_takes_a_field_of_each_arm_once() {
        let src = "module t\nsum S { A { pub a: Str, pub n: Str }, B { pub b: Str } }\nfn f(sink p: S) -> Str {\n match p {\n  A { a: x, n: y } => {\n   var u: Str = move x\n   drop y\n   ret u\n  }\n  B { b: z } => {\n   ret z\n  }\n }\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let diags = yan_check::check_optimized("t.yir", &m.expect("parses"));
        assert!(diags.is_empty(), "{diags:?}");
    }
}
