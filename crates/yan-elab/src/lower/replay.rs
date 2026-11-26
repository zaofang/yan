//! `replay(trace, live: [...])` handler recognition (7.6, E316).

use yan_diag::Span;
use yan_hir as j;

/// Capabilities whose operations return nothing the program reads (7.6).
const OUTPUT_ONLY: [&str; 2] = ["log", "metrics"];

/// The `live:` entries of a `replay(...)` call that are not output-only capabilities.
pub(super) fn live_non_output(h: &j::Expr) -> Vec<(String, Span)> {
    let j::ExprKind::Call(f, args) = &h.kind else {
        return Vec::new();
    };
    if !matches!(&f.kind, j::ExprKind::Name(n) if n == "replay") {
        return Vec::new();
    }
    let Some(j::ArgKind::Expr(j::Expr {
        kind: j::ExprKind::List(items),
        ..
    })) = args
        .iter()
        .find(|a| a.name.as_deref() == Some("live"))
        .map(|a| &a.kind)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|e| match &e.kind {
            j::ExprKind::Name(n) => Some((n.to_string(), e.span)),
            j::ExprKind::Lit(j::Lit::Str { value, .. }) => Some((value.clone(), e.span)),
            _ => None,
        })
        .filter(|(n, _)| !OUTPUT_ONLY.contains(&n.as_str()))
        .collect()
}
