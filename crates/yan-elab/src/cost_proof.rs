//! Static proof of `cost` bounds (R610, R925). The analysis gives each function an upper envelope
//! of linear forms over its integer parameters and the lengths of its list parameters: the cost
//! of every run is at most one of them. A sequence adds envelopes, a branch joins them, a counted
//! loop multiplies a constant body cost by its trip count, and a call costs the callee's `cost`
//! clause, or else its own envelope. The bound is proven when, for every form `c`, the linear
//! program `requires ∧ len ≥ 0 ∧ c ≥ bound + 1` has no integer solution. Anything outside the
//! fragment (a `while` loop, recursion, a call through a value, a nonlinear trip count) leaves the
//! bound unproven, never wrongly proven.

use std::collections::{HashMap, HashSet};

use yan_core::ast::{self as y, Arg, Atom, Conv, Literal, Rhs, StmtKind};
use yan_hir::raise::Std;

use crate::bounds::{Lin, len, unsat, var, with_lens};

/// The resources of a `cost` clause, as the interpreter counts them.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Resource {
    Steps,
    Alloc,
}

/// More forms than this in one envelope is outside the fragment.
const MAX_FORMS: usize = 32;

/// Whether `f`'s body provably stays within `bound` of `resource` on every input `requires` admits.
pub(crate) fn proves(items: &[y::Item], f: &y::FnDef, resource: Resource, bound: &y::Expr) -> bool {
    let prog = Prog::new(items);
    let Some((env, facts, forms)) = envelope(&prog, f, resource) else {
        return false;
    };
    let Some(b) = value_expr(&prog, &env, bound) else {
        return false;
    };
    forms.iter().all(|c| below(&facts, c, &b))
}

/// The inferred `cost` line of `f` (R610), shown where it has none: each resource's envelope as
/// its one dominating form, or the `max` of its forms.
pub fn inferred(items: &[y::Item], f: &y::FnDef) -> Option<String> {
    let prog = Prog::new(items);
    let mut parts = Vec::new();
    for (word, r) in [("steps", Resource::Steps), ("alloc", Resource::Alloc)] {
        let (_, facts, forms) = envelope(&prog, f, r)?;
        // A form another one covers drops out; of two that cover each other, the first stays.
        let top: Vec<&Lin> = forms
            .iter()
            .enumerate()
            .filter(|&(i, c)| {
                !forms
                    .iter()
                    .enumerate()
                    .any(|(j, d)| j != i && below(&facts, c, d) && (j < i || !below(&facts, d, c)))
            })
            .map(|(_, c)| c)
            .collect();
        let shown = match top.as_slice() {
            [one] => show(one),
            more => format!(
                "max({})",
                more.iter().map(|c| show(c)).collect::<Vec<_>>().join(", ")
            ),
        };
        parts.push(format!("{word} <= {shown}"));
    }
    Some(format!("cost {}", parts.join(", ")))
}

/// Whether `c <= b` wherever `facts` hold: `facts, c >= b + 1` has no integer solution.
fn below(facts: &[Lin], c: &Lin, b: &Lin) -> bool {
    let over = c.sub(b).add(&Lin::konst(-1));
    let cs = with_lens(&[facts.to_vec(), vec![over.clone()]].concat(), &over);
    unsat(&cs)
}

/// What is known at `f`'s entry, its `requires` facts, and its envelope for `resource`.
fn envelope(prog: &Prog, f: &y::FnDef, resource: Resource) -> Option<(Env, Vec<Lin>, Vec<Lin>)> {
    let mut a = An {
        prog,
        resource,
        stack: vec![f.path.join(".")],
    };
    let mut env = Env::default();
    for p in &f.params {
        if let y::Param::Value { conv, name, ty, .. } = p {
            env.declare(prog, name, ty, *conv);
            if is_int(ty) {
                env.vals.insert(name.clone(), Lin::term(var(name)));
            }
        }
    }
    let facts: Vec<Lin> = f
        .contracts
        .iter()
        .filter_map(|c| match c {
            y::Contract::Requires(e) => fact(&env, e),
            _ => None,
        })
        .flatten()
        .collect();
    let entry = env.clone();
    let forms = a.block(&f.body.as_ref()?.stmts, &mut env)?;
    Some((entry, facts, forms))
}

