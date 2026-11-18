//! Ownership facts shared by drop elaboration (`yan-opt`) and the post-optimization check (D2, D3).

use std::collections::{HashMap, HashSet};

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
        Rhs::Rest { root, .. } => vec![root],
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

/// What a name bound by a projection or a pattern stands for: a field, at `path`, of the owned
/// local `root`. An empty `path` is the whole of `root`.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub root: Name,
    pub path: Vec<Name>,
}

pub type Views = HashMap<Name, View>;

/// The view `let x = p` binds. An element of a list is not a field: the whole root stands for
/// it, and moving it is E413.
pub fn proj_view(views: &Views, p: &Place) -> View {
    let mut v = views.get(&p.root).cloned().unwrap_or_else(|| View {
        root: p.root.clone(),
        path: Vec::new(),
    });
    for pr in &p.projs {
        match pr {
            Proj::Field(f, _) => v.path.push(f.clone()),
            Proj::Index(_) => {
                v.path.clear();
                break;
            }
        }
    }
    v
}

/// The names `k` consumes, split by what they stand for: owned locals consumed whole, and
/// fields moved out of owned locals, which leave the rest of their root to its owner.
#[derive(Default)]
pub struct Consumption {
    pub whole: Vec<Name>,
    pub partial: Vec<(Name, Vec<Name>)>,
}

/// `move p.f`: a part moved out of the local `p`, which leaves the rest of `p` to its owner.
pub fn part_move(k: &StmtKind) -> Option<&Place> {
    match k {
        StmtKind::Let {
            rhs: Rhs::Move(p), ..
        }
        | StmtKind::Assign {
            rhs: Rhs::Move(p), ..
        }
        | StmtKind::Rhs(Rhs::Move(p))
            if !p.projs.is_empty() =>
        {
            Some(p)
        }
        _ => None,
    }
}

pub fn consumption(k: &StmtKind, views: &Views) -> Consumption {
    let mut c = Consumption::default();
    for n in consumed(k) {
        match views.get(n) {
            Some(v) if !v.path.is_empty() => {
                let moved = (v.root.clone(), v.path.clone());
                if !c.partial.contains(&moved) {
                    c.partial.push(moved);
                }
            }
            Some(v) => c.whole.push(v.root.clone()),
            None => c.whole.push(n.clone()),
        }
    }
    c
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
    /// Constructors of sums, `Ok`, `Err` and `Some` among them.
    variants: HashSet<Name>,
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
    let mut variants: HashSet<Name> = ["Ok", "Err", "Some", "None"].map(Name::from).into();
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
                    variants.insert(v.name.clone());
                    add(v.name.clone(), v.fields.as_deref().unwrap_or(&[]));
                }
            }
            _ => {}
        }
    }
    copy.retain(|k| !other.contains(k));
    Ctors {
        copy,
        units,
        variants,
    }
}

/// The names a pattern binds that view the scrutinee, each with the path of the field it binds:
/// not the names bound to a copy field, which are values, so passing one on does not move the
/// scrutinee. A path through a sum starts with the variant. Names bound under an alternative or
/// a slice stand for the whole scrutinee.
pub fn view_paths(p: &Pat, ctors: &Ctors, at: &mut Vec<Name>, out: &mut Vec<(Name, Vec<Name>)>) {
    match p {
        Pat::Bind(n) if ctors.units.contains(n) => {}
        Pat::Bind(n) => out.push((n.clone(), at.clone())),
        Pat::Ctor {
            path,
            fields: Some((fs, _)),
        } => {
            let ctor = path.last().cloned().unwrap_or_default();
            for (f, sub) in fs {
                if matches!(sub, Pat::Bind(_)) && ctors.is_copy_field(&ctor, f) {
                    continue;
                }
                let depth = at.len();
                if ctors.variants.contains(&ctor) {
                    at.push(ctor.clone());
                }
                at.push(f.clone());
                view_paths(sub, ctors, at, out);
                at.truncate(depth);
            }
        }
        Pat::Tuple(ps) => {
            for (i, sub) in ps.iter().enumerate() {
                at.push(format!("_{i}"));
                view_paths(sub, ctors, at, out);
                at.pop();
            }
        }
        _ => {
            let mut names = Vec::new();
            pat_binders(p, &mut names);
            out.extend(names.into_iter().map(|n| (n, Vec::new())));
        }
    }
}

