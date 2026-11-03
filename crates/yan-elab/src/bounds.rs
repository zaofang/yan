//! The 8.6 bounds solver: linear facts over integer locals and list lengths, decided by
//! Fourier-Motzkin elimination with integer tightening. An index it proves needs no check, one it
//! refutes is E501, and one it cannot decide keeps its check and gets W820.

use std::collections::{BTreeMap, HashMap, HashSet};

use yan_diag::Span;
use yan_hir as j;
use yan_hir::ExprKind as K;

/// What an index expression indexes, as elaboration found it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Site {
    List,
    Dict,
}

pub(crate) struct Finding {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
}

/// `sum(coef * term) + k >= 0`, or as a value, `sum(coef * term) + k`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
struct Lin {
    terms: BTreeMap<String, i128>,
    k: i128,
}

impl Lin {
    fn konst(k: i128) -> Lin {
        Lin {
            terms: BTreeMap::new(),
            k,
        }
    }

    fn term(t: String) -> Lin {
        Lin {
            terms: BTreeMap::from([(t, 1)]),
            k: 0,
        }
    }

    fn scale(&self, c: i128) -> Lin {
        let mut terms = BTreeMap::new();
        if c != 0 {
            for (t, a) in &self.terms {
                terms.insert(t.clone(), a * c);
            }
        }
        Lin {
            terms,
            k: self.k * c,
        }
    }

    fn add(&self, o: &Lin) -> Lin {
        let mut r = self.clone();
        for (t, a) in &o.terms {
            let e = r.terms.entry(t.clone()).or_insert(0);
            *e += a;
            if *e == 0 {
                r.terms.remove(t);
            }
        }
        r.k += o.k;
        r
    }

    fn sub(&self, o: &Lin) -> Lin {
        self.add(&o.scale(-1))
    }

    fn mentions(&self, t: &str) -> bool {
        self.terms.contains_key(t)
    }

    /// Replaces `t` with `by`.
    fn subst(&self, t: &str, by: &Lin) -> Lin {
        match self.terms.get(t) {
            None => self.clone(),
            Some(&a) => {
                let mut r = self.clone();
                r.terms.remove(t);
                r.add(&by.scale(a))
            }
        }
    }

    /// Divides by the gcd of the coefficients, rounding the constant down: sound over integers.
    fn tighten(mut self) -> Lin {
        let g = self.terms.values().fold(0, |g, a| gcd(g, a.abs()));
        if g > 1 {
            for a in self.terms.values_mut() {
                *a /= g;
            }
            self.k = self.k.div_euclid(g);
        }
        self
    }
}

fn gcd(a: i128, b: i128) -> i128 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The conjunction of `cs` has no integer solution, as far as elimination can tell.
fn unsat(cs: &[Lin]) -> bool {
    const LIMIT: usize = 4000;
    let mut cs: Vec<Lin> = cs.iter().cloned().map(Lin::tighten).collect();
    loop {
        if cs.iter().any(|c| c.terms.is_empty() && c.k < 0) {
            return true;
        }
        cs.retain(|c| !c.terms.is_empty());
        cs.sort_by(|a, b| (&a.terms, a.k).cmp(&(&b.terms, b.k)));
        cs.dedup();
        let Some(t) = cs.first().and_then(|c| c.terms.keys().next().cloned()) else {
            return false;
        };
        let (mut pos, mut neg, mut rest) = (Vec::new(), Vec::new(), Vec::new());
        for c in cs {
            match c.terms.get(&t).copied() {
                Some(a) if a > 0 => pos.push((a, c)),
                Some(a) => neg.push((-a, c)),
                None => rest.push(c),
            }
        }
        if pos.len() * neg.len() + rest.len() > LIMIT {
            return false;
        }
        for (a, p) in &pos {
            for (b, n) in &neg {
                rest.push(p.scale(*b).add(&n.scale(*a)).tighten());
            }
        }
        cs = rest;
    }
}

#[derive(Clone, Default)]
struct Facts {
    lin: Vec<Lin>,
    /// `(dict, key)` pairs known present; a key is `n:<name>` or `s:<text>`.
    keys: Vec<(String, String)>,
    /// Disjunctions, each alternative a conjunction; goals split over them (R605).
    ors: Vec<Vec<Vec<Lin>>>,
}

impl Facts {
    /// Forgets `name`, keeping what its facts imply about other terms (R760).
    fn kill(&mut self, name: &str) {
        let (v, l) = (var(name), len(name));
        let lin = std::mem::take(&mut self.lin);
        self.lin = project(project(lin, &v), &l);
        self.ors
            .retain(|d| !d.iter().flatten().any(|c| c.mentions(&v) || c.mentions(&l)));
        let k = format!("n:{name}");
        self.keys.retain(|(d, key)| d != name && *key != k);
    }

