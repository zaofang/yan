//! `yan semver` and the version check of `yan publish` (20.6): classify the change between two
//! versions of a package's `pub` API by the evolution table, and compare it with the declared bump.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yan_diag::{Diagnostic, Severity, Span};
use yan_hir::{Contract, Decorator, FuncDef, Item, ItemKind, Param, TypeRhs, Uses};
use yan_yag::{print_expr, print_type};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    None,
    Patch,
    Minor,
    Major,
}

impl Bump {
    pub fn name(self) -> &'static str {
        match self {
            Bump::None => "none",
            Bump::Patch => "patch",
            Bump::Minor => "minor",
            Bump::Major => "major",
        }
    }
}

pub struct Report {
    pub bump: Bump,
    /// Type-level breaks, one per item that forces the bump.
    pub breaks: Vec<(String, Bump, String)>,
    /// Contract changes that cannot be ordered (W014).
    pub possible: Vec<(String, String)>,
}

type Api = BTreeMap<String, Item>;

fn sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = if root.join("package.yag").is_file() {
        root.join("src")
    } else {
        root.to_path_buf()
    };
    let mut out = Vec::new();
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "yag") && !p.ends_with("package.yag") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The `pub` items of a package by `module::name`.
fn api(root: &Path) -> Result<Api, String> {
    let base = if root.join("package.yag").is_file() {
        root.join("src")
    } else {
        root.to_path_buf()
    };
    let mut out = Api::new();
    for path in sources(root)? {
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (m, diags) = yan_yag::parse(&path.display().to_string(), &src, false);
        let m = m.ok_or_else(|| {
            let first = diags.first().map(|d| d.message.clone()).unwrap_or_default();
            format!("{}: does not parse: {first}", path.display())
        })?;
        let module = path
            .strip_prefix(&base)
            .unwrap_or(&path)
            .with_extension("")
            .display()
            .to_string();
        for item in m.items {
            if item.is_pub && !matches!(item.kind, ItemKind::Impl(_) | ItemKind::Verbatim(_)) {
                out.insert(format!("{module}::{}", item.kind.name()), item);
            }
        }
    }
    Ok(out)
}

fn has(decorators: &[Decorator], name: &str) -> bool {
    decorators.iter().any(|d| d.path == [name])
}

fn subset<T: Ord>(a: &[T], b: &[T]) -> bool {
    a.iter().all(|x| b.contains(x))
}

/// `name OP int`, the one contract shape whose strength is decidable without a solver.
fn bound(e: &str) -> Option<(String, bool, i128)> {
    let (name, op, n) = {
        let mut it = e.split_whitespace();
        (it.next()?, it.next()?, it.next()?)
    };
    let n: i128 = n.parse().ok()?;
    match op {
        ">=" => Some((name.into(), true, n)),
        ">" => Some((name.into(), true, n + 1)),
        "<=" => Some((name.into(), false, n)),
        "<" => Some((name.into(), false, n - 1)),
        _ => None,
    }
}

/// Whether `new` demands more than `old`: `Some(true)` stronger, `Some(false)` weaker or equal,
/// `None` when the two cannot be ordered.
fn stronger(old: &[String], new: &[String]) -> Option<bool> {
    if old == new {
        return Some(false);
    }
    let (o, n): (Vec<_>, Vec<_>) = (
        old.iter().map(|e| bound(e)).collect(),
        new.iter().map(|e| bound(e)).collect(),
    );
    if o.len() != 1 || n.len() != 1 {
        return None;
    }
    let ((on, olow, ov), (nn, nlow, nv)) = (o[0].clone()?, n[0].clone()?);
    if on != nn || olow != nlow {
        return None;
    }
    Some(if olow { nv > ov } else { nv < ov })
}

struct Contracts {
    pre: Vec<String>,
    post: Vec<String>,
    ex: Vec<String>,
}

fn contracts(f: &FuncDef) -> Contracts {
    let mut c = Contracts {
        pre: Vec::new(),
        post: Vec::new(),
        ex: Vec::new(),
    };
    for k in f.body.iter().flat_map(|b| &b.contracts) {
        match k {
            Contract::Pre(e) => c.pre.push(print_expr(e, 0)),
            Contract::Post(e) => c.post.push(print_expr(e, 0)),
            Contract::Ex { expr, .. } => c.ex.push(print_expr(expr, 0)),
            _ => {}
        }
    }
    c
}

