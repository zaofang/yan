//! Affine index proof for `@access` claims on kernel bodies (RFC 0055 D2, D3, R954, R955).
//!
//! A small index IR is reduced to the 8.6 `Lin` form and every access is decided against its
//! operand's region by the 8.6 Fourier-Motzkin solver (`bounds::unsat`). `kernel` lowers `@kernel`
//! bodies to this IR (RFC 0055 D4, stage 2).

use std::collections::{BTreeMap, HashMap};

use yan_diag::Span;

use crate::bounds::{Finding, Lin, unsat};

/// An index expression as the kernel body wrote it.
#[derive(Clone, Debug)]
pub(crate) enum Idx {
    /// A literal or comptime constant, already evaluated.
    Const(i128),
    /// `TileOrigin` field `at.i<axis>`.
    Origin(usize),
    /// A name; provable only when it is a bound loop variable.
    Var(String),
    Add(Box<Idx>, Box<Idx>),
    Sub(Box<Idx>, Box<Idx>),
    Mul(Box<Idx>, Box<Idx>),
    /// Floor division; the divisor must evaluate to a positive constant.
    Div(Box<Idx>, Box<Idx>),
    /// Floored remainder; the divisor must evaluate to a positive constant.
    Mod(Box<Idx>, Box<Idx>),
    /// Anything the prover cannot read, with the reason (`a loaded value`, `a call`).
    Opaque(String),
}

/// The part of an operand a tile may touch (RFC 0055 D3). `halo` widens each bounded axis.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Region {
    Elementwise,
    Whole,
    Rows { axis: usize, halo: u64 },
    Cols { axis: usize, halo: u64 },
    Tile { m: i128, n: i128, halo: u64 },
}

#[derive(Clone, Debug)]
pub(crate) struct Operand {
    pub name: String,
    pub rank: usize,
    pub region: Region,
    pub write: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    /// `for var in lo..hi`: bounds must evaluate to constants.
    For {
        span: Span,
        var: String,
        lo: Idx,
        hi: Idx,
        body: Vec<Stmt>,
    },
    /// An element read or write of `operands[operand]`.
    Access {
        span: Span,
        operand: usize,
        write: bool,
        index: Vec<Idx>,
    },
}

pub(crate) struct Kernel {
    pub claim: Span,
    pub tile: Vec<i128>,
    pub operands: Vec<Operand>,
    pub body: Vec<Stmt>,
}

pub(crate) fn check(k: &Kernel) -> Vec<Finding> {
    let mut p = Prover {
        k,
        facts: Vec::new(),
        divs: HashMap::new(),
        loops: 0,
        out: Vec::new(),
    };
    p.claims();
    p.stmts(&k.body, &mut HashMap::new());
    p.out
}

/// A loop variable's term, or why it cannot be used in an index.
type Env = HashMap<String, Result<Lin, String>>;

struct Prover<'a> {
    k: &'a Kernel,
    /// Constraints `>= 0`: loop ranges and the definitions of division terms.
    facts: Vec<Lin>,
    /// `e / c` terms already defined, so equal quotients are one term.
    divs: HashMap<(Lin, i128), Lin>,
    loops: usize,
    out: Vec<Finding>,
}

fn fail(code: &'static str, span: Span, message: String) -> Finding {
    Finding {
        code,
        span,
        message,
    }
}

fn is_const(l: &Lin) -> Option<i128> {
    l.terms.is_empty().then_some(l.k)
}

