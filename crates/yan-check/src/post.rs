//! Post-optimization check (RFC 0028 D2, D3): every owned, non-copy local is consumed or dropped
//! exactly once on every path, and the drop-flag pattern of 5.3 is well formed.
//!
//! A local is `Live` until consumed or dropped, `Gone` after, `Both` where paths disagree. A
//! flagged local (a `var _dN: bool = true` right after its declaration) stays `Live` when moved,
//! provided `_dN = false` follows; only `if _dN { drop x }` makes it `Gone`.

use std::collections::HashMap;
use std::mem;

use yan_core::ast::*;
use yan_diag::{Diagnostic, Span};

use crate::flags::{is_flag_init, owned_params};
use crate::own;

const CODE: &str = "I001";

#[derive(Clone, Copy, PartialEq, Debug)]
enum S {
    Live,
    Gone,
    Both,
}

type St = HashMap<Name, S>;
type Pos = (usize, usize);

fn join(a: S, b: S) -> S {
    if a == b { a } else { S::Both }
}

struct Local {
    name: Name,
    flag: Option<Name>,
}

/// A join point's body is entered only by its jumps: the first jump fixes the state it expects
/// (`entry`) and every other jump must match it. A body no jump reaches starts from `decl`.
#[derive(Clone)]
struct Jp {
    pos: Pos,
    decl: St,
    entry: Option<St>,
}

struct Loop {
    label: Name,
    pos: Pos,
    entry: St,
    brk: Option<St>,
}

#[derive(Default)]
struct V {
    scopes: Vec<Vec<Local>>,
    /// `None` after a statement that does not continue.
    st: Option<St>,
    loops: Vec<Loop>,
    jps: HashMap<Name, Jp>,
    last_decl: Option<Name>,
    /// Names bound by `match` patterns, to the scrutinee local they view.
    views: HashMap<Name, Name>,
    /// Names bound to one field of a local (`let x = t._0`), to the key `t._0` that tracks the
    /// field in the state, so moving each field of a destructured tuple once is not a repeat.
    fields: HashMap<Name, Name>,
    copy: own::CopyFields,
    diags: Vec<Diagnostic>,
    file: String,
    item: String,
}

/// Check optimized Yan Core. Empty when it holds.
pub fn check_optimized(file: &str, m: &Module) -> Vec<Diagnostic> {
    let mut v = V {
        file: file.to_owned(),
        copy: own::copy_fields(m),
        ..V::default()
    };
    for it in &m.items {
        match &it.kind {
            ItemKind::Fn(f) | ItemKind::Inst(Inst { def: f, .. }) => v.func(&m.path, f),
            ItemKind::Impl(i) => i.fns.iter().for_each(|f| v.func(&m.path, f)),
            ItemKind::Test(t) => {
                v.item = format!("{}.{}", m.path.join("."), t.path.join("."));
                v.run(&[], &t.body);
            }
            _ => {}
        }
    }
    v.diags
}

fn sets_flag(s: Option<&Stmt>, f: &str, v: bool) -> bool {
    matches!(s.map(|s| &s.kind), Some(StmtKind::Assign {
        place: Place { root, projs, .. },
        rhs: Rhs::Atom(Atom::Lit(Literal::Bool(b), _)),
        ..
    }) if root == f && projs.is_empty() && *b == v)
}

impl V {
    fn func(&mut self, path: &Path, f: &FnDef) {
        self.item = format!("{}.{}", path.join("."), f.path.join("."));
        if let Some(b) = &f.body {
            let owned = owned_params(&f.params);
            self.run(&owned.into_iter().cloned().collect::<Vec<_>>(), b);
        }
    }

    fn err(&mut self, at: Span, msg: String) {
        let d = Diagnostic::error(CODE, &self.file, at, "", msg).with_item(&self.item);
        self.diags.push(d);
    }

