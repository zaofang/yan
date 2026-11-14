//! The owned fact for drop elaboration (RFC 0054 D3, stage 4): each body, and each closure body
//! in it, lowered to a `yan-flow` graph and solved once before `drops` rewrites it.
//!
//! The graph mirrors the rewrite's walk: an owned local is a [`Binding`] defined where it is
//! declared, a consumption is a move of the local (or of the local a view stands for), and a
//! whole-value assignment redefines it. The rewrite asks for ownership at [`Key`]s, which are
//! addresses of the body's statements and blocks; both walks see the same tree.

use std::collections::{HashMap, HashSet};

use yan_check::own;
use yan_core::ast::*;
use yan_flow::{Binding, Cfg, Event, Node, Owned, Point, Site};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    /// After a statement's own consumption, before what it defines.
    Mid(usize),
    /// The end of a block, where the bodies of its join points that fall through have rejoined.
    End(usize),
    /// After a statement some jump inside it left for a falling-through join point of its block.
    After(usize),
    /// Where the arms of an `if` or `match`, the `break`s of a loop, or the jumps to a join point
    /// meet.
    Join(usize),
    Head(usize),
    /// The edge an `if` without `else` takes when its condition is false.
    Fall(usize),
}

pub(crate) fn addr<T>(x: &T) -> usize {
    x as *const T as usize
}