fn params(f: &FuncDef) -> Vec<(String, bool)> {
    f.params
        .iter()
        .map(|p| match p {
            Param::Value {
                conv,
                name,
                ty,
                default,
                ..
            } => (
                format!("{conv:?} {name}: {}", print_type(ty)),
                default.is_some(),
            ),
            Param::Slash => ("/".into(), false),
            Param::Pack { name, ty } => (format!("*{name}: {}", print_type(ty)), false),
        })
        .collect()
}

fn ret(f: &FuncDef) -> (String, Vec<String>, Vec<String>) {
    let Some(r) = &f.ret else {
        return (String::new(), Vec::new(), Vec::new());
    };
    (
        r.ty.as_ref().map(print_type).unwrap_or_default(),
        r.fail.iter().map(|p| p.join(".")).collect(),
        r.from.clone(),
    )
}

fn caps(f: &FuncDef) -> Vec<String> {
    match &f.uses {
        Some(Uses::Caps(cs)) => cs.iter().map(|c| c.path.join(".")).collect(),
        _ => Vec::new(),
    }
}

fn func(old: &FuncDef, new: &FuncDef, possible: &mut Vec<String>) -> (Bump, String) {
    let (op, np) = (params(old), params(new));
    let extended = np.len() > op.len()
        && np[..op.len()] == op[..]
        && np[op.len()..].iter().all(|(_, default)| *default);
    if op != np && !extended {
        return (Bump::Major, "parameters changed".into());
    }
    let (or, nr) = (ret(old), ret(new));
    if or.0 != nr.0 || or.2 != nr.2 {
        return (Bump::Major, "return type changed".into());
    }
    let mut bump = if op != np { Bump::Minor } else { Bump::None };
    let mut why = if op != np {
        "parameter with default added".to_owned()
    } else {
        String::new()
    };
    let mut raise = |b: Bump, w: &str| {
        if b > bump {
            bump = b;
            why = w.to_owned();
        }
    };
    if !subset(&nr.1, &or.1) {
        raise(Bump::Major, "`fail` grew");
    } else if nr.1.len() < or.1.len() {
        raise(Bump::Minor, "`fail` shrank");
    }
    let (oc, nc) = (caps(old), caps(new));
    if !subset(&nc, &oc) {
        raise(Bump::Major, "capability added to `uses`");
    } else if nc.len() < oc.len() {
        raise(Bump::Minor, "capability removed from `uses`");
    }
    let (ok, nk) = (contracts(old), contracts(new));
    match stronger(&ok.pre, &nk.pre) {
        Some(true) => raise(Bump::Major, "`pre` strengthened"),
        Some(false) if ok.pre != nk.pre => raise(Bump::Minor, "`pre` weakened"),
        None => possible.push("`pre` changed".into()),
        Some(false) => {}
    }
    // `post` is the dual: a stronger post only helps callers.
    match stronger(&ok.post, &nk.post) {
        Some(true) => raise(Bump::Minor, "`post` strengthened"),
        Some(false) if ok.post != nk.post => raise(Bump::Major, "`post` weakened"),
        None => possible.push("`post` changed".into()),
        Some(false) => {}
    }
    if !subset(&ok.ex, &nk.ex) {
        raise(Bump::Major, "`ex` result changed");
    } else if nk.ex.len() > ok.ex.len() {
        raise(Bump::Minor, "`ex` added");
    }
    (bump, why)
}

/// Fields or variants as `name: type`, with whether a field has a default.
fn fields(fs: &[yan_hir::Field]) -> Vec<(String, bool)> {
    fs.iter()
        .map(|f| {
            (
                format!("{}: {}", f.name, print_type(&f.ty)),
                f.default.is_some(),
            )
        })
        .collect()
}

/// A growth of `old` into `new` that keeps every old entry: the added entries.
fn grown<'a>(old: &[(String, bool)], new: &'a [(String, bool)]) -> Option<&'a [(String, bool)]> {
    (new.len() >= old.len() && new[..old.len()] == old[..]).then(|| &new[old.len()..])
}

