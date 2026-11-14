//! Join point shapes of R764 the drop pass cannot elaborate in place, rewritten before it runs.
//!
//! A join point body that falls through continues after the statement of its block that
//! jumped to it. The drop pass ends such a body with the drops of one continuation, so:
//!
//! - a join point reached from several statements (directly or through sibling join points
//!   that jump to it) is copied once per statement, each copy jumped to only from its statement;
//! - a falling join point is declared right before the first statement that reaches it when
//!   non-copy locals sit between, so those locals are in scope in its body and live after it.

use std::collections::{HashMap, HashSet};

use yan_check::own;
use yan_core::ast::*;

pub fn split(b: &Block) -> Block {
    let mut b = b.clone();
    block(&mut b, false);
    b
}

fn block(b: &mut Block, par_branch: bool) {
    for s in &mut b.stmts {
        for (child, par) in kids_mut(&mut s.kind) {
            block(child, par);
        }
        if let StmtKind::Let { rhs, .. } | StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) =
            &mut s.kind
            && let Rhs::Closure(c) = rhs
        {
            block(&mut c.body, false);
        }
    }
    if !par_branch {
        rewrite(b);
    }
}

fn kids_mut(k: &mut StmtKind) -> Vec<(&mut Block, bool)> {
    match k {
        StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
            let mut v = vec![(then, false)];
            v.extend(els.as_mut().map(|e| (e, false)));
            v
        }
        StmtKind::Match { arms, .. } => arms.iter_mut().map(|(_, b)| (b, false)).collect(),
        StmtKind::Loop { body, .. }
        | StmtKind::Jp { body, .. }
        | StmtKind::With { body, .. }
        | StmtKind::Deadline { body, .. }
        | StmtKind::ComptimeFor { body, .. } => vec![(body, false)],
        StmtKind::Unsafe(b) => vec![(b, false)],
        StmtKind::Par { blocks, .. } => blocks.iter_mut().map(|b| (b, true)).collect(),
        _ => Vec::new(),
    }
}

fn kids(k: &StmtKind) -> Vec<&Block> {
    match k {
        StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
            let mut v = vec![then];
            v.extend(els.as_ref());
            v
        }
        StmtKind::Match { arms, .. } => arms.iter().map(|(_, b)| b).collect(),
        StmtKind::Loop { body, .. }
        | StmtKind::Jp { body, .. }
        | StmtKind::With { body, .. }
        | StmtKind::Deadline { body, .. }
        | StmtKind::ComptimeFor { body, .. } => vec![body],
        StmtKind::Unsafe(b) => vec![b],
        StmtKind::Par { blocks, .. } => blocks.iter().collect(),
        _ => Vec::new(),
    }
}

/// The names `s` jumps to, including from the bodies of join points declared inside it.
fn jumps_in(s: &Stmt, out: &mut HashSet<Name>) {
    if let StmtKind::Jump { name, .. } = &s.kind {
        out.insert(name.clone());
    }
    for b in kids(&s.kind) {
        for s in &b.stmts {
            jumps_in(s, out);
        }
    }
}

fn rename_jumps(s: &mut Stmt, to: &HashMap<Name, Name>) {
    if let StmtKind::Jump { name, .. } = &mut s.kind
        && let Some(n) = to.get(name)
    {
        *name = n.clone();
    }
    for (b, _) in kids_mut(&mut s.kind) {
        for s in &mut b.stmts {
            rename_jumps(s, to);
        }
    }
}

fn diverges(b: &Block) -> bool {
    match b.stmts.last().map(|s| &s.kind) {
        Some(
            StmtKind::Ret(_)
            | StmtKind::Fail(_)
            | StmtKind::Become(_)
            | StmtKind::Jump { .. }
            | StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Unreachable,
        ) => true,
        Some(StmtKind::If { then, els, .. }) => {
            els.as_ref().is_some_and(|e| diverges(then) && diverges(e))
        }
        Some(StmtKind::Match { arms, .. }) => {
            !arms.is_empty() && arms.iter().all(|(_, b)| diverges(b))
        }
        Some(StmtKind::With { body, .. } | StmtKind::Deadline { body, .. }) => diverges(body),
        Some(StmtKind::Unsafe(body)) => diverges(body),
        _ => false,
    }
}

