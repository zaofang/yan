//! `yan.lock` (19.1, R892 to R896): reading it, E019 for a newer format, E020 under `--locked`, and
//! `yan lock`, which writes it from the manifest and the workspace members.

use std::collections::BTreeMap;
use std::path::Path;

use yan_diag::{Diagnostic, Span};

use crate::registry::{Index, Info, Registries};

/// The newest lock `format` this toolchain reads.
const FORMAT: i64 = 1;

/// The identity a requirement `"@org/name x.y[.z]"` pins: the name and its major (the minor in 0.y.z).
pub(crate) fn identity(name: &str, version: &str) -> (String, String) {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("");
    let key = match major {
        "0" => format!("0.{}", parts.next().unwrap_or("0")),
        m => m.to_owned(),
    };
    (name.to_owned(), key)
}

fn requirement(req: &str) -> Option<(String, String)> {
    let (name, version) = req.split_once(' ')?;
    Some(identity(name, version.trim()))
}

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
    if offline {
        let store = crate::registry::configured_store(root);
        let roots = crate::store::roots(root, store.as_deref());
        for p in &records {
            let Some(digest) = p.get("digest").and_then(toml::Value::as_str) else {
                continue;
            };
            if crate::store::source(&roots, digest)?.is_none() {
                return Err(fail(
                    "E020",
                    format!(
                        "offline: no archive for `{digest}` in the workspace store, `vendor` or the user store"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// The `[[package]]` records of the existing `yan.lock`, if it reads.
fn records(root: &Path) -> Vec<toml::Value> {
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

/// The publisher identity `yan.lock` records for the package a requirement names.
pub fn locked_publisher(root: &Path, name: &str, version: &str) -> Option<String> {
    let key = identity(name, version);
    records(root).iter().find_map(|p| {
        let id = identity(p.get("name")?.as_str()?, p.get("version")?.as_str()?);
        (id == key).then(|| p.get("publisher")?.as_str().map(str::to_owned))?
    })
}

fn manifest_at(dir: &Path) -> Option<yan_elab::Manifest> {
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
    publisher: Option<String>,
    approvals: Vec<String>,
    rebuilds: Vec<String>,
    /// The directory of an override, relative to the root (R950).
    path: Option<String>,
}

type Key = (String, String);

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
                publisher: None,
                approvals: Vec::new(),
                rebuilds: Vec::new(),
                path: None,
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
            if let Some((path, manifest)) = overrides.get(key) {
                next.insert(
                    key.clone(),
                    ("override".to_owned(), overridden(m, path, manifest)?),
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
            name: key.0.clone(),
            version: info.version,
            source,
            path: overrides.get(&key).map(|(p, _)| p.clone()),
            digest: Some(info.digest).filter(|d| !d.is_empty()),
            publisher: Some(info.publisher).filter(|p| !p.is_empty()),
            key,
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// The root's `overrides` (R950): `"@org/name major": "path/to/dir"`, each replacing that
/// (name, major) with the package at the path. Anything but a path is E017.
fn load_overrides(
    root: &Path,
    top: &yan_elab::Manifest,
) -> Result<BTreeMap<Key, (String, yan_elab::Manifest)>, Box<Diagnostic>> {
    let mut out = BTreeMap::new();
    for (key, path) in &top.overrides {
        let (name, major) = key
            .split_once(' ')
            .ok_or_else(|| fail("E018", format!("override `{key}` is not `@org/name x.y`")))?;
        if path.contains("://") {
            return Err(fail(
                "E017",
                format!("override `{key}`: only a local path is supported yet"),
            ));
        }
        let manifest = manifest_at(&root.join(path)).ok_or_else(|| {
            fail(
                "E018",
                format!("override `{key}`: `{path}` has no readable `package.yag`"),
            )
        })?;
        if manifest.name.as_deref() != Some(name) {
            return Err(fail(
                "E018",
                format!("override `{key}`: `{path}` is not `{name}`"),
            ));
        }
        out.insert(identity(name, major.trim()), (path.clone(), manifest));
    }
    Ok(out)
}

/// The version an override stands for, which must meet the accumulated minimum.
fn overridden(m: &Min, path: &str, manifest: &yan_elab::Manifest) -> Result<Info, Box<Diagnostic>> {
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
        digest: String::new(),
        publisher: String::new(),
        capability_digest: None,
        requirements: manifest.requirements.clone(),
        yanked: false,
        malicious: None,
        published: 0,
    })
}

fn quoted(s: &str) -> String {
    toml::Value::String(s.to_owned()).to_string()
}

/// The canonical text: `format` first, records sorted by (name, major), fields in fixed order.
fn render(packages: &[Locked]) -> String {
    let mut text = format!("format = {FORMAT}\n");
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
    }
    text
}

/// `yan lock`: write `yan.lock` for the package at `root`. Returns whether the file changed.
pub fn write(root: &Path) -> Result<bool, Box<Diagnostic>> {
    let path = root.join("yan.lock");
    let text = render(&resolve(root)?);
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
}