    /// What holds on both paths: a fact of either that the other implies (R762).
    fn meet(a: Facts, b: &Facts) -> Facts {
        let mut lin: Vec<Lin> = a
            .lin
            .iter()
            .filter(|c| b.lin.contains(c) || b.implies(c))
            .cloned()
            .collect();
        for c in &b.lin {
            if !lin.contains(c) && a.implies(c) {
                lin.push(c.clone());
            }
        }
        Facts {
            lin,
            keys: a.keys.into_iter().filter(|k| b.keys.contains(k)).collect(),
            ors: a.ors.into_iter().filter(|d| b.ors.contains(d)).collect(),
        }
    }

    /// Every case of these facts implies `c >= 0`.
    fn implies(&self, c: &Lin) -> bool {
        self.cases().iter().all(|case| {
            let mut all = with_lens(case, c);
            all.push(c.scale(-1).add(&Lin::konst(-1)));
            unsat(&all)
        })
    }

    /// Adds `alts` as a disjunction; an alternative that says nothing makes it vacuous.
    fn disjoin(&mut self, alts: Vec<Vec<Lin>>) {
        if alts.len() > 1 && alts.len() <= CASES && alts.iter().all(|a| !a.is_empty()) {
            self.ors.push(alts);
        }
    }

    fn subst(&mut self, t: &str, by: &Lin) {
        self.lin = self.lin.iter().map(|c| c.subst(t, by)).collect();
        for d in &mut self.ors {
            for alt in d.iter_mut() {
                *alt = alt.iter().map(|c| c.subst(t, by)).collect();
            }
        }
    }

    /// The conjunctions a goal is decided under: `lin` with one alternative of each
    /// disjunction, taking disjunctions in order while there are at most `CASES`.
    fn cases(&self) -> Vec<Vec<Lin>> {
        let mut out = vec![self.lin.clone()];
        for d in &self.ors {
            if out.len() * d.len() > CASES {
                continue;
            }
            out = out
                .iter()
                .flat_map(|c| {
                    d.iter().map(move |alt| {
                        let mut c = c.clone();
                        c.extend(alt.iter().cloned());
                        c
                    })
                })
                .collect();
        }
        out
    }
}

const CASES: usize = 8;