fn item(old: &Item, new: &Item, possible: &mut Vec<String>) -> (Bump, String) {
    let exhaustive = !has(&new.decorators, "non_exhaustive");
    let grow = if exhaustive { Bump::Major } else { Bump::Minor };
    let (bump, why) = match (&old.kind, &new.kind) {
        (ItemKind::Func(a), ItemKind::Func(b)) => func(a, b, possible),
        (ItemKind::Record(a), ItemKind::Record(b)) => {
            match grown(&fields(&a.fields), &fields(&b.fields)) {
                Some([]) => (Bump::None, String::new()),
                Some(_) => (grow, "record field added".into()),
                None => (Bump::Major, "record fields changed".into()),
            }
        }
        (ItemKind::Type(a), ItemKind::Type(b)) => match (&a.rhs, &b.rhs) {
            (TypeRhs::Sum(x), TypeRhs::Sum(y)) => {
                let flat = |vs: &[yan_hir::Variant]| -> Vec<(String, bool)> {
                    vs.iter()
                        .map(|v| {
                            let fs: Vec<_> = fields(&v.fields).into_iter().map(|f| f.0).collect();
                            (format!("{}({})", v.name, fs.join(", ")), false)
                        })
                        .collect()
                };
                match grown(&flat(x), &flat(y)) {
                    Some([]) => (Bump::None, String::new()),
                    Some(_) => (grow, "sum variant added".into()),
                    None => (Bump::Major, "sum variants changed".into()),
                }
            }
            (x, y) if format!("{x:?}") == format!("{y:?}") => (Bump::None, String::new()),
            _ => (Bump::Major, "type definition changed".into()),
        },
        (ItemKind::Trait(a), ItemKind::Trait(b)) => {
            let sig = |t: &yan_hir::Trait| -> BTreeMap<String, (String, bool)> {
                t.sigs
                    .iter()
                    .map(|(_, f)| {
                        let p: Vec<_> = params(f).into_iter().map(|p| p.0).collect();
                        let (r, ..) = ret(f);
                        (f.name.clone(), (format!("{p:?} -> {r}"), f.body.is_some()))
                    })
                    .collect()
            };
            let (os, ns) = (sig(a), sig(b));
            let mut bump = Bump::None;
            let mut why = String::new();
            for (name, (s, default)) in &ns {
                let (b, w) = match os.get(name) {
                    None if *default => (Bump::Minor, "trait method with default added"),
                    None => (Bump::Major, "trait method without default added"),
                    Some((o, _)) if o != s => (Bump::Major, "trait method signature changed"),
                    Some(_) => continue,
                };
                if b > bump {
                    (bump, why) = (b, format!("{w}: `{name}`"));
                }
            }
            if os.keys().any(|k| !ns.contains_key(k)) {
                (bump, why) = (Bump::Major, "trait method removed".into());
            }
            (bump, why)
        }
        (ItemKind::Const { ty: a, .. }, ItemKind::Const { ty: b, .. }) => {
            if print_type(a) == print_type(b) {
                (Bump::None, String::new())
            } else {
                (Bump::Major, "constant type changed".into())
            }
        }
        _ => (Bump::Major, "kind of item changed".into()),
    };
    if bump == Bump::None
        && !has(&old.decorators, "deprecated")
        && has(&new.decorators, "deprecated")
    {
        return (Bump::Minor, "item deprecated".into());
    }
    (bump, why)
}

