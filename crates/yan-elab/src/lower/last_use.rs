//! Last use of a local on the control-flow graph (RFC 0054 stage 2). E413 and the choice of
//! `move` or `copy` ask whether a read is followed, on some path, by another read of the same
//! name before it is reassigned. Loop back edges are ordinary edges, so a read inside a loop is
//! followed by every read reachable around it.

use std::collections::{HashMap, HashSet};

use yan_diag::Span;
use yan_flow::{Binding, Cfg, Event, Node, Point, Site};
use yan_hir as j;

use super::{is_hole, stmt_exprs, sub_blocks, walk};

pub(super) struct LastUse {
    cfg: Cfg,
    names: HashMap<String, Binding>,
    /// The point after each read, keyed by where the read starts.
    after: HashMap<(u32, u32), Point>,
    /// Each read's span; a read's site is its index, so sites follow source order.
    spans: Vec<Span>,
    /// Names read anywhere, for W812.
    read: HashSet<String>,
}

impl LastUse {
    pub(super) fn new(stmts: &[j::Stmt]) -> Self {
        let (mut spans, mut read) = (Vec::new(), HashSet::new());
        collect(stmts, &mut spans, &mut read);
        spans.sort_by_key(|s| (s.line, s.col));
        let mut cfg = Cfg::new();
        let cur = cfg.node();
        let exit = cfg.node();
        let mut b = Build {
            cfg,
            cur,
            exit,
            names: HashMap::new(),
            after: HashMap::new(),
            spans: &spans,
            loops: Vec::new(),
        };
        b.stmts(stmts);
        let (cfg, names, after) = (b.cfg, b.names, b.after);
        Self {
            cfg,
            names,
            after,
            spans,
            read,
        }
    }

    pub(super) fn is_read(&self, n: &str) -> bool {
        self.read.contains(n)
    }

    /// A read the lowering resolves itself, such as a narrowed optional.
    pub(super) fn mark_read(&mut self, n: &str) {
        self.read.insert(n.to_owned());
    }

    /// The first read of `n` reachable from the read that starts at `at`, before `n` is
    /// reassigned.
    pub(super) fn next_use(&self, n: &str, at: Span) -> Option<Span> {
        let b = *self.names.get(n)?;
        let p = *self.after.get(&(at.line, at.col))?;
        self.cfg
            .next_use(b, p)
            .map(|Site(i)| self.spans[i as usize])
    }
}

/// Plain-name reads of a statement, outside its blocks and outside assignment targets.
fn reads(s: &j::Stmt) -> Vec<(&str, Span)> {
    let exprs = match &s.kind {
        j::StmtKind::Assign { target, value, .. } => {
            let mut v: Vec<&j::Expr> = match &target.kind {
                j::ExprKind::Name(_) => Vec::new(),
                j::ExprKind::Tuple(ts) => ts
                    .iter()
                    .filter(|t| !matches!(t.kind, j::ExprKind::Name(_)))
                    .collect(),
                _ => vec![target],
            };
            v.push(value);
            v
        }
        _ => stmt_exprs(s),
    };
    let mut out = Vec::new();
    for e in exprs {
        expr_reads(e, &mut out);
    }
    out
}

fn expr_reads<'e>(e: &'e j::Expr, out: &mut Vec<(&'e str, Span)>) {
    walk(e, &mut |e| {
        if let j::ExprKind::Name(n) = &e.kind {
            out.push((n, e.span));
        }
    });
}

fn collect(stmts: &[j::Stmt], spans: &mut Vec<Span>, names: &mut HashSet<String>) {
    for s in stmts {
        for (n, sp) in reads(s) {
            spans.push(sp);
            names.insert(n.to_owned());
        }
        for b in sub_blocks(s) {
            collect(b, spans, names);
        }
    }
}