    fn run(&mut self, owned: &[Name], b: &Block) {
        let saved = mem::take(self);
        self.copy = saved.copy.clone();
        self.file = saved.file.clone();
        self.item = saved.item.clone();
        self.st = Some(St::new());
        self.block(b, owned);
        let mut diags = saved.diags;
        diags.append(&mut self.diags);
        *self = V { diags, ..saved };
    }

    fn consumed(&self, k: &StmtKind) -> Vec<Name> {
        own::consumed(k)
            .into_iter()
            .map(|n| {
                self.fields
                    .get(n)
                    .or_else(|| self.views.get(n))
                    .unwrap_or(n)
                    .clone()
            })
            .collect()
    }

    fn flag_of(&self, x: &str) -> Option<Name> {
        self.scopes
            .iter()
            .flatten()
            .find(|l| l.name == x)
            .and_then(|l| l.flag.clone())
    }

    fn declare(&mut self, n: &Name) {
        self.scopes.last_mut().expect("in a scope").push(Local {
            name: n.clone(),
            flag: None,
        });
        if let Some(st) = &mut self.st {
            st.insert(n.clone(), S::Live);
        }
        self.last_decl = Some(n.clone());
    }

    fn declare_pat(&mut self, p: &Pat, ty: &Type) {
        match (p, ty) {
            (Pat::Bind(n), _) if !own::is_copy(ty) => self.declare(n),
            (Pat::Tuple(ps), Type::Tuple(ts)) if ps.len() == ts.len() => {
                ps.iter().zip(ts).for_each(|(p, t)| self.declare_pat(p, t));
            }
            _ => {}
        }
    }

    fn block(&mut self, b: &Block, owned: &[Name]) {
        self.scopes.push(Vec::new());
        self.last_decl = None;
        owned.iter().for_each(|n| self.declare(n));
        let (pending, prev) = self.seq(&b.stmts);
        self.jp_bodies(pending, false);
        self.restore_jps(prev);
        let end = b.stmts.last().map(|s| s.span).unwrap_or_default();
        if self.st.is_some() {
            let top = self.scopes.len() - 1;
            self.require_gone((top, 0), end, "at the end of its scope");
        }
        for l in self.scopes.pop().unwrap_or_default() {
            if let Some(st) = &mut self.st {
                st.remove(&l.name);
                let prefix = format!("{}.", l.name);
                st.retain(|k, _| !k.starts_with(&prefix));
            }
        }
    }

