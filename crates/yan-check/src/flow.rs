//! Moves and `@linear` consumption on the control-flow graph (RFC 0054 stage 3).
//!
//! The checker's walk lowers each body to a [`Cfg`] as it goes: every binding it makes is a new
//! [`Binding`], so shadowing needs no special case. E401 is a read at which the binding is
//! maybe-moved; E412 is a `@linear` local still maybe-owned where its body exits. A field taken
//! out of a `@linear` record leaves a hole, and a record whose linear fields are all holes is
//! consumed with them. A path that fails (`fail`, or the failing edge of `?`) is not an exit
//! for E412: the handler's release intrinsic frees what it still owns (5.4, R443).

use std::collections::{HashMap, HashSet};

use yan_core::ast::Name;
use yan_diag::Span;
use yan_flow::{Binding, Cfg, Event, Field, Node, Point, Site};

struct Info {
    name: Name,
    span: Span,
    /// For an owned `@linear` local: its linear fields, empty when it is not a record.
    linear: Option<Vec<Name>>,
    /// The exit node of the body that made it.
    exit: Node,
}

/// What a body saves of the enclosing one.
pub(crate) struct Body {
    cur: Node,
    exit: Node,
    loops: Vec<(Name, Node, Node)>,
}

pub(crate) enum Finding {
    UseAfterMove {
        name: Name,
        at: Span,
        moved: Span,
    },
    NotConsumed {
        name: Name,
        at: Span,
        missing: Vec<Name>,
    },
}

pub(crate) struct Flow {
    cfg: Cfg,
    pub(crate) cur: Node,
    exit: Node,
    exits: Vec<Node>,
    /// Where failing paths go, from every body.
    failed: Node,
    /// Label, head and exit of each enclosing loop.
    loops: Vec<(Name, Node, Node)>,
    jps: HashMap<Name, Node>,
    info: Vec<Info>,
    reads: Vec<(Binding, Point, Span)>,
    /// Each move's span; a move's site is its index.
    moves: Vec<Span>,
}

impl Flow {
    pub(crate) fn new() -> Self {
        let mut cfg = Cfg::new();
        let cur = cfg.node();
        let exit = cfg.node();
        let failed = cfg.node();
        Flow {
            cfg,
            cur,
            exit,
            exits: vec![exit],
            failed,
            loops: Vec::new(),
            jps: HashMap::new(),
            info: Vec::new(),
            reads: Vec::new(),
            moves: Vec::new(),
        }
    }

    /// A new binding, owned when `linear` is given.
    pub(crate) fn bind(&mut self, name: &Name, span: Span, linear: Option<Vec<Name>>) -> Binding {
        let b = Binding(self.info.len() as u32);
        let owned = linear.is_some();
        self.info.push(Info {
            name: name.clone(),
            span,
            linear,
            exit: self.exit,
        });
        self.cfg.push(self.cur, Event::Def { binding: b, owned });
        b
    }

    /// A whole-value assignment: the binding holds a fresh value.
    pub(crate) fn redefine(&mut self, b: Binding) {
        let owned = self.info[b.0 as usize].linear.is_some();
        self.cfg.push(self.cur, Event::Def { binding: b, owned });
    }

    pub(crate) fn read(&mut self, b: Binding, at: Span) {
        let site = Site(self.reads.len() as u32);
        let p = self.cfg.push(self.cur, Event::Use { binding: b, site });
        self.reads.push((b, p, at));
    }

    pub(crate) fn moved(&mut self, b: Binding, at: Span) {
        let site = Site(self.moves.len() as u32);
        self.moves.push(at);
        self.cfg.push(self.cur, Event::Move { binding: b, site });
    }

    /// Ownership leaves `b` without making a later read a use after move: a return, a store
    /// into a value, a jump argument.
    pub(crate) fn release(&mut self, b: Binding) {
        self.cfg.push(self.cur, Event::Drop { binding: b });
    }

    /// Takes field `f` out of `b` when it is one of its linear fields.
    pub(crate) fn take(&mut self, b: Binding, f: &str, at: Span) {
        let Some(i) = self.info[b.0 as usize]
            .linear
            .as_ref()
            .and_then(|fs| fs.iter().position(|x| x == f))
        else {
            return;
        };
        let site = Site(self.moves.len() as u32);
        self.moves.push(at);
        let field = Field(i as u32);
        self.cfg.push(
            self.cur,
            Event::MoveField {
                binding: b,
                field,
                site,
            },
        );
    }

    pub(crate) fn node(&mut self) -> Node {
        self.cfg.node()
    }