/// `c` in the surfaces' shared spelling: `2 * len(xs) + n + 1`.
fn show(c: &Lin) -> String {
    let mut parts: Vec<(bool, String)> = Vec::new();
    for (t, a) in &c.terms {
        let name = match t.split_once(':') {
            Some(("len", n)) => format!("len({n})"),
            Some((_, n)) => n.to_owned(),
            None => t.clone(),
        };
        let m = a.abs();
        parts.push((
            *a < 0,
            if m == 1 {
                name
            } else {
                format!("{m} * {name}")
            },
        ));
    }
    if c.k != 0 || parts.is_empty() {
        parts.push((c.k < 0, c.k.abs().to_string()));
    }
    let mut out = String::new();
    for (k, (neg, p)) in parts.into_iter().enumerate() {
        match (k, neg) {
            (0, true) => out += &format!("-{p}"),
            (0, false) => out += &p,
            (_, true) => out += &format!(" - {p}"),
            (_, false) => out += &format!(" + {p}"),
        }
    }
    out
}

/// The module's functions with bodies, the method names a call may dispatch to, and its
/// records and sums, which are values.
struct Prog<'a> {
    fns: HashMap<String, &'a y::FnDef>,
    methods: HashSet<String>,
    data: HashSet<String>,
}

impl<'a> Prog<'a> {
    fn new(items: &'a [y::Item]) -> Self {
        let mut fns = HashMap::new();
        let mut methods = HashSet::new();
        let mut data = HashSet::new();
        for it in items {
            match &it.kind {
                y::ItemKind::Record(r) => {
                    data.insert(r.path.join("."));
                }
                y::ItemKind::Sum(s) => {
                    data.insert(s.path.join("."));
                }
                y::ItemKind::Fn(d) | y::ItemKind::Inst(y::Inst { def: d, .. }) => {
                    if d.body.is_some() {
                        fns.insert(d.path.join("."), d);
                    }
                }
                y::ItemKind::Impl(i) => {
                    methods.extend(i.fns.iter().map(|d| d.path.join(".")));
                }
                _ => {}
            }
        }
        Prog { fns, methods, data }
    }
}

/// What is known at a point: linear values of integer locals, the parameter whose length each
/// list local has, and each local's type and whether it may hold a heap value.
#[derive(Clone, Default)]
struct Env {
    vals: HashMap<String, Lin>,
    lists: HashMap<String, String>,
    heap: HashMap<String, bool>,
}

impl Env {
    fn declare(&mut self, prog: &Prog, name: &str, ty: &y::Type, conv: Conv) {
        self.heap.insert(name.to_owned(), is_heap(prog, ty));
        if conv == Conv::Let && is_list(ty) {
            self.lists.insert(name.to_owned(), name.to_owned());
        }
    }

    /// Forget what `stmts` may change.
    fn forget(&mut self, stmts: &[y::Stmt]) {
        let mut names = HashSet::new();
        assigned(stmts, &mut names);
        for n in names {
            self.vals.remove(&n);
            self.lists.remove(&n);
        }
    }
}

struct An<'p, 'a> {
    prog: &'p Prog<'a>,
    resource: Resource,
    /// The functions being analyzed, for recursion.
    stack: Vec<String>,
}

impl An<'_, '_> {
    fn block(&mut self, stmts: &[y::Stmt], env: &mut Env) -> Option<Vec<Lin>> {
        let mut total = vec![Lin::konst(0)];
        for s in stmts {
            let c = self.stmt(s, env)?;
            total = seq(&total, &c)?;
        }
        Some(total)
    }

