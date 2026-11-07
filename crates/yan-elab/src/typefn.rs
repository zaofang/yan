//! Type functions (10.4): a function returning `Type`, applied in type position to stage-0
//! arguments. Applications are pure and memoized, and the type they give is identified by the
//! function and the hash of its arguments, so two applications with equal arguments are one type.

use std::collections::{HashMap, HashSet};

use yan_core::ast as y;
use yan_hir as j;

use crate::session::{self, Endpoint, Protocol};
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

/// What applying a module's type functions gives.
#[derive(Default)]
pub(crate) struct Expanded {
    pub applied: HashMap<String, Ty>,
    pub protocols: HashMap<String, Protocol>,
    pub endpoints: HashMap<String, Endpoint>,
    pub broken: Vec<String>,
}

/// Apply each `type X = f(args)` of `m` once, before any type is resolved. The first alias of
/// a protocol becomes the protocol's own type, a sum without values, and the items that
/// implement it join the module (E265 when one of their names is taken).
pub(crate) fn expand(m: &mut j::Module, cx: &mut Cx) -> R<Expanded> {
    let mut out = Expanded::default();
    let mut taken: HashSet<String> = HashSet::new();
    for it in &m.items {
        taken.insert(it.kind.name().to_owned());
        if let j::ItemKind::Type(j::TypeDef {
            rhs: j::TypeRhs::Sum(vs),
            ..
        }) = &it.kind
        {
            taken.extend(vs.iter().map(|v| v.name.clone()));
        }
    }
    let mut added = Vec::new();
    for it in &mut m.items {
        let j::ItemKind::Type(j::TypeDef {
            name,
            rhs: j::TypeRhs::Alias(ts),
            ..
        }) = &mut it.kind
        else {
            continue;
        };
        let [j::Type::Call { path, args }] = ts.as_slice() else {
            continue;
        };
        let vals = values(args).map_err(|u| u.at(it.span))?;
        let k = key(path, &vals);
        if out.applied.contains_key(&k) {
            continue;
        }
        let t = match path
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["session", "protocol"] => {
                let spec = match (args.as_slice(), vals.as_slice()) {
                    (
                        [
                            j::Arg {
                                kind: j::ArgKind::Expr(e),
                                ..
                            },
                        ],
                        [y::ConstVal::Lit(y::Literal::Str(s))],
                    ) => Some((e.span, s.clone())),
                    _ => None,
                };
                match spec {
                    None => {
                        cx.err(
                            "E201",
                            it.span,
                            name,
                            "`session.protocol` takes one string, the protocol".into(),
                        );
                        Ty::Err
                    }
                    Some((at, spec)) => match session::protocol(name, &spec, at, cx) {
                        None => {
                            out.broken.push(name.clone());
                            Ty::Err
                        }
                        Some(p) => {
                            let items = session::items(&p, it.is_pub, at);
                            let clash = items
                                .iter()
                                .flat_map(|i| {
                                    let mut ns = vec![i.kind.name().to_owned()];
                                    if let j::ItemKind::Type(j::TypeDef {
                                        rhs: j::TypeRhs::Sum(vs),
                                        ..
                                    }) = &i.kind
                                    {
                                        ns.extend(vs.iter().map(|v| v.name.clone()));
                                    }
                                    ns
                                })
                                .find(|n| !taken.insert(n.clone()));
                            if let Some(n) = clash {
                                cx.err(
                                    "E265",
                                    at,
                                    name,
                                    format!("protocol `{name}` defines `{n}`, a name already taken; rename the label or the type"),
                                );
                                out.broken.push(name.clone());
                                Ty::Err
                            } else {
                                added.extend(items);
                                out.endpoints.extend(p.endpoints());
                                // The alias becomes the protocol's type.
                                it.kind = j::ItemKind::Type(j::TypeDef {
                                    name: name.clone(),
                                    tparams: Vec::new(),
                                    rhs: j::TypeRhs::Sum(Vec::new()),
                                    where_: None,
                                });
                                let t = Ty::Data(p.name.clone());
                                out.protocols.insert(p.name.clone(), p);
                                t
                            }
                        }
                    },
                }
            }
            p if p.len() == 1 => {
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
        out.applied.insert(k, t);
    }
    m.items.extend(added);
    Ok(out)
}
