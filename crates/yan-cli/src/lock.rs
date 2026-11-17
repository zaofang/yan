//! `yan.lock` (19.1, R892 to R896): reading it, E019 for a newer format, E020 under `--locked`, and
//! `yan lock`, which writes it from the manifest and the workspace members.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yan_diag::{Diagnostic, Span};

use crate::closure;
use crate::registry::{Index, Info, Registries};
use yan_db::closure::{Key, Node};
pub(crate) use yan_db::closure::{identity, requirement};

/// The newest lock `format` this toolchain reads.
const FORMAT: i64 = 1;

/// The identity a requirement `"@org/name x.y[.z]"` pins: the name and its major (the minor in 0.y.z).
pub(crate) fn fail(code: &str, message: String) -> Box<Diagnostic> {
    Box::new(Diagnostic::error(
        code,
        "yan.lock",
        Span::default(),
        "",
        message,
    ))
}

/// Check `yan.lock` of the package at `root` against its manifest requirements. A missing lock
/// is an error only under `locked`; a lock is read, never written.
pub fn check(
    root: &Path,
    requirements: &[String],
    locked: bool,
    offline: bool,
) -> Result<(), Box<Diagnostic>> {
    let path = root.join("yan.lock");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return if locked && !requirements.is_empty() {
            Err(fail("E020", "`yan.lock` is missing".into()))
        } else {
            Ok(())
        };
    };
    let table: toml::Table = text
        .parse()
        .map_err(|e| fail("E020", format!("`yan.lock` does not parse: {e}")))?;
    let format = table.get("format").and_then(toml::Value::as_integer);
    match format {
        Some(f) if f > FORMAT => {
            return Err(fail(
                "E019",
                format!(
                    "`yan.lock` has format {f}; yan {} reads up to {FORMAT}",
                    env!("CARGO_PKG_VERSION")
                ),
            ));
        }
        Some(_) => {}
        None => return Err(fail("E020", "`yan.lock` has no integer `format`".into())),
    }
    if let Some(want) = table.get("pylock").and_then(toml::Value::as_str) {
        let have = std::fs::read(root.join("pylock.toml"))
            .map(|b| format!("sha256:{}", crate::store::sha256(&b)));
        if have.as_deref().ok() != Some(want) {
            return Err(fail(
                "E020",
                format!("`pylock.toml` does not hash to the locked `{want}`"),
            ));
        }
    }
    if let Some(top) = manifest_at(root) {
        for p in table
            .get("package")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let ffi: Vec<String> = p
                .get("ffi")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect();
            let libs: Vec<String> = recorded_set(p)
                .libs
                .into_iter()
                .map(|l| format!("lib {}", l.name))
                .collect();
            consent(
                &top,
                p.get("name").and_then(toml::Value::as_str).unwrap_or(""),
                ffi.first().or(libs.first()),
            )?;
        }
    }
    if !locked {
        return Ok(());
    }
    let records: Vec<&toml::Value> = table
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let pinned: Vec<(String, String)> = records
        .iter()
        .filter_map(|p| {
            Some(identity(
                p.get("name")?.as_str()?,
                p.get("version")?.as_str()?,
            ))
        })
        .collect();
    for r in requirements {
        match requirement(r) {
            Some(id) if pinned.contains(&id) => {}
            _ => {
                return Err(fail("E020", format!("`yan.lock` does not pin `{r}`")));
            }
        }
    }
    let store = crate::registry::configured_store(root);
    let roots = crate::store::roots(root, store.as_deref());
    for p in &records {
        let Some(digest) = p.get("digest").and_then(toml::Value::as_str) else {
            continue;
        };
        match crate::store::source(&roots, digest)? {
            Some(dir) => {
                verify_set(p, &dir)?;
                verify_identity(p, &dir)?;
            }
            None if offline => {
                return Err(fail(
                    "E020",
                    format!(
                        "offline: no archive for `{digest}` in the workspace store, `vendor` or the user store"
                    ),
                ));
            }
            None => {}
        }
    }
    let nodes: Vec<Node> = crate::graph::packages(root)
        .iter()
        .map(closure::from_package)
        .collect();
    closure::check(&nodes)
}

/// The foreign set a `[[package]]` record states (R1034).
fn recorded_set(p: &toml::Value) -> yan_db::ForeignSet {
    let mut modules: Vec<String> = p
        .get("ffi")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_owned))
        .collect();
    let mut libs: Vec<yan_db::ForeignLib> = p
        .get("libs")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| {
            Some(yan_db::ForeignLib {
                name: x.get("name")?.as_str()?.to_owned(),
                link: x.get("link")?.as_str()?.to_owned(),
            })
        })
        .collect();
    modules.sort();
    libs.sort();
    yan_db::ForeignSet { modules, libs }
}