/// Compares the packages at `base` and `new`; each is a package directory or a bare directory of
/// modules.
pub fn compare(base: &Path, new: &Path) -> Result<Report, String> {
    let (old, cur) = (api(base)?, api(new)?);
    let mut report = Report {
        bump: Bump::None,
        breaks: Vec::new(),
        possible: Vec::new(),
    };
    let mut note = |name: &str, bump: Bump, why: String| {
        if bump >= Bump::Major {
            report.breaks.push((name.to_owned(), bump, why));
        }
        report.bump = report.bump.max(bump);
    };
    for (name, o) in &old {
        match cur.get(name) {
            None => note(name, Bump::Major, "item removed or renamed".into()),
            Some(n) => {
                let mut possible = Vec::new();
                let (bump, why) = item(o, n, &mut possible);
                report
                    .possible
                    .extend(possible.into_iter().map(|p| (name.clone(), p)));
                note(name, bump, why);
            }
        }
    }
    for name in cur.keys().filter(|k| !old.contains_key(*k)) {
        note(name, Bump::Minor, "item added".into());
    }
    // Public items are the only ones compared; a private-only change is still a patch.
    if report.bump == Bump::None && sources_differ(base, new)? {
        report.bump = Bump::Patch;
    }
    Ok(report)
}

fn sources_differ(a: &Path, b: &Path) -> Result<bool, String> {
    let read = |root: &Path| -> Result<Vec<String>, String> {
        sources(root)?
            .iter()
            .map(|p| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display())))
            .collect()
    };
    Ok(read(a)? != read(b)?)
}

fn version(root: &Path) -> Result<String, String> {
    let path = root.join("package.yag");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (m, _) = yan_yag::parse("package.yag", &src, false);
    m.and_then(|m| yan_elab::manifest(&m))
        .and_then(|m| m.version)
        .ok_or_else(|| format!("{}: no `version`", path.display()))
}

fn parts(v: &str) -> Option<[u64; 3]> {
    let mut it = v.split(['-', '+']).next()?.split('.');
    let mut next = || it.next().and_then(|p| p.parse().ok());
    Some([next()?, next()?, next()?])
}

/// The bump from `base` to `new`; in 0.y.z, y is the major (19.1).
pub fn declared(base: &str, new: &str) -> Result<Bump, String> {
    let (b, n) = parts(base)
        .zip(parts(new))
        .ok_or_else(|| format!("`{base}` or `{new}` is not a version"))?;
    let lead = usize::from(b[0] == 0 && n[0] == 0);
    Ok(match (0..3).find(|&i| b[i] != n[i]) {
        Some(i) if i <= lead => Bump::Major,
        Some(i) if i == lead + 1 => Bump::Minor,
        Some(_) => Bump::Patch,
        None => Bump::None,
    })
}

/// The diagnostics of a package at `root` against the published `base`: `code` for a declared
/// bump below the minimum (E013 for `yan semver`, E008 for `yan publish`), W014 for contract
/// changes that cannot be ordered.
pub fn findings(root: &Path, base: &Path, code: &str) -> Result<Vec<Diagnostic>, String> {
    let report = compare(base, root)?;
    let have = declared(&version(base)?, &version(root)?)?;
    let mut out = Vec::new();
    if have < report.bump {
        let detail = report
            .breaks
            .iter()
            .map(|(item, _, why)| format!("{item}: {why}"))
            .collect::<Vec<_>>()
            .join("; ");
        out.push(Diagnostic::error(
            code,
            "package.yag",
            Span::default(),
            "",
            format!(
                "declared {} bump is below the minimum {} {detail}",
                have.name(),
                report.bump.name()
            ),
        ));
    }
    for (item, what) in report.possible {
        let mut d = Diagnostic::error("W014", "package.yag", Span::default(), "", what);
        d.severity = Severity::Warning;
        out.push(d.with_item(item));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_bump_by_version() {
        assert_eq!(declared("1.2.0", "2.0.0"), Ok(Bump::Major));
        assert_eq!(declared("1.2.0", "1.3.0"), Ok(Bump::Minor));
        assert_eq!(declared("1.2.0", "1.2.1"), Ok(Bump::Patch));
        assert_eq!(declared("0.4.0", "0.5.0"), Ok(Bump::Major));
        assert_eq!(declared("0.4.0", "0.4.1"), Ok(Bump::Minor));
    }

    #[test]
    fn bounds_order_contracts() {
        let one = |s: &str| vec![s.to_owned()];
        assert_eq!(stronger(&one("x >= 0"), &one("x >= 1")), Some(true));
        assert_eq!(stronger(&one("x >= 1"), &one("x >= 0")), Some(false));
        assert_eq!(stronger(&one("x >= 1"), &one("y != 0")), None);
    }
}