/// Eliminates `t` from `cs` by one Fourier-Motzkin step; past `LIMIT` derived facts, drops them.
fn project(cs: Vec<Lin>, t: &str) -> Vec<Lin> {
    const LIMIT: usize = 64;
    let (with, mut out): (Vec<Lin>, Vec<Lin>) = cs.into_iter().partition(|c| c.mentions(t));
    let (pos, neg): (Vec<&Lin>, Vec<&Lin>) = with.iter().partition(|c| c.terms[t] > 0);
    if pos.len() * neg.len() > LIMIT {
        return out;
    }
    for p in &pos {
        for n in &neg {
            let c = p.scale(-n.terms[t]).add(&n.scale(p.terms[t])).tighten();
            if !(c.terms.is_empty() && c.k >= 0) && !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// `cs` with every list length that they or `extra` mention known non-negative.
fn with_lens(cs: &[Lin], extra: &Lin) -> Vec<Lin> {
    let mut out = cs.to_vec();
    for c in cs.iter().chain([extra]) {
        for t in c.terms.keys().filter(|t| t.starts_with("len:")) {
            out.push(Lin::term(t.clone()));
        }
    }
    out
}

fn var(n: &str) -> String {
    format!("v:{n}")
}

fn len(n: &str) -> String {
    format!("len:{n}")
}

/// `a >= b`, `a == b` as constraints.
fn ge(a: &Lin, b: &Lin) -> Lin {
    a.sub(b)
}

fn eq(a: &Lin, b: &Lin) -> [Lin; 2] {
    [a.sub(b), b.sub(a)]
}

struct Solver<'a> {
    sites: &'a HashMap<Span, Site>,
    release: bool,
    fresh: usize,
    out: Vec<Finding>,
}

pub(crate) fn check(stmts: &[j::Stmt], sites: &HashMap<Span, Site>, release: bool) -> Vec<Finding> {
    let mut s = Solver {
        sites,
        release,
        fresh: 0,
        out: Vec::new(),
    };
    s.stmts(stmts, Facts::default());
    s.out
}

fn exits(stmts: &[j::Stmt]) -> bool {
    matches!(
        stmts.last().map(|s| &s.kind),
        Some(
            j::StmtKind::Return(_)
                | j::StmtKind::Fail(_)
                | j::StmtKind::Become(_)
                | j::StmtKind::Unreachable
                | j::StmtKind::Break
                | j::StmtKind::Continue
        )
    )
}

impl Solver<'_> {
    fn fresh(&mut self) -> String {
        self.fresh += 1;
        format!("t:{}", self.fresh)
    }

    /// `e` as a linear value, with the facts that bound the fresh terms it introduced.
    fn lin(&mut self, e: &j::Expr, side: &mut Vec<Lin>) -> Option<Lin> {
        Some(match &e.kind {
            K::Lit(j::Lit::Int(s)) => Lin::konst(s.replace('_', "").parse().ok()?),
            K::Name(n) => Lin::term(var(n)),
            K::Unary("-", x) => self.lin(x, side)?.scale(-1),
            K::Unary("+", x) => self.lin(x, side)?,
            K::Binary("+", a, b) => self.lin(a, side)?.add(&self.lin(b, side)?),
            K::Binary("-", a, b) => self.lin(a, side)?.sub(&self.lin(b, side)?),
            K::Binary("*", a, b) => {
                let (x, y) = (self.lin(a, side)?, self.lin(b, side)?);
                match (x.terms.is_empty(), y.terms.is_empty()) {
                    (true, _) => y.scale(x.k),
                    (_, true) => x.scale(y.k),
                    _ => return None,
                }
            }
            K::Binary(op @ ("&" | "%" | "//"), a, b) => {
                let c = self.lin(b, side)?;
                if !c.terms.is_empty() {
                    return None;
                }
                let c = c.k;
                let t = Lin::term(self.fresh());
                match *op {
                    "&" if c >= 0 => {
                        side.push(t.clone());
                        side.push(Lin::konst(c).sub(&t));
                    }
                    "%" if c > 0 => {
                        side.push(t.clone());
                        side.push(Lin::konst(c - 1).sub(&t));
                    }
                    "//" if c > 0 => {
                        let x = self.lin(a, side)?;
                        let ct = t.scale(c);
                        side.push(ge(&x, &ct));
                        side.push(ge(&ct.add(&Lin::konst(c - 1)), &x));
                    }
                    _ => return None,
                }
                t
            }
            K::Call(f, args) if matches!(&f.kind, K::Name(n) if n == "len") && args.len() == 1 => {
                match &args[0].kind {
                    j::ArgKind::Expr(x) => match &x.kind {
                        K::Name(n) => Lin::term(len(n)),
                        _ => {
                            let t = Lin::term(self.fresh());
                            side.push(t.clone());
                            t
                        }
                    },
                    _ => return None,
                }
            }
            _ => return None,
        })
    }

    /// The facts `e == pos` gives, keeping only what is a conjunction.
    fn assume(&mut self, e: &j::Expr, pos: bool, into: &mut Facts) {
        match &e.kind {
            K::Not(x) => self.assume(x, !pos, into),
            K::Binary("and", a, b) if pos => {
                self.assume(a, true, into);
                self.assume(b, true, into);
            }
            K::Binary("or", a, b) if !pos => {
                self.assume(a, false, into);
                self.assume(b, false, into);
            }
            K::Binary(op @ ("or" | "and"), a, b) => {
                let pos_or = *op == "or";
                let (mut fa, mut fb) = (Facts::default(), Facts::default());
                self.assume(a, pos_or, &mut fa);
                self.assume(b, pos_or, &mut fb);
                into.disjoin(fa.cases().into_iter().chain(fb.cases()).collect());
            }
            // A list as a condition (N15) is `len(xs) > 0`.
            K::Name(n) if self.sites.get(&e.span) == Some(&Site::List) => {
                let l = Lin::term(len(n));
                into.lin.push(if pos {
                    l.sub(&Lin::konst(1))
                } else {
                    l.scale(-1)
                });
            }
            K::Compare(op @ ("in" | "not in"), k, d) => {
                if (*op == "in") == pos
                    && let (K::Name(dn), Some(key)) = (&d.kind, key_of(k))
                {
                    into.keys.push((dn.clone(), key));
                }
            }
            K::Compare(op, a, b) => {
                let mut side = Vec::new();
                let (Some(x), Some(y)) = (self.lin(a, &mut side), self.lin(b, &mut side)) else {
                    return;
                };
                let one = Lin::konst(1);
                let op = if pos {
                    *op
                } else {
                    match *op {
                        "<" => ">=",
                        "<=" => ">",
                        ">" => "<=",
                        ">=" => "<",
                        "==" => "!=",
                        "!=" => "==",
                        _ => return,
                    }
                };
                if op == "!=" {
                    into.lin.extend(side);
                    into.disjoin(vec![vec![ge(&x, &y.add(&one))], vec![ge(&y, &x.add(&one))]]);
                    return;
                }
                let cs: Vec<Lin> = match op {
                    "<" => vec![ge(&y, &x.add(&one))],
                    "<=" => vec![ge(&y, &x)],
                    ">" => vec![ge(&x, &y.add(&one))],
                    ">=" => vec![ge(&x, &y)],
                    "==" => eq(&x, &y).into(),
                    _ => return,
                };
                into.lin.extend(side);
                into.lin.extend(cs);
            }
            _ => {}
        }
    }

    fn stmts(&mut self, stmts: &[j::Stmt], mut f: Facts) -> Facts {
        for s in stmts {
            f = self.stmt(s, f);
        }
        f
    }

    fn block(&mut self, stmts: &[j::Stmt], f: Facts) -> Option<Facts> {
        let out = self.stmts(stmts, f);
        (!exits(stmts)).then_some(out)
    }

    fn stmt(&mut self, s: &j::Stmt, mut f: Facts) -> Facts {
        use j::StmtKind as S;
        match &s.kind {
            S::Expr(e) | S::Become(e) | S::Fail(e) => self.expr(e, &mut f),
            S::Return(e) => {
                if let Some(e) = e {
                    self.expr(e, &mut f);
                }
            }
            S::Ghost { .. }
            | S::Break
            | S::Continue
            | S::BreakTo(_)
            | S::ContinueTo(_)
            | S::Pass
            | S::Unreachable
            | S::Reveal(_) => {}
            S::Bind {
                target,
                target_span,
                value,
                ..
            } => {
                let t = j::Expr {
                    kind: K::Name(target.clone()),
                    span: *target_span,
                    paren: false,
                };
                self.assign(&t, value, &mut f);
            }
            S::Assert(e) => {
                self.expr(e, &mut f);
                self.assume(e, true, &mut f);
            }
            S::Unsafe(body) => return self.stmts(body, f),
            // Stage-0 control is resolved during lowering; its bodies are not analyzed here.
            S::StageIf { then, els, .. } => {
                for n in assigned(then, self.sites)
                    .into_iter()
                    .chain(els.iter().flat_map(|b| assigned(b, self.sites)))
                {
                    f.kill(&n);
                }
            }
            S::StageFor { body, .. } => {
                for n in assigned(body, self.sites) {
                    f.kill(&n);
                }
            }
            S::Asm { args, .. } => {
                for a in args {
                    if let j::AsmArg::Operand { dir, value, .. } = a {
                        self.expr(value, &mut f);
                        if dir != "in" {
                            self.target(value, &mut f);
                        }
                    }
                }
            }
            S::Assign { target, value, .. } => self.assign(target, value, &mut f),
            S::AugAssign { target, op, value } if matches!(target.kind, K::Name(_)) => {
                let bin = j::Expr {
                    kind: K::Binary(
                        op.trim_end_matches('='),
                        Box::new(target.clone()),
                        Box::new(value.clone()),
                    ),
                    span: value.span,
                    paren: false,
                };
                self.assign(target, &bin, &mut f);
            }
            S::AugAssign { target, value, .. } => {
                self.expr(value, &mut f);
                // R438: a missing key gets the default first.
                if let K::Index(b, k) = &target.kind
                    && let (K::Name(d), Some(key)) = (&b.kind, key_of(k))
                    && self.sites.get(&target.span) == Some(&Site::Dict)
                {
                    f.keys.push((d.clone(), key));
                }
                self.target(target, &mut f);
            }
            S::If { arms, els } => {
                let mut rest = f;
                let mut outs = Vec::new();
                for (c, b) in arms {
                    self.expr(c, &mut rest);
                    let mut then = rest.clone();
                    self.assume(c, true, &mut then);
                    outs.extend(self.block(&b.stmts, then));
                    self.assume(c, false, &mut rest);
                }
                match els {
                    Some(b) => outs.extend(self.block(&b.stmts, rest)),
                    None => outs.push(rest),
                }
                return meet_all(outs);
            }
            S::While {
                cond,
                contracts,
                body,
                ..
            } => {
                let changed = assigned(body, self.sites);
                let mut kept: Vec<Lin> = if continues(body) {
                    Vec::new()
                } else {
                    f.lin
                        .iter()
                        .filter(|c| {
                            changed
                                .iter()
                                .any(|n| c.mentions(&var(n)) || c.mentions(&len(n)))
                        })
                        .cloned()
                        .collect()
                };
                for n in &changed {
                    f.kill(n);
                }
                // R763: the candidates the body preserves are invariants; findings come
                // only from the final pass below.
                let seen = self.out.len();
                while !kept.is_empty() {
                    let mut inside = f.clone();
                    inside.lin.extend(kept.iter().cloned());
                    self.expr(cond, &mut inside);
                    self.assume(cond, true, &mut inside);
                    let Some(end) = self.block(body, inside) else {
                        break;
                    };
                    let n = kept.len();
                    kept.retain(|c| end.implies(c));
                    if kept.len() == n {
                        break;
                    }
                }
                self.out.truncate(seen);
                f.lin.extend(kept);
                if !self.release {
                    for c in contracts {
                        if let j::LoopContract::Invariant(e) = c {
                            self.assume(e, true, &mut f);
                        }
                    }
                }
                self.expr(cond, &mut f);
                let mut inside = f.clone();
                self.assume(cond, true, &mut inside);
                self.stmts(body, inside);
                if !breaks(body) {
                    self.assume(cond, false, &mut f);
                }
            }
            S::For {
                target, iter, body, ..
            } => {
                self.expr(iter, &mut f);
                let changed = assigned(body, self.sites);
                let mut inside = f.clone();
                for n in changed.iter().chain(target) {
                    inside.kill(n);
                }
                self.loop_facts(target, iter, &changed, &mut inside);
                self.stmts(body, inside);
                for n in changed.iter().chain(target) {
                    f.kill(n);
                }
            }
            S::Match { scrutinee, cases } => {
                self.expr(scrutinee, &mut f);
                let changed: Vec<String> = cases
                    .iter()
                    .flat_map(|c| assigned(&c.body.stmts, self.sites))
                    .collect();
                let mut outs = Vec::new();
                for c in cases {
                    let mut g = f.clone();
                    for n in pat_names(&c.pat) {
                        g.kill(&n);
                    }
                    if let Some(guard) = &c.guard {
                        self.expr(guard, &mut g);
                        self.assume(guard, true, &mut g);
                    }
                    outs.extend(self.block(&c.body.stmts, g));
                }
                let mut out = meet_all(outs);
                for n in &changed {
                    out.kill(n);
                }
                return out;
            }
            S::With { body, .. } => return self.stmts(&body.stmts, f),
            S::Par { body, .. } => {
                let stmts = match body {
                    j::ParBody::Block(b) => b,
                    j::ParBody::For { body, .. } => body,
                };
                self.stmts(stmts, f.clone());
                for n in assigned(stmts, self.sites) {
                    f.kill(&n);
                }
            }
        }
        f
    }

    fn loop_facts(
        &mut self,
        target: &[String],
        iter: &j::Expr,
        changed: &HashSet<String>,
        f: &mut Facts,
    ) {
        let K::Call(callee, args) = &iter.kind else {
            return;
        };
        let K::Name(fun) = &callee.kind else {
            return;
        };
        let xs: Vec<&j::Expr> = args
            .iter()
            .filter_map(|a| match (&a.name, &a.kind) {
                (None, j::ArgKind::Expr(x)) => Some(x),
                _ => None,
            })
            .collect();
        let stable = |l: &Lin| {
            l.terms
                .keys()
                .all(|t| !changed.iter().any(|n| *t == var(n) || *t == len(n)))
        };
        let i = Lin::term(var(&target[0]));
        match (fun.as_str(), xs.as_slice(), target.len()) {
            ("range", [b], 1) | ("range", [_, b], 1) => {
                let lo = match xs.as_slice() {
                    [a, _] => {
                        let mut side = Vec::new();
                        self.lin(a, &mut side).filter(stable).map(|l| (l, side))
                    }
                    _ => Some((Lin::konst(0), Vec::new())),
                };
                if let Some((lo, side)) = lo {
                    f.lin.extend(side);
                    f.lin.push(ge(&i, &lo));
                }
                let mut side = Vec::new();
                if let Some(hi) = self.lin(b, &mut side).filter(stable) {
                    f.lin.extend(side);
                    f.lin.push(ge(&hi, &i.add(&Lin::konst(1))));
                }
            }
            ("enumerate", [x], 2) => {
                if let K::Name(n) = &x.kind
                    && !changed.contains(n)
                {
                    f.lin.push(i.clone());
                    f.lin.push(ge(&Lin::term(len(n)), &i.add(&Lin::konst(1))));
                }
            }
            _ => {}
        }
    }

    fn assign(&mut self, target: &j::Expr, value: &j::Expr, f: &mut Facts) {
        self.expr(value, f);
        if let K::Index(b, k) = &target.kind
            && let K::Name(d) = &b.kind
            && self.sites.get(&target.span) != Some(&Site::List)
        {
            self.expr(k, f);
            if let Some(key) = key_of(k) {
                f.keys.push((d.clone(), key));
            }
            return;
        }
        let K::Name(n) = &target.kind else {
            self.target(target, f);
            return;
        };
        let mut side = Vec::new();
        let x = var(n);
        let val = self.lin(value, &mut side);
        // R761: after `x = x + d` the old `x` is the new one minus `d`.
        if let Some(v) = &val
            && v.terms.get(&x) == Some(&1)
            && side.iter().all(|c| !c.mentions(&x))
        {
            let d = v.sub(&Lin::term(x.clone()));
            f.subst(&x, &Lin::term(x.clone()).sub(&d));
            f.lin.extend(side);
            let k = format!("n:{n}");
            f.keys.retain(|(_, key)| *key != k);
            return;
        }
        let val = val.filter(|l| !l.mentions(&x));
        let length = match &value.kind {
            K::List(xs) => Some(xs.len() as i128),
            _ => None,
        };
        f.kill(n);
        if let Some(v) = val {
            f.lin.extend(side);
            f.lin.extend(eq(&Lin::term(var(n)), &v));
        }
        if let Some(l) = length {
            f.lin.extend(eq(&Lin::term(len(n)), &Lin::konst(l)));
        }
    }

    /// A write to `t`: its indices are checked, and its root loses its facts.
    fn target(&mut self, t: &j::Expr, f: &mut Facts) {
        match &t.kind {
            K::Name(n) => f.kill(n),
            K::Tuple(xs) => xs.iter().for_each(|x| self.target(x, f)),
            K::Index(..) if self.sites.get(&t.span) == Some(&Site::List) => self.expr(t, f),
            K::Index(..) | K::Attr(..) => {
                self.expr(t, f);
                if let Some(r) = root(t) {
                    f.kill(r);
                }
            }
            _ => {}
        }
    }

    /// Checks every index in `e`, in evaluation order, and applies the effects of its calls.
    fn expr(&mut self, e: &j::Expr, f: &mut Facts) {
        match &e.kind {
            K::Stage(_) => {}
            K::Index(b, k) => {
                self.expr(b, f);
                self.expr(k, f);
                match self.sites.get(&e.span) {
                    Some(Site::List) => self.index(e.span, b, k, f),
                    Some(Site::Dict) => {
                        let known = matches!((&b.kind, key_of(k)), (K::Name(d), Some(key)) if f.keys.contains(&(d.clone(), key.clone())));
                        if !known {
                            self.unknown(e.span, "the key may be missing");
                        }
                    }
                    None => {}
                }
            }
            K::Binary(op @ ("and" | "or"), a, b) => {
                self.expr(a, f);
                let mut g = f.clone();
                self.assume(a, *op == "and", &mut g);
                self.expr(b, &mut g);
            }
            K::Cond { then, cond, els } => {
                self.expr(cond, f);
                let mut g = f.clone();
                self.assume(cond, true, &mut g);
                self.expr(then, &mut g);
                let mut g = f.clone();
                self.assume(cond, false, &mut g);
                self.expr(els, &mut g);
            }
            K::Call(callee, args) => {
                if let K::Attr(b, _, _) = &callee.kind {
                    self.expr(b, f);
                } else {
                    self.expr(callee, f);
                }
                for a in args {
                    match &a.kind {
                        j::ArgKind::Expr(x) | j::ArgKind::Become(x) => self.expr(x, f),
                        j::ArgKind::Ref(x) => {
                            self.expr(x, f);
                            if let Some(r) = root(x) {
                                f.kill(r);
                            }
                        }
                    }
                }
                if let K::Attr(b, m, _) = &callee.kind
                    && let K::Name(n) = &b.kind
                {
                    let l = len(n);
                    let by = |d: i128| Lin::term(l.clone()).add(&Lin::konst(d));
                    match m.as_str() {
                        "append" | "push" => f.subst(&l, &by(-1)),
                        "pop" => f.subst(&l, &by(1)),
                        "insert" | "extend" | "remove" | "clear" | "sort" | "reverse" | "add"
                        | "discard" | "setdefault" | "update" => f.kill(n),
                        _ => {}
                    }
                }
            }
            K::Attr(b, _, _) | K::Try(b) | K::Unary(_, b) | K::Not(b) => self.expr(b, f),
            K::Binary(_, a, b) | K::Compare(_, a, b) => {
                self.expr(a, f);
                self.expr(b, f);
            }
            K::Tuple(xs) | K::List(xs) | K::Set(xs) => xs.iter().for_each(|x| self.expr(x, f)),
            K::Dict(kvs) => kvs.iter().for_each(|(k, v)| {
                self.expr(k, f);
                self.expr(v, f);
            }),
            K::FStr { pieces, .. } => {
                for p in pieces {
                    if let j::FPiece::Expr(x, _) = p {
                        self.expr(x, f);
                    }
                }
            }
            K::Slice(b, parts, _) => {
                self.expr(b, f);
                parts.iter().flatten().for_each(|x| self.expr(x, f));
            }
            K::Reify(b) | K::Cast(b, _) => self.expr(b, f),
            K::Fallback(x, fb) => {
                self.expr(x, f);
                if let j::Fallback::Value(v) | j::Fallback::Bind(_, v) | j::Fallback::Fail(v) =
                    &**fb
                {
                    let mut g = f.clone();
                    self.expr(v, &mut g);
                }
            }
            K::Name(_) | K::Lit(_) | K::Hole | K::Wild | K::Comp { .. } | K::Lambda(..) => {}
        }
    }

    fn index(&mut self, at: Span, b: &j::Expr, k: &j::Expr, f: &Facts) {
        let mut side = Vec::new();
        let n = match &b.kind {
            K::Name(n) => Lin::term(len(n)),
            _ => {
                let t = Lin::term(self.fresh());
                side.push(t.clone());
                t
            }
        };
        let negative = matches!(&k.kind, K::Lit(j::Lit::Int(s)) if s.starts_with('-'));
        let Some(i) = self
            .lin(k, &mut side)
            .map(|i| if negative { n.add(&i) } else { i })
        else {
            self.unknown(at, "the index is outside the linear fragment");
            return;
        };
        let last = n.sub(&Lin::konst(1));
        let (mut live, mut refuted, mut proven) = (0, 0, 0);
        for case in f.cases() {
            let mut base = case;
            base.extend(side.iter().cloned());
            for c in base.clone().iter().chain([&n]) {
                for t in c.terms.keys().filter(|t| t.starts_with("len:")) {
                    base.push(Lin::term(t.clone()));
                }
            }
            if unsat(&base) {
                continue;
            }
            let with = |extra: Vec<Lin>| {
                let mut cs = base.clone();
                cs.extend(extra);
                unsat(&cs)
            };
            live += 1;
            if with(vec![i.clone(), ge(&last, &i)]) {
                refuted += 1;
            } else if with(vec![ge(&Lin::konst(-1), &i)]) && with(vec![ge(&i, &n)]) {
                proven += 1;
            }
        }
        if live > 0 && refuted == live {
            self.out.push(Finding {
                code: "E501",
                span: at,
                message: "this index is always out of bounds".into(),
            });
        } else if proven < live {
            self.unknown(at, "the solver cannot prove this index in bounds");
        }
    }

    fn unknown(&mut self, at: Span, why: &str) {
        self.out.push(Finding {
            code: "W820",
            span: at,
            message: format!("bounds check kept: {why}"),
        });
    }
}

fn meet_all(outs: Vec<Facts>) -> Facts {
    let mut it = outs.into_iter();
    match it.next() {
        None => Facts::default(),
        Some(first) => it.fold(first, |a, b| Facts::meet(a, &b)),
    }
}

fn key_of(e: &j::Expr) -> Option<String> {
    match &e.kind {
        K::Name(n) => Some(format!("n:{n}")),
        K::Lit(j::Lit::Str { value, .. }) => Some(format!("s:{value}")),
        _ => None,
    }
}

fn root(e: &j::Expr) -> Option<&str> {
    match &e.kind {
        K::Name(n) => Some(n),
        K::Index(b, _) | K::Attr(b, _, _) => root(b),
        _ => None,
    }
}

fn pat_names(p: &j::Pat) -> Vec<String> {
    let mut out = Vec::new();
    fn go(p: &j::Pat, out: &mut Vec<String>) {
        match p {
            j::Pat::Tuple(ps) => ps.iter().for_each(|p| go(p, out)),
            j::Pat::As(p, n) => {
                out.push(n.clone());
                go(p, out);
            }
            j::Pat::Path { path, args: None } if path.len() == 1 => out.push(path[0].clone()),
            j::Pat::Path { args: Some(xs), .. } => xs.iter().for_each(|(_, p)| go(p, out)),
            j::Pat::Path { .. } | j::Pat::Wild | j::Pat::Lit(_) => {}
        }
    }
    go(p, &mut out);
    out
}

/// `stmts` may continue the loop they are in, or an outer one.
fn continues(stmts: &[j::Stmt]) -> bool {
    use j::StmtKind as S;
    stmts.iter().any(|s| match &s.kind {
        S::Continue | S::ContinueTo(_) => true,
        S::If { arms, els } => {
            arms.iter().any(|(_, b)| continues(&b.stmts))
                || els.as_ref().is_some_and(|b| continues(&b.stmts))
        }
        S::Match { cases, .. } => cases.iter().any(|c| continues(&c.body.stmts)),
        S::With { body, .. } => continues(&body.stmts),
        S::Unsafe(body) => continues(body),
        S::While { body, .. } | S::For { body, .. } => continues_to(body),
        _ => false,
    })
}

/// `stmts` hold a labelled `continue`, which may target a loop outside them.
fn continues_to(stmts: &[j::Stmt]) -> bool {
    use j::StmtKind as S;
    stmts.iter().any(|s| match &s.kind {
        S::ContinueTo(_) => true,
        S::If { arms, els } => {
            arms.iter().any(|(_, b)| continues_to(&b.stmts))
                || els.as_ref().is_some_and(|b| continues_to(&b.stmts))
        }
        S::Match { cases, .. } => cases.iter().any(|c| continues_to(&c.body.stmts)),
        S::With { body, .. } => continues_to(&body.stmts),
        S::Unsafe(body) | S::While { body, .. } | S::For { body, .. } => continues_to(body),
        _ => false,
    })
}

fn breaks(stmts: &[j::Stmt]) -> bool {
    use j::StmtKind as S;
    stmts.iter().any(|s| match &s.kind {
        S::Break | S::BreakTo(_) => true,
        S::If { arms, els } => {
            arms.iter().any(|(_, b)| breaks(&b.stmts))
                || els.as_ref().is_some_and(|b| breaks(&b.stmts))
        }
        S::Match { cases, .. } => cases.iter().any(|c| breaks(&c.body.stmts)),
        S::With { body, .. } => breaks(&body.stmts),
        _ => false,
    })
}

/// Std methods that only read their receiver.
const READS: [&str; 16] = [
    "copy",
    "get",
    "index",
    "keys",
    "values",
    "items",
    "contains",
    "contains_key",
    "split",
    "splitlines",
    "partition",
    "strip",
    "upper",
    "lower",
    "startswith",
    "endswith",
];

/// Every name that `stmts` assigns or passes `&`, or calls a mutating method on.
fn assigned(stmts: &[j::Stmt], sites: &HashMap<Span, Site>) -> HashSet<String> {
    let mut out = HashSet::new();
    fn target(t: &j::Expr, sites: &HashMap<Span, Site>, out: &mut HashSet<String>) {
        match &t.kind {
            K::Tuple(xs) => xs.iter().for_each(|x| target(x, sites, out)),
            // A store to a list element keeps every length.
            K::Index(..) if sites.get(&t.span) == Some(&Site::List) => {}
            _ => {
                if let Some(r) = root(t) {
                    out.insert(r.to_owned());
                }
            }
        }
    }
    fn expr(e: &j::Expr, out: &mut HashSet<String>) {
        crate::lower::walk(e, &mut |x| {
            let K::Call(callee, args) = &x.kind else {
                return;
            };
            if let K::Attr(b, m, _) = &callee.kind
                && !READS.contains(&m.as_str())
                && let Some(r) = root(b)
            {
                out.insert(r.to_owned());
            }
            for a in args {
                if let j::ArgKind::Ref(x) = &a.kind
                    && let Some(r) = root(x)
                {
                    out.insert(r.to_owned());
                }
            }
        });
    }
    fn go(stmts: &[j::Stmt], sites: &HashMap<Span, Site>, out: &mut HashSet<String>) {
        use j::StmtKind as S;
        for s in stmts {
            match &s.kind {
                S::Assign {
                    target: t, value, ..
                }
                | S::AugAssign {
                    target: t, value, ..
                } => {
                    target(t, sites, out);
                    expr(value, out);
                }
                S::Ghost { name, .. } => {
                    out.insert(name.clone());
                }
                S::Bind { target, value, .. } => {
                    out.insert(target.clone());
                    expr(value, out);
                }
                S::Expr(e) | S::Become(e) | S::Fail(e) | S::Assert(e) => expr(e, out),
                S::Unsafe(body) | S::StageFor { body, .. } => go(body, sites, out),
                S::StageIf { then, els, .. } => {
                    go(then, sites, out);
                    if let Some(b) = els {
                        go(b, sites, out);
                    }
                }
                S::Asm { args, .. } => {
                    for a in args {
                        if let j::AsmArg::Operand { dir, value, .. } = a
                            && dir != "in"
                            && let K::Name(n) = &value.kind
                        {
                            out.insert(n.clone());
                        }
                    }
                }
                S::Return(e) => e.iter().for_each(|e| expr(e, out)),
                S::If { arms, els } => {
                    for (c, b) in arms {
                        expr(c, out);
                        go(&b.stmts, sites, out);
                    }
                    if let Some(b) = els {
                        go(&b.stmts, sites, out);
                    }
                }
                S::While { cond, body, .. } => {
                    expr(cond, out);
                    go(body, sites, out);
                }
                S::For {
                    target, iter, body, ..
                } => {
                    out.extend(target.iter().cloned());
                    expr(iter, out);
                    go(body, sites, out);
                }
                S::Match { scrutinee, cases } => {
                    expr(scrutinee, out);
                    for c in cases {
                        out.extend(pat_names(&c.pat));
                        go(&c.body.stmts, sites, out);
                    }
                }
                S::With { body, .. } => go(&body.stmts, sites, out),
                S::Par { body, .. } => match body {
                    j::ParBody::Block(b) => go(b, sites, out),
                    j::ParBody::For { body, .. } => go(body, sites, out),
                },
                _ => {}
            }
        }
    }
    go(stmts, sites, &mut out);
    out
}