/// E020 when the set recomputed from the verified archive differs from the record's (R1034).
fn verify_set(record: &toml::Value, dir: &Path) -> Result<(), Box<Diagnostic>> {
    if recorded_set(record) == yan_db::foreign_set(dir) {
        return Ok(());
    }
    let name = record
        .get("name")
        .and_then(toml::Value::as_str)
        .unwrap_or("");
    Err(fail(
        "E020",
        format!("`yan.lock` records a foreign set for `{name}` that its archive does not have"),
    ))
}

/// E020 when the archive's own manifest names another package or version than the record: the
/// closure check keys on the record, so the two must be one fact.
fn verify_identity(record: &toml::Value, dir: &Path) -> Result<(), Box<Diagnostic>> {
    let Some(m) = manifest_at(dir) else {
        return Ok(());
    };
    let field = |k: &str| record.get(k).and_then(toml::Value::as_str);
    let differs = |have: Option<&str>, want: Option<&str>| have.is_some() && have != want;
    if differs(m.name.as_deref(), field("name")) || differs(m.version.as_deref(), field("version"))
    {
        return Err(fail(
            "E020",
            format!(
                "`yan.lock` pins `{} {}`, but its archive is `{} {}`",
                field("name").unwrap_or(""),
                field("version").unwrap_or(""),
                m.name.as_deref().unwrap_or(""),
                m.version.as_deref().unwrap_or("")
            ),
        ));
    }
    Ok(())
}

