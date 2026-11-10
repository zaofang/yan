//! R769 at every exit, not only at binds: a borrowed (`let` or `inout`) parameter leaves by
//! `copy`, and so does a name that views one, a non-copy projection or a `match` binder. Without
//! the copy, `ret s`, a constructor field or a jump argument would move the caller's value, and
//! both owners would drop it.
//!
//! What consumes is `yan_check::own::consumed`, the one definition drop elaboration also uses.
//! The copy goes at the view's own definition (`let x: T = copy p.f`, or a copied scrutinee), or,
//! for the parameter itself, in a fresh local just before the consuming statement.

use std::collections::{HashMap, HashSet};

use yan_check::own;
use yan_core::ast::*;
use yan_diag::Span;

pub(crate) fn items(items: &mut [Item]) {
    for it in items {
        match &mut it.kind {
            ItemKind::Fn(f) | ItemKind::Inst(Inst { def: f, .. }) => body(f),
            ItemKind::Impl(i) => i.fns.iter_mut().for_each(body),
            _ => {}
        }
    }
}

/// How a borrowed name came to view its parameter.
#[derive(Clone)]
enum Def {
    Param,
    /// The `let` at this statement index binds a projection.
    Proj(usize),
    /// A binder of the `match` at this statement index, over `scrutinee`.
    Binder(usize, Name),
}

#[derive(Default)]
struct Plan {
    /// `let` statements whose projection becomes a `copy`.
    copy_proj: HashSet<usize>,
    /// `match` statements whose scrutinee is copied first, with the scrutinee's type.
    copy_scrutinee: HashMap<usize, Type>,
    /// Statements that consume a parameter directly, with the parameter and its type.
    copy_param: HashMap<usize, Vec<(Name, Type)>>,
}

struct Scan<'a> {
    /// Function that returns a view (4.5): its `ret` hands out the parameter's storage.
    returns_view: bool,
    params: &'a HashMap<Name, Type>,
    borrowed: HashMap<Name, Def>,
    types: HashMap<Name, Type>,
    plan: Plan,
    n: usize,
}

fn body(f: &mut FnDef) {
    let Some(b) = &mut f.body else {
        return;
    };
    let params: HashMap<Name, Type> = f
        .params
        .iter()
        .filter_map(|p| match p {
            Param::Value { conv, name, ty, .. } if *conv != Conv::Sink && !own::is_copy(ty) => {
                Some((name.clone(), ty.clone()))
            }
            _ => None,
        })
        .collect();
    if params.is_empty() {
        return;
    }
    let mut s = Scan {
        returns_view: !f.from.is_empty(),
        params: &params,
        borrowed: params.keys().map(|n| (n.clone(), Def::Param)).collect(),
        types: params.clone(),
        plan: Plan::default(),
        n: 0,
    };
    s.block(b);
    let mut r = Rewrite {
        plan: s.plan,
        n: 0,
        fresh: 0,
    };
    r.block(b);
}

/// The nested blocks of `k`, in source order.
fn blocks(k: &StmtKind) -> Vec<&Block> {
    match k {
        StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
            std::iter::once(then).chain(els).collect()
        }
        StmtKind::Match { arms, .. } => arms.iter().map(|(_, b)| b).collect(),
        StmtKind::Loop { body, .. }
        | StmtKind::Jp { body, .. }
        | StmtKind::With { body, .. }
        | StmtKind::Deadline { body, .. }
        | StmtKind::ComptimeFor { body, .. }
        | StmtKind::Unsafe(body) => vec![body],
        StmtKind::Par { blocks, .. } => blocks.iter().collect(),
        _ => Vec::new(),
    }
}

fn blocks_mut(k: &mut StmtKind) -> Vec<&mut Block> {
    match k {
        StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
            std::iter::once(then).chain(els).collect()
        }
        StmtKind::Match { arms, .. } => arms.iter_mut().map(|(_, b)| b).collect(),
        StmtKind::Loop { body, .. }
        | StmtKind::Jp { body, .. }
        | StmtKind::With { body, .. }
        | StmtKind::Deadline { body, .. }
        | StmtKind::ComptimeFor { body, .. }
        | StmtKind::Unsafe(body) => vec![body],
        StmtKind::Par { blocks, .. } => blocks.iter_mut().collect(),
        _ => Vec::new(),
    }
}

