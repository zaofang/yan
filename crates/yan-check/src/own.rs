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

/// What the module's constructors say about patterns and moves.
#[derive(Clone, Default)]
pub struct Ctors {
    /// Constructor and field name pairs whose type is copy in every declaration of that constructor.
    copy: HashSet<(Name, Name)>,
    /// Variants without fields: a pattern `Leaf` is written as a name, but binds nothing.
    units: HashSet<Name>,
}

impl Ctors {
    pub fn is_copy_field(&self, ctor: &str, field: &str) -> bool {
        self.copy.contains(&(ctor.to_owned(), field.to_owned()))
    }
}

pub fn ctors(items: &[Item]) -> Ctors {
    let last = |p: &Path| p.last().cloned().unwrap_or_default();
    let mut copy = HashSet::new();
    let mut other = HashSet::new();
    let mut units: HashSet<Name> = HashSet::from(["None".to_owned()]);
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
    for it in items {
        match &it.kind {
            ItemKind::Record(r) => add(last(&r.path), &r.fields),
            ItemKind::Sum(s) => {
                for v in &s.variants {
                    if v.fields.as_deref().is_none_or(<[Field]>::is_empty) {
                        units.insert(v.name.clone());
                    }
                    add(v.name.clone(), v.fields.as_deref().unwrap_or(&[]));
                }
            }
            _ => {}
        }
    }
    copy.retain(|k| !other.contains(k));
    Ctors { copy, units }
}

/// `pat_binders` without the names bound to a copy field: those are values, not views of the
/// scrutinee, so passing one on does not move it.
pub fn view_binders(p: &Pat, ctors: &Ctors, out: &mut Vec<Name>) {
    match p {
        Pat::Bind(n) if ctors.units.contains(n) => {}
        Pat::Ctor {
            path,
            fields: Some((fs, _)),
        } => {
            let ctor = path.last().cloned().unwrap_or_default();
            for (f, sub) in fs {
                if matches!(sub, Pat::Bind(_)) && ctors.is_copy_field(&ctor, f) {
                    continue;
                }
                view_binders(sub, ctors, out);
            }
        }
        Pat::Tuple(ps) | Pat::Or(ps) => ps.iter().for_each(|p| view_binders(p, ctors, out)),
        _ => pat_binders(p, out),
    }
}