impl Prover<'_> {
    /// Claim-level errors: a write halo (E284) and a region the tile or operand cannot carry (E274).
    fn claims(&mut self) {
        let (tile, claim) = (self.k.tile.len(), self.k.claim);
        for o in &self.k.operands {
            let msg = match &o.region {
                Region::Elementwise if o.rank != tile => Some(format!(
                    "`{}` has rank {} but the tile has rank {tile}; `elementwise` needs equal ranks",
                    o.name, o.rank
                )),
                Region::Rows { axis, .. } | Region::Cols { axis, .. }
                    if *axis >= tile || *axis >= o.rank =>
                {
                    Some(format!(
                        "axis {axis} of `{}` is outside the tile (rank {tile}) or the operand (rank {})",
                        o.name, o.rank
                    ))
                }
                Region::Tile { .. } if tile < 2 || o.rank < 2 => Some(format!(
                    "`tile(M, N)` on `{}` needs a tile and an operand of rank 2 or more",
                    o.name
                )),
                _ => None,
            };
            if let Some(m) = msg {
                self.out.push(fail("E274", claim, m));
            }
            let halo = match o.region {
                Region::Rows { halo, .. }
                | Region::Cols { halo, .. }
                | Region::Tile { halo, .. } => halo,
                _ => 0,
            };
            if o.write && halo > 0 {
                self.out.push(fail(
                    "E284",
                    claim,
                    format!(
                        "`{}` is written, and a write halo would let two tiles write one element",
                        o.name
                    ),
                ));
            }
        }
    }

    fn stmts(&mut self, body: &[Stmt], env: &mut Env) {
        for s in body {
            match s {
                Stmt::For {
                    span,
                    var,
                    lo,
                    hi,
                    body,
                } => self.for_loop(*span, var, lo, hi, body, env),
                Stmt::Access {
                    span,
                    operand,
                    write,
                    index,
                } => self.access(*span, *operand, *write, index, env),
            }
        }
    }

    fn for_loop(
        &mut self,
        span: Span,
        var: &str,
        lo: &Idx,
        hi: &Idx,
        body: &[Stmt],
        env: &mut Env,
    ) {
        let bounds = [lo, hi].map(|b| match self.eval(b, env) {
            Ok(l) => is_const(&l).ok_or_else(|| "a loop bound that is not a literal".to_owned()),
            Err(why) => Err(why),
        });
        let term = match bounds {
            [Ok(lo), Ok(hi)] => {
                self.loops += 1;
                let t = Lin::term(format!("loop{}:{var}", self.loops));
                self.facts.push(t.sub(&Lin::konst(lo)));
                self.facts.push(Lin::konst(hi - 1).sub(&t));
                Ok(t)
            }
            [Err(why), _] | [_, Err(why)] => {
                self.out.push(fail(
                    "E274",
                    span,
                    format!("loop over `{var}` has bounds that are not affine literals: {why}"),
                ));
                Err(format!("loop variable `{var}` has data-dependent bounds"))
            }
        };
        let saved = env.insert(var.to_owned(), term);
        self.stmts(body, env);
        match saved {
            Some(s) => env.insert(var.to_owned(), s),
            None => env.remove(var),
        };
    }

    fn eval(&mut self, e: &Idx, env: &Env) -> Result<Lin, String> {
        Ok(match e {
            Idx::Const(c) => Lin::konst(*c),
            Idx::Origin(a) if *a < self.k.tile.len() => Lin::term(format!("at.i{a}")),
            Idx::Origin(a) => return Err(format!("`at.i{a}` is not an axis of the tile")),
            Idx::Var(n) => match env.get(n) {
                Some(t) => t.clone()?,
                None => return Err(format!("`{n}` is not a loop variable or a constant")),
            },
            Idx::Add(a, b) => self.eval(a, env)?.add(&self.eval(b, env)?),
            Idx::Sub(a, b) => self.eval(a, env)?.sub(&self.eval(b, env)?),
            Idx::Mul(a, b) => {
                let (a, b) = (self.eval(a, env)?, self.eval(b, env)?);
                match (is_const(&a), is_const(&b)) {
                    (Some(c), _) => b.scale(c),
                    (_, Some(c)) => a.scale(c),
                    _ => return Err("a product of two variables".to_owned()),
                }
            }
            Idx::Div(a, c) | Idx::Mod(a, c) => {
                let (x, c_l) = (self.eval(a, env)?, self.eval(c, env)?);
                let c = match is_const(&c_l) {
                    Some(c) if c > 0 => c,
                    _ => return Err("a divisor that is not a positive literal".to_owned()),
                };
                let q = match is_const(&x) {
                    Some(k) => Lin::konst(k.div_euclid(c)),
                    None => self.quotient(&x, c),
                };
                if matches!(e, Idx::Div(..)) {
                    q
                } else {
                    x.sub(&q.scale(c))
                }
            }
            Idx::Opaque(why) => return Err(why.clone()),
        })
    }

    /// The term for `floor(x / c)`, defined by `c*q <= x <= c*q + c - 1`.
    fn quotient(&mut self, x: &Lin, c: i128) -> Lin {
        if let Some(q) = self.divs.get(&(x.clone(), c)) {
            return q.clone();
        }
        let q = Lin::term(format!("div{}", self.divs.len()));
        self.facts.push(x.sub(&q.scale(c)));
        self.facts.push(q.scale(c).add(&Lin::konst(c - 1)).sub(x));
        self.divs.insert((x.clone(), c), q.clone());
        q
    }

    fn access(&mut self, span: Span, operand: usize, write: bool, index: &[Idx], env: &Env) {
        let o = &self.k.operands[operand];
        let verb = if write { "write" } else { "read" };
        if o.write != write {
            let why = if write {
                "is not the output"
            } else {
                "is the output, which is write-only"
            };
            self.out.push(fail(
                "E274",
                span,
                format!("cannot {verb} `{}`: it {why}", o.name),
            ));
            return;
        }
        let mut xs = Vec::with_capacity(index.len());
        for (axis, i) in index.iter().enumerate() {
            match self.eval(i, env) {
                Ok(x) => xs.push(x),
                Err(why) => {
                    self.out.push(fail(
                        "E274",
                        span,
                        format!(
                            "index on axis {axis} of `{}` is not affine: {why}; there is no run-time check",
                            o.name
                        ),
                    ));
                    return;
                }
            }
        }
        for (axis, lo, hi) in self.bounds(&o.region, xs.len()) {
            let x = &xs[axis];
            for (side, g, a, b) in [("below", x.sub(&lo), &lo, x), ("above", hi.sub(x), x, &hi)] {
                // The access escapes the region when `g <= -1` is satisfiable under the facts.
                let mut all = self.facts.clone();
                all.push(g.scale(-1).add(&Lin::konst(-1)));
                if !unsat(&all) {
                    self.out.push(fail(
                        "E274",
                        span,
                        format!(
                            "{verb} of `{}` on axis {axis} may fall {side} region `{}`: cannot prove {} <= {}",
                            o.name,
                            region_text(&o.region),
                            show(a),
                            show(b),
                        ),
                    ));
                    return;
                }
            }
        }
    }

    /// Inclusive `(axis, lo, hi)` bounds the region puts on an access of `rank` axes.
    fn bounds(&mut self, r: &Region, rank: usize) -> Vec<(usize, Lin, Lin)> {
        let tile = self.k.tile.clone();
        let origin = |a: usize| Lin::term(format!("at.i{a}"));
        match r {
            Region::Whole => vec![],
            Region::Elementwise => (0..rank)
                .map(|a| (a, origin(a), origin(a).add(&Lin::konst(tile[a] - 1))))
                .collect(),
            Region::Rows { axis, halo } | Region::Cols { axis, halo } => {
                let h = *halo as i128;
                let (lo, hi) = (
                    origin(*axis),
                    origin(*axis).add(&Lin::konst(tile[*axis] - 1)),
                );
                vec![(*axis, lo.sub(&Lin::konst(h)), hi.add(&Lin::konst(h)))]
            }
            Region::Tile { m, n, halo } => {
                let h = *halo as i128;
                [(0, *m), (1, *n)]
                    .into_iter()
                    .map(|(a, size)| {
                        let base = self.quotient(&origin(a), size).scale(size);
                        (
                            a,
                            base.sub(&Lin::konst(h)),
                            base.add(&Lin::konst(size - 1 + h)),
                        )
                    })
                    .collect()
            }
        }
    }
}

