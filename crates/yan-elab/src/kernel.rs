//! `@kernel` functions (11.2, 17.1, R953 to R955): lowers the body to the affine index IR and
//! reports what the prover finds (RFC 0055 stage 2). A proved kernel runs through `gpu.launch`
//! (stage 3, RFC 0058): it takes `var out` and `uses gpu`, and the grid follows its write region.

use std::collections::HashMap;

use yan_core::ast as y;
use yan_diag::Span;
use yan_hir as j;

use crate::affine::{self, Idx, Kernel, Operand, Region, Stmt};
use crate::ty::Ty;
use crate::{Cx, Globals, R, Unsupported, unsupported};

pub(crate) fn is_kernel(it: &j::Item) -> bool {
    matches!(it.kind, j::ItemKind::Func(_)) && it.decorators.iter().any(|d| d.path == ["kernel"])
}

/// The value of an integer constant expression: literals, the module's integer constants, and
/// `+`, `-`, `*`, `//` and `%` (the last two by a positive divisor) over them, at stage 0 or not.
fn literal(e: &j::Expr, consts: &Consts) -> Option<i128> {
    use j::ExprKind as K;
    match &e.kind {
        K::Lit(j::Lit::Int(s)) => s.replace('_', "").parse().ok(),
        K::Name(n) => consts.get(n).copied(),
        K::Unary("-", a) => literal(a, consts)?.checked_neg(),
        K::Stage(a) => literal(a, consts),
        K::Binary(op, a, b) => {
            let (a, b) = (literal(a, consts)?, literal(b, consts)?);
            match *op {
                "+" => a.checked_add(b),
                "-" => a.checked_sub(b),
                "*" => a.checked_mul(b),
                "//" if b > 0 => Some(a.div_euclid(b)),
                "%" if b > 0 => Some(a.rem_euclid(b)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Integer constants declared at module level, in any order.
pub(crate) type Consts = HashMap<String, i128>;

pub(crate) fn consts(items: &[j::Item]) -> Consts {
    let defs: Vec<(&String, &j::Expr)> = items
        .iter()
        .filter_map(|it| match &it.kind {
            j::ItemKind::Const { name, value, .. } => Some((name, value)),
            _ => None,
        })
        .collect();
    let mut out = Consts::new();
    // Each pass folds at least one more constant, or none is left to fold.
    loop {
        let found: Vec<(String, i128)> = defs
            .iter()
            .filter(|(n, _)| !out.contains_key(*n))
            .filter_map(|(n, v)| literal(v, &out).map(|v| ((*n).clone(), v)))
            .collect();
        if found.is_empty() {
            return out;
        }
        out.extend(found);
    }
}

fn arg_expr(a: &j::Arg) -> Option<&j::Expr> {
    match &a.kind {
        j::ArgKind::Expr(e) => Some(e),
        _ => None,
    }
}

/// The tile shape of `@kernel(tile: M, N)`: positive integers.
fn tile(d: &j::Decorator, consts: &Consts) -> R<Vec<i128>> {
    let args = d.args.as_deref().unwrap_or_default();
    let named_tile = args
        .first()
        .is_some_and(|a| a.name.as_deref() == Some("tile"));
    let unnamed_rest = args.iter().skip(1).all(|a| a.name.is_none());
    if !named_tile || !unnamed_rest {
        return unsupported("a `@kernel` that is not `@kernel(tile: M, N)`");
    }
    let shape: Option<Vec<i128>> = args
        .iter()
        .map(|a| {
            arg_expr(a)
                .and_then(|e| literal(e, consts))
                .filter(|n| *n > 0)
        })
        .collect();
    shape.map_or_else(
        || unsupported("a `@kernel` tile size that is not a positive integer constant"),
        Ok,
    )
}

/// Splits `halo: h` off every region call of `@access`. A halo that is negative, not a literal,
/// repeated or on a region that takes none is E284; the rest goes to the extern checker.
fn split_halos(
    d: &j::Decorator,
    func: &str,
    at: Span,
    cx: &mut Cx,
) -> (j::Decorator, HashMap<String, u64>) {
    let mut halos = HashMap::new();
    let mut out = d.clone();
    for a in out.args.iter_mut().flatten() {
        let (Some(label), j::ArgKind::Expr(e)) = (a.name.clone(), &mut a.kind) else {
            continue;
        };
        let j::ExprKind::Call(f, args) = &mut e.kind else {
            continue;
        };
        let region = match &f.kind {
            j::ExprKind::Name(n) => n.clone(),
            _ => continue,
        };
        let (halo, rest): (Vec<_>, Vec<_>) = std::mem::take(args)
            .into_iter()
            .partition(|a| a.name.as_deref() == Some("halo"));
        *args = rest;
        match halo.as_slice() {
            [] => {}
            [h] if label != "out" && matches!(region.as_str(), "rows" | "cols" | "tile") => {
                match arg_expr(h)
                    .and_then(|e| literal(e, &Consts::new()))
                    .and_then(|n| u64::try_from(n).ok())
                {
                    Some(n) => {
                        halos.insert(label, n);
                    }
                    None => cx.err(
                        "E284",
                        at,
                        func,
                        format!("`{label}`: `halo` is a non-negative integer literal"),
                    ),
                }
            }
            [_] => cx.err(
                "E284",
                at,
                func,
                format!("`{label}`: `{region}` or `out` takes no halo here"),
            ),
            _ => cx.err("E284", at, func, format!("`{label}`: `halo` given twice")),
        }
    }
    (out, halos)
}

fn region(v: &y::ConstVal, halo: u64) -> Region {
    let int = |v: &y::ConstVal| match v {
        y::ConstVal::Lit(y::Literal::Int(s)) => s.parse().unwrap_or(0),
        _ => 0,
    };
    match v {
        y::ConstVal::Call(p, a) => match (p[0].as_str(), a.as_slice()) {
            ("rows", [x]) => Region::Rows {
                axis: int(x) as usize,
                halo,
            },
            ("cols", [x]) => Region::Cols {
                axis: int(x) as usize,
                halo,
            },
            ("tile", [m, n]) => Region::Tile {
                m: int(m),
                n: int(n),
                halo,
            },
            _ => unreachable!("access::lower accepts only these regions"),
        },
        y::ConstVal::Path { path, .. } if path[0] == "whole" => Region::Whole,
        _ => Region::Elementwise,
    }
}

struct Lowering<'a> {
    at: &'a str,
    consts: &'a Consts,
    operands: &'a [Operand],
}

impl Lowering<'_> {
    fn idx(&self, e: &j::Expr) -> Idx {
        use j::ExprKind as K;
        let bin = |a: &j::Expr, b: &j::Expr| (Box::new(self.idx(a)), Box::new(self.idx(b)));
        match &e.kind {
            K::Lit(j::Lit::Int(_)) => literal(e, self.consts).map_or_else(
                || Idx::Opaque("an integer literal out of range".into()),
                Idx::Const,
            ),
            K::Name(n) => self
                .consts
                .get(n)
                .map_or_else(|| Idx::Var(n.clone()), |c| Idx::Const(*c)),
            K::Attr(b, f, _) if matches!(&b.kind, K::Name(n) if n == self.at) => f
                .strip_prefix('i')
                .and_then(|n| n.parse().ok())
                .map_or_else(|| Idx::Opaque(format!("`{}.{f}`", self.at)), Idx::Origin),
            K::Binary("+", a, b) => {
                let (a, b) = bin(a, b);
                Idx::Add(a, b)
            }
            K::Binary("-", a, b) => {
                let (a, b) = bin(a, b);
                Idx::Sub(a, b)
            }
            K::Binary("*", a, b) => {
                let (a, b) = bin(a, b);
                Idx::Mul(a, b)
            }
            K::Binary("/" | "//", a, b) => {
                let (a, b) = bin(a, b);
                Idx::Div(a, b)
            }
            K::Binary("%", a, b) => {
                let (a, b) = bin(a, b);
                Idx::Mod(a, b)
            }
            K::Unary("-", a) => Idx::Sub(Box::new(Idx::Const(0)), Box::new(self.idx(a))),
            K::Index(..) => Idx::Opaque("a loaded value".into()),
            K::Call(..) => Idx::Opaque("a call".into()),
            _ => Idx::Opaque("an expression the prover cannot read".into()),
        }
    }

    fn operand(&self, e: &j::Expr) -> Option<usize> {
        match &e.kind {
            j::ExprKind::Name(n) => self.operands.iter().position(|o| o.name == *n),
            _ => None,
        }
    }

    fn access(&self, e: &j::Expr, write: bool, out: &mut Vec<Stmt>) -> R<bool> {
        let j::ExprKind::Index(base, k) = &e.kind else {
            return Ok(false);
        };
        let Some(operand) = self.operand(base) else {
            return Ok(false);
        };
        let parts: Vec<&j::Expr> = match &k.kind {
            j::ExprKind::Tuple(xs) => xs.iter().collect(),
            _ => vec![k],
        };
        for p in &parts {
            self.reads(p, out)?;
        }
        out.push(Stmt::Access {
            span: e.span,
            operand,
            write,
            index: parts.iter().map(|p| self.idx(p)).collect(),
        });
        Ok(true)
    }

    /// Every operand element read inside `e`.
    fn reads(&self, e: &j::Expr, out: &mut Vec<Stmt>) -> R<()> {
        use j::ExprKind as K;
        if self.access(e, false, out)? {
            return Ok(());
        }
        match &e.kind {
            K::Name(_) | K::Lit(_) => Ok(()),
            K::Binary(_, a, b) | K::Compare(_, a, b) | K::Index(a, b) => {
                self.reads(a, out)?;
                self.reads(b, out)
            }
            K::Unary(_, a) | K::Not(a) | K::Cast(a, _) | K::Attr(a, ..) => self.reads(a, out),
            K::Tuple(xs) => xs.iter().try_for_each(|x| self.reads(x, out)),
            K::Cond { then, cond, els } => [then, cond, els]
                .into_iter()
                .try_for_each(|x| self.reads(x, out)),
            K::Call(f, args) => {
                self.reads(f, out)?;
                args.iter()
                    .filter_map(arg_expr)
                    .try_for_each(|x| self.reads(x, out))
            }
            _ => unsupported("this expression in a `@kernel` body"),
        }
    }

    fn stmts(&self, ss: &[j::Stmt], out: &mut Vec<Stmt>) -> R<()> {
        use j::StmtKind as S;
        for s in ss {
            match &s.kind {
                S::Pass => {}
                S::Expr(e) | S::Return(Some(e)) | S::Bind { value: e, .. } => self.reads(e, out)?,
                S::Assign { target, value, .. } => {
                    self.reads(value, out)?;
                    if !self.access(target, true, out)? {
                        return unsupported(
                            "an assignment in a `@kernel` body to anything but an operand element",
                        );
                    }
                }
                S::If { arms, els } => {
                    for (c, b) in arms {
                        self.reads(c, out)?;
                        self.stmts(&b.stmts, out)?;
                    }
                    if let Some(b) = els {
                        self.stmts(&b.stmts, out)?;
                    }
                }
                S::For {
                    target, iter, body, ..
                } => {
                    let ([var], j::ExprKind::Call(f, args)) = (target.as_slice(), &iter.kind)
                    else {
                        return unsupported(
                            "a `for` in a `@kernel` body over anything but `range`",
                        );
                    };
                    let bounds: Vec<&j::Expr> = args.iter().filter_map(arg_expr).collect();
                    let (lo, hi) = match bounds.as_slice() {
                        [hi] => (Idx::Const(0), self.idx(hi)),
                        [lo, hi] => (self.idx(lo), self.idx(hi)),
                        _ => {
                            return unsupported(
                                "`range` with other than one or two bounds in a `@kernel` body",
                            );
                        }
                    };
                    if !matches!(&f.kind, j::ExprKind::Name(n) if n == "range") {
                        return unsupported(
                            "a `for` in a `@kernel` body over anything but `range`",
                        );
                    }
                    let mut inner = Vec::new();
                    self.stmts(body, &mut inner)?;
                    out.push(Stmt::For {
                        span: s.span,
                        var: var.clone(),
                        lo,
                        hi,
                        body: inner,
                    });
                }
                _ => return unsupported("this statement in a `@kernel` body"),
            }
        }
        Ok(())
    }
}

/// Checks one `@kernel` function after [`prepare`]. Diagnostics go to `cx`; true when the body is
/// proved and the kernel can be lowered.
pub(crate) fn check(g: &Globals, it: &j::Item, consts: &Consts, cx: &mut Cx) -> R<bool> {
    let j::ItemKind::Func(f) = &it.kind else {
        unreachable!("is_kernel")
    };
    // Decorators sit on the lines just above the function's name.
    let line_of = |k: usize| {
        let line = f.name_span.line - (it.decorators.len() - k) as u32;
        Span {
            line,
            col: 1,
            end_line: line,
            end_col: 2,
        }
    };
    let start = cx.diags.len();
    let sig = &g.fns[&f.name];
    let kd = it
        .decorators
        .iter()
        .find(|d| d.path == ["kernel"])
        .expect("is_kernel");
    let tile = tile(kd, consts)?;
    let origins: Vec<&str> = sig
        .params
        .iter()
        .filter(|p| p.ty == Ty::Data("TileOrigin".into()))
        .map(|p| p.name.as_str())
        .collect();
    let [at] = origins.as_slice() else {
        return unsupported("a `@kernel` function without exactly one `TileOrigin` parameter");
    };
    let Some((a_at, ad)) = it
        .decorators
        .iter()
        .enumerate()
        .find(|(_, d)| d.path == ["access"])
    else {
        return unsupported("a `@kernel` function without an `@access` claim");
    };
    let claim = line_of(a_at);

    let (ad, halos) = split_halos(ad, &f.name, claim, cx);
    let named: Vec<(&str, &Ty)> = sig
        .params
        .iter()
        .filter(|p| p.ty != Ty::Data("TileOrigin".into()) && p.name != "out")
        .map(|p| (p.name.as_str(), &p.ty))
        .collect();
    let ret = sig
        .params
        .iter()
        .find(|p| p.name == "out")
        .map_or(&Ty::Unit, |p| &p.ty);
    if cx.diags[start..].iter().any(|d| d.code == "E284") {
        return Ok(false);
    }
    let attrs = crate::access::lower(&f.name, &[ad], &named, ret, claim, cx)?;
    if cx.diags[start..].iter().any(|d| d.code == "E284") {
        return Ok(false);
    }

    let args = attrs.iter().flat_map(|a| a.args.iter().flatten());
    let mut operands = Vec::new();
    for a in args {
        let name = a.name.clone().expect("access::lower binds by name");
        let ty = if name == "out" {
            Some(ret)
        } else {
            named.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
        };
        let rank = match ty {
            Some(Ty::Array(axes, _)) => axes.len(),
            _ => 0,
        };
        let halo = halos.get(&name).copied().unwrap_or(0);
        operands.push(Operand {
            write: name == "out",
            region: region(&a.value, halo),
            name,
            rank,
        });
    }

    let body = f.body.as_ref().map_or(&[][..], |b| &b.stmts);
    let low = Lowering {
        at,
        consts,
        operands: &operands,
    };
    let mut stmts = Vec::new();
    low.stmts(body, &mut stmts).map_err(|u| Unsupported {
        at: u.at.or(Some(it.span)),
        ..u
    })?;
    let kernel = Kernel {
        claim,
        tile,
        operands,
        body: stmts,
    };
    for finding in affine::check(&kernel) {
        cx.err(finding.code, finding.span, &f.name, finding.message);
    }
    Ok(!cx.diags[start..]
        .iter()
        .any(|d| d.severity == yan_diag::Severity::Error))
}

/// Rewrites each `@kernel` function `k(at, args...) -> R` to `k(at, args..., var out: R) -> ()
/// fail KernelFault uses gpu`: a program instance writes its part of the launch's `out`, and a
/// failed bounds check fails it (RFC 0058 D1, R327). Adds the function `gpu.launch(k, ...)`
/// calls, `launcher(k)(args...) -> R fail KernelFault uses gpu`, whose body runs the grid.
pub(crate) fn prepare(m: &mut j::Module) {
    let mut launchers = Vec::new();
    for it in &mut m.items {
        if !is_kernel(it) {
            continue;
        }
        let j::ItemKind::Func(f) = &mut it.kind else {
            unreachable!("is_kernel")
        };
        let Some(r) = f.ret.as_mut() else {
            continue;
        };
        let Some(ty) = r.ty.take() else {
            continue;
        };
        r.fail = vec![vec![FAULT.into()]];
        let at = f.name_span;
        let gpu = || j::CapRef {
            path: vec!["gpu".into()],
            alias: None,
        };
        let args: Vec<j::Param> = f.params.iter().filter(|p| !is_origin(p)).cloned().collect();
        f.params.push(j::Param::Value {
            conv: Some(j::Conv::Var),
            name: "out".into(),
            span: at,
            ty: ty.clone(),
            default: None,
        });
        match &mut f.uses {
            Some(j::Uses::Caps(cs)) => cs.push(gpu()),
            _ => f.uses = Some(j::Uses::Caps(vec![gpu()])),
        }

        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let name = |n: &str| ex(j::ExprKind::Name(n.to_owned()));
        let arg = |e| j::Arg {
            name: None,
            kind: j::ArgKind::Expr(e),
            span: at,
        };
        let forwarded = args.iter().filter_map(|p| match p {
            j::Param::Value { name: n, .. } => Some(arg(name(n))),
            _ => None,
        });
        let launch = ex(j::ExprKind::Call(
            Box::new(ex(j::ExprKind::Attr(
                Box::new(name("gpu")),
                "launch".into(),
                at,
            ))),
            std::iter::once(arg(name(&f.name)))
                .chain(forwarded)
                .collect(),
        ));
        launchers.push(j::Item {
            decorators: Vec::new(),
            is_pub: false,
            kind: j::ItemKind::Func(j::FuncDef {
                name: launcher(&f.name),
                name_span: at,
                tparams: Vec::new(),
                params: args,
                ret: Some(j::RetType {
                    ty: Some(ty),
                    fail: vec![vec![FAULT.into()]],
                    from: Vec::new(),
                }),
                uses: Some(j::Uses::Caps(vec![gpu()])),
                body: Some(j::Body {
                    doc: None,
                    contracts: Vec::new(),
                    stmts: vec![j::Stmt {
                        kind: j::StmtKind::Return(Some(launch)),
                        span: at,
                    }],
                }),
            }),
            span: it.span,
            name_span: at,
        });
    }
    m.items.extend(launchers);
}

/// A `@kernel` body, whose element accesses lower to `std.array.checked2`.
pub(crate) fn takes_origin(f: &j::FuncDef) -> bool {
    f.params.iter().any(is_origin)
}

fn is_origin(p: &j::Param) -> bool {
    matches!(p, j::Param::Value { ty: j::Type::Named { path, .. }, .. } if path == &["TileOrigin"])
}

/// What a failed bounds check in a kernel body fails with (R327).
pub(crate) const FAULT: &str = "KernelFault";

/// The function `gpu.launch(k, ...)` calls.
pub(crate) fn launcher(kernel: &str) -> String {
    format!("__{kernel}_launch")
}

/// The kernel whose launcher `item` is.
pub(crate) fn launched(item: &str) -> Option<&str> {
    item.strip_prefix("__")?.strip_suffix("_launch")
}

/// What a launch of a kernel needs: its `out` type, element zero, sizes and grid steps.
pub(crate) struct Plan {
    pub out: Ty,
    pub elem: Ty,
    pub zero: y::Literal,
    pub dims: Vec<i64>,
    pub steps: Vec<Step>,
}

/// Why a kernel cannot be launched.
pub(crate) enum Unfit {
    /// E521: an `out` axis is not a whole number of tiles (R973).
    Partial(String),
    Unsupported(String),
}

/// The launch of `name`, or why it cannot run; the same for every launch of the kernel.
pub(crate) fn plan(g: &Globals, name: &str) -> Result<Plan, Unfit> {
    let unsupported = |w: &str| Err(Unfit::Unsupported(w.into()));
    let Some(grid) = g.kernels.get(name) else {
        return unsupported("`gpu.launch` of anything but a `@kernel` function by name");
    };
    let out = g.fns[name]
        .params
        .last()
        .map(|p| p.ty.clone())
        .expect("a kernel has `out`");
    let (Some(dims), Ty::Array(_, et)) = (out.dims(), &out) else {
        return unsupported("a `@kernel` result whose sizes are not literals");
    };
    let zero = match **et {
        Ty::Float | Ty::Fixed("f32") => y::Literal::Float("0.0".into()),
        Ty::Int | Ty::Fixed(_) => y::Literal::Int("0".into()),
        Ty::Bool => y::Literal::Bool(false),
        _ => return unsupported("a `@kernel` result of other than number or `bool` elements"),
    };
    let steps = grid.steps(dims.len()).map_err(Unfit::Unsupported)?;
    for (a, (d, s)) in dims.iter().zip(&steps).enumerate() {
        if let Step::By(s) = s
            && d % s != 0
        {
            return Err(Unfit::Partial(format!(
                "axis {a} of `{name}`'s result has {d} elements, not a whole number of \
                 {s}-element tiles"
            )));
        }
    }
    let elem = (**et).clone();
    Ok(Plan {
        out,
        elem,
        zero,
        dims,
        steps,
    })
}

/// The std functions element access and `gpu.launch` lower to, which no source names.
pub(crate) fn array_callees(stmts: &[j::Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        for e in crate::lower::stmt_exprs(s) {
            crate::lower::walk(e, &mut |e| match &e.kind {
                j::ExprKind::Index(_, k) if matches!(k.kind, j::ExprKind::Tuple(_)) => {
                    out.insert("offset2".into());
                }
                j::ExprKind::Call(f, _)
                    if matches!(&f.kind, j::ExprKind::Attr(m, n, _)
                    if n == "launch" && matches!(&m.kind, j::ExprKind::Name(g) if g == "gpu")) =>
                {
                    out.insert("filled".into());
                }
                _ => {}
            });
        }
        for b in crate::lower::sub_blocks(s) {
            array_callees(b, out);
        }
    }
}

/// `TileOrigin`'s fields: an `int` per tile axis (R953). Only `gpu.launch` builds one.
pub(crate) fn origin_fields() -> Vec<crate::Field> {
    let at = Span {
        line: 1,
        col: 1,
        end_line: 1,
        end_col: 1,
    };
    ["i0", "i1"]
        .map(|n| crate::Field {
            name: n.into(),
            ty: Ty::Int,
            ty_span: at,
            default: None,
            wire: None,
            attrs: Vec::new(),
        })
        .into()
}

/// How the launch grid steps along one axis of `out` (RFC 0058 D2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Step {
    /// Origins `0, s, 2s, ...`.
    By(i64),
    /// One program instance, origin 0: the write region leaves the axis free.
    Once,
}

/// A kernel's tile shape and the region its `out` claim names.
#[derive(Clone, Debug)]
pub(crate) struct Grid {
    tile: Vec<i64>,
    out: Region,
}

impl Grid {
    /// The step on each of the `rank` axes of `out`, or why the claim and tile do not fit it.
    pub(crate) fn steps(&self, rank: usize) -> Result<Vec<Step>, String> {
        let tile = |a: usize| {
            self.tile
                .get(a)
                .map(|&t| Step::By(t))
                .ok_or_else(|| "a `@kernel` result with more axes than its tile".to_string())
        };
        match self.out {
            Region::Elementwise => (0..rank).map(tile).collect(),
            Region::Rows { axis, .. } | Region::Cols { axis, .. } => (0..rank)
                .map(|a| if a == axis { tile(a) } else { Ok(Step::Once) })
                .collect(),
            Region::Tile { m, n, .. } if rank == 2 => {
                Ok(vec![Step::By(m as i64), Step::By(n as i64)])
            }
            Region::Tile { .. } => {
                Err("`tile(M, N)` on a result that is not two-dimensional".into())
            }
            Region::Whole => Ok(vec![Step::Once; rank]),
        }
    }
}

/// The grid of each `@kernel` function whose tile and `out` claim are well formed; [`check`]
/// reports the others.
pub(crate) fn grids(items: &[j::Item], consts: &Consts) -> HashMap<String, Grid> {
    let mut out = HashMap::new();
    for it in items.iter().filter(|it| is_kernel(it)) {
        let kd = it.decorators.iter().find(|d| d.path == ["kernel"]);
        let Some(Ok(tile)) = kd.map(|d| tile(d, consts)) else {
            continue;
        };
        let claim = it
            .decorators
            .iter()
            .filter(|d| d.path == ["access"])
            .flat_map(|d| d.args.as_deref().unwrap_or_default())
            .find(|a| a.name.as_deref() == Some("out"))
            .and_then(arg_expr)
            .and_then(out_region);
        if let Some(region) = claim {
            out.insert(
                it.kind.name().to_owned(),
                Grid {
                    tile: tile.into_iter().map(|t| t as i64).collect(),
                    out: region,
                },
            );
        }
    }
    out
}

/// The region of an `out` claim, read from the source: `out` takes no halo (R955).
fn out_region(e: &j::Expr) -> Option<Region> {
    let lit = |a: &j::Arg| match arg_expr(a).map(|e| &e.kind) {
        Some(j::ExprKind::Lit(j::Lit::Int(s))) => s.replace('_', "").parse::<i128>().ok(),
        _ => None,
    };
    match &e.kind {
        j::ExprKind::Name(n) if n == "elementwise" => Some(Region::Elementwise),
        j::ExprKind::Name(n) if n == "whole" => Some(Region::Whole),
        j::ExprKind::Call(f, args) => match (&f.kind, args.as_slice()) {
            (j::ExprKind::Name(n), [a]) if n == "rows" => Some(Region::Rows {
                axis: lit(a)? as usize,
                halo: 0,
            }),
            (j::ExprKind::Name(n), [a]) if n == "cols" => Some(Region::Cols {
                axis: lit(a)? as usize,
                halo: 0,
            }),
            (j::ExprKind::Name(n), [a, b]) if n == "tile" => Some(Region::Tile {
                m: lit(a)?,
                n: lit(b)?,
                halo: 0,
            }),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{Options, elaborate};

    /// Diagnostic codes with their line, in source order.
    fn codes(region: &str, body: &str) -> Vec<String> {
        let src = format!(
            "@kernel(tile: 8, 8)\n@access(a: {region}, out: elementwise)\n\
             scale(at: TileOrigin, a: Array[64, 64, float]) -> Array[64, 64, float] =\n\
             \x20 for i in range(0, 8)\n    for j in range(0, 8)\n      out[at.i0 + i, at.i1 + j] = {body}\n\nmain() = 0\n"
        );
        let (ast, d) = yan_yn::parse("t.yn", &src);
        assert!(d.is_empty(), "{d:?}");
        let (h, d) = yan_yn::lower("t.yn", &src, &ast).expect("lowers");
        assert!(d.is_empty(), "{d:?}");
        let opts = Options {
            yn: true,
            ..Options::default()
        };
        let (_, d) = elaborate("t.yn", &src, &h, opts).expect("supported");
        d.iter()
            .map(|d| format!("{}@{}", d.code, d.span.line))
            .collect()
    }

    #[test]
    fn a_proved_kernel_lowers() {
        assert!(codes("elementwise", "a[at.i0 + i, at.i1 + j]").is_empty());
    }

    #[test]
    fn an_access_past_the_tile_is_e274() {
        assert_eq!(
            codes("elementwise", "a[at.i0 + i + 1, at.i1 + j]"),
            ["E274@6"]
        );
    }

    #[test]
    fn a_halo_widens_the_region() {
        assert!(codes("rows(0, halo: 1)", "a[at.i0 + i + 1, j + 9]").is_empty());
        assert_eq!(codes("rows(0, halo: 1)", "a[at.i0 + i + 2, j]"), ["E274@6"]);
    }

    #[test]
    fn a_product_of_variables_is_not_affine() {
        assert_eq!(
            codes("elementwise", "a[at.i0 * at.i1, at.i1 + j]"),
            ["E274@6"]
        );
    }

    #[test]
    fn a_halo_on_elementwise_is_e284() {
        assert_eq!(codes("elementwise(halo: 1)", "a[i, j]"), ["E284@2"]);
    }

    /// Elaborates, checks and runs a Yan program on the interpreter; the diagnostics' codes on
    /// failure.
    fn run(src: &str) -> Result<String, Vec<String>> {
        let (ast, d) = yan_yn::parse("t.yn", src);
        assert!(d.is_empty(), "{d:?}");
        let (h, d) = yan_yn::lower("t.yn", src, &ast).expect("lowers");
        assert!(d.is_empty(), "{d:?}");
        let opts = Options {
            yn: true,
            ..Options::default()
        };
        let (y, mut d) = elaborate("t.yn", src, &h, opts).expect("supported");
        d.extend(yan_check::check("t.yn", src, &y));
        let errs: Vec<String> = d
            .iter()
            .filter(|d| d.severity == yan_diag::Severity::Error)
            .map(|d| format!("{}@{}", d.code, d.span.line))
            .collect();
        if !errs.is_empty() {
            return Err(errs);
        }
        let mut out = Vec::new();
        yan_interp::run(&y, &mut out).expect("runs");
        Ok(String::from_utf8(out).expect("utf-8"))
    }

    const INC: &str = "@kernel(tile: 2, 2)\n@access(a: elementwise, out: elementwise)\n\
        inc(at: TileOrigin, a: Array[2, 4, int]) -> Array[2, 4, int] =\n\
        \x20 for i in range(0, 2)\n    for j in range(0, 2)\n      \
        out[at.i0 + i, at.i1 + j] = a[at.i0 + i, at.i1 + j] * 10 + at.i1\n\n";

    #[test]
    fn a_launch_runs_one_instance_per_tile() {
        let src = format!(
            "{INC}main() fail KernelFault uses io, gpu =\n\
             \x20 let a: Array[2, 4, int] = [1 2 3 4; 5 6 7 8]\n\
             \x20 let r = gpu.launch(inc, a)?\n\
             \x20 io.print(\"\\(r[0, 0]) \\(r[0, 3]) \\(r[1, 2])\")\n"
        );
        assert_eq!(run(&src), Ok("10 42 72\n".into()));
    }

    #[test]
    fn a_rows_write_region_gives_one_instance_per_row_band() {
        let src = "@kernel(tile: 1, 3)\n@access(a: rows(0), out: rows(0))\n\
            total(at: TileOrigin, a: Array[2, 3, int]) -> Array[2, 3, int] =\n\
            \x20 for j in range(0, 3)\n    out[at.i0, j] = a[at.i0, 0] + a[at.i0, 1] + a[at.i0, 2]\n\n\
            main() fail KernelFault uses io, gpu =\n\
            \x20 let r = gpu.launch(total, [1 2 3; 4 5 6])?\n\
            \x20 io.print(\"\\(r[0, 2]) \\(r[1, 0])\")\n";
        assert_eq!(run(src), Ok("6 15\n".into()));
    }

    #[test]
    fn a_failed_bounds_check_fails_the_launch() {
        let src = "@kernel(tile: 1, 3)\n@access(a: rows(0, halo: 1), out: rows(0))\n\
            prev(at: TileOrigin, a: Array[2, 3, int]) -> Array[2, 3, int] =\n\
            \x20 for j in range(0, 3)\n    out[at.i0, j] = a[at.i0 - 1, j]\n\n\
            fault(a: Array[2, 3, int]) -> str uses gpu =\n\
            \x20 let r = gpu.launch(prev, a) else e ->\n\
            \x20   return \"\\(e.i0) \\(e.i1) \\(e.line):\\(e.col)\"\n\
            \x20 \"\\(r[1, 0])\"\n\n\
            main() uses io, gpu =\n  io.print(fault([1 2 3; 4 5 6]))\n";
        assert_eq!(run(src), Ok("0 0 5:23\n".into()));
    }

    #[test]
    fn a_literal_index_past_an_axis_is_e501() {
        let src = "main() uses io =\n\
            \x20 let a: Array[2, 3, int] = [1 2 3; 4 5 6]\n\
            \x20 io.print(\"\\(a[1, 2]) \\(a[2, 0]) \\(a[0, -1])\")\n";
        assert_eq!(run(src), Err(vec!["E501@3".into(), "E501@3".into()]));
    }

    #[test]
    fn constant_expressions_fold_in_tiles_and_indices() {
        let src = "let w: int = #(h * 2)\nlet h: int = 1\nlet last: int = #(w - h)\n\n\
            @kernel(tile: h, w)\n@access(a: rows(0), out: rows(0))\n\
            pick(at: TileOrigin, a: Array[2, 2, int]) -> Array[2, 2, int] =\n\
            \x20 for j in range(0, w)\n    out[at.i0, j] = a[at.i0, last % w]\n\n\
            main() uses io, gpu =\n\
            \x20 let r = gpu.launch(pick, [1 2; 3 4]) else e ->\n    return\n\
            \x20 io.print(\"\\(r[0, 0]) \\(r[1, 1])\")\n";
        assert_eq!(run(src), Ok("2 4\n".into()));
    }

    #[test]
    fn a_result_of_fixed_width_or_bool_elements_starts_zeroed() {
        let src = "@kernel(tile: 1, 2)\n@access(a: elementwise, out: elementwise)\n\
            pos(at: TileOrigin, a: Array[1, 4, i32]) -> Array[1, 4, bool] =\n\
            \x20 for j in range(0, 2)\n    out[at.i0, at.i1 + j] = a[at.i0, at.i1 + j] > 0\n\n\
            @kernel(tile: 1, 2)\n@access(a: elementwise, out: elementwise)\n\
            keep(at: TileOrigin, a: Array[1, 4, i32]) -> Array[1, 4, i32] =\n\
            \x20 out[at.i0, at.i1] = a[at.i0, at.i1]\n\n\
            main() uses io, gpu =\n\
            \x20 let a: Array[1, 4, i32] = [1 0 3 4]\n\
            \x20 let p = gpu.launch(pos, a) else e ->\n    return\n\
            \x20 let k = gpu.launch(keep, a) else e ->\n    return\n\
            \x20 io.print(\"\\(p[0, 0]) \\(p[0, 1]) \\(p[0, 3]) \\(k[0, 0]) \\(k[0, 1])\")\n";
        assert_eq!(run(src), Ok("True False True 1 0\n".into()));
    }

    #[test]
    fn a_result_that_is_not_whole_tiles_is_e521() {
        let src = INC
            .replace("2, 4, int] =", "2, 5, int] =")
            .replace("-> Array[2, 4", "-> Array[2, 5");
        let src = format!(
            "{src}f(a: Array[2, 5, int]) -> Array[2, 5, int] fail KernelFault uses gpu =\n  gpu.launch(inc, a)?\n\nmain() = 0\n"
        );
        assert_eq!(run(&src), Err(vec!["E521@9".into()]));
    }

    #[test]
    fn a_launch_needs_the_gpu_capability() {
        let src = format!(
            "{INC}f(a: Array[2, 4, int]) -> Array[2, 4, int] fail KernelFault =\n  gpu.launch(inc, a)?\n\nmain() = 0\n"
        );
        assert_eq!(run(&src), Err(vec!["E301@9".into()]));
    }

    #[test]
    fn only_a_launch_calls_a_kernel_or_builds_its_origin() {
        let call = format!("{INC}f(a: Array[2, 4, int]) =\n  inc(a, a)\n\nmain() = 0\n");
        assert_eq!(run(&call), Err(vec!["E017@9".into()]));
        let origin = format!("{INC}f() -> int =\n  TileOrigin(i0: 0, i1: 0).i0\n\nmain() = 0\n");
        assert_eq!(run(&origin), Err(vec!["E017@9".into()]));
    }
}
