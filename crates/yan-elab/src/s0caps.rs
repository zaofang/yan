//! What stage 0 may not do (10.4, E268): a type function and the items it reaches may not use
//! `unsafe`, `asm`, `extern` or any capability beyond `alloc`, `embed` and `diag`.

use yan_hir as j;

const ALLOWED: [&str; 3] = ["alloc", "embed", "diag"];

/// The first construct in `items` stage 0 rejects, as a short description.
pub(crate) fn violation(items: &[j::Item]) -> Option<String> {
    items.iter().find_map(|it| match &it.kind {
        j::ItemKind::Extern(_) => Some(format!("`extern` item `{}`", it.kind.name())),
        j::ItemKind::Func(f) => func(f),
        _ => None,
    })
}

fn func(f: &j::FuncDef) -> Option<String> {
    if let Some(j::Uses::Caps(caps)) = &f.uses
        && let Some(c) = caps
            .iter()
            .find(|c| !matches!(c.path.as_slice(), [n] if ALLOWED.contains(&n.as_str())))
    {
        return Some(format!("`{}` uses `{}`", f.name, c.path.join(".")));
    }
    let body = f.body.as_ref()?;
    stmts(&body.stmts).map(|what| format!("`{}` contains {what}", f.name))
}

fn stmts(ss: &[j::Stmt]) -> Option<&'static str> {
    use j::StmtKind as K;
    ss.iter().find_map(|s| match &s.kind {
        K::Unsafe(_) => Some("`unsafe`"),
        K::Asm { .. } => Some("`asm`"),
        K::StageFor { body: b, .. } | K::While { body: b, .. } | K::For { body: b, .. } => stmts(b),
        K::StageIf { then, els, .. } => stmts(then).or_else(|| els.as_deref().and_then(stmts)),
        K::If { arms, els } => arms
            .iter()
            .find_map(|(_, b)| stmts(&b.stmts))
            .or_else(|| els.as_ref().and_then(|b| stmts(&b.stmts))),
        K::Match { cases, .. } => cases.iter().find_map(|c| stmts(&c.body.stmts)),
        K::With { body, .. } => stmts(&body.stmts),
        K::Par { body, .. } => match body {
            j::ParBody::Block(b) | j::ParBody::For { body: b, .. } => stmts(b),
        },
        _ => None,
    })
}
