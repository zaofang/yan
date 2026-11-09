//! `@access` claims between Core attributes and HIR decorators (RFC 0052).

use yan_core::ast as y;

use crate::raise::{AT, R, no};
use crate::{Arg, ArgKind, Expr, ExprKind, Lit};

pub(crate) fn expr(c: &y::ConstVal) -> R<Expr> {
    let kind = match c {
        y::ConstVal::Lit(y::Literal::Int(i)) => ExprKind::Lit(Lit::Int(i.clone())),
        y::ConstVal::Path { path, fields: None } if path.len() == 1 => {
            ExprKind::Name(path[0].clone())
        }
        y::ConstVal::Call(path, a) if path.len() == 1 => ExprKind::Call(
            Box::new(Expr {
                kind: ExprKind::Name(path[0].clone()),
                span: AT,
                paren: false,
            }),
            args(a)?,
        ),
        _ => return no("this constant in an attribute"),
    };
    Ok(Expr {
        kind,
        span: AT,
        paren: false,
    })
}

pub(crate) fn args(args: &[y::AttrArg]) -> R<Vec<Arg>> {
    args.iter()
        .map(|a| {
            Ok(Arg {
                name: a.name.clone(),
                kind: ArgKind::Expr(expr(&a.value)?),
                span: AT,
            })
        })
        .collect()
}
