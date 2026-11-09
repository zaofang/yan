//! The schema `json.parse[T]` and `json.dump[T]` pass to the interpreter (R874 to R877): the
//! root type and every record or sum reachable from it, as JSON.

use serde_json::{Value as J, json};

use crate::{Field, Globals, cli, ty::Ty};

fn ty(g: &Globals, t: &Ty, todo: &mut Vec<String>) -> Option<J> {
    Some(match t {
        Ty::Bool => json!("bool"),
        Ty::Int => json!("int"),
        Ty::Float => json!("float"),
        Ty::Char => json!("char"),
        Ty::Str => json!("str"),
        Ty::List(e) => json!({"list": ty(g, e, todo)?}),
        Ty::Set(e) => json!({"set": ty(g, e, todo)?}),
        Ty::Dict(k, v) => json!({"dict": [ty(g, k, todo)?, ty(g, v, todo)?]}),
        Ty::Option(e) => json!({"opt": ty(g, e, todo)?}),
        Ty::Tuple(ts) => {
            json!({"tuple": ts.iter().map(|t| ty(g, t, todo)).collect::<Option<Vec<_>>>()?})
        }
        Ty::Data(n) if g.wires.contains(n) => {
            todo.push(n.clone());
            json!({"named": n})
        }
        _ => return None,
    })
}

fn field(g: &Globals, f: &Field, todo: &mut Vec<String>) -> Option<J> {
    let mut out = json!({"n": f.name, "t": ty(g, &f.ty, todo)?});
    if let Some(w) = &f.wire {
        out["w"] = json!(w);
    }
    // A default that is not a literal makes the field required on input.
    if let Some(d) = f.default.as_ref().and_then(cli::literal) {
        out["d"] = d;
    }
    Some(out)
}

/// The schema of `t` and what it reaches, or `None` when a part of it has no json form yet.
pub(crate) fn schema(g: &Globals, root: &Ty) -> Option<String> {
    let mut todo = Vec::new();
    let root = ty(g, root, &mut todo)?;
    let defs = defs(g, todo)?;
    Some(json!({"root": root, "defs": defs}).to_string())
}

/// The schema of a served tool (RFC 0062): its parameters as the record `$args`, and its
/// result, `"unit"` for none. `Err` names the first parameter or result with no json form.
pub(crate) fn tool(g: &Globals, params: &[Field], ret: &Ty) -> Result<J, String> {
    let mut todo = Vec::new();
    let mut fs = Vec::new();
    for p in params {
        fs.push(field(g, p, &mut todo).ok_or_else(|| format!("parameter `{}: {}`", p.name, p.ty))?);
    }
    let ret = match ret {
        Ty::Unit => json!("unit"),
        t => ty(g, t, &mut todo).ok_or_else(|| format!("result `{t}`"))?,
    };
    let mut defs = defs(g, todo).ok_or("a type it reaches")?;
    defs.insert("$args".into(), json!({"rec": fs}));
    Ok(json!({"args": {"named": "$args"}, "ret": ret, "defs": defs}))
}

fn defs(g: &Globals, mut todo: Vec<String>) -> Option<serde_json::Map<String, J>> {
    let mut defs = serde_json::Map::new();
    while let Some(n) = todo.pop() {
        if defs.contains_key(&n) {
            continue;
        }
        let def = if let Some(fs) = g.records.get(&n) {
            json!({"rec": fs.iter().map(|f| field(g, f, &mut todo)).collect::<Option<Vec<_>>>()?})
        } else {
            let vs = g.sums.get(&n)?;
            let mut out = Vec::new();
            for (v, fs) in vs {
                let fs = fs
                    .iter()
                    .map(|f| field(g, f, &mut todo))
                    .collect::<Option<Vec<_>>>()?;
                let mut out_v = json!({"n": v, "fs": fs});
                if let Some(w) = g.variant_wire.get(v) {
                    out_v["w"] = json!(w);
                }
                out.push(out_v);
            }
            json!({"sum": out})
        };
        defs.insert(n.clone(), g.with_wire_opts(&n, def));
    }
    Some(defs)
}
