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
    todo: &mut Vec<String>,
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
            todo.push(n.clone());
            Schema::Named(n.clone())
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

/// Not called yet: the std codecs (json, csv) read it.
///
/// The schemas reachable from record or sum `name`, or why it cannot derive `Wire`.
#[allow(dead_code)]
pub(crate) fn schemas(g: &Globals, name: &str) -> Result<Schemas, String> {
    let mut defs = HashMap::new();
    let mut todo = vec![name.to_owned()];
    while let Some(n) = todo.pop() {
        if defs.contains_key(&n) {
            continue;
        }
        let mut fields = |owner: &str, fs: &[Field]| {
            fs.iter()
                .map(|f| Ok((f.name.clone(), schema(g, owner, f, &f.ty, &mut todo)?)))
                .collect::<Result<Vec<_>, String>>()
        };
        let def = if let Some(fs) = g.records.get(&n) {
            Def::Record(fields(&n, fs)?)
        } else if let Some(vs) = g.sums.get(&n) {
            let mut out = Vec::new();
            for (v, fs) in vs {
                out.push((v.clone(), fields(v, fs)?));
            }
            Def::Sum(out)
        } else {
            return Err(format!("`{n}` is not a record or a sum"));
        };
        defs.insert(n, def);
    }
    Ok(Schemas {
        root: name.to_owned(),
        defs,
    })
}
