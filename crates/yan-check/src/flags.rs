//! The drop-flag pattern of 5.3: a `var _dN: bool = true` right after the declaration of the
//! local it guards (R770). Which local each flag guards, for the passes after the check.

use std::collections::HashMap;

use yan_core::ast::*;

use crate::own;

pub(crate) fn is_flag_init(s: &Stmt) -> Option<&Name> {
    match &s.kind {
        StmtKind::Let {
            pat: Pat::Bind(f),
            rhs: Rhs::Atom(Atom::Lit(Literal::Bool(true), _)),
            ..
        } if f.starts_with("_d") => Some(f),
        _ => None,
    }
}

/// Each flagged local of a body to its flag. Closure bodies are left to their own call.
pub fn drop_flags(params: &[Param], body: &Block) -> HashMap<Name, Name> {
    let mut out = HashMap::new();
    walk(owned_params(params).last().copied(), body, &mut out);
    out
}

fn walk<'a>(mut last: Option<&'a Name>, b: &'a Block, out: &mut HashMap<Name, Name>) {
    for s in &b.stmts {
        if let Some(f) = is_flag_init(s) {
            if let Some(x) = last.take() {
                out.insert(x.clone(), f.clone());
            }
            continue;
        }
        last = None;
        match &s.kind {
            StmtKind::Let { pat, ty, rhs, .. } if !matches!(rhs, Rhs::Proj(_)) => {
                last = last_decl(pat, ty);
            }
            StmtKind::Jp { params, body, .. } => {
                let owned = owned_params(params);
                walk(owned.last().copied(), body, out);
            }
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                walk(None, then, out);
                if let Some(e) = els {
                    walk(None, e, out);
                }
            }
            StmtKind::Match { arms, .. } => arms.iter().for_each(|(_, a)| walk(None, a, out)),
            StmtKind::Par { blocks, .. } => blocks.iter().for_each(|a| walk(None, a, out)),
            StmtKind::Loop { body, .. }
            | StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::ComptimeFor { body, .. }
            | StmtKind::Unsafe(body) => walk(None, body, out),
            _ => {}
        }
    }
}

/// The last non-copy name `p` declares.
fn last_decl<'a>(p: &'a Pat, ty: &Type) -> Option<&'a Name> {
    match (p, ty) {
        (Pat::Bind(n), _) if !own::is_copy(ty) => Some(n),
        (Pat::Tuple(ps), Type::Tuple(ts)) if ps.len() == ts.len() => {
            ps.iter().zip(ts).rev().find_map(|(p, t)| last_decl(p, t))
        }
        _ => None,
    }
}

pub(crate) fn owned_params(params: &[Param]) -> Vec<&Name> {
    params
        .iter()
        .filter_map(|p| match p {
            Param::Value {
                conv: Conv::Sink,
                name,
                ty,
                ..
            } if !own::is_copy(ty) => Some(name),
            _ => None,
        })
        .collect()
}