    fn stmt(&mut self, s: &y::Stmt, env: &mut Env) -> Option<Vec<Lin>> {
        let zero = || Some(vec![Lin::konst(0)]);
        match &s.kind {
            StmtKind::Let {
                mutable,
                pat,
                ty,
                rhs,
                ..
            } => {
                let c = self.rhs(rhs, Some(is_heap(self.prog, ty)), env)?;
                if let y::Pat::Bind(n) = pat {
                    env.heap.insert(n.clone(), is_heap(self.prog, ty));
                    match value_rhs(self.prog, env, rhs) {
                        Some(v) => env.vals.insert(n.clone(), v),
                        None => env.vals.remove(n),
                    };
                    match list_alias(env, rhs) {
                        Some(root) if !mutable => env.lists.insert(n.clone(), root),
                        _ => env.lists.remove(n),
                    };
                }
                Some(c)
            }
            StmtKind::Assign { place, rhs, .. } => {
                let heap = place
                    .projs
                    .is_empty()
                    .then(|| env.heap.get(&place.root).copied().unwrap_or(true));
                let c = self.rhs(rhs, Some(heap.unwrap_or(true)), env)?;
                let v = place
                    .projs
                    .is_empty()
                    .then(|| value_rhs(self.prog, env, rhs))
                    .flatten();
                match v {
                    Some(v) => env.vals.insert(place.root.clone(), v),
                    None => env.vals.remove(&place.root),
                };
                env.lists.remove(&place.root);
                Some(c)
            }
            StmtKind::Rhs(r) => self.rhs(r, None, env),
            StmtKind::If { then, els, .. } => {
                let mut arms = vec![then.stmts.as_slice()];
                arms.extend(els.as_ref().map(|b| b.stmts.as_slice()));
                if els.is_none() {
                    arms.push(&[]);
                }
                self.join(&arms, env)
            }
            StmtKind::Match { arms, .. } => {
                let arms: Vec<&[y::Stmt]> = arms.iter().map(|(_, b)| b.stmts.as_slice()).collect();
                self.join(&arms, env)
            }
            StmtKind::Loop { label, body, .. } => self.counted_loop(label, &body.stmts, env),
            StmtKind::Jp { body, .. } => {
                let mut inner = env.clone();
                let c = self.block(&body.stmts, &mut inner)?;
                env.forget(&body.stmts);
                Some(c)
            }
            StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::Unsafe(body) => {
                let c = self.block(&body.stmts, env)?;
                Some(c)
            }
            StmtKind::Par { blocks, .. } => {
                let mut total = vec![Lin::konst(0)];
                for b in blocks {
                    let mut inner = env.clone();
                    total = seq(&total, &self.block(&b.stmts, &mut inner)?)?;
                }
                for b in blocks {
                    env.forget(&b.stmts);
                }
                Some(total)
            }
            // A tail call costs what the call would; its work counts against this frame.
            StmtKind::Become(c) => self.call(c, Some(false), env),
            StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Jump { .. }
            | StmtKind::Ret(_)
            | StmtKind::Fail(_)
            | StmtKind::Assert(_)
            | StmtKind::Unreachable
            | StmtKind::Drop(_) => zero(),
            StmtKind::Yield(_)
            | StmtKind::ComptimeFor { .. }
            | StmtKind::ComptimeIf { .. }
            | StmtKind::Asm { .. }
            | StmtKind::Reuse { .. } => None,
        }
    }

