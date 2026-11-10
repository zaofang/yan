//! `@derive(Wire)` (17.4): the schema of a record or sum that the std codecs and the actor
//! transport read through `yan_rt::wire`.

use std::collections::HashMap;

use yan_rt::wire::{Def, Schema, Schemas};

use crate::{Field, Globals, ty::Ty};

pub(crate) fn schema(
    g: &Globals,
    owner: &str,
    f: &Field,
    t: &Ty,
    todo: &mut Vec<Ty>,
) -> Result<Schema, String> {
    let mut sub = |t: &Ty| schema(g, owner, f, t, todo).map(Box::new);
    Ok(match t {
        Ty::Unit => Schema::Unit,
        Ty::Bool => Schema::Bool,
        Ty::Int => Schema::Int,
        Ty::Float => Schema::Float,
        Ty::Char => Schema::Char,
        Ty::Str => Schema::Str,
        Ty::Coll("bytes", _) => Schema::Bytes,
        Ty::List(e) => Schema::List(sub(e)?),
        Ty::Set(e) => Schema::Set(sub(e)?),
        Ty::Dict(k, v) => Schema::Dict(sub(k)?, sub(v)?),
        Ty::Option(e) => Schema::Option(sub(e)?),
        Ty::Tuple(ts) => Schema::Tuple(
            ts.iter()
                .map(|t| schema(g, owner, f, t, todo))
                .collect::<Result<_, _>>()?,
        ),
        Ty::Data(n) if g.wires.contains(n) => {
            todo.push(t.clone());
            Schema::Named(n.clone())
        }
        // A call through `Remote` carries a result and a failure union (R402).
        Ty::Result(..) | Ty::Union(_) => {
            todo.push(t.clone());
            Schema::Named(t.to_string())
        }
        Ty::Data(n) => {
            return Err(format!(
                "`{owner}.{}` has type `{n}`, which does not derive `Wire`",
                f.name
            ));
        }
        _ => {
            return Err(format!(
                "`{owner}.{}` has type `{t}`, which is not `Wire`",
                f.name
            ));
        }
    })
}

/// The schemas for a value of type `root` crossing a node boundary (R402): the value is the
/// one field of the record `$`, so any `Wire` type, a result or a failure union can be a root.
pub(crate) fn schemas(g: &Globals, root: &Ty) -> Result<Schemas, String> {
    let one = |name: &str, ty: &Ty| Field {
        name: name.into(),
        ty: ty.clone(),
        ty_span: Default::default(),
        default: None,
        wire: None,
        attrs: Vec::new(),
    };
    let at = one("value", root);
    let mut todo = Vec::new();
    let top = schema(g, "a remote call", &at, root, &mut todo)?;
    let mut defs = HashMap::new();
    while let Some(t) = todo.pop() {
        let n = t.to_string();
        if defs.contains_key(&n) {
            continue;
        }
        let mut fields = |owner: &str, fs: &[Field]| {
            fs.iter()
                .map(|f| Ok((f.name.clone(), schema(g, owner, f, &f.ty, &mut todo)?)))
                .collect::<Result<Vec<_>, String>>()
        };
        let def = match &t {
            Ty::Result(a, b) => Def::Sum(vec![
                ("Ok".into(), fields("Ok", &[one("value", a)])?),
                ("Err".into(), fields("Err", &[one("error", b)])?),
            ]),
            Ty::Union(ms) => {
                let mut out = Vec::new();
                for m in ms {
                    let Ty::Data(v) = m else {
                        return Err(format!("failure `{m}` is not a class"));
                    };
                    let fs = g
                        .records
                        .get(v)
                        .or_else(|| {
                            g.sums
                                .values()
                                .flatten()
                                .find(|(n, _)| n == v)
                                .map(|(_, f)| f)
                        })
                        .ok_or_else(|| format!("failure `{v}` is not a class"))?;
                    out.push((v.clone(), fields(v, fs)?));
                }
                Def::Sum(out)
            }
            _ if g.records.contains_key(&n) => Def::Record(fields(&n, &g.records[&n])?),
            _ => {
                let vs = g
                    .sums
                    .get(&n)
                    .ok_or_else(|| format!("`{n}` is not a record or a sum"))?;
                let mut out = Vec::new();
                for (v, fs) in vs {
                    out.push((v.clone(), fields(v, fs)?));
                }
                Def::Sum(out)
            }
        };
        defs.insert(n, def);
    }
    defs.insert("$".into(), Def::Record(vec![("value".into(), top)]));
    Ok(Schemas {
        root: "$".into(),
        defs,
    })
}
