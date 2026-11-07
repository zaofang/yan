//! `@constant_time` (R611): every value computed from `expose()` is secret-derived, and a branch, `match`,
//! index, division or remainder that depends on one is E424. Taint is by local name, flow-insensitive,
//! and loop bodies are walked twice so a value tainted late in one pass is seen at the top of the next.

use std::collections::HashSet;

use yan_diag::Span;
use yan_hir as j;

use crate::Cx;

struct Walk {
    tainted: HashSet<String>,
    hits: Vec<(Span, String)>,
}

pub(crate) fn check(it: &j::Item, cx: &mut Cx) {
    let j::ItemKind::Func(f) = &it.kind else {
        return;
    };
    let Some(body) = &f.body else { return };
    let mut w = Walk {
        tainted: HashSet::new(),
        hits: Vec::new(),
    };
    w.stmts(&body.stmts);
    let item = f.name.clone();
    for (span, what) in w.hits {
        cx.err(
            "E424",
            span,
            &item,
            format!("{what} depends on a secret-derived value in a `@constant_time` function"),
        );
    }
}

impl Walk {
    fn hit(&mut self, span: Span, what: &str) {
        if !self.hits.iter().any(|(s, _)| *s == span) {
            self.hits.push((span, what.to_owned()));
        }
    }

    fn secret(&self, e: &j::Expr) -> bool {
        use j::ExprKind as K;
        match &e.kind {
            K::Name(n) => self.tainted.contains(n),
            K::Call(f, args) => {
                matches!(&f.kind, K::Attr(_, m, _) if m == "expose")
                    || self.secret(f)
                    || args.iter().any(|a| match &a.kind {
                        j::ArgKind::Expr(x) | j::ArgKind::Ref(x) | j::ArgKind::Become(x) => {
                            self.secret(x)
                        }
                    })
            }
            K::Attr(x, ..)
            | K::Try(x)
            | K::Unary(_, x)
            | K::Not(x)
            | K::Reify(x)
            | K::Cast(x, _)
            | K::Stage(x) => self.secret(x),
            K::Binary(_, a, b) | K::Compare(_, a, b) | K::Index(a, b) => {
                self.secret(a) || self.secret(b)
            }
            K::Cond { then, cond, els } => {
                self.secret(then) || self.secret(cond) || self.secret(els)
            }
            K::Tuple(xs) | K::List(xs) | K::Set(xs) => xs.iter().any(|x| self.secret(x)),
            _ => false,
        }
    }

    /// Records the violations inside `e`.
    fn expr(&mut self, e: &j::Expr) {
        use j::ExprKind as K;
        match &e.kind {
            K::Binary(op @ ("and" | "or"), a, b) => {
                if self.secret(a) {
                    self.hit(a.span, &format!("the short-circuit `{op}`"));
                }
                self.expr(a);
                self.expr(b);
            }
            K::Binary(op @ ("/" | "//" | "%"), a, b) => {
                if self.secret(e) {
                    self.hit(e.span, &format!("the `{op}` operation"));
                }
                self.expr(a);
                self.expr(b);
            }
            K::Index(a, i) => {
                if self.secret(i) {
                    self.hit(i.span, "an index");
                }
                self.expr(a);
                self.expr(i);
            }
            K::Cond { then, cond, els } => {
                if self.secret(cond) {
                    self.hit(cond.span, "a conditional expression");
                }
                self.expr(then);
                self.expr(cond);
                self.expr(els);
            }
            K::Binary(_, a, b) | K::Compare(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            K::Attr(x, ..)
            | K::Try(x)
            | K::Unary(_, x)
            | K::Not(x)
            | K::Reify(x)
            | K::Cast(x, _)
            | K::Stage(x) => self.expr(x),
            K::Call(f, args) => {
                self.expr(f);
                for a in args {
                    let (j::ArgKind::Expr(x) | j::ArgKind::Ref(x) | j::ArgKind::Become(x)) =
                        &a.kind;
                    self.expr(x);
                }
            }
            K::Tuple(xs) | K::List(xs) | K::Set(xs) => xs.iter().for_each(|x| self.expr(x)),
            _ => {}
        }
    }

    fn assign(&mut self, target: &j::Expr, tainted: bool) {
        match &target.kind {
            j::ExprKind::Name(n) if tainted => {
                self.tainted.insert(n.clone());
            }
            j::ExprKind::Tuple(xs) => xs.iter().for_each(|x| self.assign(x, tainted)),
            _ => {}
        }
    }

    fn stmts(&mut self, ss: &[j::Stmt]) {
        ss.iter().for_each(|s| self.stmt(s));
    }

    fn looped(&mut self, f: impl Fn(&mut Self)) {
        f(self);
        f(self);
    }

    fn stmt(&mut self, s: &j::Stmt) {
        use j::StmtKind as S;
        match &s.kind {
            S::Expr(e) | S::Assert(e) | S::Become(e) | S::Fail(e) | S::Return(Some(e)) => {
                self.expr(e)
            }
            S::Assign { target, value, .. } | S::AugAssign { target, value, .. } => {
                self.expr(value);
                let t = self.secret(value);
                self.assign(target, t);
            }
            S::Bind { target, value, .. } => {
                self.expr(value);
                if self.secret(value) {
                    self.tainted.insert(target.clone());
                }
            }
            S::If { arms, els } => {
                for (c, b) in arms {
                    if self.secret(c) {
                        self.hit(c.span, "a branch");
                    }
                    self.expr(c);
                    self.stmts(&b.stmts);
                }
                if let Some(b) = els {
                    self.stmts(&b.stmts);
                }
            }
            S::While { cond, body, .. } => self.looped(|w| {
                if w.secret(cond) {
                    w.hit(cond.span, "a loop condition");
                }
                w.expr(cond);
                w.stmts(body);
            }),
            S::For {
                target, iter, body, ..
            } => self.looped(|w| {
                if w.secret(iter) {
                    w.hit(iter.span, "a loop bound");
                    w.tainted.extend(target.iter().cloned());
                }
                w.expr(iter);
                w.stmts(body);
            }),
            S::Match { scrutinee, cases } => {
                let bound = self.secret(scrutinee);
                if bound {
                    self.hit(scrutinee.span, "a `match`");
                }
                self.expr(scrutinee);
                for c in cases {
                    if let Some(g) = &c.guard {
                        if self.secret(g) {
                            self.hit(g.span, "a guard");
                        }
                        self.expr(g);
                    }
                    self.stmts(&c.body.stmts);
                }
            }
            S::Unsafe(b) | S::StageIf { then: b, .. } => self.stmts(b),
            S::With { body, .. } => self.stmts(&body.stmts),
            _ => {}
        }
    }
}
