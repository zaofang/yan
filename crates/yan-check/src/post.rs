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
    jps: HashMap<Name, (Pos, St)>,
    last_decl: Option<Name>,
    /// Names bound by `match` patterns, to the scrutinee local they view.
    views: HashMap<Name, Name>,
    /// Names bound to one field of a local (`let x = t._0`), to the key `t._0` that tracks the
    /// field in the state, so moving each field of a destructured tuple once is not a repeat.
    fields: HashMap<Name, Name>,
    diags: Vec<Diagnostic>,
    file: String,
    item: String,
}

/// Check optimized Yan Core. Empty when it holds.
pub fn check_optimized(file: &str, m: &Module) -> Vec<Diagnostic> {
    let mut v = V {
        file: file.to_owned(),
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

fn owned_params(params: &[Param]) -> Vec<(&Name, &Type)> {
    params
        .iter()
        .filter_map(|p| match p {
            Param::Value {
                conv: Conv::Sink,
                name,
                ty,
                ..
            } if !own::is_copy(ty) => Some((name, ty)),
            _ => None,
        })
        .collect()
}

fn is_flag_init(s: &Stmt) -> Option<&Name> {
    match &s.kind {
        StmtKind::Let {
            pat: Pat::Bind(f),
            rhs: Rhs::Atom(Atom::Lit(Literal::Bool(true), _)),
            ..
        } if f.starts_with("_d") => Some(f),
        _ => None,
    }
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
            self.run(
                &owned
                    .into_iter()
                    .map(|(n, _)| n.clone())
                    .collect::<Vec<_>>(),
                b,
            );
        }
    }

    fn err(&mut self, at: Span, msg: String) {
        let d = Diagnostic::error(CODE, &self.file, at, "", msg).with_item(&self.item);
        self.diags.push(d);
    }

    fn run(&mut self, owned: &[Name], b: &Block) {
        let saved = mem::take(self);
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
        for (i, s) in b.stmts.iter().enumerate() {
            if self.st.is_none() {
                break;
            }
            self.stmt(s, b.stmts.get(i + 1));
        }
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

    fn closure(&mut self, r: &Rhs) {
        if let Rhs::Closure(c) = r {
            let owned: Vec<Name> = owned_params(&c.params)
                .into_iter()
                .map(|(n, _)| n.clone())
                .collect();
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
                if let Atom::Name(x, _) = scrutinee {
                    let root = self.views.get(x).unwrap_or(x).clone();
                    if self.st.as_ref().is_some_and(|st| st.contains_key(&root)) {
                        for (p, _) in arms {
                            own::pat_binders(p, &mut bound);
                        }
                        for n in &bound {
                            self.views.insert(n.clone(), root.clone());
                        }
                    }
                }
                self.branches(arms.iter().map(|(_, b)| Some(b)));
                for n in &bound {
                    self.views.remove(n);
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
            StmtKind::Jp { name, params, body } => {
                let pos = (
                    self.scopes.len() - 1,
                    self.scopes.last().map_or(0, Vec::len),
                );
                let saved = self.st.clone().expect("reachable");
                self.jps.insert(name.clone(), (pos, saved.clone()));
                let owned: Vec<Name> = owned_params(params)
                    .into_iter()
                    .map(|(n, _)| n.clone())
                    .collect();
                self.block(body, &owned);
                self.st = Some(saved);
            }
            StmtKind::Jump { name, .. } => {
                self.consume(&s.kind, next, at, true);
                if let Some((pos, def)) = self.jps.get(name).cloned() {
                    self.require_gone(pos, at, "before the jump");
                    self.st = Some(self.restrict(&def));
                    self.same(&def, at, "the join point");
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
            StmtKind::Par { blocks, .. } => blocks.iter().for_each(|b| self.block(b, &[])),
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
