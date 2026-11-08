//! The configuration data file `.yan/config.yag` (19.3, R895).

use yan_hir as j;

/// The `registries` table of `config: Config = Config(registries={...})`, namespace to index, as written.
pub fn registries(m: &j::Module) -> Vec<(String, String)> {
    let Some(value) = m.items.iter().find_map(|it| match &it.kind {
        j::ItemKind::Const { name, value, .. } if name == "config" => Some(value),
        _ => None,
    }) else {
        return Vec::new();
    };
    let j::ExprKind::Call(f, args) = &value.kind else {
        return Vec::new();
    };
    if !matches!(&f.kind, j::ExprKind::Name(n) if n == "Config") {
        return Vec::new();
    }
    let lit = |e: &j::Expr| match &e.kind {
        j::ExprKind::Lit(j::Lit::Str { value, .. }) => Some(value.clone()),
        _ => None,
    };
    args.iter()
        .find_map(|a| match (&a.name, &a.kind) {
            (
                Some(n),
                j::ArgKind::Expr(j::Expr {
                    kind: j::ExprKind::Dict(kvs),
                    ..
                }),
            ) if n == "registries" => Some(
                kvs.iter()
                    .filter_map(|(k, v)| Some((lit(k)?, lit(v)?)))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}
