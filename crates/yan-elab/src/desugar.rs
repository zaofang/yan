//! Module rewrites before elaboration: newtypes become one-field records and capability sets
//! expand in place (14.1, 7.1).

use std::collections::HashMap;

use yan_hir as j;

use yan_diag::Span;

use crate::Cx;

/// Marks the record a newtype became, so reflection still reports a newtype (`info`).
pub(crate) fn newtype_mark() -> j::Decorator {
    j::Decorator {
        path: vec!["newtype".into()],
        args: None,
    }
}

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
                    it.decorators.push(newtype_mark());
                    it.kind = j::ItemKind::Record(j::Record {
                        name: t.name.clone(),
                        tparams: Vec::new(),
                        doc: None,
                        fields: vec![j::Field {
                            decorators: Vec::new(),
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
    /// The requirements of `dependencies`, `dev_dependencies` and `tools`, as written (R642).
    pub requirements: Vec<String>,
    /// The package's own `name`.
    pub name: Option<String>,
    /// The package's own `version`.
    pub version: Option<String>,
    /// The workspace members' directories, relative to the manifest (root only, 19.1).
    pub members: Vec<String>,
    /// The requirements of `dependencies` alone, the ones dependents inherit (R636).
    pub dependencies: Vec<String>,
    /// Whether `overrides` names any package (R636).
    pub has_overrides: bool,
}

/// The fields of the manifest table (19.1).
const MANIFEST_FIELDS: &[&str] = &[
    "name",
    "version",
    "edition",
    "yan",
    "dependencies",
    "dev_dependencies",
    "tools",
    "overrides",
    "python",
    "exclude",
    "capabilities",
    "platform",
    "allow_ffi",
    "native",
    "preview",
    "members",
    "approvers",
    "trust",
    "rebuilders",
    "min_release_age",
    "license",
    "license_policy",
    "profile",
    "embedded",
    "i18n",
];

/// E018: a `Package(...)` field outside the manifest table (R891).
pub(crate) fn fields(m: &j::Module, cx: &mut Cx) {
    let args = package_args(m).unwrap_or_default();
    let manifest_file = std::path::Path::new(cx.file)
        .file_name()
        .is_some_and(|n| n == "package.yag");
    let members_only = args.iter().any(|a| a.name.as_deref() == Some("members"));
    if manifest_file && !members_only {
        let span = m
            .items
            .iter()
            .find(|it| matches!(&it.kind, j::ItemKind::Const { name, .. } if name == "package"))
            .map(|it| it.span);
        for req in ["name", "version", "edition", "yan"] {
            if let Some(span) =
                span.filter(|_| !args.iter().any(|a| a.name.as_deref() == Some(req)))
            {
                cx.err(
                    "E018",
                    span,
                    "package",
                    format!("the manifest lacks `{req}`"),
                );
            }
        }
    }
    for a in args {
        let Some(n) = &a.name else { continue };
        if !MANIFEST_FIELDS.contains(&n.as_str()) {
            cx.err(
                "E018",
                a.span,
                "package",
                format!("`{n}` is not a manifest field"),
            );
        }
    }
}

/// E144: a `package.yag` outside the data profile (17.3, R827).
pub(crate) fn profile(m: &j::Module, cx: &mut Cx) {
    let manifest_file = std::path::Path::new(cx.file)
        .file_name()
        .is_some_and(|n| n == "package.yag");
    if !manifest_file {
        return;
    }
    for (span, why) in yan_yag::profile::violations(m) {
        cx.err("E144", span, "package", why);
    }
}

/// E332: a manifest `edition` that is neither `next` nor a year of 2026 or later (20.4).
pub(crate) fn edition(m: &j::Module, cx: &mut Cx) {
    let Some(args) = package_args(m) else {
        return;
    };
    for a in args {
        let (Some(n), j::ArgKind::Expr(e)) = (&a.name, &a.kind) else {
            continue;
        };
        let j::ExprKind::Lit(j::Lit::Str { value, .. }) = &e.kind else {
            continue;
        };
        let year = value.len() == 4 && value.bytes().all(|b| b.is_ascii_digit());
        if n == "edition" && value != "next" && !(year && value.as_str() >= "2026") {
            cx.err(
                "E332",
                e.span,
                "package",
                format!("`{value}` is not `next` or an edition year of 2026 or later"),
            );
        }
    }
}

/// The manifest a module declares at top level, as `package.yag` and script-form files do.
pub fn manifest(m: &j::Module) -> Option<Manifest> {
    let args = package_args(m)?;
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
    let reqs = |name: &str| -> Vec<String> {
        let value = args.iter().find_map(|a| match (&a.name, &a.kind) {
            (Some(n), j::ArgKind::Expr(e)) if n == name => Some(&e.kind),
            _ => None,
        });
        let lit = |e: &j::Expr| match &e.kind {
            j::ExprKind::Lit(j::Lit::Str { value, .. }) => Some(value.clone()),
            _ => None,
        };
        match value {
            Some(j::ExprKind::List(xs)) => xs.iter().filter_map(lit).collect(),
            Some(j::ExprKind::Dict(kvs)) => kvs.iter().filter_map(|(_, v)| lit(v)).collect(),
            _ => Vec::new(),
        }
    };
    let string = |field: &str| {
        args.iter().find_map(|a| match (&a.name, &a.kind) {
            (
                Some(n),
                j::ArgKind::Expr(j::Expr {
                    kind: j::ExprKind::Lit(j::Lit::Str { value, .. }),
                    ..
                }),
            ) if n == field => Some(value.clone()),
            _ => None,
        })
    };
    Some(Manifest {
        name: string("name"),
        version: string("version"),
        members: strs("members").unwrap_or_default(),
        dependencies: reqs("dependencies"),
        has_overrides: !reqs("overrides").is_empty(),
        requirements: ["dependencies", "dev_dependencies", "tools"]
            .into_iter()
            .flat_map(reqs)
            .collect(),
        capabilities: strs("capabilities"),
        previews: strs("preview").unwrap_or_default(),
    })
}

/// The arguments of the `Package(...)` constant a module declares.
fn package_args(m: &j::Module) -> Option<&[j::Arg]> {
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
    Some(args)
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
