//! `@access` claims on extern functions (11.2, R627, R952): the binding rules of RFC 0052 D1.

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

/// The region's text for diagnostics, or why it is malformed for an operand of `rank` axes.
fn region(e: &j::Expr, rank: usize) -> Result<y::ConstVal, String> {
    let name = |e: &j::Expr| match &e.kind {
        j::ExprKind::Name(n) => Some(n.clone()),
        _ => None,
    };
    let path = |n: &str| vec![n.to_owned()];
    match &e.kind {
        j::ExprKind::Name(n) if n == "elementwise" || n == "whole" => Ok(y::ConstVal::Path {
            path: path(n),
            fields: None,
        }),
        j::ExprKind::Call(f, args) => {
            let f = name(f).unwrap_or_default();
            let ints: Vec<Option<u64>> =
                args.iter().map(|a| arg_expr(a).and_then(literal)).collect();
            let lit = |n: u64| y::ConstVal::Lit(y::Literal::Int(n.to_string()));
            match (f.as_str(), ints.as_slice()) {
                ("rows" | "cols", [Some(axis)]) if (*axis as usize) < rank => {
                    Ok(y::ConstVal::Call(path(&f), vec![lit(*axis)]))
                }
                ("rows" | "cols", [Some(axis)]) => Err(format!(
                    "axis {axis} is not below the operand's rank {rank}"
                )),
                ("tile", [Some(m), Some(n)]) if *m > 0 && *n > 0 => {
                    Ok(y::ConstVal::Call(path(&f), vec![lit(*m), lit(*n)]))
                }
                ("tile", [Some(_), Some(_)]) => Err("`tile` sizes must be positive".into()),
                ("rows" | "cols" | "tile", _) => Err(format!("`{f}` takes integer literals")),
                _ => Err(format!("`{f}` is not a region")),
            }
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
            match arg_expr(a).map(|e| region(e, rank)) {
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
