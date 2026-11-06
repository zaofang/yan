//! Module rewrites before elaboration: newtypes become one-field records and capability sets
//! expand in place (14.1, 7.1).

use std::collections::HashMap;

use yan_hir as j;

use yan_diag::Span;

use crate::Cx;

/// `type N = newtype[T]` is a record `N` with the one field `value: T`: built `N(v)`, unwrapped
/// `.value`, with no implicit conversion. `type S = uses a, B` names a set of capabilities that
/// `uses S` expands to; a set that reaches itself is E251, reported at the first set of the cycle.
pub(crate) fn module(m: &j::Module, cx: &mut Cx) -> j::Module {
    let mut sets: HashMap<String, (Vec<j::CapRef>, Span)> = HashMap::new();
    let mut order = Vec::new();
    for it in &m.items {
        if let j::ItemKind::Type(t) = &it.kind
            && let j::TypeRhs::Caps(cs) = &t.rhs
        {
            sets.insert(t.name.clone(), (cs.clone(), it.name_span));
            order.push(t.name.clone());
        }
    }
    let mut cyclic = Vec::new();
    for n in &order {
        if !cyclic.contains(n) && reaches(&sets, n, n, &mut Vec::new()) {
            let mut members = Vec::new();
            for o in &order {
                if reaches(&sets, o, n, &mut Vec::new()) && reaches(&sets, n, o, &mut Vec::new()) {
                    members.push(o.clone());
                }
            }
            cx.err(
                "E251",
                sets[n].1,
                n,
                format!(
                    "capability set `{n}` contains itself: {}",
                    members.join(" -> ")
                ),
            );
            cyclic.extend(members);
        }
    }
    let mut out = m.clone();
    out.items.retain(
        |it| !matches!(&it.kind, j::ItemKind::Type(t) if matches!(t.rhs, j::TypeRhs::Caps(_))),
    );
    for it in &mut out.items {
        match &mut it.kind {
            j::ItemKind::Type(t) if t.where_.is_none() && t.tparams.is_empty() => {
                if let j::TypeRhs::Newtype(ty) = &t.rhs {
                    it.kind = j::ItemKind::Record(j::Record {
                        name: t.name.clone(),
                        tparams: Vec::new(),
                        doc: None,
                        fields: vec![j::Field {
                            name: "value".into(),
                            ty: ty.clone(),
                            ty_span: Span::default(),
                            default: None,
                        }],
                    });
                }
            }
            j::ItemKind::Func(f) | j::ItemKind::Lemma(f) => expand(f, &sets, &cyclic),
            j::ItemKind::Impl(i) => i.fns.iter_mut().for_each(|f| expand(f, &sets, &cyclic)),
            j::ItemKind::Trait(t) => t
                .sigs
                .iter_mut()
                .for_each(|(_, f)| expand(f, &sets, &cyclic)),
            _ => {}
        }
    }
    out
}

type Sets = HashMap<String, (Vec<j::CapRef>, Span)>;

fn set_name(c: &j::CapRef) -> Option<&str> {
    match c.path.as_slice() {
        [n] if c.alias.is_none() => Some(n),
        _ => None,
    }
}

/// Whether set `from` names set `to`, directly or through other sets.
fn reaches(sets: &Sets, from: &str, to: &str, seen: &mut Vec<String>) -> bool {
    if seen.iter().any(|s| s == from) {
        return false;
    }
    seen.push(from.to_owned());
    sets.get(from).is_some_and(|(cs, _)| {
        cs.iter()
            .filter_map(set_name)
            .any(|n| n == to || (sets.contains_key(n) && reaches(sets, n, to, seen)))
    })
}

fn expand(f: &mut j::FuncDef, sets: &Sets, cyclic: &[String]) {
    let Some(j::Uses::Caps(cs)) = &mut f.uses else {
        return;
    };
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for c in cs.drain(..) {
        flatten(c, sets, cyclic, &mut seen, &mut out);
    }
    *cs = out;
}

fn flatten(
    c: j::CapRef,
    sets: &Sets,
    cyclic: &[String],
    seen: &mut Vec<String>,
    out: &mut Vec<j::CapRef>,
) {
    match set_name(&c).filter(|n| sets.contains_key(*n)) {
        Some(n) if cyclic.iter().any(|x| x == n) || seen.iter().any(|x| x == n) => {}
        Some(n) => {
            seen.push(n.to_owned());
            for m in sets[n].0.clone() {
                flatten(m, sets, cyclic, seen, out);
            }
        }
        None if !out.contains(&c) => out.push(c),
        None => {}
    }
}

/// What a package manifest (`package: Package = Package(...)`, 19.1) grants its modules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Manifest {
    /// The capability ceiling; `None` when the manifest names none.
    pub capabilities: Option<Vec<String>>,
    /// The previews the package opts into (20.6).
    pub previews: Vec<String>,
}

/// The manifest a module declares at top level, as `package.yag` and script-form files do.
pub fn manifest(m: &j::Module) -> Option<Manifest> {
    let value = m.items.iter().find_map(|it| match &it.kind {
        j::ItemKind::Const { name, value, .. } if name == "package" => Some(value),
        _ => None,
    })?;
    let j::ExprKind::Call(f, args) = &value.kind else {
        return None;
    };
    if !matches!(&f.kind, j::ExprKind::Name(n) if n == "Package") {
        return None;
    }
    let strs = |name: &str| {
        args.iter().find_map(|a| match (&a.name, &a.kind) {
            (
                Some(n),
                j::ArgKind::Expr(j::Expr {
                    kind: j::ExprKind::List(xs),
                    ..
                }),
            ) if n == name => Some(
                xs.iter()
                    .filter_map(|x| match &x.kind {
                        j::ExprKind::Lit(j::Lit::Str { value, .. }) => Some(value.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
    };
    Some(Manifest {
        capabilities: strs("capabilities"),
        previews: strs("preview").unwrap_or_default(),
    })
}

/// The module without its manifest constant, which is package data rather than an item.
pub(crate) fn without_manifest(m: &mut j::Module) {
    m.items
        .retain(|it| !matches!(&it.kind, j::ItemKind::Const { name, .. } if name == "package"));
}

/// E314: a function uses a capability the package's ceiling leaves out (19.1).
pub(crate) fn ceiling(m: &j::Module, caps: &[String], cx: &mut Cx) {
    for it in &m.items {
        let j::ItemKind::Func(f) = &it.kind else {
            continue;
        };
        let Some(j::Uses::Caps(cs)) = &f.uses else {
            continue;
        };
        for c in cs {
            let n = c.path.join(".");
            if caps.contains(&n) {
                continue;
            }
            let line = cx
                .src
                .lines()
                .nth(f.name_span.line as usize - 1)
                .unwrap_or("");
            let after = line.find(" uses ").map_or(0, |k| k + 6);
            let col = word_at(line, after, &n).map_or(f.name_span.col, |k| k as u32 + 1);
            let at = Span {
                line: f.name_span.line,
                col,
                end_line: f.name_span.line,
                end_col: col + n.len() as u32,
            };
            cx.err(
                "E314",
                at,
                &f.name,
                format!("`{n}` is outside the package's capability ceiling"),
            );
        }
    }
}

/// The byte offset of `w` as a whole word in `line`, at or after `from`.
fn word_at(line: &str, from: usize, w: &str) -> Option<usize> {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    line[from.min(line.len())..]
        .match_indices(w)
        .map(|(k, _)| k + from)
        .find(|&k| {
            !line[..k].chars().next_back().is_some_and(ident)
                && !line[k + w.len()..].chars().next().is_some_and(ident)
        })
}
