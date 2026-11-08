//! The configuration data file `.yan/config.yag` (19.1, R943).

use yan_hir as j;

/// `config: Config = Config(...)` as written; `unknown` lists fields outside the table (E018).
#[derive(Default)]
pub struct Config {
    pub registries: Vec<(String, String)>,
    pub min_release_age: Option<i64>,
    pub trust: Vec<String>,
    pub offline: bool,
    pub store: Option<String>,
    pub unknown: Vec<String>,
}

pub fn config(m: &j::Module) -> Config {
    let mut out = Config::default();
    let Some(value) = m.items.iter().find_map(|it| match &it.kind {
        j::ItemKind::Const { name, value, .. } if name == "config" => Some(value),
        _ => None,
    }) else {
        return out;
    };
    let j::ExprKind::Call(f, args) = &value.kind else {
        return out;
    };
    if !matches!(&f.kind, j::ExprKind::Name(n) if n == "Config") {
        return out;
    }
    let str_of = |e: &j::Expr| match &e.kind {
        j::ExprKind::Lit(j::Lit::Str { value, .. }) => Some(value.clone()),
        _ => None,
    };
    for a in args {
        let (Some(n), j::ArgKind::Expr(e)) = (&a.name, &a.kind) else {
            continue;
        };
        match (n.as_str(), &e.kind) {
            ("registries", j::ExprKind::Dict(kvs)) => out.registries.extend(
                kvs.iter()
                    .filter_map(|(k, v)| Some((str_of(k)?, str_of(v)?))),
            ),
            ("min_release_age", j::ExprKind::Lit(j::Lit::Int(d))) => {
                out.min_release_age = d.replace('_', "").parse().ok();
            }
            ("trust", j::ExprKind::List(xs)) => out.trust = xs.iter().filter_map(str_of).collect(),
            ("offline", j::ExprKind::Lit(j::Lit::Bool(b))) => out.offline = *b,
            ("store", _) => out.store = str_of(e),
            ("registries" | "min_release_age" | "trust" | "offline", _) => {}
            _ => out.unknown.push(n.clone()),
        }
    }
    out
}