    /// The join of `arms`, each from what is known now.
    fn join(&mut self, arms: &[&[y::Stmt]], env: &mut Env) -> Option<Vec<Lin>> {
        let mut out: Vec<Lin> = Vec::new();
        for a in arms {
            let mut inner = env.clone();
            for c in self.block(a, &mut inner)? {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        for a in arms {
            env.forget(a);
        }
        (out.len() <= MAX_FORMS).then_some(out)
    }

    /// A loop the elaborator emits for `for`: `let c = ge_i64(i, n)`, `if c { break }`, then
    /// `i = add_checked_i64(i, 1)` before any `continue`, with `i` assigned nowhere else and `n`
    /// unchanged. It runs `max(0, n - i0)` times.
    fn counted_loop(&mut self, label: &str, body: &[y::Stmt], env: &mut Env) -> Option<Vec<Lin>> {
        let [check, exit, rest @ ..] = body else {
            return None;
        };
        let StmtKind::Let {
            pat: y::Pat::Bind(c),
            rhs: Rhs::Call(cmp),
            ..
        } = &check.kind
        else {
            return None;
        };
        let StmtKind::If {
            cond: Atom::Name(cond, _),
            then,
            els: None,
        } = &exit.kind
        else {
            return None;
        };
        let breaks = matches!(then.stmts.as_slice(), [y::Stmt { kind: StmtKind::Break { label: l, .. }, .. }] if l == label);
        if cond != c || !breaks || cmp.path != ["ge_i64"] {
            return None;
        }
        let [Arg::Value(_, Atom::Name(i, _), _), Arg::Value(_, limit, _)] = cmp.args.as_slice()
        else {
            return None;
        };
        let mut changed = HashSet::new();
        assigned(body, &mut changed);
        if let Atom::Name(n, _) = limit
            && changed.contains(n)
        {
            return None;
        }
        let n = value_atom(env, limit)?;
        let i0 = env.vals.get(i)?.clone();
        // `i` steps by one, once per iteration, before anything can `continue`.
        let step = rest.iter().position(|s| increments(s, i))?;
        if rest[..step].iter().any(|s| continues(s, label)) {
            return None;
        }
        let mut others = HashSet::new();
        for (k, s) in rest.iter().enumerate() {
            if k != step {
                assigned(std::slice::from_ref(s), &mut others);
            }
        }
        if others.contains(i) {
            return None;
        }
        env.forget(body);
        let mut inner = env.clone();
        let per = self.block(rest, &mut inner)?;
        let back_edge = i128::from(self.resource == Resource::Steps);
        let trips = n.sub(&i0);
        let mut out = vec![Lin::konst(0)];
        for b in per {
            let b = b.add(&Lin::konst(back_edge));
            let form = if trips.terms.is_empty() {
                b.scale(trips.k.max(0))
            } else if b.terms.is_empty() {
                trips.scale(b.k)
            } else {
                // A trip count times a varying body cost is not linear.
                return None;
            };
            if !out.contains(&form) {
                out.push(form);
            }
        }
        (out.len() <= MAX_FORMS).then_some(out)
    }

    /// The cost of `r`; `heap` is whether its result may be a heap value, none for a statement.
    fn rhs(&mut self, r: &Rhs, heap: Option<bool>, env: &Env) -> Option<Vec<Lin>> {
        match r {
            Rhs::Call(c) | Rhs::Try(c) | Rhs::Propagate { call: c, .. } => self.call(c, heap, env),
            Rhs::Closure(k) => Some(vec![Lin::konst(i128::from(
                self.resource == Resource::Alloc && !k.captures.is_empty(),
            ))]),
            Rhs::Atom(_) | Rhs::Ctor(_) | Rhs::Proj(_) | Rhs::Copy(_) | Rhs::Move(_) => {
                Some(vec![Lin::konst(0)])
            }
        }
    }

    fn call(&mut self, c: &y::Call, heap: Option<bool>, env: &Env) -> Option<Vec<Lin>> {
        let name = c.path.join(".");
        if let Some(&d) = self.prog.fns.get(&name) {
            let callee = self.callee(d, c, env)?;
            let step = i128::from(self.resource == Resource::Steps);
            return Some(callee.iter().map(|f| f.add(&Lin::konst(step))).collect());
        }
        let suffix = format!(".{name}");
        if self
            .prog
            .methods
            .iter()
            .any(|m| m.ends_with(&suffix) || *m == name)
            || (c.path.len() == 1 && env.heap.contains_key(&name))
        {
            // A dispatched or closure call: its target is not known here.
            return None;
        }
        // A std operation: no steps; an alloc for a fresh heap result and one for each `inout`
        // argument it may grow (R925).
        if self.resource == Resource::Steps {
            return Some(vec![Lin::konst(0)]);
        }
        let fresh = match heap {
            Some(h) => h,
            None => crate::stdlib::StdInfo.returns_value(&name) || !known_std(&name),
        };
        let grown = c
            .args
            .iter()
            .filter(|a| match a {
                Arg::Value(Conv::Inout, Atom::Name(n, _), _) => {
                    env.heap.get(n).copied().unwrap_or(true)
                }
                _ => false,
            })
            .count() as i128;
        Some(vec![Lin::konst(i128::from(fresh) + grown)])
    }

    /// A module function's cost for this call: its `cost` clause as the specification, else its
    /// own envelope with the arguments' values.
    fn callee(&mut self, d: &y::FnDef, c: &y::Call, env: &Env) -> Option<Vec<Lin>> {
        let mut inner = Env::default();
        let args: Vec<&Atom> = c
            .args
            .iter()
            .filter_map(|a| match a {
                Arg::Value(_, a, _) => Some(a),
                Arg::Cap(..) => None,
            })
            .collect();
        let params: Vec<(&String, &y::Type, Conv)> = d
            .params
            .iter()
            .filter_map(|p| match p {
                y::Param::Value { name, ty, conv, .. } => Some((name, ty, *conv)),
                y::Param::Cap { .. } => None,
            })
            .collect();
        if args.len() != params.len() {
            return None;
        }
        for ((p, ty, conv), a) in params.iter().zip(&args) {
            inner.heap.insert((*p).clone(), is_heap(self.prog, ty));
            if let Some(v) = value_atom(env, a) {
                inner.vals.insert((*p).clone(), v);
            }
            if *conv == Conv::Let
                && let Atom::Name(n, _) = a
                && let Some(root) = env.lists.get(n)
            {
                inner.lists.insert((*p).clone(), root.clone());
            }
        }
        let clause = d.contracts.iter().find_map(|k| match (k, self.resource) {
            (y::Contract::Cost { steps: Some(e), .. }, Resource::Steps)
            | (y::Contract::Cost { alloc: Some(e), .. }, Resource::Alloc) => Some(e),
            _ => None,
        });
        if let Some(e) = clause {
            return Some(vec![value_expr(self.prog, &inner, e)?]);
        }
        let key = d.path.join(".");
        if self.stack.contains(&key) {
            return None;
        }
        self.stack.push(key);
        let body = &d.body.as_ref()?.stmts;
        let r = self.block(body, &mut inner);
        self.stack.pop();
        r
    }
}

/// Every sum of one form of `a` and one of `b`.
fn seq(a: &[Lin], b: &[Lin]) -> Option<Vec<Lin>> {
    let mut out: Vec<Lin> = Vec::new();
    for x in a {
        for y in b {
            let s = x.add(y);
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    (out.len() <= MAX_FORMS).then_some(out)
}

/// `i = add_checked_i64(i, 1)`.
fn increments(s: &y::Stmt, i: &str) -> bool {
    matches!(&s.kind, StmtKind::Assign { place, rhs: Rhs::Call(c), .. }
        if place.root == i && place.projs.is_empty() && c.path == ["add_checked_i64"]
            && matches!(c.args.as_slice(), [Arg::Value(_, Atom::Name(a, _), _), Arg::Value(_, Atom::Lit(Literal::Int(one), _), _)] if a == i && one == "1"))
}

fn continues(s: &y::Stmt, label: &str) -> bool {
    let mut hit = false;
    visit(std::slice::from_ref(s), &mut |s| {
        hit |= matches!(&s.kind, StmtKind::Continue { label: l } if l == label);
    });
    hit
}

/// The names `stmts` assign or bind, nested blocks included.
fn assigned(stmts: &[y::Stmt], out: &mut HashSet<String>) {
    visit(stmts, &mut |s| match &s.kind {
        StmtKind::Assign { place, .. } => {
            out.insert(place.root.clone());
        }
        StmtKind::Let {
            pat: y::Pat::Bind(n),
            ..
        } => {
            out.insert(n.clone());
        }
        StmtKind::Rhs(Rhs::Call(c))
        | StmtKind::Let {
            rhs: Rhs::Call(c), ..
        } => {
            for a in &c.args {
                if let Arg::Value(Conv::Inout, Atom::Name(n, _), _) = a {
                    out.insert(n.clone());
                }
            }
        }
        _ => {}
    });
}

fn visit(stmts: &[y::Stmt], f: &mut impl FnMut(&y::Stmt)) {
    for s in stmts {
        f(s);
        match &s.kind {
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                visit(&then.stmts, f);
                if let Some(e) = els {
                    visit(&e.stmts, f);
                }
            }
            StmtKind::Match { arms, .. } => {
                for (_, b) in arms {
                    visit(&b.stmts, f);
                }
            }
            StmtKind::Loop { body, .. }
            | StmtKind::Jp { body, .. }
            | StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::ComptimeFor { body, .. }
            | StmtKind::Unsafe(body) => visit(&body.stmts, f),
            StmtKind::Par { blocks, .. } => {
                for b in blocks {
                    visit(&b.stmts, f);
                }
            }
            _ => {}
        }
    }
}

fn value_atom(env: &Env, a: &Atom) -> Option<Lin> {
    match a {
        Atom::Lit(Literal::Int(s), _) => s.parse().ok().map(Lin::konst),
        Atom::Name(n, _) => env.vals.get(n).cloned(),
        Atom::Lit(..) => None,
    }
}

/// The linear value of an integer right-hand side.
fn value_rhs(prog: &Prog, env: &Env, r: &Rhs) -> Option<Lin> {
    match r {
        Rhs::Atom(a) => value_atom(env, a),
        Rhs::Call(c) => value_call(prog, env, c),
        Rhs::Copy(p) | Rhs::Move(p) if p.projs.is_empty() => env.vals.get(&p.root).cloned(),
        _ => None,
    }
}

fn value_expr(prog: &Prog, env: &Env, e: &y::Expr) -> Option<Lin> {
    match e {
        y::Expr::Atom(a) => value_atom(env, a),
        y::Expr::Call(c) => value_call(prog, env, c),
    }
}

fn value_call(prog: &Prog, env: &Env, c: &y::Call) -> Option<Lin> {
    let vals: Vec<&Atom> = c
        .args
        .iter()
        .filter_map(|a| match a {
            Arg::Value(_, a, _) => Some(a),
            Arg::Cap(..) => None,
        })
        .collect();
    let path: Vec<&str> = c.path.iter().map(String::as_str).collect();
    match (path.as_slice(), vals.as_slice()) {
        (["add_checked_i64"], [a, b]) => Some(value_atom(env, a)?.add(&value_atom(env, b)?)),
        (["sub_checked_i64"], [a, b]) => Some(value_atom(env, a)?.sub(&value_atom(env, b)?)),
        (["mul_checked_i64"], [a, b]) => {
            let (a, b) = (value_atom(env, a)?, value_atom(env, b)?);
            match (a.terms.is_empty(), b.terms.is_empty()) {
                (true, _) => Some(b.scale(a.k)),
                (_, true) => Some(a.scale(b.k)),
                _ => None,
            }
        }
        (["std", "list", "len"], [Atom::Name(xs, _)]) => {
            env.lists.get(xs).map(|root| Lin::term(len(root)))
        }
        _ => {
            // A generated bound helper: straight-line integer code ending in `ret`.
            let d = prog.fns.get(&c.path.join("."))?;
            if !c.path[0].starts_with("__cost_") {
                return None;
            }
            let mut inner = Env::default();
            for (p, a) in d
                .params
                .iter()
                .filter_map(|p| match p {
                    y::Param::Value { name, .. } => Some(name),
                    y::Param::Cap { .. } => None,
                })
                .zip(&vals)
            {
                if let Some(v) = value_atom(env, a) {
                    inner.vals.insert(p.clone(), v);
                }
                if let Atom::Name(n, _) = a
                    && let Some(root) = env.lists.get(n)
                {
                    inner.lists.insert(p.clone(), root.clone());
                }
            }
            for s in &d.body.as_ref()?.stmts {
                match &s.kind {
                    StmtKind::Let {
                        pat: y::Pat::Bind(n),
                        rhs,
                        ..
                    } => {
                        let v = value_rhs(prog, &inner, rhs)?;
                        inner.vals.insert(n.clone(), v);
                    }
                    StmtKind::Ret(a) => return value_atom(&inner, a),
                    _ => return None,
                }
            }
            None
        }
    }
}

/// The list parameter `r` reads the length of, when it is a plain copy of one.
fn list_alias(env: &Env, r: &Rhs) -> Option<String> {
    match r {
        Rhs::Atom(Atom::Name(n, _)) => env.lists.get(n).cloned(),
        Rhs::Copy(p) | Rhs::Move(p) if p.projs.is_empty() => env.lists.get(&p.root).cloned(),
        _ => None,
    }
}

/// A `requires` comparison of two linear atoms, as constraints `>= 0`.
fn fact(env: &Env, e: &y::Expr) -> Option<Vec<Lin>> {
    let y::Expr::Call(c) = e else { return None };
    let [Arg::Value(_, a, _), Arg::Value(_, b, _)] = c.args.as_slice() else {
        return None;
    };
    let (a, b) = (value_atom(env, a)?, value_atom(env, b)?);
    let one = Lin::konst(1);
    Some(match c.path.join(".").as_str() {
        "ge_i64" => vec![a.sub(&b)],
        "gt_i64" => vec![a.sub(&b).sub(&one)],
        "le_i64" => vec![b.sub(&a)],
        "lt_i64" => vec![b.sub(&a).sub(&one)],
        "eq_i64" => vec![a.sub(&b), b.sub(&a)],
        _ => return None,
    })
}

fn named(ty: &y::Type) -> Option<&y::Named> {
    match ty {
        y::Type::Named(n) => Some(n),
        y::Type::Refined { ty, .. } => named(ty),
        _ => None,
    }
}

fn is_int(ty: &y::Type) -> bool {
    named(ty).is_some_and(|n| n.path == ["i64"])
}

fn is_list(ty: &y::Type) -> bool {
    named(ty).is_some_and(|n| n.path == ["List"])
}

/// Whether a value of `ty` may be a heap value the interpreter counts (R925): anything but a
/// scalar, `Option`, `Result`, a time value, a tuple or a record or sum of the module. A type
/// parameter may be one.
fn is_heap(prog: &Prog, ty: &y::Type) -> bool {
    let Some(n) = named(ty) else {
        return false;
    };
    const VALUES: [&str; 25] = [
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "f32",
        "f64",
        "bool",
        "char",
        "Unit",
        "Never",
        "Option",
        "Result",
        "Duration",
        "Instant",
        "MonoInstant",
        "PlainDate",
        "PlainTime",
        "PlainDateTime",
        "Period",
    ];
    let name = n.path.join(".");
    !(VALUES.contains(&name.as_str()) || prog.data.contains(&name))
}

fn known_std(name: &str) -> bool {
    crate::stdlib::StdInfo.inserted(name) || crate::stdlib::StdInfo.params(name).is_some()
}
