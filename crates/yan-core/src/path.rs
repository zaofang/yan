//! Node paths (RFC 0034 D1): the one walk over an item that elaboration and `raise` share, so the
//! writer and the reader of a trivia mark cannot disagree about where a node is.
//!
//! Child indices: parameter k is `[0, k]`, contract k `[1, k]`, body statement s `[2, s]` and, for
//! the b-th nested block of a statement, `[2, s, b, s']`. The call of a statement is its path plus
//! `0`, argument k of a call is the call's path plus `k`. A literal atom is the path of its
//! parent (an argument, or the statement for a right-hand side) plus `0`. For an `impl`, function
//! f is walked under the prefix `[3, f]`. Field k of a named constructor is its statement's path
//! plus `0, k`. Closure bodies are not walked.

use crate::ast::*;

#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Item(&'a Item),
    Param(&'a Param),
    Contract(&'a Contract),
    Stmt(&'a Stmt),
    Call(&'a Call),
    Arg(&'a Arg),
    Literal(&'a Literal),
    /// Field `k` of a named constructor.
    Field(&'a Ctor, usize),
}

/// Calls `f` with the path and node of every markable node of `item`, parents first.
pub fn walk<'a>(item: &'a Item, f: &mut impl FnMut(&[u32], Node<'a>)) {
    let mut w = Walker {
        path: Vec::new(),
        f,
    };
    w.visit(Node::Item(item));
    match &item.kind {
        ItemKind::Fn(d) | ItemKind::Gen(d) => w.func(d),
        ItemKind::Inst(i) => w.func(&i.def),
        ItemKind::Test(t) => w.block(2, &t.body),
        ItemKind::Impl(i) => {
            for (k, d) in i.fns.iter().enumerate() {
                w.path.extend([3, k as u32]);
                w.func(d);
                w.path.truncate(w.path.len() - 2);
            }
        }
        _ => {}
    }
}

struct Walker<'f, F> {
    path: Vec<u32>,
    f: &'f mut F,
}

impl<'a, F: FnMut(&[u32], Node<'a>)> Walker<'_, F> {
    fn visit(&mut self, n: Node<'a>) {
        (self.f)(&self.path, n);
    }

    fn child(&mut self, k: usize, g: impl FnOnce(&mut Self)) {
        self.path.push(k as u32);
        g(self);
        self.path.pop();
    }

    fn func(&mut self, d: &'a FnDef) {
        for (k, p) in d.params.iter().enumerate() {
            self.path.extend([0, k as u32]);
            self.visit(Node::Param(p));
            self.path.truncate(self.path.len() - 2);
        }
        for (k, c) in d.contracts.iter().enumerate() {
            self.path.extend([1, k as u32]);
            self.visit(Node::Contract(c));
            self.path.truncate(self.path.len() - 2);
        }
        if let Some(b) = &d.body {
            self.block(2, b);
        }
    }

    /// The statements of `b` at `[.., head, s]`.
    fn block(&mut self, head: u32, b: &'a Block) {
        self.path.push(head);
        self.stmts(b);
        self.path.pop();
    }

    fn stmts(&mut self, b: &'a Block) {
        for (s, st) in b.stmts.iter().enumerate() {
            self.child(s, |w| w.stmt(st));
        }
    }

    /// The b-th nested block of the current statement.
    fn nested(&mut self, b: usize, blk: &'a Block) {
        self.child(b, |w| w.stmts(blk));
    }

    fn stmt(&mut self, st: &'a Stmt) {
        self.visit(Node::Stmt(st));
        match &st.kind {
            StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) => {
                self.rhs(rhs)
            }
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                self.nested(0, then);
                if let Some(e) = els {
                    self.nested(1, e);
                }
            }
            StmtKind::Match { arms, .. } => {
                for (i, (_, b)) in arms.iter().enumerate() {
                    self.nested(i, b);
                }
            }
            StmtKind::Loop { body, .. }
            | StmtKind::Jp { body, .. }
            | StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::ComptimeFor { body, .. }
            | StmtKind::Unsafe(body) => self.nested(0, body),
            StmtKind::Par { blocks, .. } => {
                for (i, b) in blocks.iter().enumerate() {
                    self.nested(i, b);
                }
            }
            StmtKind::Become(c) => self.call(c),
            _ => {}
        }
    }

    fn rhs(&mut self, rhs: &'a Rhs) {
        match rhs {
            Rhs::Atom(Atom::Lit(l, _)) => self.child(0, |w| w.visit(Node::Literal(l))),
            Rhs::Call(c) | Rhs::Propagate { call: c, .. } | Rhs::Try(c) => self.call(c),
            Rhs::Ctor(c @ Ctor::Named { fields, .. }) => self.child(0, |w| {
                for k in 0..fields.len() {
                    w.child(k, |w| w.visit(Node::Field(c, k)));
                }
            }),
            _ => {}
        }
    }

    fn call(&mut self, c: &'a Call) {
        self.child(0, |w| {
            w.visit(Node::Call(c));
            for (k, a) in c.args.iter().enumerate() {
                w.child(k, |w| {
                    w.visit(Node::Arg(a));
                    if let Arg::Value(_, Atom::Lit(l, _), _) = a {
                        w.child(0, |w| w.visit(Node::Literal(l)));
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    const SRC: &str = r#"module m

fn f(let a: i64, let b: i64) -> i64
    requires lt_i64(let a, let 10)
{
    let x: i64 = add_checked_i64(let a, let 1)
    let t: bool = true
    if t {
        let y: i64 = add_checked_i64(let x, let b)
        ret y
    } else {
        ret x
    }
}
"#;

    fn paths(src: &str) -> Vec<(Vec<u32>, &'static str)> {
        let m = crate::parse("m.yir", src).0.expect("parses");
        let mut out = Vec::new();
        walk(&m.items[0], &mut |p, n| {
            let k = match n {
                Node::Item(_) => "item",
                Node::Param(_) => "param",
                Node::Contract(_) => "contract",
                Node::Stmt(_) => "stmt",
                Node::Call(_) => "call",
                Node::Arg(_) => "arg",
                Node::Literal(_) => "lit",
                Node::Field(..) => "field",
            };
            out.push((p.to_vec(), k));
        });
        out
    }

    #[test]
    fn paths_of_each_statement_kind() {
        let got = paths(SRC);
        let has = |p: &[u32], k: &str| got.iter().any(|(q, j)| q == p && *j == k);
        assert!(has(&[], "item"));
        assert!(has(&[0, 1], "param"));
        assert!(has(&[1, 0], "contract"));
        assert!(has(&[2, 0], "stmt"));
        assert!(has(&[2, 0, 0], "call"));
        assert!(has(&[2, 0, 0, 1], "arg"));
        assert!(has(&[2, 0, 0, 1, 0], "lit"));
        assert!(has(&[2, 1, 0], "lit"));
        assert!(has(&[2, 2], "stmt"));
        assert!(has(&[2, 2, 0, 0], "stmt"));
        assert!(has(&[2, 2, 0, 0, 0], "call"));
        assert!(has(&[2, 2, 1, 0], "stmt"));
    }

    #[test]
    fn paths_are_unique() {
        let got = paths(SRC);
        let mut seen = std::collections::HashSet::new();
        assert!(got.iter().all(|e| seen.insert(e.clone())));
    }
}
