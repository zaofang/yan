//! `@derive(Cli)` (RFC 0019 D14): the argument schema of a record, which `cli.parse_args[T]`
//! passes to the interpreter's parser as JSON.

use std::collections::HashMap;

use serde_json::{Value as J, json};
use yan_hir as j;

use crate::{Field, Globals, ty::Ty};

/// `max_depth` as `max-depth`, `DryRun` as `dry-run`.
pub(crate) fn kebab(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        } else if c == '_' {
            out.push('-');
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn literal(e: &j::Expr) -> Option<J> {
    Some(match &e.kind {
        j::ExprKind::Lit(j::Lit::Int(s)) => json!(s.replace('_', "").parse::<i64>().ok()?),
        j::ExprKind::Lit(j::Lit::Float(s)) => json!(s.replace('_', "").parse::<f64>().ok()?),
        j::ExprKind::Lit(j::Lit::Str { value, .. }) => json!(value),
        j::ExprKind::Lit(j::Lit::Bool(b)) => json!(b),
        j::ExprKind::Lit(j::Lit::None) => J::Null,
        j::ExprKind::Unary("-", x) => match literal(x)? {
            J::Number(n) if n.is_i64() => json!(-n.as_i64()?),
            J::Number(n) => json!(-n.as_f64()?),
            _ => return None,
        },
        _ => return None,
    })
}

fn value_ty(g: &Globals, t: &Ty) -> Option<J> {
    Some(match t {
        Ty::Str => json!({"ty": "str"}),
        Ty::Int => json!({"ty": "int"}),
        Ty::Float => json!({"ty": "float"}),
        Ty::Coll("Path", _) => json!({"ty": "Path"}),
        Ty::Data(n) => {
            let vs = g.sums.get(n)?;
            if vs.iter().any(|(_, fs)| !fs.is_empty()) {
                return None;
            }
            let names: Vec<_> = vs.iter().map(|(v, _)| json!([v, kebab(v)])).collect();
            json!({"ty": "choice", "choices": names})
        }
        _ => return None,
    })
}

/// The schema of the fields of one record or command, or why a field is not supported.
fn fields(
    g: &Globals,
    owner: &str,
    fs: &[Field],
    help: &HashMap<String, String>,
) -> Result<Vec<J>, String> {
    let mut out = Vec::new();
    let (mut rest, mut cmd) = (false, false);
    for f in fs {
        if cmd {
            return Err(format!("`{owner}.{}` follows the command field", f.name));
        }
        let default = match &f.default {
            Some(d) => Some(literal(d).ok_or_else(|| {
                format!("`{owner}.{}` has a default that is not a literal", f.name)
            })?),
            None => None,
        };
        let mut s = match (&f.ty, &default) {
            (Ty::Bool, None | Some(J::Bool(false))) => json!({"kind": "flag", "ty": "bool"}),
            (Ty::Bool, _) => {
                return Err(format!(
                    "`{owner}.{}` is a flag, so its default is `False`",
                    f.name
                ));
            }
            (Ty::Option(t), None | Some(J::Null)) => {
                let mut v = value_ty(g, t).ok_or_else(|| unsupported(owner, f))?;
                v["kind"] = json!("opt");
                v["optional"] = json!(true);
                v
            }
            (Ty::List(t), None) => {
                if rest {
                    return Err(format!("`{owner}` has two list fields"));
                }
                rest = true;
                let mut v = value_ty(g, t).ok_or_else(|| unsupported(owner, f))?;
                v["kind"] = json!("rest");
                v
            }
            (Ty::Data(n), None)
                if g.sums
                    .get(n)
                    .is_some_and(|vs| vs.iter().any(|(_, fs)| !fs.is_empty())) =>
            {
                cmd = true;
                let mut cmds = Vec::new();
                for (v, vfs) in &g.sums[n] {
                    cmds.push(json!({
                        "name": v,
                        "cmd": kebab(v),
                        "about": help.get(v).cloned().unwrap_or_default(),
                        "fields": fields(g, v, vfs, &HashMap::new())?,
                    }));
                }
                json!({"kind": "cmd", "commands": cmds})
            }
            (t, d) => {
                let mut v = value_ty(g, t).ok_or_else(|| unsupported(owner, f))?;
                v["kind"] = json!(if d.is_some() { "opt" } else { "pos" });
                if let Some(d) = d {
                    v["default"] = d.clone();
                }
                v
            }
        };
        s["name"] = json!(f.name);
        s["flag"] = json!(kebab(&f.name));
        s["help"] = json!(help.get(&f.name).cloned().unwrap_or_default());
        out.push(s);
    }
    Ok(out)
}

fn unsupported(owner: &str, f: &Field) -> String {
    format!(
        "`{owner}.{}` has type `{}`, which `Cli` cannot parse",
        f.name, f.ty
    )
}

/// The schema of record `name`, or why it cannot derive `Cli`.
pub(crate) fn schema(g: &Globals, name: &str) -> Result<String, String> {
    let help = g.clis.get(name).cloned().unwrap_or_default();
    let fs = g
        .records
        .get(name)
        .ok_or_else(|| format!("`{name}` is not a record"))?;
    for k in help.keys() {
        if k != "about" && !fs.iter().any(|f| f.name == *k) {
            let cmd_named = fs.iter().any(|f| {
                matches!(&f.ty, Ty::Data(n) if g.sums.get(n).is_some_and(|vs| vs.iter().any(|(v, _)| v == k)))
            });
            if !cmd_named {
                return Err(format!(
                    "`@help` names `{k}`, which is not a field of `{name}`"
                ));
            }
        }
    }
    let s = json!({
        "name": name,
        "prog": kebab(name),
        "about": help.get("about").cloned().unwrap_or_default(),
        "fields": fields(g, name, fs, &help)?,
    });
    Ok(s.to_string())
}

/// The `@help(key="text", ...)` arguments of a record, or why one is not a string.
pub(crate) fn help(d: &j::Decorator) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    for a in d.args.as_deref().unwrap_or_default() {
        match (&a.name, &a.kind) {
            (
                Some(k),
                j::ArgKind::Expr(j::Expr {
                    kind: j::ExprKind::Lit(j::Lit::Str { value, .. }),
                    ..
                }),
            ) => {
                out.insert(k.clone(), value.clone());
            }
            _ => return Err("`@help` takes keyword arguments with string values".into()),
        }
    }
    Ok(out)
}