    pub(crate) fn edge(&mut self, from: Node, to: Node) {
        self.cfg.edge(from, to);
    }

    /// A node entered from `from`.
    pub(crate) fn branch(&mut self, from: Node) -> Node {
        let n = self.cfg.node();
        self.cfg.edge(from, n);
        n
    }

    /// Control leaves for `to`; what follows is unreachable until a join.
    pub(crate) fn jump(&mut self, to: Option<Node>) {
        if let Some(to) = to {
            self.cfg.edge(self.cur, to);
        }
        self.cur = self.cfg.node();
    }

    /// The body returns here.
    pub(crate) fn exit(&mut self) {
        self.jump(Some(self.exit));
    }

    pub(crate) fn fail(&mut self) {
        self.jump(Some(self.failed));
    }

    /// The body may fail here and otherwise goes on (`?`).
    pub(crate) fn may_fail(&mut self) {
        self.cfg.edge(self.cur, self.failed);
        // An edge leaves a node after all its events, so the body goes on in a new one.
        self.cur = self.branch(self.cur);
    }

    pub(crate) fn enter_loop(&mut self, label: &Name) -> Node {
        let head = self.branch(self.cur);
        self.cur = head;
        let exit = self.cfg.node();
        self.loops.push((label.clone(), head, exit));
        head
    }

    pub(crate) fn leave_loop(&mut self) {
        let (_, head, exit) = self.loops.pop().expect("entered");
        self.cfg.edge(self.cur, head);
        self.cur = exit;
    }

    /// Where `break` (or `continue`) of the loop labelled `label` goes.
    pub(crate) fn loop_target(&self, label: &str, cont: bool) -> Option<Node> {
        self.loops
            .iter()
            .rev()
            .find(|(l, ..)| l == label)
            .map(|&(_, head, exit)| if cont { head } else { exit })
    }

    pub(crate) fn declare_jp(&mut self, name: &Name) -> Option<Node> {
        let n = self.cfg.node();
        self.jps.insert(name.clone(), n)
    }

    pub(crate) fn jp(&self, name: &str) -> Option<Node> {
        self.jps.get(name).copied()
    }

    pub(crate) fn restore_jp(&mut self, name: &Name, prev: Option<Node>) {
        match prev {
            Some(n) => self.jps.insert(name.clone(), n),
            None => self.jps.remove(name),
        };
    }

    /// A closure body: entered from here, it returns to its caller, not to this body. The
    /// enclosing body goes on in a new node, as after `?`.
    pub(crate) fn enter_body(&mut self) -> Body {
        let entry = self.branch(self.cur);
        let resume = self.branch(self.cur);
        let exit = self.cfg.node();
        self.exits.push(exit);
        self.cur = entry;
        Body {
            cur: resume,
            exit: std::mem::replace(&mut self.exit, exit),
            loops: std::mem::take(&mut self.loops),
        }
    }

    pub(crate) fn leave_body(&mut self, saved: Body) {
        self.cfg.edge(self.cur, self.exit);
        self.cur = saved.cur;
        self.exit = saved.exit;
        self.loops = saved.loops;
    }

    /// Ends the function body and reports, in order, each first use after move and each
    /// `@linear` local that some path leaves unconsumed.
    pub(crate) fn finish(mut self) -> Vec<Finding> {
        self.cfg.edge(self.cur, self.exit);
        let sol = self.cfg.solve();
        let mut out = Vec::new();
        let mut reported = HashSet::new();
        for &(b, p, at) in &self.reads {
            if !reported.contains(&b) && sol.maybe_moved(b, None, p) {
                reported.insert(b);
                let Site(last) = *sol.moves(b, p).last().expect("maybe-moved has a move");
                out.push(Finding::UseAfterMove {
                    name: self.info[b.0 as usize].name.clone(),
                    at,
                    moved: self.moves[last as usize],
                });
            }
        }
        for &exit in &self.exits {
            let Some(owned) = sol.owned_at(Point {
                node: exit,
                event: 0,
            }) else {
                continue;
            };
            for &b in &owned.may {
                let info = &self.info[b.0 as usize];
                let Some(fields) = info.linear.as_ref().filter(|_| info.exit == exit) else {
                    continue;
                };
                let missing: Vec<Name> = fields
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| !owned.holes.contains(&(b, Field(i as u32))))
                    .map(|(_, f)| f.clone())
                    .collect();
                if fields.is_empty() || !missing.is_empty() {
                    out.push(Finding::NotConsumed {
                        name: info.name.clone(),
                        at: info.span,
                        missing,
                    });
                }
            }
        }
        out
    }
}