/// The `[[package]]` records of the existing `yan.lock`, if it reads.
pub(crate) fn records(root: &Path) -> Vec<toml::Value> {
    std::fs::read_to_string(root.join("yan.lock"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| match t.get("package") {
            Some(toml::Value::Array(a)) => Some(a.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Whether `yan.lock` has a record for `name`, at any version.
pub fn is_locked(root: &Path, name: &str) -> bool {
    records(root)
        .iter()
        .any(|p| p.get("name").and_then(toml::Value::as_str) == Some(name))
}

/// The version `yan.lock` records for the package a requirement names.
pub fn locked_version(root: &Path, name: &str, version: &str) -> Option<String> {
    let key = identity(name, version);
    records(root).iter().find_map(|p| {
        let v = p.get("version")?.as_str()?;
        (identity(p.get("name")?.as_str()?, v) == key).then(|| v.to_owned())
    })
}

/// The publisher identity `yan.lock` records for the package a requirement names.
pub fn locked_publisher(root: &Path, name: &str, version: &str) -> Option<String> {
    let key = identity(name, version);
    records(root).iter().find_map(|p| {
        let id = identity(p.get("name")?.as_str()?, p.get("version")?.as_str()?);
        (id == key).then(|| p.get("publisher")?.as_str().map(str::to_owned))?
    })
}

pub(crate) fn manifest_at(dir: &Path) -> Option<yan_elab::Manifest> {
    let src = std::fs::read_to_string(dir.join("package.yag")).ok()?;
    let (m, _) = yan_yag::parse("package.yag", &src, false);
    yan_elab::manifest(&m?)
}

/// Numeric dotted components, so that `1.10` orders after `1.9`.
pub(crate) fn parts(version: &str) -> Vec<u64> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

struct Locked {
    name: String,
    version: String,
    source: String,
    key: (String, String),
    digest: Option<String>,
    capability_digest: Option<String>,
    /// The `ffi` modules and extern libraries the package holds (R1018).
    ffi: Vec<String>,
    libs: Vec<yan_db::ForeignLib>,
    publisher: Option<String>,
    approvals: Vec<String>,
    rebuilds: Vec<String>,
    /// The directory of an override, relative to the root (R950).
    path: Option<String>,
    /// The subgraphs that use the package: `dep`, `dev`, `tool` (R951).
    kinds: Vec<&'static str>,
}

const MAX_ROUNDS: usize = 64;

#[derive(Clone)]
/// A registry requirement's accumulated minimum (R894): the largest minimum any requirer wrote.
struct Min {
    name: String,
    want: String,
    requirers: Vec<String>,
}

fn raise(mins: &mut BTreeMap<Key, Min>, requirer: &str, req: &str) -> Result<(), Box<Diagnostic>> {
    let Some((name, want)) = req.split_once(' ') else {
        return Err(fail("E018", format!("`{req}` is not `@org/name x.y[.z]`")));
    };
    let want = want.trim();
    let min = mins.entry(identity(name, want)).or_insert_with(|| Min {
        name: name.to_owned(),
        want: want.to_owned(),
        requirers: Vec::new(),
    });
    if parts(want) > parts(&min.want) {
        min.want = want.to_owned();
    }
    if !min.requirers.iter().any(|r| r == requirer) {
        min.requirers.push(requirer.to_owned());
    }
    Ok(())
}

/// Minimal version selection: the smallest published version at or above the minimum. A yanked
/// version is never newly selected but stays where the lock already pins it (19.3).
fn select(index: &Index, m: &Min, pinned: Option<&str>) -> Result<Info, Box<Diagnostic>> {
    let mut best: Option<Info> = None;
    for v in index.versions(&m.name)?.into_iter().flatten() {
        if !crate::registry::satisfies(&v, &m.want) {
            continue;
        }
        let Some(i) = index.info(&m.name, &v)? else {
            continue;
        };
        let older = best
            .as_ref()
            .is_none_or(|b| parts(&i.version) < parts(&b.version));
        if (!i.yanked || pinned == Some(i.version.as_str())) && older {
            best = Some(i);
        }
    }
    let chosen = best.ok_or_else(|| {
        fail(
            "E021",
            format!(
                "no published version of `{}` satisfies {} (required by {})",
                m.name,
                m.want,
                m.requirers.join(", ")
            ),
        )
    })?;
    match &chosen.malicious {
        Some(advisory) => Err(malicious(index, &m.name, &chosen, advisory)),
        None => Ok(chosen),
    }
}

/// E007 for `bad`, naming the nearest clean version above it (R947).
pub(crate) fn malicious(index: &Index, name: &str, bad: &Info, advisory: &str) -> Box<Diagnostic> {
    let clean = index
        .versions(name)
        .ok()
        .flatten()
        .into_iter()
        .flatten()
        .filter(|v| parts(v) > parts(&bad.version) && !v.contains('-'))
        .filter_map(|v| index.info(name, &v).ok().flatten())
        .filter(|i| i.malicious.is_none() && !i.yanked)
        .min_by_key(|i| parts(&i.version));
    fail(
        "E007",
        format!(
            "`{name}` {} is marked malicious: {advisory}; nearest clean version: {}",
            bad.version,
            clean.map_or("none".into(), |c| c.version)
        ),
    )
}

/// Resolve every requirement of the package at `root` and of its members: against the members
/// first (19.1: members share one lock), then against the indexes `registries` maps (R894,
/// R895). A namespace with no mapping is E021.
fn resolve(root: &Path) -> Result<Vec<Locked>, Box<Diagnostic>> {
    let top = manifest_at(root)
        .ok_or_else(|| fail("E018", "`package.yag` is missing or unreadable".into()))?;
    let mut members = Vec::new();
    for dir in &top.members {
        let m = manifest_at(&root.join(dir)).ok_or_else(|| {
            fail(
                "E018",
                format!("member `{dir}` has no readable `package.yag`"),
            )
        })?;
        members.push((dir.clone(), m));
    }
    let registries = Registries::load(root)?;
    let overrides = load_overrides(root, &top)?;
    let previous = records(root);
    let pinned = |key: &Key| {
        previous.iter().find_map(|p| {
            let (n, v) = (p.get("name")?.as_str()?, p.get("version")?.as_str()?);
            (identity(n, v) == *key).then(|| v.to_owned())
        })
    };
    let wanted = top
        .requirements
        .iter()
        .map(|r| (top.name.as_deref().unwrap_or("."), r))
        .chain(members.iter().flat_map(|(_, m)| {
            m.requirements
                .iter()
                .map(|r| (m.name.as_deref().unwrap_or("."), r))
        }));
    let mut out: Vec<Locked> = Vec::new();
    let mut mins: BTreeMap<Key, Min> = BTreeMap::new();
    for (requirer, r) in wanted {
        let Some((name, version)) = r.split_once(' ') else {
            return Err(fail("E018", format!("`{r}` is not `@org/name x.y[.z]`")));
        };
        let key = identity(name, version.trim());
        let found = members.iter().find_map(|(dir, m)| {
            let (n, v) = (m.name.as_deref()?, m.version.as_deref()?);
            (identity(n, v) == key).then(|| (dir, n.to_owned(), v.to_owned()))
        });
        let Some((dir, n, v)) = found else {
            raise(&mut mins, requirer, r)?;
            continue;
        };
        if parts(&v) < parts(version.trim()) {
            return Err(fail("E021", format!("member `{n}` is {v}, below `{r}`")));
        }
        if !out.iter().any(|l| l.key == key) {
            out.push(Locked {
                name: n,
                version: v,
                source: dir.clone(),
                key,
                digest: None,
                capability_digest: None,
                ffi: Vec::new(),
                libs: Vec::new(),
                publisher: None,
                approvals: Vec::new(),
                rebuilds: Vec::new(),
                path: None,
                kinds: Vec::new(),
            });
        }
    }
    // Minimums are rebuilt each round from the roots and the requirements of the versions
    // currently chosen, so a superseded version's requirements stop constraining (R894).
    let roots = mins;
    let mut chosen: BTreeMap<Key, (String, Info)> = BTreeMap::new();
    let mut settled = false;
    for _ in 0..MAX_ROUNDS {
        let mut mins = roots.clone();
        for (key, (_, info)) in &chosen {
            let who = format!("{} {}", key.0, info.version);
            for req in &info.requirements {
                raise(&mut mins, &who, req)?;
            }
        }
        let mut next: BTreeMap<Key, (String, Info)> = BTreeMap::new();
        for (key, m) in &mins {
            if let Some((path, manifest, digest, ffi)) = overrides.get(key) {
                next.insert(
                    key.clone(),
                    (
                        "override".to_owned(),
                        overridden(m, path, manifest, digest.clone(), ffi.clone())?,
                    ),
                );
                continue;
            }
            let index = registries.index(&m.name)?;
            let info = select(&index, m, pinned(key).as_deref())?;
            next.insert(key.clone(), (index.source, info));
        }
        let stable = next.len() == chosen.len()
            && next
                .iter()
                .all(|(k, (_, i))| chosen.get(k).is_some_and(|(_, c)| c.version == i.version));
        chosen = next;
        if stable {
            settled = true;
            break;
        }
    }
    if !settled {
        return Err(fail(
            "E021",
            "the requirements do not settle on one set of versions".into(),
        ));
    }
    for (key, (_, info)) in &chosen {
        let lib = info.libs.first().map(|l| format!("lib {}", l.name));
        consent(&top, &key.0, info.ffi.first().or(lib.as_ref()))?;
    }
    closure::check(&closure_nodes(root, &top, &members, &chosen))?;
    let kinds = subgraph_kinds(&top, &members, &chosen);
    for (key, (source, info)) in chosen {
        // Approvals and attestations belong to a version; `yan lock` keeps those it already has.
        let kept = |field: &str| {
            previous
                .iter()
                .find(|p| {
                    p.get("name").and_then(toml::Value::as_str) == Some(&key.0)
                        && p.get("version").and_then(toml::Value::as_str) == Some(&info.version)
                })
                .and_then(|p| p.get(field)?.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        out.push(Locked {
            approvals: {
                let mut a: Vec<String> = kept("approvals");
                for b in crate::vendor::approvals_of(root, &key.0, &info.version) {
                    if !a.contains(&b) {
                        a.push(b);
                    }
                }
                a
            },
            rebuilds: kept("rebuilds"),
            capability_digest: info.capability_digest,
            ffi: info.ffi,
            libs: info.libs,
            name: key.0.clone(),
            version: info.version,
            source,
            kinds: kinds.get(&key).cloned().unwrap_or_default(),
            path: overrides.get(&key).map(|(p, ..)| p.clone()),
            digest: Some(info.digest).filter(|d| !d.is_empty()),
            publisher: Some(info.publisher).filter(|p| !p.is_empty()),
            key,
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// The graph the closure check reads at resolution: the root, the members and every chosen package.
fn closure_nodes(
    root: &Path,
    top: &yan_elab::Manifest,
    members: &[(String, yan_elab::Manifest)],
    chosen: &BTreeMap<Key, (String, Info)>,
) -> Vec<Node> {
    let own = |dir: &Path, m: &yan_elab::Manifest| Node {
        name: m.name.clone().unwrap_or_else(|| ".".into()),
        key: m
            .name
            .as_deref()
            .zip(m.version.as_deref())
            .map(|(n, v)| identity(n, v)),
        ceiling: m.capabilities.clone(),
        libs: yan_db::foreign_set(dir)
            .libs
            .into_iter()
            .map(|l| l.name)
            .collect(),
        deps: closure::deps_of(&m.dependencies),
    };
    let mut nodes = vec![own(root, top)];
    nodes.extend(members.iter().map(|(dir, m)| own(&root.join(dir), m)));
    nodes.extend(chosen.iter().map(|(key, (_, info))| Node {
        name: key.0.clone(),
        key: Some(key.clone()),
        ceiling: info.ceiling.clone(),
        libs: info.libs.iter().map(|l| l.name.clone()).collect(),
        deps: closure::deps_of(&info.requirements),
    }));
    nodes
}

/// The subgraphs each chosen package is reachable from: `dependencies` (`dep`), `dev_dependencies`
/// (`dev`) and `tools` (`tool`), from the root and the members (R951).
fn subgraph_kinds(
    top: &yan_elab::Manifest,
    members: &[(String, yan_elab::Manifest)],
    chosen: &BTreeMap<Key, (String, Info)>,
) -> BTreeMap<Key, Vec<&'static str>> {
    let mut out: BTreeMap<Key, Vec<&'static str>> = BTreeMap::new();
    let manifests: Vec<&yan_elab::Manifest> = std::iter::once(top)
        .chain(members.iter().map(|(_, m)| m))
        .collect();
    type Roots = fn(&yan_elab::Manifest) -> &Vec<String>;
    let kinds: [(&'static str, Roots); 3] = [
        ("dep", |m| &m.dependencies),
        ("dev", |m| &m.dev_dependencies),
        ("tool", |m| &m.tools),
    ];
    for (kind, roots) in kinds {
        let mut todo: Vec<Key> = manifests
            .iter()
            .flat_map(|m| roots(m))
            .filter_map(|r| requirement(r))
            .collect();
        let mut seen: Vec<Key> = Vec::new();
        while let Some(key) = todo.pop() {
            if seen.contains(&key) {
                continue;
            }
            if let Some((_, info)) = chosen.get(&key) {
                todo.extend(info.requirements.iter().filter_map(|r| requirement(r)));
                out.entry(key.clone()).or_default().push(kind);
            }
            seen.push(key);
        }
    }
    out
}

/// An override's package: where it is recorded, its manifest and, for a git commit, the digest of
/// its archive.
type Overridden = (
    String,
    yan_elab::Manifest,
    Option<String>,
    yan_db::ForeignSet,
);

/// The root's `overrides` (R950): `"@org/name major": "path/to/dir"` or
/// `"git+https://host/repo@<commit> sha256:<hex>"`, each replacing that (name, major).
fn load_overrides(
    root: &Path,
    top: &yan_elab::Manifest,
) -> Result<BTreeMap<Key, Overridden>, Box<Diagnostic>> {
    let mut out = BTreeMap::new();
    for (key, path) in &top.overrides {
        let (name, major) = key
            .split_once(' ')
            .ok_or_else(|| fail("E018", format!("override `{key}` is not `@org/name x.y`")))?;
        let (recorded, dir, digest) = match path.strip_prefix("git+") {
            Some(spec) => {
                let (url, commit, digest) = git_spec(key, spec)?;
                let dir = fetch_git(&root.join(".yan/store"), key, url, commit, digest)?;
                (
                    format!("git+{url}@{commit}"),
                    dir,
                    Some(format!("sha256:{digest}")),
                )
            }
            None if path.contains("://") => {
                return Err(fail(
                    "E017",
                    format!(
                        "override `{key}`: a URL must be `git+https://host/repo@<commit> sha256:<hex>`"
                    ),
                ));
            }
            None => (path.clone(), root.join(path), None),
        };
        let manifest = manifest_at(&dir).ok_or_else(|| {
            fail(
                "E018",
                format!("override `{key}`: `{recorded}` has no readable `package.yag`"),
            )
        })?;
        if manifest.name.as_deref() != Some(name) {
            return Err(fail(
                "E018",
                format!("override `{key}`: `{recorded}` is not `{name}`"),
            ));
        }
        let ffi = yan_db::foreign_set(&dir);
        out.insert(
            identity(name, major.trim()),
            (recorded, manifest, digest, ffi),
        );
    }
    Ok(out)
}

/// `https://host/repo@<commit> sha256:<hex>`: the URL, the 40-hex commit and the archive digest.
pub(crate) fn git_spec<'a>(
    key: &str,
    spec: &'a str,
) -> Result<(&'a str, &'a str, &'a str), Box<Diagnostic>> {
    let bad = || {
        fail(
            "E018",
            format!(
                "override `{key}`: `git+{spec}` is not `git+https://host/repo@<commit> sha256:<hex>`"
            ),
        )
    };
    let (at, digest) = spec.split_once(" sha256:").ok_or_else(bad)?;
    let (url, commit) = at.rsplit_once('@').ok_or_else(bad)?;
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit());
    if !url.starts_with("https://") || !hex(commit, 40) || !hex(digest, 64) {
        return Err(bad());
    }
    Ok((url, commit, digest))
}

/// The unpacked tree of `commit` of `url` under `store`, whose zip archive (`git archive`) must
/// hash to `digest` (E020). The archive is kept in the store, so a rebuild needs no network.
pub(crate) fn fetch_git(
    store: &Path,
    key: &str,
    url: &str,
    commit: &str,
    digest: &str,
) -> Result<PathBuf, Box<Diagnostic>> {
    let kept = store.join("archives").join(format!("{digest}.zip"));
    let bytes = match std::fs::read(&kept) {
        Ok(b) => b,
        Err(_) => {
            let git = |dir: &Path, args: &[&str]| {
                let out = std::process::Command::new("git")
                    .args(args)
                    .current_dir(dir)
                    .output()
                    .map_err(|e| fail("E017", format!("override `{key}`: cannot run git: {e}")))?;
                if out.status.success() {
                    Ok(out.stdout)
                } else {
                    Err(fail(
                        "E017",
                        format!(
                            "override `{key}`: git {}: {}",
                            args[0],
                            String::from_utf8_lossy(&out.stderr).trim()
                        ),
                    ))
                }
            };
            let tmp = store.join(format!("git-{digest}"));
            let io = |e: std::io::Error| fail("E017", format!("override `{key}`: {e}"));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).map_err(io)?;
            git(&tmp, &["init", "--quiet"])?;
            git(&tmp, &["fetch", "--quiet", "--depth", "1", url, commit])?;
            let bytes = git(&tmp, &["archive", "--format=zip", commit])?;
            std::fs::remove_dir_all(&tmp).map_err(io)?;
            bytes
        }
    };
    if crate::store::sha256(&bytes) != digest {
        return Err(fail(
            "E020",
            format!("override `{key}`: commit {commit} does not archive to `sha256:{digest}`"),
        ));
    }
    std::fs::create_dir_all(store.join("archives"))
        .and_then(|()| std::fs::write(&kept, &bytes))
        .map_err(|e| fail("E017", format!("override `{key}`: {e}")))?;
    crate::store::unpack(store, digest, &bytes)
}

/// The version an override stands for, which must meet the accumulated minimum.
fn overridden(
    m: &Min,
    path: &str,
    manifest: &yan_elab::Manifest,
    digest: Option<String>,
    foreign: yan_db::ForeignSet,
) -> Result<Info, Box<Diagnostic>> {
    let version = manifest.version.clone().unwrap_or_default();
    if parts(&version) < parts(&m.want) {
        return Err(fail(
            "E021",
            format!(
                "override `{path}` is {} of `{}`, below {} (required by {})",
                version,
                m.name,
                m.want,
                m.requirers.join(", ")
            ),
        ));
    }
    Ok(Info {
        version,
        digest: digest.unwrap_or_default(),
        publisher: String::new(),
        capability_digest: None,
        ceiling: manifest.capabilities.clone(),
        ffi: foreign.modules,
        libs: foreign.libs,
        requirements: manifest.dependencies.clone(),
        yanked: false,
        yank_reason: None,
        fixes: Vec::new(),
        affects: Vec::new(),
        malicious: None,
        published: 0,
    })
}

/// E023: a package holding `ffi` modules or extern libraries that the root's `allow_ffi` does not
/// name (R1018, R1019). The consent is by name, so an update that gives a package its first
/// `ffi` module is refused too.
fn consent(
    top: &yan_elab::Manifest,
    name: &str,
    foreign: Option<&String>,
) -> Result<(), Box<Diagnostic>> {
    match foreign {
        Some(first) if !top.allow_ffi.iter().any(|a| a == name) => Err(fail(
            "E023",
            format!(
                "`{name}` holds foreign code (`{first}`) and the root's `allow_ffi` does not name it"
            ),
        )),
        _ => Ok(()),
    }
}

fn quoted(s: &str) -> String {
    toml::Value::String(s.to_owned()).to_string()
}

/// The canonical text: `format` first, records sorted by (name, major), fields in fixed order.
fn render(packages: &[Locked], pylock: Option<&str>) -> String {
    let mut text = format!("format = {FORMAT}\n");
    if let Some(d) = pylock {
        text += &format!("pylock = {}\n", quoted(d));
    }
    for p in packages {
        text += &format!(
            "\n[[package]]\nname = {}\nversion = {}\nsource = {}\n",
            quoted(&p.name),
            quoted(&p.version),
            quoted(&p.source)
        );
        for (field, value) in [
            ("digest", &p.digest),
            ("capability_digest", &p.capability_digest),
            ("publisher", &p.publisher),
            ("path", &p.path),
        ] {
            if let Some(v) = value {
                text += &format!("{field} = {}\n", quoted(v));
            }
        }
        for (field, values) in [("approvals", &p.approvals), ("rebuilds", &p.rebuilds)] {
            if !values.is_empty() {
                let items: Vec<_> = values.iter().map(|v| quoted(v)).collect();
                text += &format!("{field} = [{}]\n", items.join(", "));
            }
        }
        if !p.ffi.is_empty() {
            let items: Vec<_> = p.ffi.iter().map(|k| quoted(k)).collect();
            text += &format!("ffi = [{}]\n", items.join(", "));
        }
        if !p.libs.is_empty() {
            let items: Vec<_> = p
                .libs
                .iter()
                .map(|l| {
                    format!(
                        "{{ name = {}, link = {} }}",
                        quoted(&l.name),
                        quoted(&l.link)
                    )
                })
                .collect();
            text += &format!("libs = [{}]\n", items.join(", "));
        }
        if !p.kinds.is_empty() {
            let items: Vec<_> = p.kinds.iter().map(|k| quoted(k)).collect();
            text += &format!("kinds = [{}]\n", items.join(", "));
        }
    }
    text
}

/// The digest of `pylock.toml` when the root or a member declares `python` requirements: the
/// environment is `uv`'s, the lock pins its result (R950). A missing file is E021.
fn pylock_digest(root: &Path) -> Result<Option<String>, Box<Diagnostic>> {
    let top = manifest_at(root)
        .ok_or_else(|| fail("E018", "`package.yag` is missing or unreadable".into()))?;
    let python = !top.python.is_empty()
        || top
            .members
            .iter()
            .any(|d| manifest_at(&root.join(d)).is_some_and(|m| !m.python.is_empty()));
    if !python {
        return Ok(None);
    }
    let bytes = std::fs::read(root.join("pylock.toml")).map_err(|_| {
        fail(
            "E021",
            "`python` requirements need a `pylock.toml` at the root; resolve one with uv".into(),
        )
    })?;
    Ok(Some(format!("sha256:{}", crate::store::sha256(&bytes))))
}

/// `yan lock`: write `yan.lock` for the package at `root`. Returns whether the file changed.
/// Resolve without writing: the check `yan add` and `yan update` make before touching `yan.lock`.
pub(crate) fn resolves(root: &Path) -> Result<(), Box<Diagnostic>> {
    resolve(root).map(drop)
}

pub fn write(root: &Path) -> Result<bool, Box<Diagnostic>> {
    let path = root.join("yan.lock");
    let packages = resolve(root)?;
    let text = render(&packages, pylock_digest(root)?.as_deref());
    if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    std::fs::write(&path, text)
        .map_err(|e| fail("E020", format!("cannot write `yan.lock`: {e}")))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lock: Option<&str>, reqs: &[&str], locked: bool) -> Option<String> {
        let dir = std::env::temp_dir().join(format!("yan-lock-{}", std::process::id()));
        let dir = dir.join(format!(
            "{:x}",
            lock.map_or(0, |l| l.len()) + reqs.len() * 7919
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("yan.lock"));
        if let Some(l) = lock {
            std::fs::write(dir.join("yan.lock"), l).unwrap();
        }
        let reqs: Vec<String> = reqs.iter().map(|s| (*s).to_owned()).collect();
        check(&dir, &reqs, locked, false).err().map(|d| d.code)
    }

    #[test]
    fn locked_package_with_ffi_needs_the_roots_consent() {
        let lock =
            "format = 1\n\n[[package]]\nname = \"@a/b\"\nversion = \"1.4.2\"\nffi = [\"ffi/\"]\n";
        let root = |allow: &str| {
            let dir = std::env::temp_dir().join(format!(
                "yan-lock-ffi-{}-{}",
                std::process::id(),
                allow.len()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("yan.lock"), lock).unwrap();
            std::fs::write(
                dir.join("package.yag"),
                format!("package: Package = Package(name=\"@r/root\", version=\"0.1.0\", yan=\"1.x\"{allow})\n"),
            )
            .unwrap();
            check(&dir, &[], false, false).err().map(|d| d.code)
        };
        assert_eq!(root("").as_deref(), Some("E023"));
        assert_eq!(root(", allow_ffi=[\"@a/b\"]"), None);
    }

    const LOCK: &str = "format = 1\n\n[[package]]\nname = \"@a/b\"\nversion = \"1.4.2\"\n";

    #[test]
    fn newer_format_is_e019_even_unlocked() {
        assert_eq!(
            run(Some("format = 2\n"), &[], false).as_deref(),
            Some("E019")
        );
        assert_eq!(run(Some("format = 1\n"), &[], false), None);
    }

    #[test]
    fn missing_lock_is_e020_only_under_locked_with_dependencies() {
        assert_eq!(run(None, &["@a/b 1.4"], true).as_deref(), Some("E020"));
        assert_eq!(run(None, &["@a/b 1.4"], false), None);
        assert_eq!(run(None, &[], true), None);
    }

    #[test]
    fn locked_requires_each_name_and_major_pinned() {
        assert_eq!(run(Some(LOCK), &["@a/b 1.0"], true), None);
        assert_eq!(
            run(Some(LOCK), &["@a/b 2.0"], true).as_deref(),
            Some("E020")
        );
        assert_eq!(
            run(Some(LOCK), &["@a/c 1.0"], true).as_deref(),
            Some("E020")
        );
    }

    fn workspace(tag: &str, root_reqs: &str, member_pkg: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yan-ws-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(
            dir.join("package.yag"),
            format!("package: Package = Package(members=[\"b\"], dependencies=[{root_reqs}])\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("b/package.yag"),
            format!("package: Package = Package({member_pkg})\n"),
        )
        .unwrap();
        dir
    }

    #[test]
    fn writes_members_canonically_and_is_idempotent() {
        let dir = workspace("w", "\"@a/b 1.4\"", "name=\"@a/b\", version=\"1.4.2\"");
        assert!(write(&dir).unwrap());
        assert_eq!(
            std::fs::read_to_string(dir.join("yan.lock")).unwrap(),
            "format = 1\n\n[[package]]\nname = \"@a/b\"\nversion = \"1.4.2\"\nsource = \"b\"\n"
        );
        assert!(!write(&dir).unwrap());
        assert!(check(&dir, &["@a/b 1.0".into()], true, false).is_ok());
    }

    #[test]
    fn unprovided_or_too_old_requirement_is_e021() {
        let dir = workspace("x", "\"@a/c 1.0\"", "name=\"@a/b\", version=\"1.4.2\"");
        assert_eq!(write(&dir).unwrap_err().code, "E021");
        let dir = workspace("y", "\"@a/b 1.5\"", "name=\"@a/b\", version=\"1.4.2\"");
        assert_eq!(write(&dir).unwrap_err().code, "E021");
    }

    const Z: &[u8] = b"unsafe extern \"C\" lib zlib link \"z\"\n  safe f() -> c_int\n";

    /// A workspace whose lock pins an archive holding `ffi/z.yn`, with `record` as foreign fields.
    fn foreign_world(tag: &str, record: &str) -> (PathBuf, [String; 1]) {
        let dir = std::env::temp_dir().join(format!("yan-lock-set-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bytes = crate::store::tests::zip_of(&[("package.yag", b""), ("ffi/z.yn", Z)]);
        let digest = crate::store::sha256(&bytes);
        std::fs::create_dir_all(dir.join("vendor/archives")).unwrap();
        std::fs::write(dir.join(format!("vendor/archives/{digest}.zip")), bytes).unwrap();
        std::fs::write(
            dir.join("yan.lock"),
            format!(
                "format = 1\n\n[[package]]\nname = \"@a/b\"\nversion = \"1.0.0\"\ndigest = \"sha256:{digest}\"\n{record}"
            ),
        )
        .unwrap();
        (dir, ["@a/b 1.0.0".to_owned()])
    }

    #[test]
    fn locked_build_recomputes_the_foreign_set_from_the_archive() {
        let ok = "ffi = [\"ffi/z.yn\"]\nlibs = [{ name = \"zlib\", link = \"z\" }]\n";
        let (dir, reqs) = foreign_world("ok", ok);
        assert!(check(&dir, &reqs, true, false).is_ok());
        for (tag, bad) in [
            ("none", ""),
            (
                "ffi",
                "ffi = [\"ffi/y.yn\"]\nlibs = [{ name = \"zlib\", link = \"z\" }]\n",
            ),
            (
                "libs",
                "ffi = [\"ffi/z.yn\"]\nlibs = [{ name = \"zlib\", link = \"zz\" }]\n",
            ),
            ("nolibs", "ffi = [\"ffi/z.yn\"]\n"),
        ] {
            let (dir, reqs) = foreign_world(tag, bad);
            assert_eq!(
                check(&dir, &reqs, true, false).unwrap_err().code,
                "E020",
                "{tag}"
            );
        }
    }

    #[test]
    fn locked_build_checks_the_ceiling_against_the_closure_libraries() {
        let ok = "ffi = [\"ffi/z.yn\"]\nlibs = [{ name = \"zlib\", link = \"z\" }]\n";
        let (dir, reqs) = foreign_world("closure", ok);
        let manifest = b"package: Package = Package(name=\"@a/b\", version=\"1.0.0\", capabilities=[\"zlib\"])\n";
        let bytes = crate::store::tests::zip_of(&[("package.yag", manifest), ("ffi/z.yn", Z)]);
        let digest = crate::store::sha256(&bytes);
        std::fs::write(dir.join(format!("vendor/archives/{digest}.zip")), bytes).unwrap();
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        let old = lock
            .split("sha256:")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim_end_matches('"')
            .to_owned();
        std::fs::write(dir.join("yan.lock"), lock.replace(&old, &digest)).unwrap();
        let root = |caps: &str| {
            std::fs::write(
                dir.join("package.yag"),
                format!("package: Package = Package(name=\"@a/app\", version=\"0.1.0\", dependencies={{\"b\": \"@a/b 1.0.0\"}}, allow_ffi=[\"@a/b\"], capabilities=[{caps}])\n"),
            )
            .unwrap();
        };
        root("\"@a/b:zlib\"");
        let r = check(&dir, &reqs, true, false);
        assert!(r.is_ok(), "{:?}", r.err().map(|e| (e.code, e.message)));
        root("\"zlib\"");
        let e = check(&dir, &reqs, true, false).unwrap_err();
        assert_eq!(e.code, "E314");
        assert!(
            e.message.contains("`@a/b:zlib`") && e.message.contains("@a/app -> @a/b"),
            "{}",
            e.message
        );
    }

    #[test]
    fn libs_round_trip_through_the_lock() {
        let p = Locked {
            name: "@a/b".into(),
            version: "1.0.0".into(),
            source: "index".into(),
            key: identity("@a/b", "1.0.0"),
            digest: None,
            capability_digest: None,
            ffi: vec!["ffi/z.yn".into()],
            libs: vec![yan_db::ForeignLib {
                name: "zlib".into(),
                link: "z".into(),
            }],
            publisher: None,
            approvals: Vec::new(),
            rebuilds: Vec::new(),
            path: None,
            kinds: Vec::new(),
        };
        let text = render(&[p], None);
        assert!(
            text.contains("ffi = [\"ffi/z.yn\"]\nlibs = [{ name = \"zlib\", link = \"z\" }]\n")
        );
        let table: toml::Table = text.parse().unwrap();
        let rec = &table["package"].as_array().unwrap()[0];
        assert_eq!(recorded_set(rec).libs[0].link, "z");
    }
}
