//! `@access` claims on extern and `@kernel` functions (11.2, R627, R952, R955): the binding rules
//! of RFC 0052 D1 and the halos of RFC 0055 D3.

use std::collections::HashSet;

use yan_core::ast as y;
use yan_diag::Span;
use yan_hir as j;

use crate::ty::Ty;
use crate::{Cx, R, unsupported};

fn literal(e: &j::Expr) -> Option<u64> {
    match &e.kind {
        j::ExprKind::Lit(j::Lit::Int(s)) => s.parse().ok(),
        _ => None,
    }
}

fn arg_expr(a: &j::Arg) -> Option<&j::Expr> {
    match &a.kind {
        j::ArgKind::Expr(e) => Some(e),
        _ => None,
    }
}

/// The region in Core, or why it is malformed for an operand of `rank` axes. `rows`, `cols` and
/// `tile` take an optional `halo: h`, except on `out` (R955).
fn region(e: &j::Expr, rank: usize, out: bool) -> Result<y::ConstVal, String> {
    let path = |n: &str| vec![n.to_owned()];
    let lit = |n: u64| y::ConstVal::Lit(y::Literal::Int(n.to_string()));
    let pos = |v| y::AttrArg {
        name: None,
        value: v,
    };
    match &e.kind {
        j::ExprKind::Name(n) if n == "elementwise" || n == "whole" => Ok(y::ConstVal::Path {
            path: path(n),
            fields: None,
        }),
        j::ExprKind::Call(f, args) => {
            let f = match &f.kind {
                j::ExprKind::Name(n) => n.clone(),
                _ => String::new(),
            };
            let (halo, rest): (Vec<_>, Vec<_>) =
                args.iter().partition(|a| a.name.as_deref() == Some("halo"));
            if !matches!(f.as_str(), "rows" | "cols" | "tile") {
                return Err(match (f.as_str(), halo.is_empty()) {
                    ("elementwise" | "whole", false) => format!("`{f}` takes no halo"),
                    _ => format!("`{f}` is not a region"),
                });
            }
            if rest.iter().any(|a| a.name.is_some()) {
                return Err("`halo` is the one named argument a region accepts".into());
            }
            let halo = match halo.as_slice() {
                [] => None,
                [_] if out => return Err("`out` takes no halo".into()),
                [h] => match arg_expr(h).and_then(literal) {
                    Some(n) => Some(n),
                    None => return Err("`halo` is a non-negative integer literal".into()),
                },
                _ => return Err("`halo` given twice".into()),
            };
            let ints: Vec<Option<u64>> =
                rest.iter().map(|a| arg_expr(a).and_then(literal)).collect();
            let mut bound: Vec<y::AttrArg> = match (f.as_str(), ints.as_slice()) {
                ("rows" | "cols", [Some(axis)]) if (*axis as usize) < rank => vec![pos(lit(*axis))],
                ("rows" | "cols", [Some(axis)]) => {
                    return Err(format!(
                        "axis {axis} is not below the operand's rank {rank}"
                    ));
                }
                ("tile", [Some(m), Some(n)]) if *m > 0 && *n > 0 => {
                    vec![pos(lit(*m)), pos(lit(*n))]
                }
                ("tile", [Some(_), Some(_)]) => return Err("`tile` sizes must be positive".into()),
                _ => return Err(format!("`{f}` takes integer literals")),
            };
            bound.extend(halo.map(|h| y::AttrArg {
                name: Some("halo".into()),
                value: lit(h),
            }));
            Ok(y::ConstVal::Call(path(&f), bound))
        }
        _ => Err(
            "expected `elementwise`, `rows(axis)`, `cols(axis)`, `tile(M, N)` or `whole`".into(),
        ),
    }
}

/// Checks the claims on one extern function and returns them as Core attributes.
pub(crate) fn lower(
    func: &str,
    attrs: &[j::Decorator],
    params: &[(&str, &Ty)],
    ret: &Ty,
    at: Span,
    cx: &mut Cx,
) -> R<Vec<y::Attr>> {
    let mut out = Vec::new();
    for d in attrs {
        if d.path != ["access"] {
            return unsupported("attributes other than `@access` on extern items");
        }
        let mut seen = HashSet::new();
        let mut bound = Vec::new();
        let fail = |cx: &mut Cx, msg: String| cx.err("E284", at, func, msg);
        for a in d.args.iter().flatten() {
            let Some(label) = &a.name else {
                fail(
                    cx,
                    "`@access` binds operands by name: `name: region`".into(),
                );
                continue;
            };
            let operand = if label == "out" {
                Some(ret)
            } else {
                params.iter().find(|(n, _)| n == label).map(|(_, t)| *t)
            };
            let Some(t) = operand else {
                fail(
                    cx,
                    format!("`@access` names `{label}`, which is not a parameter"),
                );
                continue;
            };
            if !seen.insert(label.clone()) {
                fail(cx, format!("`@access` names `{label}` twice"));
                continue;
            }
            let rank = match t {
                Ty::Array(axes, _) => axes.len(),
                _ if label == "out" && *t != Ty::Unit => 0,
                _ => {
                    fail(cx, format!("`{label}` is not an array operand"));
                    continue;
                }
            };
            match arg_expr(a).map(|e| region(e, rank, label == "out")) {
                Some(Ok(v)) => bound.push(y::AttrArg {
                    name: Some(label.clone()),
                    value: v,
                }),
                Some(Err(m)) => fail(cx, format!("`{label}`: {m}")),
                None => fail(cx, format!("`{label}`: expected a region")),
            }
        }
        let needed = params
            .iter()
            .filter(|(_, t)| matches!(t, Ty::Array(..)))
            .map(|(n, _)| *n)
            .chain((*ret != Ty::Unit).then_some("out"));
        for n in needed {
            if !seen.contains(n) {
                fail(cx, format!("`@access` does not bind `{n}`"));
            }
        }
        out.push(y::Attr {
            name: "access".into(),
            args: Some(bound),
        });
    }
    Ok(out)
}