    /// Walk `stmts` up to the first that does not continue. A join point is registered, and its
    /// body is left to [`Self::jp_bodies`], once its jumps have been seen. Also returns the join
    /// points these shadow.
    fn seq<'b>(&mut self, stmts: &'b [Stmt]) -> (Vec<&'b Stmt>, Vec<(Name, Option<Jp>)>) {
        let mut pending = Vec::new();
        let mut prev = Vec::new();
        for (i, s) in stmts.iter().enumerate() {
            let Some(st) = &self.st else { break };
            if let StmtKind::Jp { name, .. } = &s.kind {
                let jp = Jp {
                    pos: (
                        self.scopes.len() - 1,
                        self.scopes.last().map_or(0, Vec::len),
                    ),
                    decl: st.clone(),
                    entry: None,
                };
                prev.push((name.clone(), self.jps.insert(name.clone(), jp)));
                pending.push(s);
            } else {
                self.stmt(s, stmts.get(i + 1));
            }
        }
        (pending, prev)
    }

    fn restore_jps(&mut self, prev: Vec<(Name, Option<Jp>)>) {
        for (name, jp) in prev.into_iter().rev() {
            match jp {
                Some(jp) => self.jps.insert(name, jp),
                None => self.jps.remove(&name),
            };
        }
    }

    /// Walk the bodies of `pending`, each from the state its jumps expect, reached ones first
    /// since a body may jump to another. Without `hoisted` a body is a scope of its own and the
    /// state afterwards is the one before; with it (a `par` branch) the body declares into the
    /// enclosing scope and the state afterwards is its end.
    fn jp_bodies(&mut self, mut pending: Vec<&Stmt>, hoisted: bool) {
        let rest = self.st.take();
        let mut end = None;
        while !pending.is_empty() {
            let i = pending
                .iter()
                .position(|s| matches!(&s.kind, StmtKind::Jp { name, .. } if self.jps[name].entry.is_some()))
                .unwrap_or(0);
            let StmtKind::Jp { name, params, body } = &pending.remove(i).kind else {
                unreachable!("only join points are pending");
            };
            let jp = self.jps[name].clone();
            self.st = Some(jp.entry.unwrap_or(jp.decl));
            let owned: Vec<Name> = owned_params(params).into_iter().cloned().collect();
            // The body sees only what was declared before the join point.
            let after = self.scopes[jp.pos.0].split_off(jp.pos.1);
            if hoisted {
                owned.iter().for_each(|n| self.declare(n));
                let (inner, prev) = self.seq(&body.stmts);
                self.jp_bodies(inner, false);
                self.restore_jps(prev);
                end = Some(self.st.take());
            } else {
                self.block(body, &owned);
                self.scopes[jp.pos.0].extend(after);
            }
        }
        self.st = end.unwrap_or(rest);
    }

    /// Everything declared at or after `pos` is gone.
    fn require_gone(&mut self, pos: Pos, at: Span, where_: &str) {
        let Some(st) = self.st.clone() else { return };
        let mut bad = Vec::new();
        for (si, scope) in self.scopes.iter().enumerate() {
            if si < pos.0 {
                continue;
            }
            let from = if si == pos.0 { pos.1 } else { 0 };
            for l in &scope[from..] {
                if st.get(&l.name) != Some(&S::Gone) {
                    bad.push(l.name.clone());
                }
            }
        }
        for n in bad {
            self.err(
                at,
                format!("`{n}` is not consumed or dropped on every path {where_}"),
            );
        }
    }

    /// The failing edge of `?`: sink arguments are consumed by the call, the drop list drops the
    /// rest (a flagged local under its flag), and nothing may stay owned.
    fn failing_edge(&mut self, r: &Rhs, k: &StmtKind, at: Span) {
        let Rhs::Propagate { drops, .. } = r else {
            return;
        };
        let Some(mut st) = self.st.clone() else {
            return;
        };
        for n in &self.consumed(k) {
            if let Some(s) = st.get_mut(n) {
                *s = S::Gone;
            }
        }
        for n in drops {
            match st.get(n).copied() {
                None => self.err(
                    at,
                    format!("`{n}` in the `?` drop list is not an owned local"),
                ),
                Some(S::Live) => {
                    st.insert(n.clone(), S::Gone);
                }
                Some(_) if self.flag_of(n).is_some() => {}
                Some(_) => self.err(at, format!("`{n}` was already consumed or dropped")),
            }
        }
        let saved = self.st.replace(st);
        self.require_gone((0, 0), at, "on the failing edge of `?`");
        self.st = saved;
    }

    fn consume(&mut self, k: &StmtKind, next: Option<&Stmt>, at: Span, exits: bool) {
        for n in &self.consumed(k) {
            let Some(s) = self.st.as_ref().and_then(|st| st.get(n)).copied() else {
                continue;
            };
            if let Some(f) = self.flag_of(n) {
                if matches!(k, StmtKind::Drop(_)) {
                    self.err(at, format!("`{n}` has a drop flag; drop it under `if {f}`"));
                } else if !exits && !sets_flag(next, &f, false) {
                    self.err(at, format!("`{f}` is not cleared after `{n}` moved"));
                }
                continue;
            }
            if s != S::Live {
                let how = if s == S::Gone { "was" } else { "may have been" };
                self.err(at, format!("`{n}` {how} already consumed or dropped"));
            }
            if let Some(st) = &mut self.st {
                st.insert(n.clone(), S::Gone);
            }
            if let Some((root, _)) = n.split_once('.')
                && s == S::Live
            {
                self.move_field_root(root, n, at);
            }
        }
    }

    /// Moving a field moves its root, whose other fields leak (as in drop elaboration); moving
    /// another field of a root already moved through a field is not a repeat.
    fn move_field_root(&mut self, root: &str, key: &str, at: Span) {
        let Some(st) = &mut self.st else { return };
        let Some(s) = st.get(root).copied() else {
            return;
        };
        let prefix = format!("{root}.");
        let by_field = st
            .iter()
            .any(|(k, v)| k != key && k.starts_with(&prefix) && *v != S::Live);
        if s != S::Live && !by_field {
            let how = if s == S::Gone { "was" } else { "may have been" };
            self.err(at, format!("`{root}` {how} already consumed or dropped"));
        }
        if let Some(st) = &mut self.st {
            st.insert(root.to_owned(), S::Gone);
        }
    }

    /// A field bound whole by a record pattern is tracked like `let x = root.f`, so a pattern
    /// that binds two owned fields can move each once.
    fn track_pattern_fields<'a>(
        &mut self,
        root: &str,
        pats: impl Iterator<Item = &'a Pat>,
        bound: &mut Vec<Name>,
    ) {
        if self.flag_of(root).is_some() {
            return;
        }
        for p in pats {
            let Pat::Ctor {
                path,
                fields: Some((fs, _)),
            } = p
            else {
                continue;
            };
            let ctor = path.last().cloned().unwrap_or_default();
            for (f, sub) in fs {
                let Pat::Bind(n) = sub else { continue };
                if self.copy.contains(&(ctor.clone(), f.clone())) {
                    continue;
                }
                let key = format!("{root}.{f}");
                if let Some(st) = &mut self.st {
                    st.entry(key.clone()).or_insert(S::Live);
                }
                self.fields.insert(n.clone(), key);
                bound.push(n.clone());
            }
        }
    }

    fn closure(&mut self, r: &Rhs) {
        if let Rhs::Closure(c) = r {
            let owned: Vec<Name> = owned_params(&c.params).into_iter().cloned().collect();
            self.run(&owned, &c.body);
        }
    }

    fn branches<'b>(&mut self, bs: impl IntoIterator<Item = Option<&'b Block>>) {
        let pre = self.st.clone().expect("reachable");
        let mut ends: Vec<St> = Vec::new();
        for b in bs {
            self.st = Some(pre.clone());
            if let Some(b) = b {
                self.block(b, &[]);
            }
            if let Some(st) = self.st.take() {
                ends.push(st);
            }
        }
        self.st = merge(&pre, ends);
    }

    fn same(&mut self, want: &St, at: Span, what: &str) {
        let Some(st) = self.st.clone() else { return };
        let mut names: Vec<&Name> = want.keys().collect();
        names.sort();
        for n in names {
            if st.get(n) != Some(&want[n]) {
                self.err(at, format!("`{n}` is not in the state {what} expects"));
            }
        }
    }

    fn restrict(&self, within: &St) -> St {
        let st = self.st.as_ref();
        within
            .keys()
            .filter_map(|k| Some((k.clone(), *st?.get(k)?)))
            .collect()
    }

    fn stmt(&mut self, s: &Stmt, next: Option<&Stmt>) {
        let at = s.span;
        let decl = self.last_decl.take();
        if let StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) =
            &s.kind
        {
            self.failing_edge(rhs, &s.kind, at);
        }
        match &s.kind {
            StmtKind::Let { pat, ty, rhs, .. } => {
                if let (Some(f), Some(x)) = (is_flag_init(s), decl) {
                    if let Some(l) = self
                        .scopes
                        .last_mut()
                        .and_then(|sc| sc.iter_mut().find(|l| l.name == x))
                    {
                        l.flag = Some(f.clone());
                    }
                    return;
                }
                self.closure(rhs);
                self.consume(&s.kind, next, at, false);
                if let Pat::Bind(x) = pat {
                    self.fields.remove(x);
                }
                if let (Rhs::Proj(p), Pat::Bind(x)) = (rhs, pat)
                    && !own::is_copy(ty)
                {
                    let root = self.views.get(&p.root).unwrap_or(&p.root).clone();
                    if let [Proj::Field(f, _)] = p.projs.as_slice()
                        && root == p.root
                        && let Some(st) = &mut self.st
                        && st.contains_key(&root)
                    {
                        let key = format!("{root}.{f}");
                        st.entry(key.clone()).or_insert(S::Live);
                        self.fields.insert(x.clone(), key);
                    }
                    self.views.insert(x.clone(), root);
                } else if let Pat::Bind(x) = pat {
                    self.views.remove(x);
                }
                if !matches!(rhs, Rhs::Proj(_)) {
                    self.declare_pat(pat, ty);
                }
            }
            StmtKind::Assign { place, rhs, .. } => {
                self.closure(rhs);
                self.consume(&s.kind, next, at, false);
                let x = &place.root;
                if place.projs.is_empty()
                    && let Some(old) = self.st.as_ref().and_then(|st| st.get(x)).copied()
                {
                    if old != S::Gone {
                        self.err(
                            at,
                            format!("`{x}` is overwritten while it still owns a value"),
                        );
                    }
                    if let Some(f) = self.flag_of(x) {
                        if !sets_flag(next, &f, true) {
                            self.err(at, format!("`{f}` is not set after `{x}` is assigned"));
                        }
                        // the flag, not the state, tracks a flagged local; it is armed again
                        self.st
                            .as_mut()
                            .expect("reachable")
                            .insert(x.clone(), S::Live);
                    } else {
                        self.st
                            .as_mut()
                            .expect("reachable")
                            .insert(x.clone(), S::Live);
                    }
                }
            }
            StmtKind::Rhs(rhs) => {
                self.closure(rhs);
                self.consume(&s.kind, next, at, false);
            }
            StmtKind::Yield(_) => self.consume(&s.kind, next, at, false),
            StmtKind::Drop(_) | StmtKind::Reuse { .. } => self.consume(&s.kind, next, at, false),
            StmtKind::If {
                cond,
                then,
                els: None,
            } if self.guarded(cond, then).is_some() => {
                let x = self.guarded(cond, then).expect("checked");
                if let Some(st) = &mut self.st {
                    st.insert(x, S::Gone);
                }
            }
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                self.branches([Some(then), els.as_ref()]);
            }
            StmtKind::Match { scrutinee, arms } => {
                let mut bound = Vec::new();
                let mut tracked = None;
                if let Atom::Name(x, _) = scrutinee {
                    let root = self.views.get(x).unwrap_or(x).clone();
                    if self.st.as_ref().is_some_and(|st| st.contains_key(&root)) {
                        for (p, _) in arms {
                            own::view_binders(p, &self.copy, &mut bound);
                        }
                        for n in &bound {
                            self.views.insert(n.clone(), root.clone());
                        }
                        tracked = (root == *x).then_some(root);
                    }
                }
                let pre = self.st.clone().expect("reachable");
                let mut ends = Vec::new();
                for (p, b) in arms {
                    self.st = Some(pre.clone());
                    if let Some(root) = &tracked {
                        self.track_pattern_fields(root, std::iter::once(p), &mut bound);
                    }
                    self.block(b, &[]);
                    if let Some(st) = self.st.take() {
                        ends.push(st);
                    }
                }
                self.st = merge(&pre, ends);
                for n in &bound {
                    self.views.remove(n);
                    self.fields.remove(n);
                }
            }
            StmtKind::Loop { label, body, .. } => {
                let entry = self.st.clone().expect("reachable");
                self.loops.push(Loop {
                    label: label.clone(),
                    pos: (self.scopes.len(), 0),
                    entry: entry.clone(),
                    brk: None,
                });
                self.block(body, &[]);
                if self.st.is_some() {
                    let lp = self.restrict(&entry);
                    self.st = Some(lp);
                    self.same(&entry, at, "the next loop iteration");
                }
                let lp = self.loops.pop().expect("pushed above");
                self.st = lp.brk;
            }
            StmtKind::ComptimeFor { body, .. } => {
                let entry = self.st.clone().expect("reachable");
                self.block(body, &[]);
                if self.st.is_some() {
                    self.st = Some(self.restrict(&entry));
                    self.same(&entry, at, "the next iteration");
                }
                self.st = Some(entry);
            }
            StmtKind::Break { label, .. } | StmtKind::Continue { label } => {
                let Some(i) = self
                    .loops
                    .iter()
                    .rposition(|l| l.label == *label)
                    .or(self.loops.len().checked_sub(1))
                else {
                    self.st = None;
                    return;
                };
                self.consume(&s.kind, next, at, true);
                let (pos, entry) = (self.loops[i].pos, self.loops[i].entry.clone());
                self.require_gone(pos, at, "before leaving the loop body");
                self.st = Some(self.restrict(&entry));
                if matches!(s.kind, StmtKind::Break { .. }) {
                    match self.loops[i].brk.clone() {
                        None => self.loops[i].brk = self.st.clone(),
                        Some(t) => self.same(&t, at, "the other `break`s"),
                    }
                } else {
                    self.same(&entry, at, "the next loop iteration");
                }
                self.st = None;
            }
            StmtKind::Jp { .. } => unreachable!("join points are walked by `seq`"),
            StmtKind::Jump { name, .. } => {
                self.consume(&s.kind, next, at, true);
                if let Some(jp) = self.jps.get(name).cloned() {
                    self.require_gone(jp.pos, at, "before the jump");
                    match jp.entry {
                        Some(want) => {
                            self.st = Some(self.restrict(&want));
                            self.same(&want, at, "the join point");
                        }
                        None => {
                            let entry = self.restrict(&jp.decl);
                            self.jps.get_mut(name).expect("found").entry = Some(entry);
                        }
                    }
                }
                self.st = None;
            }
            StmtKind::Ret(_) | StmtKind::Fail(_) | StmtKind::Become(_) => {
                self.consume(&s.kind, next, at, true);
                self.require_gone((0, 0), at, "before leaving the function");
                self.st = None;
            }
            StmtKind::Unreachable => self.st = None,
            StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::Unsafe(body) => {
                self.consume(&s.kind, next, at, false);
                self.block(body, &[]);
            }
            // Branch bindings stay visible after the `par`: they belong to the enclosing scope.
            // A join point at the top of a branch (`r = f() else x`) declares its body into the
            // enclosing scope too, and the code after the `par` continues from the body's end.
            StmtKind::Par { blocks, .. } => blocks.iter().for_each(|b| {
                let (pending, prev) = self.seq(&b.stmts);
                self.jp_bodies(pending, true);
                self.restore_jps(prev);
            }),
            StmtKind::Assert(_) | StmtKind::Asm { .. } => {}
        }
    }

    /// `if _dN { drop x }` for the flag of `x`.
    fn guarded(&self, cond: &Atom, then: &Block) -> Option<Name> {
        let (
            Atom::Name(f, _),
            [
                Stmt {
                    kind: StmtKind::Drop(x),
                    ..
                },
            ],
        ) = (cond, &then.stmts[..])
        else {
            return None;
        };
        (self.flag_of(x).as_deref() == Some(f)).then(|| x.clone())
    }
}

fn merge(pre: &St, ends: Vec<St>) -> Option<St> {
    let mut it = ends.into_iter();
    let first = it.next()?;
    Some(
        pre.keys()
            .filter_map(|k| {
                let v = it.clone().fold(*first.get(k)?, |a, e| join(a, e[k]));
                Some((k.clone(), v))
            })
            .collect(),
    )
}