fn region_text(r: &Region) -> String {
    let halo = |h: &u64| {
        if *h > 0 {
            format!(", halo: {h}")
        } else {
            String::new()
        }
    };
    match r {
        Region::Elementwise => "elementwise".to_owned(),
        Region::Whole => "whole".to_owned(),
        Region::Rows { axis, halo: h } => format!("rows({axis}{})", halo(h)),
        Region::Cols { axis, halo: h } => format!("cols({axis}{})", halo(h)),
        Region::Tile { m, n, halo: h } => format!("tile({m}, {n}{})", halo(h)),
    }
}

/// A `Lin` in source-like text; loop terms print as their variable, `divN` as a quotient.
fn show(l: &Lin) -> String {
    let name = |t: &str| match t.split_once(':') {
        Some((_, v)) if t.starts_with("loop") => v.to_owned(),
        _ if t.starts_with("div") => format!("({t})"),
        _ => t.to_owned(),
    };
    let sorted: BTreeMap<String, i128> = l.terms.iter().map(|(t, c)| (name(t), *c)).collect();
    let mut s = String::new();
    for (t, c) in sorted {
        let sign = if c < 0 {
            " - "
        } else if s.is_empty() {
            ""
        } else {
            " + "
        };
        let sign = if s.is_empty() && c < 0 { "-" } else { sign };
        let mag = c.abs();
        s += sign;
        s += &if mag == 1 { t } else { format!("{mag}*{t}") };
    }
    match (s.is_empty(), l.k) {
        (true, k) => k.to_string(),
        (false, 0) => s,
        (false, k) if k < 0 => format!("{s} - {}", -k),
        (false, k) => format!("{s} + {k}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(n: i128) -> Box<Idx> {
        Box::new(Idx::Const(n))
    }
    fn v(n: &str) -> Box<Idx> {
        Box::new(Idx::Var(n.to_owned()))
    }
    fn at(a: usize) -> Box<Idx> {
        Box::new(Idx::Origin(a))
    }
    fn add(a: Box<Idx>, b: Box<Idx>) -> Box<Idx> {
        Box::new(Idx::Add(a, b))
    }
    fn sp() -> Span {
        Span::default()
    }
    fn op(name: &str, rank: usize, region: Region, write: bool) -> Operand {
        Operand {
            name: name.to_owned(),
            rank,
            region,
            write,
        }
    }
    fn lp(var: &str, hi: i128, body: Vec<Stmt>) -> Stmt {
        Stmt::For {
            span: sp(),
            var: var.to_owned(),
            lo: Idx::Const(0),
            hi: Idx::Const(hi),
            body,
        }
    }
    fn rd(operand: usize, index: Vec<Idx>) -> Stmt {
        Stmt::Access {
            span: sp(),
            operand,
            write: false,
            index,
        }
    }
    fn wr(operand: usize, index: Vec<Idx>) -> Stmt {
        Stmt::Access {
            span: sp(),
            operand,
            write: true,
            index,
        }
    }
    /// A 4x8 tile over operand `a` (read, `region`) and `out` (elementwise, write).
    fn kernel(region: Region, body: Vec<Stmt>) -> Kernel {
        Kernel {
            claim: sp(),
            tile: vec![4, 8],
            operands: vec![
                op("a", 2, region, false),
                op("out", 2, Region::Elementwise, true),
            ],
            body,
        }
    }
    fn nest(inner: Vec<Stmt>) -> Vec<Stmt> {
        vec![lp("i", 4, vec![lp("j", 8, inner)])]
    }
    fn ij(di: i128, dj: i128) -> Vec<Idx> {
        vec![
            *add(at(0), add(v("i"), c(di))),
            *add(at(1), add(v("j"), c(dj))),
        ]
    }
    fn codes(k: &Kernel) -> Vec<&'static str> {
        check(k).iter().map(|f| f.code).collect()
    }

    #[test]
    fn elementwise_in_region_is_proved() {
        let k = kernel(
            Region::Elementwise,
            nest(vec![rd(0, ij(0, 0)), wr(1, ij(0, 0))]),
        );
        assert!(check(&k).is_empty());
    }

    #[test]
    fn off_by_one_is_e274_with_reason() {
        let k = kernel(Region::Elementwise, nest(vec![rd(0, ij(1, 0))]));
        let f = check(&k);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].code, "E274");
        assert!(
            f[0].message.contains("above region `elementwise`"),
            "{}",
            f[0].message
        );
        assert!(
            f[0].message.contains("at.i0 + i + 1 <= at.i0 + 3"),
            "{}",
            f[0].message
        );
    }

    #[test]
    fn loop_wider_than_tile_escapes() {
        let body = vec![lp("i", 5, vec![rd(0, vec![*add(at(0), v("i")), *at(1)])])];
        assert_eq!(
            codes(&kernel(Region::Whole, body.clone())),
            Vec::<&str>::new()
        );
        let k = Kernel {
            operands: vec![op("a", 2, Region::Rows { axis: 0, halo: 0 }, false)],
            ..kernel(Region::Whole, body)
        };
        assert_eq!(codes(&k), ["E274"]);
    }

    #[test]
    fn rows_constrains_only_its_axis() {
        let body = nest(vec![rd(
            0,
            vec![*add(at(0), v("i")), Idx::Opaque("a loaded value".into())],
        )]);
        let f = check(&kernel(Region::Rows { axis: 0, halo: 0 }, body));
        assert_eq!(f.len(), 1);
        assert!(
            f[0].message.contains("not affine: a loaded value"),
            "{}",
            f[0].message
        );
        let ok = nest(vec![rd(0, vec![*add(at(0), v("i")), *c(1000)])]);
        assert!(check(&kernel(Region::Rows { axis: 0, halo: 0 }, ok)).is_empty());
    }

    #[test]
    fn cols_matches_rows() {
        let body = nest(vec![rd(0, ij(0, 1))]);
        assert_eq!(
            codes(&kernel(Region::Cols { axis: 1, halo: 0 }, body.clone())),
            ["E274"]
        );
        assert!(check(&kernel(Region::Cols { axis: 1, halo: 1 }, body)).is_empty());
    }

    #[test]
    fn halo_widens_both_sides() {
        let one = Region::Rows { axis: 0, halo: 1 };
        let ok = nest(vec![rd(0, ij(-1, 0)), rd(0, ij(1, 0))]);
        assert!(check(&kernel(one.clone(), ok)).is_empty());
        // i + 4 reaches at.i0 + 7, past [at.i0 - 1, at.i0 + 4]
        assert_eq!(
            codes(&kernel(one.clone(), nest(vec![rd(0, ij(2, 0))]))),
            ["E274"]
        );
        assert_eq!(codes(&kernel(one, nest(vec![rd(0, ij(-2, 0))]))), ["E274"]);
    }

    #[test]
    fn write_halo_is_e284() {
        let k = Kernel {
            operands: vec![op("out", 2, Region::Rows { axis: 0, halo: 1 }, true)],
            ..kernel(Region::Whole, vec![])
        };
        assert_eq!(codes(&k), ["E284"]);
    }

    #[test]
    fn write_outside_out_region_and_read_of_out() {
        let k = kernel(Region::Whole, nest(vec![wr(1, ij(0, 8))]));
        assert_eq!(codes(&k), ["E274"]);
        let k = kernel(Region::Whole, nest(vec![wr(0, ij(0, 0)), rd(1, ij(0, 0))]));
        assert_eq!(codes(&k), ["E274", "E274"]);
    }

    #[test]
    fn non_affine_forms_name_the_reason() {
        let cases: [(Idx, &str); 4] = [
            (Idx::Mul(v("i"), v("j")), "a product of two variables"),
            (
                Idx::Div(v("i"), v("j")),
                "a divisor that is not a positive literal",
            ),
            (
                Idx::Mod(v("i"), c(0)),
                "a divisor that is not a positive literal",
            ),
            (Idx::Var("n".into()), "`n` is not a loop variable"),
        ];
        for (idx, why) in cases {
            let body = nest(vec![rd(0, vec![idx, *c(0)])]);
            let f = check(&kernel(Region::Whole, body));
            assert_eq!(f.len(), 1, "{why}");
            assert!(f[0].message.contains(why), "{}", f[0].message);
        }
    }

    #[test]
    fn data_dependent_loop_bound_is_e274() {
        let body = vec![Stmt::For {
            span: sp(),
            var: "i".into(),
            lo: Idx::Const(0),
            hi: Idx::Opaque("a loaded value".into()),
            body: vec![rd(0, vec![*v("i"), *c(0)])],
        }];
        let f = check(&kernel(Region::Whole, body));
        assert_eq!(f.len(), 2);
        assert!(f[0].message.contains("bounds that are not affine literals"));
        assert!(f[1].message.contains("data-dependent bounds"));
    }

    #[test]
    fn constant_scaling_is_affine() {
        // 2*i for i in 0..4 reaches 6, inside [at.i0, at.i0 + 7] only as an offset of at.i0
        let idx = vec![*add(at(0), Box::new(Idx::Mul(c(2), v("i")))), *at(1)];
        let r = Region::Rows { axis: 0, halo: 0 };
        let body = vec![lp("i", 4, vec![rd(0, idx.clone())])];
        assert_eq!(codes(&kernel(r.clone(), body)), ["E274"]);
        let body = vec![lp("i", 2, vec![rd(0, idx)])];
        assert!(check(&kernel(r, body)).is_empty());
    }

    #[test]
    fn div_and_mod_by_literal() {
        // (i % 4) stays in 0..4 inside rows(0); i / 2 for i in 0..8 gives 0..=3 as well
        let m = vec![*add(at(0), Box::new(Idx::Mod(v("i"), c(4)))), *at(1)];
        let d = vec![*add(at(0), Box::new(Idx::Div(v("i"), c(2)))), *at(1)];
        let r = Region::Rows { axis: 0, halo: 0 };
        for idx in [m, d] {
            let body = vec![lp("i", 8, vec![rd(0, idx)])];
            assert!(check(&kernel(r.clone(), body)).is_empty());
        }
        let d = vec![*add(at(0), Box::new(Idx::Div(v("i"), c(2)))), *at(1)];
        let body = vec![lp("i", 10, vec![rd(0, d)])];
        assert_eq!(codes(&kernel(r, body)), ["E274"]);
    }

    #[test]
    fn tile_region_uses_the_rounded_origin() {
        let r = Region::Tile {
            m: 4,
            n: 8,
            halo: 0,
        };
        // (at.i0 / 4) * 4 + i is the block's row, for any origin
        let row = add(
            Box::new(Idx::Mul(Box::new(Idx::Div(at(0), c(4))), c(4))),
            v("i"),
        );
        let col = Box::new(Idx::Mul(Box::new(Idx::Div(at(1), c(8))), c(8)));
        let ok = vec![lp("i", 4, vec![rd(0, vec![*row.clone(), *col.clone()])])];
        assert!(check(&kernel(r.clone(), ok)).is_empty());
        let bad = vec![lp("i", 5, vec![rd(0, vec![*row.clone(), *col])])];
        assert_eq!(codes(&kernel(r.clone(), bad)), ["E274"]);
        // the raw origin is not the block origin: at.i1 + 8 leaves the block
        let off = vec![lp("i", 4, vec![rd(0, vec![*row, *add(at(1), c(8))])])];
        assert_eq!(codes(&kernel(r, off)), ["E274"]);
    }

    #[test]
    fn tile_halo() {
        let r = Region::Tile {
            m: 4,
            n: 8,
            halo: 1,
        };
        let row = *Box::new(Idx::Sub(
            Box::new(Idx::Mul(Box::new(Idx::Div(at(0), c(4))), c(4))),
            c(1),
        ));
        let col = Idx::Mul(Box::new(Idx::Div(at(1), c(8))), c(8));
        assert!(
            check(&kernel(
                r.clone(),
                vec![rd(0, vec![row.clone(), col.clone()])]
            ))
            .is_empty()
        );
        let row2 = Idx::Sub(Box::new(row), c(1));
        assert_eq!(codes(&kernel(r, vec![rd(0, vec![row2, col])])), ["E274"]);
    }

    #[test]
    fn claim_rank_mismatches_are_e274() {
        let k = Kernel {
            operands: vec![op("a", 1, Region::Elementwise, false)],
            ..kernel(Region::Whole, vec![])
        };
        assert_eq!(codes(&k), ["E274"]);
        let k = Kernel {
            operands: vec![op("a", 2, Region::Rows { axis: 2, halo: 0 }, false)],
            ..kernel(Region::Whole, vec![])
        };
        assert_eq!(codes(&k), ["E274"]);
    }

    #[test]
    fn shadowed_loop_variable_restores() {
        let inner = lp("i", 100, vec![]);
        let body = vec![lp(
            "i",
            4,
            vec![inner, rd(0, vec![*add(at(0), v("i")), *at(1)])],
        )];
        let k = Kernel {
            operands: vec![op("a", 2, Region::Rows { axis: 0, halo: 0 }, false)],
            ..kernel(Region::Whole, body)
        };
        assert!(check(&k).is_empty());
    }
}