/// The owned locals a pattern declares, with their types.
pub(crate) fn pat_decls<'a>(p: &'a Pat, ty: &'a Type, out: &mut Vec<(&'a Name, &'a Type)>) {
    match (p, ty) {
        (Pat::Bind(n), _) if !own::is_copy(ty) => out.push((n, ty)),
        (Pat::Tuple(ps), Type::Tuple(ts)) if ps.len() == ts.len() => {
            for (p, t) in ps.iter().zip(ts) {
                pat_decls(p, t, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn owned_params(params: &[Param]) -> Vec<(Name, Type)> {
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

pub(crate) fn rhs_of(k: &StmtKind) -> Option<&Rhs> {
    match k {
        StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) => Some(rhs),
        _ => None,
    }
}

#[derive(Default)]
pub(crate) struct Facts {
    at: HashMap<Key, Owned>,
    decls: HashMap<(usize, Name), Binding>,
}

impl Facts {
    pub(crate) fn new(params: &[Param], body: &Block) -> Self {
        let mut facts = Facts::default();
        let mut bodies = vec![(owned_params(params), body)];
        while let Some((owned, b)) = bodies.pop() {
            let mut l = Lower::new(facts.decls.len() as u32);
            l.block(b, &owned);
            let end = l.cur;
            l.cfg.edge(end, l.exit);
            let sol = l.cfg.solve();
            for (k, p) in l.points {
                if let Some(o) = sol.owned_at(p) {
                    facts.at.insert(k, o);
                }
            }
            facts.decls.extend(l.decls);
            bodies.extend(
                l.closures
                    .into_iter()
                    .map(|c| (owned_params(&c.params), &c.body)),
            );
        }
        facts
    }

    pub(crate) fn binding(&self, site: usize, name: &Name) -> Binding {
        self.decls[&(site, name.clone())]
    }

    pub(crate) fn must(&self, k: Key, b: Binding) -> bool {
        self.at.get(&k).is_some_and(|o| o.must.contains(&b))
    }

    pub(crate) fn may(&self, k: Key, b: Binding) -> bool {
        self.at.get(&k).is_some_and(|o| o.may.contains(&b))
    }
}

/// A join point name with the node and position it shadows.
type Shadowed<'a> = (&'a Name, Option<Node>, Option<(usize, usize)>);

struct Lower<'a> {
    cfg: Cfg,
    cur: Node,
    exit: Node,
    failed: Node,
    next: u32,
    scopes: Vec<Vec<(Name, Binding)>>,
    /// Names bound by a non-copy projection or a `match` pattern, to the local they view.
    views: HashMap<Name, Name>,
    /// Label, head and exit of each enclosing loop.
    loops: Vec<(Name, Node, Node)>,
    jps: HashMap<Name, Node>,
    /// Scope index and local count where each join point of an enclosing block was declared.
    jp_pos: HashMap<Name, (usize, usize)>,
    /// Every jump's target, in lowering order.
    hits: Vec<Node>,
    sites: u32,
    points: Vec<(Key, Point)>,
    decls: HashMap<(usize, Name), Binding>,
    closures: Vec<&'a Closure>,
}

impl<'a> Lower<'a> {
    fn new(next: u32) -> Self {
        let mut cfg = Cfg::new();
        let cur = cfg.node();
        let exit = cfg.node();
        let failed = cfg.node();
        Lower {
            cfg,
            cur,
            exit,
            failed,
            next,
            scopes: Vec::new(),
            views: HashMap::new(),
            loops: Vec::new(),
            jps: HashMap::new(),
            jp_pos: HashMap::new(),
            hits: Vec::new(),
            sites: 0,
            points: Vec::new(),
            decls: HashMap::new(),
            closures: Vec::new(),
        }
    }

    fn mark(&mut self, k: Key) {
        self.points.push((k, self.cfg.end(self.cur)));
    }

    fn branch(&mut self, from: Node) -> Node {
        let n = self.cfg.node();
        self.cfg.edge(from, n);
        n
    }

    /// Control leaves for `to`; what follows is unreachable until a join.
    fn jump(&mut self, to: Option<Node>) {
        if let Some(to) = to {
            self.cfg.edge(self.cur, to);
        }
        self.cur = self.cfg.node();
    }

    fn local(&self, x: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|(n, _)| n == x)
            .map(|&(_, b)| b)
    }

    fn declare(&mut self, site: usize, n: &Name) {
        let b = Binding(self.next);
        self.next += 1;
        self.decls.insert((site, n.clone()), b);
        self.scopes
            .last_mut()
            .expect("in a scope")
            .push((n.clone(), b));
        self.cfg.push(
            self.cur,
            Event::Def {
                binding: b,
                owned: true,
            },
        );
    }

    fn consume(&mut self, k: &StmtKind) {
        for n in own::consumed(k) {
            let root = self.views.get(n).unwrap_or(n);
            if let Some(b) = self.local(root) {
                let site = Site(self.sites);
                self.sites += 1;
                self.cfg.push(self.cur, Event::Move { binding: b, site });
            }
        }
    }

    fn block(&mut self, b: &'a Block, owned: &[(Name, Type)]) {
        self.scopes.push(Vec::new());
        for (n, ty) in owned {
            if !own::is_copy(ty) {
                self.declare(addr(b), n);
            }
        }
        self.seq(b, false);
        self.scopes.pop();
    }

    /// A join point's body is entered only by its jumps; when it falls through it continues
    /// after the statement of the declaring block that jumped (R764), so what its body owns is
    /// the meet of its jumps and what follows that statement meets the body's end. A jump
    /// leaves the locals the block declared after the join point. In a `par` branch (`hoisted`)
    /// the body declares into the enclosing scope.
    fn seq(&mut self, b: &'a Block, hoisted: bool) {
        let declared: Vec<Shadowed> = b
            .stmts
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::Jp { name, .. } => {
                    let n = self.cfg.node();
                    Some((
                        name,
                        self.jps.insert(name.clone(), n),
                        self.jp_pos.remove(name),
                    ))
                }
                _ => None,
            })
            .collect();
        // Entry node, end node and jump targets of each join point body.
        let mut bodies: Vec<(Node, Node, Vec<Node>)> = Vec::new();
        // The node after each statement that jumped, with the targets it jumped to.
        let mut conts: Vec<(Node, Vec<Node>)> = Vec::new();
        for s in &b.stmts {
            let h0 = self.hits.len();
            let StmtKind::Jp { name, params, body } = &s.kind else {
                self.stmt(s);
                if self.hits.len() > h0 {
                    self.mark(Key::After(addr(s)));
                    conts.push((self.cur, self.hits[h0..].to_vec()));
                }
                continue;
            };
            let from = self.cur;
            let resume = self.branch(from);
            let entry = self.jps[name];
            if !hoisted {
                let si = self.scopes.len() - 1;
                self.jp_pos
                    .insert(name.clone(), (si, self.scopes[si].len()));
            }
            self.cur = entry;
            self.mark(Key::Join(addr(s)));
            if hoisted {
                for (n, ty) in owned_params(params) {
                    if !own::is_copy(&ty) {
                        self.declare(addr(body), &n);
                    }
                }
                self.seq(body, false);
            } else {
                self.block(body, &owned_params(params));
            }
            bodies.push((entry, self.cur, self.hits[h0..].to_vec()));
            self.cur = resume;
        }
        for (after, hit) in conts {
            // A body that jumps to a sibling continues after the same statement.
            let mut reach: HashSet<Node> = hit.into_iter().collect();
            loop {
                let n = reach.len();
                for (entry, _, inner) in &bodies {
                    if reach.contains(entry) {
                        reach.extend(inner);
                    }
                }
                if reach.len() == n {
                    break;
                }
            }
            for (entry, last, _) in &bodies {
                if reach.contains(entry) {
                    self.cfg.edge(*last, after);
                }
            }
        }
        self.mark(Key::End(addr(b)));
        for (name, prev, pos) in declared {
            match prev {
                Some(n) => self.jps.insert(name.clone(), n),
                None => self.jps.remove(name),
            };
            match pos {
                Some(p) => self.jp_pos.insert(name.clone(), p),
                None => self.jp_pos.remove(name),
            };
        }
    }

    fn arms(&mut self, s: &Stmt, blocks: &[&'a Block], fall: bool) {
        let from = self.cur;
        let join = self.cfg.node();
        for b in blocks {
            self.cur = self.branch(from);
            self.block(b, &[]);
            self.cfg.edge(self.cur, join);
        }
        if fall {
            self.points.push((Key::Fall(addr(s)), self.cfg.end(from)));
            self.cfg.edge(from, join);
        }
        self.cur = join;
        self.mark(Key::Join(addr(s)));
    }

    fn stmt(&mut self, s: &'a Stmt) {
        let key = addr(s);
        self.consume(&s.kind);
        self.mark(Key::Mid(key));
        if let Some(r) = rhs_of(&s.kind) {
            match r {
                Rhs::Propagate { .. } => {
                    self.cfg.edge(self.cur, self.failed);
                    self.cur = self.branch(self.cur);
                }
                Rhs::Closure(c) => self.closures.push(c),
                _ => {}
            }
        }
        match &s.kind {
            StmtKind::Let { pat, ty, rhs, .. } => {
                if let (Rhs::Proj(p), Pat::Bind(x)) = (rhs, pat)
                    && !own::is_copy(ty)
                {
                    let root = self.views.get(&p.root).unwrap_or(&p.root).clone();
                    self.views.insert(x.clone(), root);
                } else if let Pat::Bind(x) = pat {
                    self.views.remove(x);
                }
                if !matches!(rhs, Rhs::Proj(_)) {
                    let mut ds = Vec::new();
                    pat_decls(pat, ty, &mut ds);
                    for (n, _) in ds {
                        self.declare(key, n);
                    }
                }
            }
            StmtKind::Assign { place, .. } => {
                if place.projs.is_empty()
                    && let Some(b) = self.local(&place.root)
                {
                    self.cfg.push(
                        self.cur,
                        Event::Def {
                            binding: b,
                            owned: true,
                        },
                    );
                }
            }
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                let blocks: Vec<&Block> = std::iter::once(then).chain(els).collect();
                self.arms(s, &blocks, els.is_none());
            }
            StmtKind::Match { scrutinee, arms } => {
                let mut bound = Vec::new();
                if let Atom::Name(x, _) = scrutinee {
                    let root = self.views.get(x).unwrap_or(x).clone();
                    if self.local(&root).is_some() {
                        for (p, _) in arms {
                            own::pat_binders(p, &mut bound);
                        }
                        for n in &bound {
                            self.views.insert(n.clone(), root.clone());
                        }
                    }
                }
                let blocks: Vec<&Block> = arms.iter().map(|(_, b)| b).collect();
                self.arms(s, &blocks, false);
                for n in &bound {
                    self.views.remove(n);
                }
            }
            StmtKind::Loop { label, body, .. } => {
                let head = self.branch(self.cur);
                let exit = self.cfg.node();
                self.cur = head;
                self.mark(Key::Head(key));
                self.loops.push((label.clone(), head, exit));
                self.block(body, &[]);
                self.cfg.edge(self.cur, head);
                self.loops.pop();
                self.cur = exit;
                self.mark(Key::Join(key));
            }
            StmtKind::ComptimeFor { body, .. } => {
                let head = self.branch(self.cur);
                self.cur = head;
                self.mark(Key::Head(key));
                self.cur = self.branch(head);
                self.block(body, &[]);
                self.cfg.edge(self.cur, head);
                self.cur = self.branch(head);
            }
            StmtKind::Break { label, .. } | StmtKind::Continue { label } => {
                let target = self
                    .loops
                    .iter()
                    .rposition(|l| l.0 == *label)
                    .or(self.loops.len().checked_sub(1))
                    .map(|i| {
                        let (_, head, exit) = self.loops[i];
                        if matches!(s.kind, StmtKind::Break { .. }) {
                            exit
                        } else {
                            head
                        }
                    });
                self.jump(target);
            }
            StmtKind::Jump { name, .. } => {
                let target = self.jps.get(name).copied();
                if let Some(n) = target {
                    self.hits.push(n);
                }
                if let Some(&(si, seen)) = self.jp_pos.get(name) {
                    let gone: Vec<Binding> =
                        self.scopes[si][seen..].iter().map(|&(_, b)| b).collect();
                    for binding in gone {
                        self.cfg.push(self.cur, Event::Drop { binding });
                    }
                }
                self.jump(target);
            }
            StmtKind::Ret(_) | StmtKind::Become(_) => self.jump(Some(self.exit)),
            StmtKind::Fail(_) => self.jump(Some(self.failed)),
            StmtKind::Unreachable => self.jump(None),
            StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::Unsafe(body) => self.block(body, &[]),
            StmtKind::Par { blocks, .. } => {
                for b in blocks {
                    self.seq(b, true);
                }
            }
            StmtKind::Rhs(_)
            | StmtKind::Assert(_)
            | StmtKind::Asm { .. }
            | StmtKind::Yield(_)
            | StmtKind::Drop(_)
            | StmtKind::Reuse { .. }
            | StmtKind::Jp { .. } => {}
        }
    }
}
