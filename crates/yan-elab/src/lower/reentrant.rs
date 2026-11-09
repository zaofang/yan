//! A `@reentrant` handler (9.6) is called through an actor as `f__lent`: a copy of `f` that takes
//! the handle last, lends the state back to the actor before each suspension and retakes it
//! after, so other handlers of the actor run while it waits (RFC 0056 D5).

use super::*;

const HANDLE: &str = "lent__h";
const POLICY: &str = "lent__P";

/// The std operations that can wait on another task. A call to a user function may reach one.
const SUSPENDS: [&str; 8] = [
    "std.actor.send",
    "std.chan.receiver.recv",
    "std.chan.sender.send",
    "std.time.sleep",
    "std.net.listener.accept",
    "std.net.stream.recv",
    "std.net.stream.send",
    "std.net.tcp.connect",
];

pub(crate) fn lent_name(f: &str) -> String {
    format!("{f}__lent")
}

/// `f__lent`, with `sig` the signature of `f`, whose first parameter is the actor state.
pub(crate) fn lent(g: &Globals, f: &y::FnDef, sig: &Sig) -> y::FnDef {
    let state = sig.params[0].name.clone();
    let st = sig.params[0].ty.clone();
    let fail = super::actor::actor_fail(sig.fail.as_ref());
    let mut l = Lending {
        g,
        state,
        st: st.clone(),
        fail: fail.clone(),
        n: 0,
    };
    let mut body = f.body.clone().expect("lowered with a body");
    l.block(&mut body);
    let mut params = f.params.clone();
    params.push(y::Param::Value {
        comptime: false,
        conv: Conv::Let,
        name: HANDLE.into(),
        ty: Ty::Coll("Actor", vec![st, Ty::Param(POLICY.into())]).core(),
    });
    let mut tparams = f.tparams.clone();
    tparams.push(y::TParam {
        comptime: false,
        name: POLICY.into(),
        kind: y::Kind::Type,
        bounds: Vec::new(),
    });
    y::FnDef {
        is_pub: false,
        path: vec![lent_name(&f.path.join("."))],
        tparams,
        params,
        fail: Some(fail.core()),
        body: Some(body),
        ..f.clone()
    }
}

struct Lending<'g> {
    g: &'g Globals,
    state: String,
    st: Ty,
    fail: Ty,
    n: usize,
}

impl Lending<'_> {
    fn block(&mut self, b: &mut y::Block) {
        let stmts = mem::take(&mut b.stmts);
        let mut it = stmts.into_iter().peekable();
        while let Some(mut s) = it.next() {
            let at = s.span;
            // A handler call on another actor (`handler_call`'s shape): `take`, the join point
            // that continues with its outcome, then the match that runs the handler. The whole
            // call is one suspension.
            if is_take(&s) {
                let jp = it.next_if(|j| matches!(j.kind, StmtKind::Jp { .. }));
                let m = jp
                    .is_some()
                    .then(|| it.next_if(|m| matches!(m.kind, StmtKind::Match { .. })))
                    .flatten();
                let (Some(mut jp), Some(m)) = (jp, m) else {
                    unreachable!("`handler_call` emits take, jp, match");
                };
                if mentions(&m, &self.state) {
                    b.stmts.extend([s, jp, m]);
                    continue;
                }
                let since = self.lend(&mut b.stmts, at);
                let StmtKind::Jp { body, .. } = &mut jp.kind else {
                    unreachable!()
                };
                self.block(body);
                body.stmts.insert(0, self.retake(since, at));
                b.stmts.extend([s, jp, m]);
                continue;
            }
            match &mut s.kind {
                StmtKind::If { then, els, .. } => {
                    self.block(then);
                    if let Some(e) = els {
                        self.block(e);
                    }
                }
                StmtKind::Match { arms, .. } => arms.iter_mut().for_each(|(_, a)| self.block(a)),
                StmtKind::Loop { body, .. }
                | StmtKind::Jp { body, .. }
                | StmtKind::With { body, .. }
                | StmtKind::Deadline { body, .. }
                | StmtKind::Unsafe(body) => self.block(body),
                _ => {}
            }
            if self.suspends(&s) {
                let since = self.lend(&mut b.stmts, at);
                b.stmts.push(s);
                b.stmts.push(self.retake(since, at));
            } else {
                b.stmts.push(s);
            }
        }
    }

    /// A call that may wait and leaves the state alone.
    fn suspends(&self, s: &y::Stmt) -> bool {
        let rhs = match &s.kind {
            StmtKind::Let { rhs, .. } | StmtKind::Rhs(rhs) => rhs,
            StmtKind::Assign { place, rhs, .. } if place.root != self.state => rhs,
            _ => return false,
        };
        let c = match rhs {
            Rhs::Call(c) | Rhs::Try(c) | Rhs::Propagate { call: c, .. } => c,
            _ => return false,
        };
        let path = c.path.join(".");
        let waits = match path.strip_prefix("std.") {
            Some(_) => SUSPENDS.contains(&path.as_str()),
            None => self.g.fns.contains_key(&path),
        };
        waits
            && !c
                .args
                .iter()
                .any(|a| matches!(a, Arg::Value(_, Atom::Name(n, _), _) if *n == self.state))
    }

    /// `let since = std.actor.lend(h, inout state)`: the state goes back to the actor.
    fn lend(&mut self, out: &mut Vec<y::Stmt>, at: Span) -> String {
        self.n += 1;
        let since = format!("lent__g{}", self.n);
        let c = call(
            "std.actor.lend",
            vec![self.st.targ(), Ty::Param(POLICY.into()).targ()],
            vec![self.handle(at), self.inout_state(at)],
            at,
        );
        out.push(y::Stmt {
            kind: StmtKind::Let {
                mutable: false,
                pat: y::Pat::Bind(since.clone()),
                pat_span: at,
                ty: Ty::Int.core(),
                rhs: Rhs::Call(c),
                rhs_span: at,
            },
            span: at,
        });
        since
    }

    /// `std.actor.retake(h, since, inout state)?`: it fails when the actor stopped meanwhile.
    fn retake(&self, since: String, at: Span) -> y::Stmt {
        let c = call(
            "std.actor.retake",
            vec![
                self.st.targ(),
                Ty::Param(POLICY.into()).targ(),
                self.fail.targ(),
            ],
            vec![
                self.handle(at),
                let_arg(Atom::Name(since, at)),
                self.inout_state(at),
            ],
            at,
        );
        y::Stmt {
            kind: StmtKind::Rhs(Rhs::Propagate {
                call: c,
                via: None,
                drops: Vec::new(),
            }),
            span: at,
        }
    }

    fn handle(&self, at: Span) -> Arg {
        let_arg(Atom::Name(HANDLE.into(), at))
    }

    fn inout_state(&self, at: Span) -> Arg {
        Arg::Value(Conv::Inout, Atom::Name(self.state.clone(), at), at)
    }
}

fn is_take(s: &y::Stmt) -> bool {
    matches!(&s.kind, StmtKind::Let { rhs: Rhs::Call(c), .. } if c.path == ["std", "actor", "take"])
}

/// Whether `s` names `n` anywhere, closures included.
fn mentions(s: &y::Stmt, n: &str) -> bool {
    format!("{s:?}").contains(&format!("{n:?}"))
}
