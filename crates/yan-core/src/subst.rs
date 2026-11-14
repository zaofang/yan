//! Substitution of type parameters in Core types, and the canonical instance text.

use std::collections::HashMap;

use crate::ast::*;
use crate::printer;

pub type Env = HashMap<String, TArg>;

/// The binding of `ps` to `args`, positionally.
pub fn env(ps: &[TParam], args: &[TArg]) -> Env {
    ps.iter()
        .zip(args)
        .map(|(p, a)| (p.name.clone(), a.clone()))
        .collect()
}

/// Replaces the parameters of `env` in `t`; `expr` rewrites the predicate of a refined type.
pub fn ty(t: &mut Type, env: &Env, expr: &mut dyn FnMut(&mut Expr)) {
    match t {
        Type::Named(n) => {
            if let ([p], []) = (n.path.as_slice(), n.args.as_slice())
                && let Some(TArg::Type(c)) = env.get(p)
            {
                *t = c.clone();
                return;
            }
            targs(&mut n.args, env, expr);
        }
        Type::Dyn(n) => targs(&mut n.args, env, expr),
        Type::Tuple(ts) | Type::Union(ts) => ts.iter_mut().for_each(|t| ty(t, env, expr)),
        Type::Fn(f) => {
            f.params.iter_mut().for_each(|(_, t)| ty(t, env, expr));
            ty(&mut f.ret, env, expr);
            if let Some(t) = &mut f.fail {
                ty(t, env, expr);
            }
        }
        Type::Refined {
            ty: inner, pred, ..
        } => {
            ty(inner, env, expr);
            expr(pred);
        }
    }
}

/// As [`ty`] for arguments; a bare parameter bound to a `Nat` becomes that `Nat`.
pub fn targs(args: &mut [TArg], env: &Env, expr: &mut dyn FnMut(&mut Expr)) {
    for a in args {
        let TArg::Type(t) = a else { continue };
        if let Type::Named(n) = t
            && let ([p], []) = (n.path.as_slice(), n.args.as_slice())
            && let Some(c @ TArg::Nat(_)) = env.get(p)
        {
            *a = c.clone();
        } else {
            ty(t, env, expr);
        }
    }
}

/// `t` with the parameters `ps` replaced by `args`.
pub fn apply(t: &Type, ps: &[TParam], args: &[TArg]) -> Type {
    let mut t = t.clone();
    ty(&mut t, &env(ps, args), &mut |_| {});
    t
}

/// Canonical text of the instance of the item `path` at `args`: `S[i64]`.
pub fn instance(path: &Path, args: &[TArg]) -> String {
    printer::ty(&Type::Named(Named {
        path: path.clone(),
        args: args.to_vec(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(p: &str, args: Vec<TArg>) -> Type {
        Type::Named(Named {
            path: vec![p.into()],
            args,
        })
    }

    fn param(n: &str) -> TParam {
        TParam {
            comptime: false,
            name: n.into(),
            kind: Kind::Type,
            bounds: vec![],
        }
    }

    #[test]
    fn replaces_nested_parameters() {
        let t = Type::Tuple(vec![
            named("T", vec![]),
            named("List", vec![TArg::Type(named("T", vec![]))]),
        ]);
        let got = apply(&t, &[param("T")], &[TArg::Type(named("i64", vec![]))]);
        assert_eq!(printer::ty(&got), "Tuple(i64, List[i64])");
    }

    #[test]
    fn instance_text_is_the_key() {
        let s = instance(&vec!["S".into()], &[TArg::Type(named("i64", vec![]))]);
        assert_eq!(s, "S[i64]");
    }
}
