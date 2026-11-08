//! Accuracy lint (R613). W852 flags a float expression in a pure function whose mean error over
//! sampled inputs exceeds [`THRESHOLD_BITS`]. Inputs are float parameters drawn uniformly over
//! float bit patterns, seeded from the item hash and narrowed by `pre` bounds. Error is measured
//! against evaluation in arbitrary precision. The fix is the rewrite with the lowest measured error among
//! those an e-graph search finds.

use std::time::Duration;

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

use egg::{Id, Language, RecExpr, Rewrite, Runner, Symbol, define_language, rewrite as rw};
use yan_core::hash::Hashed;
use yan_diag::{Applicability, Span};
use yan_hir as j;

use crate::Cx;

/// Mean bits of error above which an expression is flagged.
const THRESHOLD_BITS: f64 = 4.0;
/// A rewrite is offered only when it is at least this many bits more accurate.
const MIN_GAIN_BITS: f64 = 1.0;
const SAMPLES: usize = 256;
/// Expressions with fewer operations cannot lose more than a few ulps.
const MIN_OPS: usize = 2;
/// Bounds on the e-graph search; there is no time limit, so the result is deterministic.
const ITER_LIMIT: usize = 10;
const NODE_LIMIT: usize = 5_000;
const EXTRACT_PASSES: usize = 8;
const MAX_TERM: usize = 40;
/// The highest precision, in bits, tried for the reference value.
const MAX_PREC: u64 = 8192;
/// Terms within this many bits of each other count as equally accurate; the smaller is kept.
const TIE_BITS: f64 = 0.1;

define_language! {
    enum Fp {
        "+" = Add([Id; 2]),
        "-" = Sub([Id; 2]),
        "*" = Mul([Id; 2]),
        "/" = Div([Id; 2]),
        "neg" = Neg(Id),
        "sqrt" = Sqrt(Id),
        // A parameter name or a numeric literal's text.
        Leaf(Symbol),
    }
}

