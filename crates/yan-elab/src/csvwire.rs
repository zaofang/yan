//! The schema `csv.parse[T]` and `csv.dump[T]` pass to the interpreter (R878 to R880): the row
//! type as JSON, with each field reduced to a csv cell type.

use serde_json::{Value as J, json};

use crate::{Field, Globals, cli, ty::Ty};

/// The cell type of `t`, or why it is not a csv scalar (R879).
fn cell(g: &Globals, t: &Ty) -> Result<J, String> {
    Ok(match t {
        Ty::Bool => json!("bool"),
        Ty::Int => json!("int"),
        Ty::Float => json!("float"),
        Ty::Str => json!("str"),
        Ty::Option(e) if !matches!(**e, Ty::Option(_)) => json!({"opt": cell(g, e)?}),
        Ty::Data(n)
            if g.sums
                .get(n)
                .is_some_and(|vs| vs.iter().all(|(_, fs)| fs.is_empty())) =>
        {
            let vs = &g.sums[n];
            json!({"enum": vs.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>()})
        }
        t => return Err(format!("`{t}` is not a csv cell")),
    })
}

fn field(g: &Globals, owner: &str, f: &Field) -> Result<J, String> {
    let t = cell(g, &f.ty).map_err(|m| format!("`{owner}.{}`: {m}", f.name))?;
    let mut out = json!({"n": f.name, "t": t});
    // A default that is not a literal makes the field required on input.
    if let Some(d) = f.default.as_ref().and_then(cli::literal) {
        out["d"] = d;
    }
    Ok(out)
}

/// The schema of row type `root`, or the first reason it has no csv form.
pub(crate) fn schema(g: &Globals, root: &Ty) -> Result<String, String> {
    let Ty::Data(n) = root else {
        return Err(format!("the row type `{root}` is not a record or a sum"));
    };
    let fields = |owner: &str, fs: &[Field]| {
        fs.iter()
            .map(|f| field(g, owner, f))
            .collect::<Result<Vec<_>, _>>()
    };
    let def = if let Some(fs) = g.records.get(n) {
        json!({"rec": fields(n, fs)?})
    } else if let Some(vs) = g.sums.get(n) {
        let mut seen: Vec<(&str, &Ty)> = Vec::new();
        let mut out = Vec::new();
        for (v, fs) in vs {
            for f in fs {
                match seen.iter().find(|(k, _)| *k == f.name) {
                    Some((_, t)) if **t != f.ty => {
                        return Err(format!(
                            "field `{}` has different types in the variants of `{n}`",
                            f.name
                        ));
                    }
                    Some(_) => {}
                    None => seen.push((&f.name, &f.ty)),
                }
            }
            out.push(json!({"n": v, "fs": fields(v, fs)?}));
        }
        json!({"sum": out})
    } else {
        return Err(format!("the row type `{n}` is not a record or a sum"));
    };
    Ok(json!({"root": n, "def": def}).to_string())
}
