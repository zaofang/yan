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