impl Scan<'_> {
    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            let at = self.n;
            self.n += 1;
            self.stmt(at, &s.kind);
            for b in blocks(&s.kind) {
                self.block(b);
            }
        }
    }

    fn stmt(&mut self, at: usize, k: &StmtKind) {
        let consumed: Vec<Name> = match k {
            StmtKind::Ret(_) if self.returns_view => Vec::new(),
            StmtKind::Let {
                rhs: Rhs::Closure(_),
                ..
            }
            | StmtKind::Rhs(Rhs::Closure(_)) => Vec::new(),
            k => own::consumed(k).into_iter().cloned().collect(),
        };
        for x in consumed {
            self.cut(at, &x);
        }
        match k {
            StmtKind::Let { pat, ty, rhs, .. } => {
                let mut bound = Vec::new();
                own::pat_binders(pat, &mut bound);
                for n in &bound {
                    self.borrowed.remove(n);
                }
                if let (Pat::Bind(x), Rhs::Proj(p)) = (pat, rhs)
                    && !own::is_copy(ty)
                    && self.borrowed.contains_key(&p.root)
                {
                    self.borrowed.insert(x.clone(), Def::Proj(at));
                }
                if let Pat::Bind(x) = pat {
                    self.types.insert(x.clone(), ty.clone());
                }
            }
            StmtKind::Match {
                scrutinee: Atom::Name(s, _),
                arms,
            } if self.borrowed.contains_key(s) => {
                for (p, _) in arms {
                    let mut bound = Vec::new();
                    own::pat_binders(p, &mut bound);
                    for n in bound {
                        self.borrowed.insert(n, Def::Binder(at, s.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    /// Make `x`, consumed at statement `at`, own what it leaves with.
    fn cut(&mut self, at: usize, x: &Name) {
        let Some(def) = self.borrowed.get(x).cloned() else {
            return;
        };
        match def {
            Def::Param => {
                let entry = self.plan.copy_param.entry(at).or_default();
                if !entry.iter().any(|(n, _)| n == x) {
                    entry.push((x.clone(), self.params[x].clone()));
                }
            }
            Def::Proj(d) => {
                self.plan.copy_proj.insert(d);
            }
            Def::Binder(m, s) => match self.types.get(&s) {
                Some(t) => {
                    self.plan.copy_scrutinee.insert(m, t.clone());
                }
                // A binder of a binder: copy the outer scrutinee, which has a declared type.
                None => self.cut(m, &s),
            },
        }
        self.borrowed.remove(x);
    }
}

struct Rewrite {
    plan: Plan,
    n: usize,
    fresh: usize,
}

fn copy_of(name: &Name, at: Span) -> Rhs {
    Rhs::Copy(Place {
        root: name.clone(),
        root_span: at,
        projs: Vec::new(),
    })
}

impl Rewrite {
    fn fresh(&mut self) -> Name {
        self.fresh += 1;
        format!("_cp{}", self.fresh)
    }

    fn copy_stmt(&mut self, from: &Name, ty: &Type, at: Span) -> (Name, Stmt) {
        let n = self.fresh();
        let s = Stmt {
            kind: StmtKind::Let {
                mutable: false,
                pat: Pat::Bind(n.clone()),
                pat_span: at,
                ty: ty.clone(),
                rhs: copy_of(from, at),
                rhs_span: at,
            },
            span: at,
        };
        (n, s)
    }

    fn block(&mut self, b: &mut Block) {
        let mut out = Vec::with_capacity(b.stmts.len());
        for mut s in std::mem::take(&mut b.stmts) {
            let at = self.n;
            self.n += 1;
            if self.plan.copy_proj.contains(&at)
                && let StmtKind::Let { rhs, .. } = &mut s.kind
                && let Rhs::Proj(p) = rhs
            {
                *rhs = Rhs::Copy(p.clone());
            }
            if let Some(t) = self.plan.copy_scrutinee.remove(&at)
                && let StmtKind::Match { scrutinee, .. } = &mut s.kind
                && let Atom::Name(x, sp) = scrutinee
            {
                let (n, c) = self.copy_stmt(x, &t, *sp);
                out.push(c);
                *x = n;
            }
            for (p, t) in self.plan.copy_param.remove(&at).unwrap_or_default() {
                self.param(&p, &t, &mut s, &mut out);
            }
            for b in blocks_mut(&mut s.kind) {
                self.block(b);
            }
            out.push(s);
        }
        b.stmts = out;
    }

    /// `s` consumes parameter `p` directly.
    fn param(&mut self, p: &Name, t: &Type, s: &mut Stmt, out: &mut Vec<Stmt>) {
        let at = s.span;
        if let StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } = &mut s.kind {
            match rhs {
                Rhs::Atom(Atom::Name(x, sp)) if x == p => {
                    *rhs = copy_of(p, *sp);
                    return;
                }
                Rhs::Move(pl) if pl.root == *p => {
                    *rhs = Rhs::Copy(pl.clone());
                    return;
                }
                _ => {}
            }
        }
        let (n, c) = self.copy_stmt(p, t, at);
        out.push(c);
        rename(&mut s.kind, p, &n);
    }
}

fn rename_atom(a: &mut Atom, from: &Name, to: &Name) {
    if let Atom::Name(n, _) = a
        && n == from
    {
        *n = to.clone();
    }
}

fn rename_call(c: &mut Call, from: &Name, to: &Name) {
    for a in &mut c.args {
        if let Arg::Value(Conv::Sink, a, _) = a {
            rename_atom(a, from, to);
        }
    }
}

/// Rename `from` where the statement itself consumes it, as `own::consumed` lists.
fn rename(k: &mut StmtKind, from: &Name, to: &Name) {
    match k {
        StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) => {
            match rhs {
                Rhs::Atom(a) => rename_atom(a, from, to),
                Rhs::Call(c) | Rhs::Propagate { call: c, .. } | Rhs::Try(c) => {
                    rename_call(c, from, to)
                }
                Rhs::Ctor(Ctor::Named { fields, .. }) => fields
                    .iter_mut()
                    .for_each(|(_, a)| rename_atom(a, from, to)),
                Rhs::Ctor(Ctor::Tuple(xs)) => xs.iter_mut().for_each(|a| rename_atom(a, from, to)),
                _ => {}
            }
        }
        StmtKind::Ret(a) | StmtKind::Fail(a) | StmtKind::Yield(a) => rename_atom(a, from, to),
        StmtKind::Break { value: Some(a), .. } => rename_atom(a, from, to),
        StmtKind::Jump { args, .. } => args.iter_mut().for_each(|a| rename_atom(a, from, to)),
        StmtKind::Become(c) => rename_call(c, from, to),
        StmtKind::With { handler, .. } => rename_atom(handler, from, to),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    fn yir(src: &str) -> String {
        let (m, d) = yan_yag::parse("t.yag", src, false);
        assert!(d.is_empty(), "{d:?}");
        let (m, d) = crate::elaborate("t.yag", src, &m.expect("parses"), Default::default())
            .expect("supported");
        assert!(d.is_empty(), "{d:?}");
        let checked = yan_check::check("t.yag", src, &m);
        assert!(checked.is_empty(), "{checked:?}");
        yan_core::print(&m)
    }

    fn has(text: &str, lines: &[&str]) {
        let mut rest = text;
        for l in lines {
            let k = rest
                .find(l)
                .unwrap_or_else(|| panic!("no `{l}` in order in\n{text}"));
            rest = &rest[k + l.len()..];
        }
    }

    #[test]
    fn a_borrowed_parameter_leaves_by_copy_at_ret_field_and_jump() {
        let text = yir("class Pt:\n    tag: str | None\n    xs: list[float]\n\n\
             def echo(p: Pt, t: tuple[int, str]) -> Pt:\n    return Pt(tag=t[1], xs=p.xs)\n\n\
             def same(s: str) -> str:\n    return s\n\n\
             def pair(s: str) -> tuple[str, int]:\n    return (s, 1)\n\n\
             def some(o: str | None) -> str:\n    if o is not None:\n        return o\n    return \"n\"\n");
        has(
            &text,
            &[
                "let _t1: Str = copy t._1",
                "let _t3: List[f64] = copy p.xs",
                "let _cp1: Str = copy s\n    ret _cp1",
                "let _cp1: Str = copy s\n    let _t1: Tuple(Str, i64) = (_cp1, 1)",
                "let _cp1: Option[Str] = copy o\n            match _cp1",
            ],
        );
    }

    #[test]
    fn a_projection_only_read_stays_a_view() {
        let text = yir("def n(t: tuple[int, str]) -> int:\n    return t[1].char_count()\n");
        assert!(!text.contains("copy"), "{text}");
    }
}