/// Names a plain or tuple assignment target binds.
fn targets(target: &j::Expr) -> Vec<&str> {
    match &target.kind {
        j::ExprKind::Name(n) => vec![n],
        j::ExprKind::Tuple(ts) => ts
            .iter()
            .filter_map(|t| match &t.kind {
                j::ExprKind::Name(n) => Some(n.as_str()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

struct Loop<'a> {
    label: Option<&'a str>,
    head: Node,
    exit: Node,
}

struct Build<'a> {
    cfg: Cfg,
    cur: Node,
    exit: Node,
    names: HashMap<String, Binding>,
    after: HashMap<(u32, u32), Point>,
    spans: &'a [Span],
    loops: Vec<Loop<'a>>,
}

impl<'a> Build<'a> {
    fn binding(&mut self, n: &str) -> Binding {
        let next = Binding(self.names.len() as u32);
        *self.names.entry(n.to_owned()).or_insert(next)
    }

    fn read_all(&mut self, mut rs: Vec<(&str, Span)>) {
        rs.sort_by_key(|(_, s)| (s.line, s.col));
        for (n, at) in rs {
            let binding = self.binding(n);
            let site = self
                .spans
                .binary_search_by_key(&(at.line, at.col), |s| (s.line, s.col))
                .expect("collected");
            let p = self.cfg.push(
                self.cur,
                Event::Use {
                    binding,
                    site: Site(site as u32),
                },
            );
            self.after.insert((at.line, at.col), p);
        }
    }

    fn expr(&mut self, e: &j::Expr) {
        let mut rs = Vec::new();
        expr_reads(e, &mut rs);
        self.read_all(rs);
    }

    fn def(&mut self, n: &str) {
        let binding = self.binding(n);
        self.cfg.push(
            self.cur,
            Event::Def {
                binding,
                owned: false,
            },
        );
    }

    /// A node entered from the current one.
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

    fn target(&self, label: Option<&str>) -> Option<&Loop<'a>> {
        match label {
            None => self.loops.last(),
            Some(l) => self.loops.iter().rev().find(|lp| lp.label == Some(l)),
        }
    }

    fn arms(&mut self, from: Node, arms: &[&'a [j::Stmt]], join: Node) {
        for b in arms {
            self.cur = self.branch(from);
            self.stmts(b);
            self.cfg.edge(self.cur, join);
        }
    }

    /// `targets` are bound at the head of each iteration.
    fn looped(&mut self, label: Option<&'a str>, targets: &[String], body: &'a [j::Stmt]) {
        let head = self.branch(self.cur);
        let exit = self.branch(head);
        self.cur = self.branch(head);
        for t in targets {
            self.def(t);
        }
        self.loops.push(Loop { label, head, exit });
        self.stmts(body);
        self.loops.pop();
        self.cfg.edge(self.cur, head);
        self.cur = exit;
    }

    fn stmts(&mut self, stmts: &'a [j::Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &'a j::Stmt) {
        use j::StmtKind as K;
        match &s.kind {
            K::If { arms, els } => {
                let join = self.cfg.node();
                for (c, b) in arms {
                    self.expr(c);
                    let from = self.cur;
                    self.arms(from, &[&b.stmts], join);
                    self.cur = self.branch(from);
                }
                if let Some(b) = els {
                    self.stmts(&b.stmts);
                }
                self.cfg.edge(self.cur, join);
                self.cur = join;
            }
            K::StageIf { cond, then, els } => {
                self.expr(cond);
                let (from, join) = (self.cur, self.cfg.node());
                let empty: &[j::Stmt] = &[];
                self.arms(from, &[then, els.as_deref().unwrap_or(empty)], join);
                self.cur = join;
            }
            K::Match { scrutinee, cases } => {
                self.expr(scrutinee);
                let (from, join) = (self.cur, self.cfg.node());
                let bodies: Vec<&[j::Stmt]> = cases.iter().map(|c| &c.body.stmts[..]).collect();
                self.arms(from, &bodies, join);
                self.cur = join;
            }
            K::While {
                label, cond, body, ..
            } => {
                let head = self.branch(self.cur);
                self.cur = head;
                self.expr(cond);
                let exit = self.branch(self.cur);
                self.cur = self.branch(self.cur);
                self.loops.push(Loop {
                    label: label.as_deref(),
                    head,
                    exit,
                });
                self.stmts(body);
                self.loops.pop();
                self.cfg.edge(self.cur, head);
                self.cur = exit;
            }
            K::For {
                label,
                target,
                iter,
                body,
                ..
            } => {
                self.expr(iter);
                self.looped(label.as_deref(), target, body);
            }
            K::StageFor { target, iter, body } => {
                self.expr(iter);
                self.looped(None, std::slice::from_ref(target), body);
            }
            K::Par { body, .. } => match body {
                j::ParBody::Block(b) => self.stmts(b),
                j::ParBody::For {
                    target, iter, body, ..
                } => {
                    self.expr(iter);
                    self.looped(None, target, body);
                }
            },
            K::With { kind, body } => {
                self.read_all(reads(s));
                if let j::WithKind::Bind(n, _) = kind {
                    self.def(n);
                }
                self.stmts(&body.stmts);
            }
            K::Unsafe(b) => self.stmts(b),
            K::Assign { target, .. } | K::AugAssign { target, .. } => {
                self.read_all(reads(s));
                for n in targets(target) {
                    self.def(n);
                }
            }
            K::Bind { target, .. } | K::Ghost { name: target, .. } => {
                self.read_all(reads(s));
                self.def(target);
            }
            K::Return(_) | K::Fail(_) | K::Become(_) | K::Unreachable => {
                self.read_all(reads(s));
                self.jump(Some(self.exit));
            }
            K::Expr(_) if is_hole(s) => self.jump(Some(self.exit)),
            K::Break | K::BreakTo(_) | K::Continue | K::ContinueTo(_) => {
                let (label, cont) = match &s.kind {
                    K::BreakTo(l) => (Some(l.as_str()), false),
                    K::ContinueTo(l) => (Some(l.as_str()), true),
                    K::Continue => (None, true),
                    _ => (None, false),
                };
                let to = self
                    .target(label)
                    .map(|l| if cont { l.head } else { l.exit });
                self.jump(to);
            }
            _ => self.read_all(reads(s)),
        }
    }
}
