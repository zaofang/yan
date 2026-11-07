//! Type functions (10.4): a function returning `Type`, applied in type position to stage-0
//! arguments. Applications are pure and memoized, and the type they give is identified by the
//! function and the hash of its arguments, so two applications with equal arguments are one type.

use yan_core::ast as y;
use yan_diag::Span;
use yan_hir as j;

use crate::{Cx, Globals, R, Ty, unsupported};

/// The identity of `path(args)`: the function and a stable hash of its stage-0 arguments.
fn key(path: &[String], args: &[y::ConstVal]) -> String {
    // FNV-1a over the arguments' structure: stable across runs and platforms.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{args:?}").bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{}#{h:016x}", path.join("."))
}

/// The stage-0 arguments of an application; each is a literal.
fn values(args: &[j::Arg]) -> R<Vec<y::ConstVal>> {
    args.iter()
        .map(|a| match (&a.name, &a.kind) {
            (None, j::ArgKind::Expr(e)) => crate::const_val(e)
                .or_else(|_| unsupported("a type-function argument that is not a literal")),
            _ => unsupported("a named or `&` type-function argument"),
        })
        .collect()
}

/// The type a memoized application names, for [`Globals::ty`].
pub(crate) fn applied(g: &Globals, path: &[String], args: &[j::Arg]) -> R<Ty> {
    match g.applied.get(&key(path, &values(args)?)) {
        Some(t) => Ok(t.clone()),
        None => unsupported("a type function applied anywhere but `type X = f(...)`"),
    }
}

/// Apply each `type X = f(args)` of the module once, before any type is resolved.
pub(crate) fn collect(g: &mut Globals, m: &j::Module, cx: &mut Cx) -> R<()> {
    for it in &m.items {
        let j::ItemKind::Type(j::TypeDef {
            name,
            rhs: j::TypeRhs::Alias(ts),
            ..
        }) = &it.kind
        else {
            continue;
        };
        let [j::Type::Call { path, args }] = ts.as_slice() else {
            continue;
        };
        let vals = values(args).map_err(|u| u.at(it.span))?;
        let k = key(path, &vals);
        if g.applied.contains_key(&k) {
            continue;
        }
        let t = match path
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["session", "protocol"] => protocol(g, name, args, &vals, it.span, cx)?,
            _ if g.fns.contains_key(&path.join(".")) || path.len() == 1 => {
                return unsupported(
                    "a user type function: building a type needs reflection (`record_type`, `sum_type`, `newtype`)",
                )
                .map_err(|u| u.at(it.span));
            }
            _ => {
                let f = path.join(".");
                cx.err(
                    "E200",
                    it.span,
                    name,
                    format!("cannot find type function `{f}`"),
                );
                Ty::Err
            }
        };
        g.applied.insert(k, t);
    }
    Ok(())
}

fn protocol(
    g: &mut Globals,
    name: &str,
    args: &[j::Arg],
    vals: &[y::ConstVal],
    at: Span,
    cx: &mut Cx,
) -> R<Ty> {
    let (span, spec) = match (args, vals) {
        ([a], [y::ConstVal::Lit(y::Literal::Str(s))]) => (a.span, s.clone()),
        _ => {
            cx.err(
                "E201",
                at,
                name,
                "`session.protocol` takes one string, the protocol".into(),
            );
            return Ok(Ty::Err);
        }
    };
    let span = match &args[0].kind {
        j::ArgKind::Expr(e) => e.span,
        _ => span,
    };
    Ok(match crate::session::protocol(g, name, &spec, span, cx)? {
        Some(p) => {
            let t = Ty::Data(p.name.clone());
            g.sums.insert(p.name.clone(), Vec::new());
            g.protocols.insert(p.name.clone(), p);
            t
        }
        None => Ty::Err,
    })
}
