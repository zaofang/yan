//! `yan.lock` (19.1, R892 to R896): reading it, E019 for a newer format, E020 under `--locked`, and
//! `yan lock`, which writes it from the manifest and the workspace members.

use std::path::Path;

use yan_diag::{Diagnostic, Span};

/// The newest lock `format` this toolchain reads.
const FORMAT: i64 = 1;

/// The identity a requirement `"@org/name x.y[.z]"` pins: the name and its major (the minor in 0.y.z).
fn identity(name: &str, version: &str) -> (String, String) {
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

fn fail(code: &str, message: String) -> Box<Diagnostic> {
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
pub fn check(root: &Path, requirements: &[String], locked: bool) -> Result<(), Box<Diagnostic>> {
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
    let pinned: Vec<(String, String)> = table
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
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
    Ok(())
}

fn manifest_at(dir: &Path) -> Option<yan_elab::Manifest> {
    let src = std::fs::read_to_string(dir.join("package.yag")).ok()?;
    let (m, _) = yan_yag::parse("package.yag", &src, false);
    yan_elab::manifest(&m?)
}

/// Numeric dotted components, so that `1.10` orders after `1.9`.
fn parts(version: &str) -> Vec<u64> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

struct Locked {
    name: String,
    version: String,
    source: String,
    key: (String, String),
}

/// Resolve every requirement of the package at `root` and of its members against the members
/// themselves (19.1: members share one lock). Anything else needs an index (E021, R895).
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
    let requirements = top
        .requirements
        .iter()
        .chain(members.iter().flat_map(|(_, m)| &m.requirements));
    let mut out: Vec<Locked> = Vec::new();
    for r in requirements {
        let Some((name, version)) = r.split_once(' ') else {
            return Err(fail("E018", format!("`{r}` is not `@org/name x.y[.z]`")));
        };
        let key = identity(name, version.trim());
        let found = members.iter().find_map(|(dir, m)| {
            let (n, v) = (m.name.as_deref()?, m.version.as_deref()?);
            (identity(n, v) == key).then(|| (dir, n.to_owned(), v.to_owned()))
        });
        let Some((dir, n, v)) = found else {
            return Err(fail(
                "E021",
                format!(
                    "no member provides `{r}` and its namespace has no `registries` mapping in `.yan/config.yag`"
                ),
            ));
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
            });
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
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
        check(&dir, &reqs, locked).err().map(|d| d.code)
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
        assert!(check(&dir, &["@a/b 1.0".into()], true).is_ok());
    }

    #[test]
    fn unprovided_or_too_old_requirement_is_e021() {
        let dir = workspace("x", "\"@a/c 1.0\"", "name=\"@a/b\", version=\"1.4.2\"");
        assert_eq!(write(&dir).unwrap_err().code, "E021");
        let dir = workspace("y", "\"@a/b 1.5\"", "name=\"@a/b\", version=\"1.4.2\"");
        assert_eq!(write(&dir).unwrap_err().code, "E021");
    }
}