fn rewrite(b: &mut Block) {
    let n = b.stmts.len();
    let jp_at: Vec<usize> = (0..n)
        .filter(|&i| matches!(b.stmts[i].kind, StmtKind::Jp { .. }))
        .collect();
    if jp_at.is_empty() {
        return;
    }
    let name_of = |s: &Stmt| match &s.kind {
        StmtKind::Jp { name, .. } => name.clone(),
        _ => unreachable!("a join point"),
    };
    let names: Vec<Name> = jp_at.iter().map(|&i| name_of(&b.stmts[i])).collect();
    let direct: Vec<HashSet<Name>> = b
        .stmts
        .iter()
        .map(|s| {
            let mut set = HashSet::new();
            jumps_in(s, &mut set);
            set.retain(|n| names.contains(n));
            set
        })
        .collect();
    let calls = |j: usize| &direct[jp_at[j]];

    // The join points each statement can run, through jumps between siblings.
    let reach: Vec<HashSet<usize>> = (0..n)
        .map(|i| {
            let mut r: HashSet<usize> = direct[i]
                .iter()
                .filter_map(|x| names.iter().position(|n| n == x))
                .collect();
            if matches!(b.stmts[i].kind, StmtKind::Jp { .. }) {
                return HashSet::new();
            }
            loop {
                let before = r.len();
                for j in r.clone() {
                    r.extend(
                        calls(j)
                            .iter()
                            .filter_map(|x| names.iter().position(|n| n == x)),
                    );
                }
                if r.len() == before {
                    break r;
                }
            }
        })
        .collect();
    let stmts_of = |j: usize| -> Vec<usize> { (0..n).filter(|i| reach[*i].contains(&j)).collect() };
    let callees = |j: usize| -> Vec<usize> {
        calls(j)
            .iter()
            .filter_map(|x| names.iter().position(|n| n == x))
            .filter(|c| *c != j)
            .collect()
    };

    let mut falls: Vec<bool> = jp_at
        .iter()
        .map(|&i| match &b.stmts[i].kind {
            StmtKind::Jp { body, .. } => !diverges(body),
            _ => false,
        })
        .collect();
    loop {
        let mut changed = false;
        for j in 0..names.len() {
            if !falls[j] && callees(j).iter().any(|&c| falls[c]) {
                falls[j] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut split = vec![false; names.len()];
    loop {
        let mut changed = false;
        for j in 0..names.len() {
            if !split[j]
                && stmts_of(j).len() > 1
                && (falls[j] || callees(j).iter().any(|&c| split[c]))
            {
                split[j] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut before: HashMap<usize, Vec<Stmt>> = HashMap::new();
    let mut keep: Vec<(usize, Stmt)> = Vec::new();
    for (j, &at) in jp_at.iter().enumerate() {
        let users = stmts_of(j);
        // A join point moves with the join points it jumps to, which must stay before it.
        let mut family = vec![j];
        let mut i = 0;
        while i < family.len() {
            for c in callees(family[i]) {
                if !family.contains(&c) {
                    family.push(c);
                }
            }
            i += 1;
        }
        let start = family.iter().map(|&c| jp_at[c]).min().unwrap_or(at);
        let orig = b.stmts[at].clone();
        let make = |s: usize, rename_self: bool| {
            let mut copy = orig.clone();
            let map: HashMap<Name, Name> = reach[s]
                .iter()
                .filter(|&&c| split[c])
                .map(|&c| (names[c].clone(), format!("{}__{s}", names[c])))
                .collect();
            if let StmtKind::Jp { name, body, .. } = &mut copy.kind {
                for st in &mut body.stmts {
                    rename_jumps(st, &map);
                }
                if rename_self {
                    *name = format!("{name}__{s}");
                }
            }
            copy
        };
        let instances: Vec<(Option<usize>, Stmt)> = if split[j] {
            users.iter().map(|&s| (Some(s), make(s, true))).collect()
        } else if let [s] = users[..] {
            vec![(Some(s), make(s, false))]
        } else {
            vec![(None, orig.clone())]
        };
        for (s, inst) in instances {
            let moves = falls[j]
                && s.is_some_and(|s| {
                    s + 1 < n
                        && (start + 1..s).any(|k| {
                            matches!(&b.stmts[k].kind,
                                StmtKind::Let { ty, .. } if !own::is_copy(ty))
                        })
                });
            match s {
                Some(s) if moves => {
                    before.entry(s).or_default().push(inst);
                }
                _ => keep.push((at, inst)),
            }
        }
    }
    let mut out: Vec<Stmt> = Vec::new();
    let mut kept = keep.into_iter().peekable();
    for (i, s) in b.stmts.iter().enumerate() {
        if matches!(s.kind, StmtKind::Jp { .. }) {
            while let Some((_, inst)) = kept.next_if(|(at, _)| *at == i) {
                out.push(inst);
            }
        } else {
            out.append(&mut before.remove(&i).unwrap_or_default());
            let mut s = s.clone();
            let map: HashMap<Name, Name> = reach[i]
                .iter()
                .filter(|&&c| split[c])
                .map(|&c| (names[c].clone(), format!("{}__{i}", names[c])))
                .collect();
            rename_jumps(&mut s, &map);
            out.push(s);
        }
    }
    b.stmts = out;
}