pub(crate) fn check(m: &j::Module, hashes: &[Hashed], cx: &mut Cx) {
    let rules = rules();
    for it in &m.items {
        let j::ItemKind::Func(f) = &it.kind else {
            continue;
        };
        let Some(body) = &f.body else { continue };
        if !matches!(f.uses, None | Some(j::Uses::None)) {
            continue;
        }
        let params: Vec<&str> = f
            .params
            .iter()
            .filter_map(|p| match p {
                j::Param::Value { name, ty, .. } if is_f64(ty) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        if params.is_empty() {
            continue;
        }
        let Some(h) = hashes.iter().find(|h| h.key == f.name) else {
            continue;
        };
        let seed = u64::from_le_bytes(h.identity.group.as_bytes()[..8].try_into().unwrap());
        let mut ranges = vec![(-f64::MAX, f64::MAX); params.len()];
        for c in &body.contracts {
            if let j::Contract::Pre(e) = c {
                narrow(e, &params, &mut ranges);
            }
        }
        if ranges.iter().any(|(lo, hi)| lo > hi) {
            continue;
        }
        let mut found = Found {
            params: &params,
            sqrt: "math.sqrt".to_owned(),
            exprs: Vec::new(),
            rebound: false,
        };
        found.stmts(&body.stmts, cx);
        if found.rebound {
            continue;
        }
        let inputs = sample(seed, &ranges);
        for (e, expr) in &found.exprs {
            lint(&f.name, e, expr, &params, &inputs, &found.sqrt, &rules, cx);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lint(
    item: &str,
    e: &j::Expr,
    expr: &RecExpr<Fp>,
    params: &[&str],
    inputs: &[Vec<f64>],
    sqrt: &str,
    rules: &[Rewrite<Fp, ()>],
    cx: &mut Cx,
) {
    let exact = reference(expr, params, inputs);
    let refs: Vec<f64> = exact.iter().map(|r| r.0).collect();
    let Some(orig) = error(&eval(expr, params, inputs), &refs) else {
        return;
    };
    if orig <= THRESHOLD_BITS {
        return;
    }
    let precs: Vec<Option<u64>> = exact.iter().map(|r| r.1).collect();
    let best =
        search(expr, params, inputs, &precs, rules).filter(|(err, _)| *err + MIN_GAIN_BITS < orig);
    let mut message = format!(
        "float expression loses {orig:.1} bits on average over {} sampled inputs",
        inputs.len()
    );
    if let Some((err, term)) = &best {
        message += &format!("; `{}` loses {err:.1}", print(term, sqrt));
    }
    let span = extent(e, cx);
    cx.warn("W852", span, item, message);
    if let Some((_, term)) = best
        && span.line == span.end_line
    {
        cx.fix(
            Applicability::Choice,
            "use the more accurate rewrite",
            span,
            &print(&term, sqrt),
        );
    }
}

/// The span of the whole expression: HIR spans of operators and calls cover their first token.
fn extent(e: &j::Expr, cx: &Cx) -> Span {
    fn last(e: &j::Expr) -> &j::Expr {
        match &e.kind {
            j::ExprKind::Binary(_, _, b) | j::ExprKind::Unary(_, b) => last(b),
            j::ExprKind::Call(_, args) => match args.last().map(|a| &a.kind) {
                Some(j::ArgKind::Expr(a)) => last(a),
                _ => e,
            },
            _ => e,
        }
    }
    fn first(e: &j::Expr) -> &j::Expr {
        match &e.kind {
            j::ExprKind::Binary(_, a, _) => first(a),
            _ => e,
        }
    }
    let (start, end) = (first(e).span, last(e).span);
    let mut span = Span {
        line: start.line,
        col: start.col,
        end_line: end.end_line,
        end_col: end.end_col,
    };
    if span.line != span.end_line {
        return span;
    }
    // Parentheses around the first and last operands lie outside their spans.
    let line = cx.src.lines().nth(span.line as usize - 1).unwrap_or("");
    let (mut from, mut to) = (span.col as usize - 1, span.end_col as usize - 1);
    let (mut depth, mut low) = (0i32, 0i32);
    for c in line[from..to].chars() {
        depth += match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        low = low.min(depth);
    }
    for _ in 0..-low {
        let Some(k) = line[..from].rfind('(') else {
            break;
        };
        from = k;
    }
    for _ in 0..depth - low {
        let Some(k) = line[to..].find(')') else { break };
        to += k + 1;
    }
    span.col = from as u32 + 1;
    span.end_col = to as u32 + 1;
    span
}

fn is_f64(ty: &j::Type) -> bool {
    matches!(ty, j::Type::Named { path, args } if args.is_empty() && path == &["f64"])
}

/// The float-arithmetic expressions of a function body, outermost first.
struct Found<'p> {
    params: &'p [&'p str],
    /// How the source spells the square root.
    sqrt: String,
    exprs: Vec<(j::Expr, RecExpr<Fp>)>,
    /// A parameter is assigned in the body, so sampled values may not be the ones used.
    rebound: bool,
}

impl Found<'_> {
    fn stmts(&mut self, ss: &[j::Stmt], cx: &Cx) {
        for s in ss {
            self.stmt(s, cx);
        }
    }

    fn stmt(&mut self, s: &j::Stmt, cx: &Cx) {
        use j::StmtKind as S;
        match &s.kind {
            S::Expr(e) | S::Return(Some(e)) | S::Become(e) | S::Fail(e) | S::Assert(e) => {
                self.expr(e, cx)
            }
            S::Assign { target, value, .. } | S::AugAssign { target, value, .. } => {
                self.target(target);
                self.expr(value, cx);
            }
            S::Bind { target, value, .. } => {
                self.rebound |= self.params.contains(&target.as_str());
                self.expr(value, cx);
            }
            S::If { arms, els } => {
                for (c, b) in arms {
                    self.expr(c, cx);
                    self.stmts(&b.stmts, cx);
                }
                if let Some(b) = els {
                    self.stmts(&b.stmts, cx);
                }
            }
            S::While { cond, body, .. } => {
                self.expr(cond, cx);
                self.stmts(body, cx);
            }
            S::For {
                target, iter, body, ..
            } => {
                self.rebound |= target.iter().any(|t| self.params.contains(&t.as_str()));
                self.expr(iter, cx);
                self.stmts(body, cx);
            }
            S::Match { scrutinee, cases } => {
                self.expr(scrutinee, cx);
                for c in cases {
                    self.stmts(&c.body.stmts, cx);
                }
            }
            _ => {}
        }
    }

    fn target(&mut self, t: &j::Expr) {
        match &t.kind {
            j::ExprKind::Name(n) => self.rebound |= self.params.contains(&n.as_str()),
            j::ExprKind::Tuple(ts) => ts.iter().for_each(|t| self.target(t)),
            _ => {}
        }
    }

    fn expr(&mut self, e: &j::Expr, cx: &Cx) {
        let mut out = RecExpr::default();
        let mut ops = 0;
        let mut uses_param = false;
        if self
            .build(e, &mut out, &mut ops, &mut uses_param, cx)
            .is_some()
        {
            if ops >= MIN_OPS && uses_param {
                self.exprs.push((e.clone(), out));
            }
            return;
        }
        use j::ExprKind as E;
        match &e.kind {
            E::Tuple(es) | E::List(es) | E::Set(es) => es.iter().for_each(|e| self.expr(e, cx)),
            E::Call(f, args) => {
                self.expr(f, cx);
                for a in args {
                    let (j::ArgKind::Expr(e) | j::ArgKind::Ref(e) | j::ArgKind::Become(e)) =
                        &a.kind;
                    self.expr(e, cx);
                }
            }
            E::Attr(e, ..) | E::Unary(_, e) | E::Not(e) | E::Cast(e, _) | E::Try(e) => {
                self.expr(e, cx)
            }
            E::Index(a, b) | E::Binary(_, a, b) | E::Compare(_, a, b) => {
                self.expr(a, cx);
                self.expr(b, cx);
            }
            E::Cond { then, cond, els } => {
                self.expr(then, cx);
                self.expr(cond, cx);
                self.expr(els, cx);
            }
            _ => {}
        }
    }

    fn build(
        &mut self,
        e: &j::Expr,
        out: &mut RecExpr<Fp>,
        ops: &mut usize,
        uses_param: &mut bool,
        cx: &Cx,
    ) -> Option<Id> {
        use j::ExprKind as E;
        let node = match &e.kind {
            E::Name(n) if self.params.contains(&n.as_str()) => {
                *uses_param = true;
                Fp::Leaf(n.into())
            }
            E::Lit(j::Lit::Float(t) | j::Lit::Int(t)) => {
                let t = t.replace('_', "");
                t.parse::<f64>().ok()?;
                Fp::Leaf(t.into())
            }
            E::Unary("-", a) => {
                let a = self.build(a, out, ops, uses_param, cx)?;
                Fp::Neg(a)
            }
            E::Binary(op @ ("+" | "-" | "*" | "/"), a, b) => {
                let a = self.build(a, out, ops, uses_param, cx)?;
                let b = self.build(b, out, ops, uses_param, cx)?;
                *ops += 1;
                match *op {
                    "+" => Fp::Add([a, b]),
                    "-" => Fp::Sub([a, b]),
                    "*" => Fp::Mul([a, b]),
                    _ => Fp::Div([a, b]),
                }
            }
            E::Call(callee, args) if is_sqrt(callee) => {
                let [
                    j::Arg {
                        name: None,
                        kind: j::ArgKind::Expr(a),
                        ..
                    },
                ] = args.as_slice()
                else {
                    return None;
                };
                let a = self.build(a, out, ops, uses_param, cx)?;
                *ops += 1;
                self.sqrt = match &callee.kind {
                    E::Attr(m, n, _) => format!("{}.{n}", cx.text(m.span)),
                    _ => "sqrt".to_owned(),
                };
                Fp::Sqrt(a)
            }
            _ => return None,
        };
        Some(out.add(node))
    }
}

fn is_sqrt(callee: &j::Expr) -> bool {
    match &callee.kind {
        j::ExprKind::Attr(m, n, _) => {
            n == "sqrt" && matches!(&m.kind, j::ExprKind::Name(m) if m == "math")
        }
        j::ExprKind::Name(n) => n == "sqrt",
        _ => false,
    }
}

/// Narrows `ranges` by the comparisons of a parameter with a literal in `pre`, joined by `and`.
fn narrow(e: &j::Expr, params: &[&str], ranges: &mut [(f64, f64)]) {
    use j::ExprKind as E;
    match &e.kind {
        E::Binary("and", a, b) => {
            narrow(a, params, ranges);
            narrow(b, params, ranges);
        }
        E::Compare(op, a, b) => {
            let (name, value, op) = match (name(a), number(b), name(b), number(a)) {
                (Some(n), Some(v), ..) => (n, v, *op),
                (_, _, Some(n), Some(v)) => (n, v, flip(op)),
                _ => return,
            };
            let Some(i) = params.iter().position(|p| *p == name) else {
                return;
            };
            let (lo, hi) = &mut ranges[i];
            match op {
                ">" => *lo = lo.max(step(value, 1)),
                ">=" => *lo = lo.max(value),
                "<" => *hi = hi.min(step(value, -1)),
                "<=" => *hi = hi.min(value),
                "==" => (*lo, *hi) = (lo.max(value), hi.min(value)),
                _ => {}
            }
        }
        _ => {}
    }
}

fn name(e: &j::Expr) -> Option<&str> {
    match &e.kind {
        j::ExprKind::Name(n) => Some(n),
        _ => None,
    }
}

fn number(e: &j::Expr) -> Option<f64> {
    match &e.kind {
        j::ExprKind::Lit(j::Lit::Float(t) | j::Lit::Int(t)) => t.replace('_', "").parse().ok(),
        j::ExprKind::Unary("-", a) => number(a).map(|v| -v),
        _ => None,
    }
}

fn flip(op: &str) -> &str {
    match op {
        ">" => "<",
        ">=" => "<=",
        "<" => ">",
        "<=" => ">=",
        op => op,
    }
}

/// The float `k` steps along the ordered float line from `x`.
fn step(x: f64, k: i64) -> f64 {
    unordered(ordered(x) + k)
}

/// Maps floats to integers monotonically, with both zeros at 0.
fn ordered(x: f64) -> i64 {
    let b = x.to_bits() as i64;
    if b < 0 { i64::MIN - b } else { b }
}

fn unordered(k: i64) -> f64 {
    f64::from_bits(if k < 0 { i64::MIN - k } else { k } as u64)
}

/// `SAMPLES` input vectors, each parameter uniform over the float bit patterns of its range.
fn sample(seed: u64, ranges: &[(f64, f64)]) -> Vec<Vec<f64>> {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    (0..SAMPLES)
        .map(|_| {
            ranges
                .iter()
                .map(|&(lo, hi)| {
                    let (lo, hi) = (ordered(lo), ordered(hi));
                    let span = (i128::from(hi) - i128::from(lo) + 1) as u128;
                    let k = i128::from(lo) + (u128::from(next()) % span) as i128;
                    unordered(k as i64)
                })
                .collect()
        })
        .collect()
}

/// The float value of `expr` per sample, as the program computes it.
fn eval(expr: &RecExpr<Fp>, params: &[&str], inputs: &[Vec<f64>]) -> Vec<f64> {
    inputs
        .iter()
        .map(|x| {
            let mut v: Vec<f64> = Vec::new();
            for node in expr.as_ref() {
                let y = apply_f64(node, params, x, |k| v[usize::from(*k)]);
                v.push(y);
            }
            v[v.len() - 1]
        })
        .collect()
}

/// The exact value of `expr` per sample rounded to a float, NaN where it is undefined, with the
/// precision in bits that settled it: the first of 256, 512, ... for division and square root at
/// which halving it gives the same rounded result (`None` when none up to `MAX_PREC` does).
fn reference(expr: &RecExpr<Fp>, params: &[&str], inputs: &[Vec<f64>]) -> Vec<(f64, Option<u64>)> {
    let at = |x: &[f64], p: u64| {
        let mut v: Vec<Option<Big>> = Vec::new();
        for node in expr.as_ref() {
            let y = apply_big(node, params, x, p, |k| v[usize::from(*k)].clone());
            v.push(y);
        }
        v.pop().flatten().map_or(f64::NAN, |b| b.to_f64())
    };
    inputs
        .iter()
        .map(|x| {
            let mut p = 128;
            let mut last = at(x, p);
            while p < MAX_PREC {
                let next = at(x, 2 * p);
                if next.to_bits() == last.to_bits() {
                    return (last, Some(2 * p));
                }
                (p, last) = (2 * p, next);
            }
            (f64::NAN, None)
        })
        .collect()
}

fn leaf(s: Symbol, params: &[&str], x: &[f64]) -> f64 {
    match params.iter().position(|p| *p == s.as_str()) {
        Some(i) => x[i],
        None => s.as_str().parse().unwrap_or(f64::NAN),
    }
}

fn apply_f64(node: &Fp, params: &[&str], x: &[f64], v: impl Fn(&Id) -> f64) -> f64 {
    match node {
        Fp::Add([a, b]) => v(a) + v(b),
        Fp::Sub([a, b]) => v(a) - v(b),
        Fp::Mul([a, b]) => v(a) * v(b),
        Fp::Div([a, b]) => v(a) / v(b),
        Fp::Neg(a) => -v(a),
        Fp::Sqrt(a) => v(a).sqrt(),
        Fp::Leaf(s) => leaf(*s, params, x),
    }
}

fn apply_big(
    node: &Fp,
    params: &[&str],
    x: &[f64],
    p: u64,
    r: impl Fn(&Id) -> Option<Big>,
) -> Option<Big> {
    Some(match node {
        Fp::Add([a, b]) => r(a)?.add(&r(b)?),
        Fp::Sub([a, b]) => r(a)?.add(&r(b)?.neg()),
        Fp::Mul([a, b]) => r(a)?.mul(&r(b)?),
        Fp::Div([a, b]) => r(a)?.div(&r(b)?, p)?,
        Fp::Neg(a) => r(a)?.neg(),
        Fp::Sqrt(a) => r(a)?.sqrt(p)?,
        Fp::Leaf(s) => Big::from_f64(leaf(*s, params, x))?,
    })
}

/// Mean bits of error of `vals` against `refs`, over the samples whose reference is finite;
/// `None` when fewer than half are.
fn error(vals: &[f64], refs: &[f64]) -> Option<f64> {
    let (sum, n) = vals
        .iter()
        .zip(refs)
        .filter(|(_, r)| r.is_finite())
        .fold((0.0, 0usize), |(sum, n), (&v, &r)| {
            (sum + bits(v, r), n + 1)
        });
    (n * 2 >= vals.len()).then(|| sum / n as f64)
}

fn bits(v: f64, exact: f64) -> f64 {
    if !v.is_finite() {
        return 64.0;
    }
    (ordered(v).abs_diff(ordered(exact)) as f64 + 1.0).log2()
}

/// A binary float of arbitrary precision, `m * 2^e`. Addition and multiplication are exact, so
/// cancellation loses nothing; division and square root round to the precision asked for.
#[derive(Clone, Debug)]
struct Big {
    m: BigInt,
    e: i64,
}

impl Big {
    fn from_f64(x: f64) -> Option<Big> {
        if !x.is_finite() {
            return None;
        }
        let bits = x.to_bits();
        let exp = ((bits >> 52) & 0x7ff) as i64;
        let frac = bits & ((1 << 52) - 1);
        let (m, e) = if exp == 0 {
            (frac, -1074)
        } else {
            (frac | (1 << 52), exp - 1075)
        };
        let m = BigInt::from(m);
        Some(Big {
            m: if x < 0.0 { -m } else { m },
            e,
        })
    }

    /// Truncated to `p` significant bits.
    fn round(mut self, p: u64) -> Big {
        let b = self.m.bits();
        if b > p {
            self.m >>= b - p;
            self.e += (b - p) as i64;
        }
        self
    }

    fn neg(self) -> Big {
        Big {
            m: -self.m,
            e: self.e,
        }
    }

    fn add(&self, o: &Big) -> Big {
        if self.m.is_zero() {
            return o.clone();
        }
        if o.m.is_zero() {
            return self.clone();
        }
        let e = self.e.min(o.e);
        let m = (&self.m << (self.e - e) as u64) + (&o.m << (o.e - e) as u64);
        Big { m, e }
    }

    fn mul(&self, o: &Big) -> Big {
        Big {
            m: &self.m * &o.m,
            e: self.e + o.e,
        }
    }

    fn div(&self, o: &Big, p: u64) -> Option<Big> {
        if o.m.is_zero() {
            return None;
        }
        let shift = (p + o.m.bits()).saturating_sub(self.m.bits()) + 2;
        Some(
            Big {
                m: (&self.m << shift) / &o.m,
                e: self.e - o.e - shift as i64,
            }
            .round(p),
        )
    }

    fn sqrt(&self, p: u64) -> Option<Big> {
        if self.m.is_negative() {
            return None;
        }
        let mut shift = (2 * p).saturating_sub(self.m.bits()) as i64;
        if (self.e - shift) % 2 != 0 {
            shift += 1;
        }
        Some(Big {
            m: (&self.m << shift as u64).sqrt(),
            e: (self.e - shift) / 2,
        })
    }

    fn to_f64(&self) -> f64 {
        let b = self.m.bits();
        let (top, mut e) = if b > 64 {
            (&self.m >> (b - 64), self.e + (b - 64) as i64)
        } else {
            (self.m.clone(), self.e)
        };
        let mut v = top.to_f64().unwrap_or(f64::NAN);
        while e != 0 && v.is_finite() && v != 0.0 {
            let k = e.clamp(-1000, 1000);
            v *= 2f64.powi(k as i32);
            e -= k;
        }
        v
    }
}

fn rules() -> Vec<Rewrite<Fp, ()>> {
    let mut rs = vec![
        rw!("comm-add"; "(+ ?a ?b)" => "(+ ?b ?a)"),
        rw!("comm-mul"; "(* ?a ?b)" => "(* ?b ?a)"),
        rw!("neg-neg"; "(neg (neg ?a))" => "?a"),
        rw!("add-inverse"; "(+ ?a (neg ?a))" => "0.0"),
        rw!("sub-self"; "(- ?a ?a)" => "0.0"),
        rw!("cancel-left"; "(- (+ ?a ?b) ?a)" => "?b"),
        rw!("cancel-right"; "(- (+ ?a ?b) ?b)" => "?a"),
        rw!("cancel-sub"; "(- (- ?a ?b) ?a)" => "(neg ?b)"),
        rw!("cancel-under"; "(- ?a (+ ?a ?b))" => "(neg ?b)"),
        rw!("sqrt-square"; "(* (sqrt ?a) (sqrt ?a))" => "?a"),
        rw!("sqrt-conjugate"; "(- (sqrt ?a) (sqrt ?b))"
            => "(/ (- ?a ?b) (+ (sqrt ?a) (sqrt ?b)))"),
        rw!("sub-fractions"; "(- (/ ?a ?b) (/ ?c ?d))"
            => "(/ (- (* ?a ?d) (* ?b ?c)) (* ?b ?d))"),
    ];
    // Literals keep their spelling, so each spelling of zero and one gets its identities.
    for (zero, one) in [("0", "1"), ("0.0", "1.0")] {
        let id = |name: &str, lhs: String| {
            let lhs: egg::Pattern<Fp> = lhs.parse().unwrap();
            let rhs: egg::Pattern<Fp> = "?a".parse().unwrap();
            Rewrite::new(format!("{name}-{one}"), lhs, rhs).unwrap()
        };
        rs.push(id("add-zero", format!("(+ ?a {zero})")));
        rs.push(id("mul-one", format!("(* ?a {one})")));
        rs.push(id("div-one", format!("(/ ?a {one})")));
    }
    rs.extend(
        [
            rw!("assoc-add"; "(+ ?a (+ ?b ?c))" <=> "(+ (+ ?a ?b) ?c)"),
            rw!("assoc-mul"; "(* ?a (* ?b ?c))" <=> "(* (* ?a ?b) ?c)"),
            rw!("sub-as-add"; "(- ?a ?b)" <=> "(+ ?a (neg ?b))"),
            rw!("neg-add"; "(neg (+ ?a ?b))" <=> "(+ (neg ?a) (neg ?b))"),
            rw!("neg-mul"; "(neg (* ?a ?b))" <=> "(* (neg ?a) ?b)"),
            rw!("neg-div"; "(neg (/ ?a ?b))" <=> "(/ (neg ?a) ?b)"),
            rw!("distribute"; "(* ?a (+ ?b ?c))" <=> "(+ (* ?a ?b) (* ?a ?c))"),
            rw!("squares"; "(- (* ?a ?a) (* ?b ?b))" <=> "(* (+ ?a ?b) (- ?a ?b))"),
            rw!("add-fractions"; "(+ (/ ?a ?c) (/ ?b ?c))" <=> "(/ (+ ?a ?b) ?c)"),
            rw!("mul-div"; "(* ?a (/ ?b ?c))" <=> "(/ (* ?a ?b) ?c)"),
        ]
        .into_iter()
        .flatten(),
    );
    rs
}

/// A term of one e-class with its float values and its error against the class's reference.
struct Cand {
    err: f64,
    vals: Vec<f64>,
    term: RecExpr<Fp>,
}

/// The lowest-error term equal to `expr` that the e-graph finds, with its mean error.
///
/// Each class keeps the most accurate term built from its children's kept terms; passes repeat
/// until nothing improves. Error is local to a class, so the choice is greedy.
fn search(
    expr: &RecExpr<Fp>,
    params: &[&str],
    inputs: &[Vec<f64>],
    precs: &[Option<u64>],
    rules: &[Rewrite<Fp, ()>],
) -> Option<(f64, RecExpr<Fp>)> {
    let runner = Runner::default()
        .with_expr(expr)
        .with_iter_limit(ITER_LIMIT)
        .with_node_limit(NODE_LIMIT)
        .with_time_limit(Duration::from_secs(365 * 86_400))
        .run(rules);
    let g = &runner.egraph;
    let root = g.find(runner.roots[0]);
    let mut classes: Vec<Id> = g.classes().map(|c| c.id).collect();
    classes.sort();
    let index = |id: Id| classes.binary_search(&g.find(id)).ok();

    // Each class's value comes from one node whose children's values come first.
    let mut order: Vec<(usize, &Fp)> = Vec::new();
    let mut done = vec![false; classes.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for (i, &c) in classes.iter().enumerate() {
            if done[i] {
                continue;
            }
            let node = g[c].nodes.iter().find(|n| {
                n.children()
                    .iter()
                    .all(|k| index(*k).is_some_and(|k| done[k]))
            });
            if let Some(node) = node {
                order.push((i, node));
                done[i] = true;
                changed = true;
            }
        }
    }
    let mut refs: Vec<Vec<f64>> = vec![vec![f64::NAN; inputs.len()]; classes.len()];
    for (s, (x, p)) in inputs.iter().zip(precs).enumerate() {
        let Some(p) = *p else { continue };
        let mut v: Vec<Option<Big>> = vec![None; classes.len()];
        for &(i, node) in &order {
            v[i] = apply_big(node, params, x, p, |k| v[index(*k).unwrap()].clone());
            refs[i][s] = v[i].as_ref().map_or(f64::NAN, Big::to_f64);
        }
    }

    let mut best: Vec<Option<Cand>> = (0..classes.len()).map(|_| None).collect();
    for _ in 0..EXTRACT_PASSES {
        let mut changed = false;
        for (i, &c) in classes.iter().enumerate() {
            if !done[i] {
                continue;
            }
            let r = &refs[i];
            for node in &g[c].nodes {
                let Some(kids) = node
                    .children()
                    .iter()
                    .map(|k| index(*k).and_then(|k| best[k].as_ref()))
                    .collect::<Option<Vec<&Cand>>>()
                else {
                    continue;
                };
                let size = 1 + kids.iter().map(|k| k.term.as_ref().len()).sum::<usize>();
                if size > MAX_TERM {
                    continue;
                }
                let pos = |k: &Id| node.children().iter().position(|c| c == k).unwrap();
                let vals: Vec<f64> = inputs
                    .iter()
                    .enumerate()
                    .map(|(s, x)| apply_f64(node, params, x, |k| kids[pos(k)].vals[s]))
                    .collect();
                let Some(err) = error(&vals, r) else { continue };
                let better = match &best[i] {
                    None => true,
                    Some(b) => {
                        err + TIE_BITS < b.err
                            || (err <= b.err + TIE_BITS && size < b.term.as_ref().len())
                    }
                };
                if better {
                    let term = graft(node, &kids);
                    best[i] = Some(Cand { err, vals, term });
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    best[index(root)?].take().map(|b| (b.err, b.term))
}

/// `node` over its children's terms, as one term.
fn graft(node: &Fp, kids: &[&Cand]) -> RecExpr<Fp> {
    let mut out: Vec<Fp> = Vec::new();
    let mut roots = Vec::new();
    for k in kids {
        let off = out.len();
        for n in k.term.as_ref() {
            out.push(n.clone().map_children(|c| Id::from(usize::from(c) + off)));
        }
        roots.push(Id::from(out.len() - 1));
    }
    let mut roots = roots.into_iter();
    out.push(node.clone().map_children(|_| roots.next().unwrap()));
    RecExpr::from(out)
}

/// The term in surface syntax, parenthesised so float evaluation order is kept.
fn print(term: &RecExpr<Fp>, sqrt: &str) -> String {
    fn prec(n: &Fp) -> u8 {
        match n {
            Fp::Add(_) | Fp::Sub(_) => 1,
            Fp::Mul(_) | Fp::Div(_) => 2,
            Fp::Neg(_) => 3,
            Fp::Sqrt(_) | Fp::Leaf(_) => 4,
        }
    }
    fn go(t: &[Fp], id: Id, sqrt: &str) -> String {
        let n = &t[usize::from(id)];
        let sub = |k: Id, min: u8| {
            let s = go(t, k, sqrt);
            if prec(&t[usize::from(k)]) < min {
                format!("({s})")
            } else {
                s
            }
        };
        match n {
            Fp::Add([a, b]) | Fp::Sub([a, b]) | Fp::Mul([a, b]) | Fp::Div([a, b]) => {
                let op = match n {
                    Fp::Add(_) => "+",
                    Fp::Sub(_) => "-",
                    Fp::Mul(_) => "*",
                    _ => "/",
                };
                let p = prec(n);
                format!("{} {op} {}", sub(*a, p), sub(*b, p + 1))
            }
            Fp::Neg(a) => format!("-{}", sub(*a, 3)),
            Fp::Sqrt(a) => format!("{sqrt}({})", go(t, *a, sqrt)),
            Fp::Leaf(s) => s.to_string(),
        }
    }
    let t = term.as_ref();
    go(t, Id::from(t.len() - 1), sqrt)
}
