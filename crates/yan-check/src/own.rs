//! Ownership facts shared by drop elaboration (`yan-opt`) and the post-optimization check (D2, D3).

use std::collections::HashSet;

use yan_core::ast::*;
use yan_core::intrinsics;

const COPY_SCALARS: [&str; 6] = ["bool", "char", "Unit", "f32", "f64", "dec128"];

/// Values of a copy type are never owned resources: nothing consumes or drops them.
pub fn is_copy(t: &Type) -> bool {
    match t {
        Type::Named(n) => n
            .path
            .last()
            .is_some_and(|b| intrinsics::is_int(b) || COPY_SCALARS.contains(&b.as_str())),
        Type::Tuple(ts) => ts.iter().all(is_copy),
        Type::Refined { ty, .. } => is_copy(ty),
        _ => false,
    }
}

fn name(a: &Atom) -> Option<&Name> {
    match a {
        Atom::Name(n, _) => Some(n),
        Atom::Lit(..) => None,
    }
}

fn call_sinks(c: &Call) -> impl Iterator<Item = &Name> {
    c.args.iter().filter_map(|a| match a {
        Arg::Value(Conv::Sink, a, _) => name(a),
        _ => None,
    })
}

fn rhs(r: &Rhs) -> Vec<&Name> {
    match r {
        Rhs::Atom(a) => name(a).into_iter().collect(),
        Rhs::Call(c) | Rhs::Propagate { call: c, .. } | Rhs::Try(c) => call_sinks(c).collect(),
        Rhs::Ctor(Ctor::Named { fields, .. }) => {
            fields.iter().filter_map(|(_, a)| name(a)).collect()
        }
        Rhs::Ctor(Ctor::Tuple(xs)) => xs.iter().filter_map(name).collect(),
        Rhs::Move(p) => vec![&p.root],
        Rhs::Closure(c) => c
            .captures
            .iter()
            .filter(|(cv, ..)| *cv == Conv::Sink)
            .map(|(_, n, _)| n)
            .collect(),
        Rhs::Proj(_) | Rhs::Copy(_) => Vec::new(),
    }
}

/// The names a statement consumes by itself, not counting its nested blocks. Whether a name is
/// an owned local is the caller's question.
pub fn consumed(k: &StmtKind) -> Vec<&Name> {
    match k {
        StmtKind::Let { rhs: r, .. } | StmtKind::Assign { rhs: r, .. } | StmtKind::Rhs(r) => rhs(r),
        StmtKind::Ret(a) | StmtKind::Fail(a) | StmtKind::Yield(a) => name(a).into_iter().collect(),
        StmtKind::Break { value: Some(a), .. } => name(a).into_iter().collect(),
        StmtKind::Jump { args, .. } => args.iter().filter_map(name).collect(),
        StmtKind::Become(c) => call_sinks(c).collect(),
        StmtKind::With { handler, .. } => name(handler).into_iter().collect(),
        StmtKind::Drop(n) => vec![n],
        StmtKind::Reuse { name, .. } => vec![name],
        _ => Vec::new(),
    }
}

/// Names a pattern binds. They view the scrutinee: moving one moves the scrutinee.
pub fn pat_binders(p: &Pat, out: &mut Vec<Name>) {
    match p {
        Pat::Bind(n) => out.push(n.clone()),
        Pat::Ctor {
            fields: Some((fs, _)),
            ..
        } => fs.iter().for_each(|(_, p)| pat_binders(p, out)),
        Pat::Tuple(ps) | Pat::Or(ps) => ps.iter().for_each(|p| pat_binders(p, out)),
        Pat::Slice { elems, rest } => {
            elems.iter().for_each(|p| pat_binders(p, out));
            out.extend(rest.iter().flatten().cloned());
        }
        _ => {}
    }
}

/// Constructor and field name pairs whose type is copy in every declaration of that constructor.
pub type CopyFields = HashSet<(Name, Name)>;

pub fn copy_fields(m: &Module) -> CopyFields {
    let last = |p: &Path| p.last().cloned().unwrap_or_default();
    let mut copy = CopyFields::new();
    let mut other = CopyFields::new();
    let mut add = |ctor: Name, fs: &[Field]| {
        for f in fs {
            let key = (ctor.clone(), f.name.clone());
            if is_copy(&f.ty) {
                copy.insert(key);
            } else {
                other.insert(key);
            }
        }
    };
    for it in &m.items {
        match &it.kind {
            ItemKind::Record(r) => add(last(&r.path), &r.fields),
            ItemKind::Sum(s) => s
                .variants
                .iter()
                .for_each(|v| add(v.name.clone(), v.fields.as_deref().unwrap_or(&[]))),
            _ => {}
        }
    }
    copy.retain(|k| !other.contains(k));
    copy
}

/// `pat_binders` without the names bound to a copy field: those are values, not views of the
/// scrutinee, so passing one on does not move it.
pub fn view_binders(p: &Pat, copy: &CopyFields, out: &mut Vec<Name>) {
    match p {
        Pat::Ctor {
            path,
            fields: Some((fs, _)),
        } => {
            let ctor = path.last().cloned().unwrap_or_default();
            for (f, sub) in fs {
                if matches!(sub, Pat::Bind(_)) && copy.contains(&(ctor.clone(), f.clone())) {
                    continue;
                }
                view_binders(sub, copy, out);
            }
        }
        Pat::Tuple(ps) | Pat::Or(ps) => ps.iter().for_each(|p| view_binders(p, copy, out)),
        _ => pat_binders(p, out),
    }
}
