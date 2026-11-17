//! E323 (R395): a `MutexGuard` live across an operation that can wait on another task. A guard
//! is live from its binding to the end of its block, where drop elaboration unlocks it.

use std::collections::HashSet;

use yan_core::ast as y;
use yan_diag::Span;

use crate::Cx;
use crate::lower::reentrant::SUSPENDS;

pub(crate) fn check(items: &[y::Item], cx: &mut Cx) {
    let fns: Vec<(String, &y::Block)> = items
        .iter()
        .filter_map(|it| match &it.kind {
            y::ItemKind::Fn(f) | y::ItemKind::Inst(y::Inst { def: f, .. }) => {
                f.body.as_ref().map(|b| (f.path.join("."), b))
            }
            _ => None,
        })
        .collect();
    let mut waits: HashSet<String> = HashSet::new();
    loop {
        let before = waits.len();
        for (name, body) in &fns {
            if !waits.contains(name) && suspension(&body.stmts, &waits).is_some() {
                waits.insert(name.clone());
            }
        }
        if waits.len() == before {
            break;
        }
    }
    for (name, body) in &fns {
        scan(&body.stmts, &waits, name, cx);
    }
}

fn is_guard(t: &y::Type) -> bool {
    matches!(t, y::Type::Named(n) if n.path.last().is_some_and(|s| s == "MutexGuard"))
}

fn scan(stmts: &[y::Stmt], waits: &HashSet<String>, item: &str, cx: &mut Cx) {
    for (i, s) in stmts.iter().enumerate() {
        for b in blocks(&s.kind) {
            scan(&b.stmts, waits, item, cx);
        }
        if let y::StmtKind::Let { ty, .. } = &s.kind
            && is_guard(ty)
            && let Some(at) = suspension(&stmts[i + 1..], waits)
        {
            cx.err(
                "E323",
                at,
                item,
                "a `MutexGuard` is held across an operation that can wait on another task".into(),
            );
        }
    }
}

/// The first statement or call in `stmts` that can wait on another task.
fn suspension(stmts: &[y::Stmt], waits: &HashSet<String>) -> Option<Span> {
    for s in stmts {
        let call = match &s.kind {
            y::StmtKind::Let { rhs, .. }
            | y::StmtKind::Assign { rhs, .. }
            | y::StmtKind::Rhs(rhs) => match rhs {
                y::Rhs::Call(c) | y::Rhs::Try(c) | y::Rhs::Propagate { call: c, .. } => Some(c),
                _ => None,
            },
            y::StmtKind::Become(c) => Some(c),
            y::StmtKind::Par { .. } => return Some(s.span),
            _ => None,
        };
        if let Some(c) = call {
            let path = c.path.join(".");
            if SUSPENDS.contains(&path.as_str())
                || path.ends_with(".select_key")
                || waits.contains(&path)
            {
                return Some(c.span);
            }
        }
        for b in blocks(&s.kind) {
            if let Some(at) = suspension(&b.stmts, waits) {
                return Some(at);
            }
        }
    }
    None
}

fn blocks(k: &y::StmtKind) -> Vec<&y::Block> {
    use y::StmtKind as K;
    match k {
        K::If { then, els, .. } | K::ComptimeIf { then, els, .. } => {
            std::iter::once(then).chain(els.iter()).collect()
        }
        K::Match { arms, .. } => arms.iter().map(|(_, b)| b).collect(),
        K::Loop { body, .. }
        | K::Jp { body, .. }
        | K::With { body, .. }
        | K::Deadline { body, .. }
        | K::ComptimeFor { body, .. }
        | K::Unsafe(body) => vec![body],
        K::Par { blocks, .. } => blocks.iter().collect(),
        _ => Vec::new(),
    }
}
