//! `yan add` and `yan update` (19.4, R894, R896): choose a minimum from the index, write it into
//! `package.yag`, then write `yan.lock`.

use std::path::Path;

use yan_diag::Diagnostic;

use crate::lock::{self, fail, identity};
use crate::registry::{self, Registries};

#[derive(Debug)]
pub enum Failure {
    Diag(Box<Diagnostic>),
    Msg(String),
}

impl From<Box<Diagnostic>> for Failure {
    fn from(d: Box<Diagnostic>) -> Self {
        Self::Diag(d)
    }
}

fn manifest(root: &Path) -> Result<(String, yan_elab::Manifest), Failure> {
    let src = std::fs::read_to_string(root.join("package.yag"))
        .map_err(|e| Failure::Msg(format!("cannot read package.yag: {e}")))?;
    let (m, _) = yan_yag::parse("package.yag", &src, false);
    let m = m
        .and_then(|m| yan_elab::manifest(&m))
        .ok_or_else(|| fail("E018", "`package.yag` has no `Package(...)`".into()))?;
    Ok((src, m))
}

/// The byte range of the `[...]` or `{...}` after `dependencies=`, brackets included.
fn deps_span(src: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(i) = src[from..].find("dependencies") {
        let at = from + i;
        from = at + "dependencies".len();
        if src[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let Some(rest) = src[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        let start = src.len() - rest.trim_start().len();
        let open = src[start..].chars().next()?;
        let close = match open {
            '[' => ']',
            '{' => '}',
            _ => return None,
        };
        let (mut depth, mut in_str, mut escaped) = (0, false, false);
        for (j, c) in src[start..].char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' if in_str => escaped = true,
                '"' => in_str = !in_str,
                _ if in_str => {}
                c if c == open => depth += 1,
                c if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((start, start + j + 1));
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    None
}

/// `src` with the requirement `req` added to `dependencies`, which is created when absent.
fn with_dependency(src: &str, local: &str, req: &str) -> Result<String, Failure> {
    let Some((start, end)) = deps_span(src) else {
        let at = src
            .find("Package(")
            .ok_or_else(|| Failure::Msg("package.yag has no `Package(`".into()))?
            + "Package(".len();
        let item = format!("dependencies={{\"{local}\": \"{req}\"}},");
        return Ok(if src[at..].starts_with('\n') {
            format!("{}\n    {item}{}", &src[..at], &src[at..])
        } else {
            format!("{}{item} {}", &src[..at], &src[at..])
        });
    };
    let (open, close) = (&src[start..=start], &src[end - 1..end]);
    let item = if open == "{" {
        format!("\"{local}\": \"{req}\"")
    } else {
        format!("\"{req}\"")
    };
    let body = src[start + 1..end - 1].trim().trim_end_matches(',');
    let body = if body.is_empty() {
        item
    } else {
        format!("{body}, {item}")
    };
    Ok(format!(
        "{}{open}{body}{close}{}",
        &src[..start],
        &src[end..]
    ))
}

fn local_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// `yan add @org/name[@text | @=x.y.z]`: the newest version outside the cool-off window that
/// satisfies `text`, written as the requirement's minimum.
pub fn add(root: &Path, spec: &str, now: i64) -> Result<String, Failure> {
    let (name, text) = match spec[1..].find('@') {
        Some(i) => (&spec[..=i], &spec[i + 2..]),
        None => (spec, ""),
    };
    let (exact, text) = text.strip_prefix('=').map_or((false, text), |t| (true, t));
    let (src, m) = manifest(root)?;
    let index = Registries::load(root).index(name)?;
    let Some(versions) = index.versions(name) else {
        let near = index.nearest(name);
        return Err(fail(
            "E003",
            format!(
                "`{name}` does not exist; nearest: {}",
                if near.is_empty() {
                    "none".into()
                } else {
                    near.join(", ")
                }
            ),
        )
        .into());
    };
    let want = if text.is_empty() {
        versions
            .iter()
            .filter_map(|v| index.info(name, v))
            .map(|i| i.version)
            .max_by_key(|v| lock::parts(v))
            .map(|v| v.split('.').next().unwrap_or("0").to_owned())
            .unwrap_or_default()
    } else {
        text.to_owned()
    };
    let info = if exact {
        index.info(name, &want).filter(|i| !i.yanked)
    } else {
        registry::newest(&index, name, &want, false, now)
    }
    .ok_or_else(|| {
        Failure::Msg(format!(
            "no version of `{name}` satisfies `{want}` outside the 7-day cool-off"
        ))
    })?;
    let req = format!("{name} {}", info.version);
    if m.requirements.iter().any(|r| {
        r.split_once(' ')
            .is_some_and(|(n, v)| identity(n, v) == identity(name, &info.version))
    }) {
        return Err(Failure::Msg(format!(
            "`{name}` is already required; use `yan update`"
        )));
    }
    std::fs::write(
        root.join("package.yag"),
        with_dependency(&src, local_name(name), &req)?,
    )
    .map_err(|e| Failure::Msg(format!("cannot write package.yag: {e}")))?;
    lock::write(root)?;
    Ok(format!("added {req}"))
}

/// `yan update [@org/name]`: move each registry requirement to the newest version outside the
/// cool-off window in its major, unless the publisher changed (E005).
pub fn update(root: &Path, only: Option<&str>, now: i64) -> Result<Vec<String>, Failure> {
    let (mut src, m) = manifest(root)?;
    let registries = Registries::load(root);
    let mut moved = Vec::new();
    let mut seen = false;
    for r in &m.requirements {
        let Some((name, current)) = r.split_once(' ') else {
            continue;
        };
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let Ok(index) = registries.index(name) else {
            continue;
        };
        seen = true;
        let Some(info) = registry::newest(&index, name, current, false, now) else {
            continue;
        };
        if lock::parts(&info.version) <= lock::parts(current) {
            continue;
        }
        if lock::locked_publisher(root, name, current).is_some_and(|p| p != info.publisher) {
            return Err(fail(
                "E005",
                format!(
                    "`{name}` {} changes publisher identity; needs an approval",
                    info.version
                ),
            )
            .into());
        }
        let new = format!("{name} {}", info.version);
        src = src.replace(&format!("\"{r}\""), &format!("\"{new}\""));
        moved.push(format!("{name} {current} -> {}", info.version));
    }
    if let Some(o) = only
        && !seen
    {
        return Err(Failure::Msg(format!("`{o}` is not a registry requirement")));
    }
    if !moved.is_empty() {
        std::fs::write(root.join("package.yag"), src)
            .map_err(|e| Failure::Msg(format!("cannot write package.yag: {e}")))?;
    }
    lock::write(root)?;
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_goes_into_a_dict_a_list_or_a_new_field() {
        let dict =
            "package: Package = Package(name=\"@a/x\", dependencies={\"b\": \"@a/b 1.0.0\"})";
        assert!(
            with_dependency(dict, "c", "@a/c 2.0.0")
                .unwrap()
                .contains("{\"b\": \"@a/b 1.0.0\", \"c\": \"@a/c 2.0.0\"}")
        );
        let list = "package: Package = Package(dependencies=[\"@a/b 1.0.0\",])";
        assert!(
            with_dependency(list, "c", "@a/c 2.0.0")
                .unwrap()
                .contains("[\"@a/b 1.0.0\", \"@a/c 2.0.0\"]")
        );
        let none = "package: Package = Package(\n    name=\"@a/x\",\n)";
        assert!(
            with_dependency(none, "c", "@a/c 2.0.0")
                .unwrap()
                .contains("Package(\n    dependencies={\"c\": \"@a/c 2.0.0\"},\n    name")
        );
    }

    #[test]
    fn dev_dependencies_are_not_dependencies() {
        let src = "Package(dev_dependencies=[\"@a/b 1.0\"])";
        assert!(deps_span(src).is_none());
    }

    fn info(version: &str, time: &str, publisher: &str, reqs: &str) -> String {
        format!(
            "{{\"digest\":\"sha256:{version}\",\"publisher\":\"{publisher}\",\"requirements\":[{reqs}],\"yanked\":false,\"time\":\"{time}\"}}"
        )
    }

    /// A workspace whose `@a` namespace maps to a directory index with `@a/b` 1.0.0, 1.2.0
    /// (old) and 1.3.0 (published in 2999), and `@a/c` 1.0.0 requiring `@a/b 1.2`.
    fn world(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yan-deps-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let at = |n: &str, v: &str, body: String| {
            let d = dir.join("index/@a").join(n).join("@v");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{v}.info")), body).unwrap();
        };
        let list = |n: &str, vs: &str| {
            std::fs::write(dir.join("index/@a").join(n).join("@v/list"), vs).unwrap();
        };
        at(
            "b",
            "1.0.0",
            info("1.0.0", "2020-01-01T00:00:00Z", "pub", ""),
        );
        at(
            "b",
            "1.2.0",
            info("1.2.0", "2020-02-01T00:00:00Z", "pub", ""),
        );
        at(
            "b",
            "1.3.0",
            info("1.3.0", "2999-01-01T00:00:00Z", "pub", ""),
        );
        list("b", "1.0.0\n1.2.0\n1.3.0\n");
        at(
            "c",
            "1.0.0",
            info("1.0.0", "2020-01-01T00:00:00Z", "pub", "\"@a/b 1.2\""),
        );
        list("c", "1.0.0\n");
        std::fs::create_dir_all(dir.join(".yan")).unwrap();
        std::fs::write(
            dir.join(".yan/config.yag"),
            "config: Config = Config(registries={\"@a\": \"index\"})\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("package.yag"),
            "package: Package = Package(name=\"@a/app\", version=\"0.1.0\")\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn add_skips_the_cool_off_pins_exactly_and_locks_transitively() {
        let dir = world("add");
        let now = registry::now();
        assert_eq!(add(&dir, "@a/b", now).ok().unwrap(), "added @a/b 1.2.0");
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(lock.contains("name = \"@a/b\"\nversion = \"1.2.0\"\nsource = \"index\"\ndigest = \"sha256:1.2.0\"\npublisher = \"pub\""));
        assert!(
            std::fs::read_to_string(dir.join("package.yag"))
                .unwrap()
                .contains("dependencies={\"b\": \"@a/b 1.2.0\"}")
        );
        assert!(add(&dir, "@a/b", now).is_err());
        let dir = world("exact");
        assert_eq!(
            add(&dir, "@a/b@=1.3.0", now).ok().unwrap(),
            "added @a/b 1.3.0"
        );
        let dir = world("transitive");
        add(&dir, "@a/c", now).ok().unwrap();
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(lock.contains("name = \"@a/b\"\nversion = \"1.2.0\""));
        assert!(lock.contains("name = \"@a/c\"\nversion = \"1.0.0\""));
    }

    #[test]
    fn unknown_name_is_e003_and_unmapped_namespace_is_e021() {
        let dir = world("names");
        let Err(Failure::Diag(d)) = add(&dir, "@a/bb", registry::now()) else {
            panic!("expected a diagnostic");
        };
        assert_eq!(d.code, "E003");
        assert!(d.message.contains("@a/b"));
        let Err(Failure::Diag(d)) = add(&dir, "@z/b", registry::now()) else {
            panic!("expected a diagnostic");
        };
        assert_eq!(d.code, "E021");
    }

    #[test]
    fn update_moves_to_the_newest_and_a_publisher_change_is_e005() {
        let dir = world("update");
        let now = registry::now();
        add(&dir, "@a/b@=1.0.0", now).ok().unwrap();
        let moved = update(&dir, None, now).ok().unwrap();
        assert_eq!(moved, vec!["@a/b 1.0.0 -> 1.2.0".to_owned()]);
        assert!(
            std::fs::read_to_string(dir.join("yan.lock"))
                .unwrap()
                .contains("version = \"1.2.0\"")
        );
        assert!(update(&dir, None, now).ok().unwrap().is_empty());
        let v = dir.join("index/@a/b/@v");
        std::fs::write(
            v.join("1.4.0.info"),
            info("1.4.0", "2020-03-01T00:00:00Z", "other", ""),
        )
        .unwrap();
        std::fs::write(v.join("list"), "1.0.0\n1.2.0\n1.3.0\n1.4.0\n").unwrap();
        let Err(Failure::Diag(d)) = update(&dir, None, now) else {
            panic!("expected a diagnostic");
        };
        assert_eq!(d.code, "E005");
    }

    #[test]
    fn offline_locked_needs_each_archive_in_the_store_or_vendor() {
        let dir = world("offline");
        add(&dir, "@a/b", registry::now()).ok().unwrap();
        let reqs = ["@a/b 1.2.0".to_owned()];
        assert_eq!(
            lock::check(&dir, &reqs, true, true).unwrap_err().code,
            "E020"
        );
        std::fs::create_dir_all(dir.join("vendor")).unwrap();
        std::fs::write(dir.join("vendor/1.2.0.zip"), "").unwrap();
        assert!(lock::check(&dir, &reqs, true, true).is_ok());
    }
}