/// The views an arm's pattern binds when the scrutinee is `base`.
pub fn pat_views(p: &Pat, ctors: &Ctors, base: &View) -> Vec<(Name, View)> {
    let mut out = Vec::new();
    view_paths(p, ctors, &mut Vec::new(), &mut out);
    out.into_iter()
        .map(|(n, path)| {
            let mut v = base.clone();
            v.path.extend(path);
            (n, v)
        })
        .collect()
}

/// What the scrutinee `x` of a `match` stands for.
pub fn scrutinee_view(views: &Views, x: &Name) -> View {
    views.get(x).cloned().unwrap_or_else(|| View {
        root: x.clone(),
        path: Vec::new(),
    })
}

/// Types whose drop can be seen: custom `Drop`, `@linear`, extern handles, and anything holding one.
pub struct Observable(HashSet<String>, HashSet<String>, HashSet<String>);

impl Observable {
    pub fn new(m: &Module) -> Self {
        let last = |p: &Path| p.last().cloned().unwrap_or_default();
        let mut set = HashSet::new();
        let mut custom = HashSet::new();
        for it in &m.items {
            if it.attrs.iter().any(|a| a.name == "linear") {
                match &it.kind {
                    ItemKind::Record(r) => drop(set.insert(last(&r.path))),
                    ItemKind::Sum(s) => drop(set.insert(last(&s.path))),
                    _ => {}
                }
            }
            match &it.kind {
                ItemKind::Impl(i) if i.trait_.path.last().is_some_and(|n| n == "Drop") => {
                    if let Type::Named(n) = &i.for_ty {
                        set.insert(last(&n.path));
                        custom.insert(last(&n.path));
                    }
                }
                ItemKind::Extern(x) => {
                    for xi in &x.items {
                        if let XItem::Type { name, .. } = xi {
                            set.insert(name.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        // Fields may not be moved out of these themselves (4.2), though they may out of a holder.
        let own_drop = set.clone();
        loop {
            let before = set.len();
            for it in &m.items {
                let (name, tys): (String, Vec<&Type>) = match &it.kind {
                    ItemKind::Record(r) => {
                        (last(&r.path), r.fields.iter().map(|f| &f.ty).collect())
                    }
                    ItemKind::Sum(s) => (
                        last(&s.path),
                        s.variants
                            .iter()
                            .flat_map(|v| v.fields.iter().flatten().map(|f| &f.ty))
                            .collect(),
                    ),
                    ItemKind::Newtype(n) => (last(&n.path), vec![&n.ty]),
                    _ => continue,
                };
                if tys.iter().any(|t| mentions(t, &set)) {
                    set.insert(name);
                }
            }
            if set.len() == before {
                return Observable(set, own_drop, custom);
            }
        }
    }

    pub fn of(&self, t: &Type) -> bool {
        mentions(t, &self.0)
    }

    /// `t` itself, not something it holds, has a custom `Drop`, is `@linear` or is an extern handle.
    pub fn is_drop(&self, t: &Type) -> bool {
        named_in(t, &self.1)
    }

    /// `t` itself has a custom `Drop`.
    pub fn is_custom_drop(&self, t: &Type) -> bool {
        named_in(t, &self.2)
    }

    /// A type with a custom `Drop`: dropping part of it would run the `Drop` on a partial value.
    pub fn has_custom_drop(&self, t: &Type) -> bool {
        mentions(t, &self.2)
    }

    /// A type with a custom `Drop` or `@linear`, or an extern handle.
    pub fn has_drop(&self, t: &Type) -> bool {
        mentions(t, &self.1)
    }
}

fn named_in(t: &Type, set: &HashSet<String>) -> bool {
    match t {
        Type::Named(n) => n.path.last().is_some_and(|b| set.contains(b)),
        Type::Refined { ty, .. } => named_in(ty, set),
        _ => false,
    }
}

fn mentions(t: &Type, set: &HashSet<String>) -> bool {
    match t {
        Type::Named(n) => {
            n.path.last().is_some_and(|b| set.contains(b))
                || n.args
                    .iter()
                    .any(|a| matches!(a, TArg::Type(t) if mentions(t, set)))
        }
        Type::Tuple(ts) | Type::Union(ts) => ts.iter().any(|t| mentions(t, set)),
        Type::Refined { ty, .. } => mentions(ty, set),
        Type::Dyn(_) => true,
        Type::Fn(_) => false,
    }
}
