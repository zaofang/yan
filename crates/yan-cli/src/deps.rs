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

/// `src` without the `dependencies` entry that requires `name`.
fn without_dependency(src: &str, name: &str) -> Option<String> {
    let (start, end) = deps_span(src)?;
    let needle = format!("\"{name} ");
    let items: Vec<&str> = src[start + 1..end - 1]
        .split(',')
        .map(str::trim)
        .filter(|i| !i.is_empty())
        .collect();
    let kept: Vec<&str> = items
        .iter()
        .copied()
        .filter(|i| !i.contains(&needle))
        .collect();
    (kept.len() < items.len()).then(|| {
        format!(
            "{}{}{}{}",
            &src[..=start],
            kept.join(", "),
            &src[end - 1..end],
            &src[end..]
        )
    })
}

fn local_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Who adds: a human at a terminal, or an agent through the `toolchain` capability (R947).
#[derive(Clone, Copy, PartialEq)]
pub enum Caller {
    Human,
    Agent,
}

/// `yan add @org/name[@text | @=x.y.z]`: the newest version outside the cool-off window that
/// satisfies `text`, written as the requirement's minimum.
pub fn add(root: &Path, spec: &str, now: i64, caller: Caller) -> Result<String, Failure> {
    let (name, text) = match spec[1..].find('@') {
        Some(i) => (&spec[..=i], &spec[i + 2..]),
        None => (spec, ""),
    };
    let (exact, text) = text.strip_prefix('=').map_or((false, text), |t| (true, t));
    let (src, m) = manifest(root)?;
    let index = Registries::load(root)?.index(name)?;
    let Some(versions) = index.versions(name)? else {
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
        let mut newest: Option<String> = None;
        for v in &versions {
            if let Some(i) = index.info(name, v)?
                && newest
                    .as_ref()
                    .is_none_or(|n| lock::parts(&i.version) > lock::parts(n))
            {
                newest = Some(i.version);
            }
        }
        newest
            .map(|v| v.split('.').next().unwrap_or("0").to_owned())
            .unwrap_or_default()
    } else {
        text.to_owned()
    };
    let info = if exact {
        index.info(name, &want)?.filter(|i| !i.yanked)
    } else {
        registry::newest(&index, name, &want, false, now)?
    }
    .ok_or_else(|| {
        Failure::Msg(format!(
            "no version of `{name}` satisfies `{want}` outside the 7-day cool-off"
        ))
    })?;
    if let Some(advisory) = &info.malicious {
        return Err(lock::malicious(&index, name, &info, advisory).into());
    }
    // No search snapshot exists yet, so nothing is established (R947).
    if caller == Caller::Agent
        && !lock::is_locked(root, name)
        && crate::vendor::approvals_of(root, name, &info.version).is_empty()
    {
        return Err(fail(
            "E004",
            format!(
                "`{name}` is neither locked nor established; run `yan approve {name}@{}` to allow the add",
                info.version
            ),
        )
        .into());
    }
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

/// `yan remove @org/name`: drop the requirement from `dependencies` and rewrite the lock.
pub fn remove(root: &Path, name: &str) -> Result<String, Failure> {
    let (src, _) = manifest(root)?;
    let out = without_dependency(&src, name)
        .ok_or_else(|| Failure::Msg(format!("`{name}` is not in `dependencies`")))?;
    std::fs::write(root.join("package.yag"), out)
        .map_err(|e| Failure::Msg(format!("cannot write package.yag: {e}")))?;
    lock::write(root)?;
    Ok(format!("removed {name}"))
}

/// `yan update [@org/name]`: move each registry requirement to the newest version outside the
/// cool-off window in its major, unless the publisher changed (E005).
pub fn update(root: &Path, only: Option<&str>, now: i64) -> Result<Vec<String>, Failure> {
    let (mut src, m) = manifest(root)?;
    let registries = Registries::load(root)?;
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
        let Some(info) = registry::newest(&index, name, current, false, now)? else {
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
    fn config_fields_are_checked_and_the_workspace_may_only_raise_the_cool_off() {
        let dir = world("config-fields");
        let write = |body: &str| {
            std::fs::write(
                dir.join(".yan/config.yag"),
                format!("config: Config = Config(registries={{\"@a\": \"index\"}}, {body})\n"),
            )
            .unwrap();
        };
        write("colour=1");
        assert_eq!(Registries::load(&dir).err().unwrap().code, "E018");
        write("min_release_age=0, trust=[\"x\"], offline=false, store=\"s\"");
        let now = registry::now();
        assert!(add(&dir, "@a/b@2", now, Caller::Human).is_err());
        assert_eq!(
            add(&dir, "@a/b", now, Caller::Human).ok().unwrap(),
            "added @a/b 1.2.0"
        );
        let dir = world("raised");
        std::fs::write(
            dir.join(".yan/config.yag"),
            "config: Config = Config(registries={\"@a\": \"index\"}, min_release_age=100000)\n",
        )
        .unwrap();
        assert!(add(&dir, "@a/b", now, Caller::Human).is_err());
    }

    #[test]
    fn lock_fields_follow_the_package_table_order_and_keep_approvals() {
        let dir = world("lock-fields");
        let p = dir.join("index/@a/b/@v/1.2.0.info");
        let body = std::fs::read_to_string(&p).unwrap();
        std::fs::write(
            &p,
            body.replace(
                "{\"digest",
                "{\"capability_digest\":\"sha256:cap\",\"digest",
            ),
        )
        .unwrap();
        let now = registry::now();
        add(&dir, "@a/b", now, Caller::Human).ok().unwrap();
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(lock.contains(
            "digest = \"sha256:1.2.0\"\ncapability_digest = \"sha256:cap\"\npublisher = \"pub\"\n"
        ));
        let approved = lock.replace(
            "publisher = \"pub\"\n",
            "publisher = \"pub\"\napprovals = [\"sha256:ap\"]\nrebuilds = [\"sha256:rb\"]\n",
        );
        std::fs::write(dir.join("yan.lock"), &approved).unwrap();
        assert!(!crate::lock::write(&dir).ok().unwrap());
        assert_eq!(
            std::fs::read_to_string(dir.join("yan.lock")).unwrap(),
            approved
        );
    }

    #[test]
    fn add_skips_the_cool_off_pins_exactly_and_locks_transitively() {
        let dir = world("add");
        let now = registry::now();
        assert_eq!(
            add(&dir, "@a/b", now, Caller::Human).ok().unwrap(),
            "added @a/b 1.2.0"
        );
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(lock.contains("name = \"@a/b\"\nversion = \"1.2.0\"\nsource = \"index\"\ndigest = \"sha256:1.2.0\"\npublisher = \"pub\""));
        assert!(
            std::fs::read_to_string(dir.join("package.yag"))
                .unwrap()
                .contains("dependencies={\"b\": \"@a/b 1.2.0\"}")
        );
        assert!(add(&dir, "@a/b", now, Caller::Human).is_err());
        let dir = world("exact");
        assert_eq!(
            add(&dir, "@a/b@=1.3.0", now, Caller::Human).ok().unwrap(),
            "added @a/b 1.3.0"
        );
        let dir = world("transitive");
        add(&dir, "@a/c", now, Caller::Human).ok().unwrap();
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(lock.contains("name = \"@a/b\"\nversion = \"1.2.0\""));
        assert!(lock.contains("name = \"@a/c\"\nversion = \"1.0.0\""));
    }

    #[test]
    fn unknown_name_is_e003_and_unmapped_namespace_is_e021() {
        let dir = world("names");
        let Err(Failure::Diag(d)) = add(&dir, "@a/bb", registry::now(), Caller::Human) else {
            panic!("expected a diagnostic");
        };
        assert_eq!(d.code, "E003");
        assert!(d.message.contains("@a/b"));
        let Err(Failure::Diag(d)) = add(&dir, "@z/b", registry::now(), Caller::Human) else {
            panic!("expected a diagnostic");
        };
        assert_eq!(d.code, "E021");
    }

    #[test]
    fn update_moves_to_the_newest_and_a_publisher_change_is_e005() {
        let dir = world("update");
        let now = registry::now();
        add(&dir, "@a/b@=1.0.0", now, Caller::Human).ok().unwrap();
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
        add(&dir, "@a/b", registry::now(), Caller::Human)
            .ok()
            .unwrap();
        let reqs = ["@a/b 1.2.0".to_owned()];
        assert_eq!(
            lock::check(&dir, &reqs, true, true).unwrap_err().code,
            "E020"
        );
        let bytes = crate::store::tests::zip_of(&[("package.yag", b"")]);
        let digest = crate::store::sha256(&bytes);
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        std::fs::write(
            dir.join("yan.lock"),
            lock.replace("sha256:1.2.0", &format!("sha256:{digest}")),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("vendor/archives")).unwrap();
        std::fs::write(
            dir.join("vendor/archives").join(format!("{digest}.zip")),
            bytes,
        )
        .unwrap();
        assert!(lock::check(&dir, &reqs, true, true).is_ok());
    }

    #[test]
    fn a_superseded_version_stops_constraining() {
        let dir = world("superseded");
        let put = |n: &str, v: &str, reqs: &str| {
            let d = dir.join("index/@a").join(n).join("@v");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join(format!("{v}.info")),
                info(v, "2020-01-01T00:00:00Z", "pub", reqs),
            )
            .unwrap();
        };
        put("x", "1.0.0", "");
        put("x", "1.5.0", "");
        std::fs::write(dir.join("index/@a/x/@v/list"), "1.0.0\n1.5.0\n").unwrap();
        put("b", "1.2.0", "");
        put("b", "1.0.0", "\"@a/x 1.5\"");
        put("b", "1.3.0", "\"@a/x 1.0\"");
        std::fs::write(dir.join("index/@a/b/@v/list"), "1.0.0\n1.2.0\n1.3.0\n").unwrap();
        put("c", "1.0.0", "\"@a/b 1.3\"");
        std::fs::write(
            dir.join("package.yag"),
            "package: Package = Package(name=\"@a/app\", version=\"0.1.0\", dependencies=[\"@a/b 1.0.0\", \"@a/c 1.0.0\"])\n",
        )
        .unwrap();
        lock::write(&dir).unwrap();
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(lock.contains("name = \"@a/b\"\nversion = \"1.3.0\""));
        assert!(lock.contains("name = \"@a/x\"\nversion = \"1.0.0\""));
    }

    #[test]
    fn remove_drops_the_entry_and_its_lock_record() {
        let dir = world("remove");
        let now = registry::now();
        add(&dir, "@a/b", now, Caller::Human).ok().unwrap();
        add(&dir, "@a/c", now, Caller::Human).ok().unwrap();
        assert_eq!(remove(&dir, "@a/c").ok().unwrap(), "removed @a/c");
        let yag = std::fs::read_to_string(dir.join("package.yag")).unwrap();
        assert!(yag.contains("dependencies={\"b\": \"@a/b 1.2.0\"}"));
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(!lock.contains("@a/c"));
        assert!(remove(&dir, "@a/c").is_err());
    }

    #[test]
    fn an_agent_add_of_an_unlocked_package_is_e004() {
        let dir = world("agent");
        let now = registry::now();
        let Err(Failure::Diag(d)) = add(&dir, "@a/b", now, Caller::Agent) else {
            panic!("expected a diagnostic");
        };
        assert_eq!(d.code, "E004");
        assert!(d.message.contains("yan approve @a/b@1.2.0"));
        assert!(!dir.join("yan.lock").exists());
        add(&dir, "@a/b", now, Caller::Human).ok().unwrap();
        assert!(lock::is_locked(&dir, "@a/b"));
    }

    #[test]
    fn a_malicious_version_is_e007_even_when_pinned_and_names_the_clean_one() {
        let dir = world("malicious");
        add(&dir, "@a/b", registry::now(), Caller::Human)
            .ok()
            .unwrap();
        let bad = dir.join("index/@a/b/@v/1.2.0.info");
        let text = std::fs::read_to_string(&bad).unwrap();
        std::fs::write(
            &bad,
            text.replace(
                "\"yanked\"",
                "\"malicious\":true,\"advisory\":\"steals keys\",\"yanked\"",
            ),
        )
        .unwrap();
        let err = lock::write(&dir).unwrap_err();
        assert_eq!(err.code, "E007");
        assert!(err.message.contains("steals keys"));
        assert!(err.message.contains("nearest clean version: 1.3.0"));
        let Err(Failure::Diag(d)) = add(&dir, "@a/b@=1.2.0", registry::now(), Caller::Human) else {
            panic!("expected a diagnostic");
        };
        assert_eq!(d.code, "E007");
    }

    #[test]
    fn approve_clears_e004_and_vendor_copies_and_prunes() {
        let dir = world("vendor");
        let now = registry::now();
        let Err(Failure::Diag(d)) = add(&dir, "@a/c", now, Caller::Agent) else {
            panic!("expected E004");
        };
        assert_eq!(d.code, "E004");
        crate::vendor::approve(&dir, "@a/c@1.0.0").ok().unwrap();
        add(&dir, "@a/c", now, Caller::Agent).ok().unwrap();
        let lock = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(lock.contains("approvals = [\"sha256:"));

        // Give every locked package a real archive in the index and the lock its digest.
        let bytes = crate::store::tests::zip_of(&[("package.yag", b"")]);
        let digest = crate::store::sha256(&bytes);
        for (n, v) in [("b", "1.2.0"), ("c", "1.0.0")] {
            let at = dir.join("index/@a").join(n).join("@v");
            std::fs::write(at.join(format!("{v}.zip")), &bytes).unwrap();
            let info = at.join(format!("{v}.info"));
            let text = std::fs::read_to_string(&info).unwrap();
            std::fs::write(
                &info,
                text.replace(&format!("sha256:{v}"), &format!("sha256:{digest}")),
            )
            .unwrap();
        }
        std::fs::remove_file(dir.join("yan.lock")).unwrap();
        lock::write(&dir).unwrap();
        let out = crate::vendor::vendor(&dir).ok().unwrap();
        assert!(out.iter().any(|l| l.starts_with("vendored")));
        let archive = dir.join("vendor/archives").join(format!("{digest}.zip"));
        assert_eq!(std::fs::read(&archive).unwrap(), bytes);
        assert_eq!(
            std::fs::read_dir(dir.join("vendor/bundles"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            crate::vendor::vendor(&dir).ok().unwrap(),
            ["vendor is up to date"]
        );
        std::fs::write(dir.join("vendor/archives/stale.zip"), "x").unwrap();
        assert_eq!(
            crate::vendor::vendor(&dir).ok().unwrap(),
            ["removed archives/stale.zip"]
        );
    }

    #[test]
    fn an_override_replaces_its_name_and_major_and_is_recorded_with_its_path() {
        let dir = world("override");
        std::fs::create_dir_all(dir.join("fix/b")).unwrap();
        std::fs::write(
            dir.join("fix/b/package.yag"),
            "package: Package = Package(name=\"@a/b\", version=\"1.2.5\")\n",
        )
        .unwrap();
        let manifest = |overrides: &str| {
            std::fs::write(
                dir.join("package.yag"),
                format!("package: Package = Package(name=\"@a/app\", version=\"0.1.0\", dependencies=[\"@a/b 1.0.0\"], overrides={{{overrides}}})\n"),
            )
            .unwrap();
        };
        manifest("\"@a/b 1\": \"fix/b\"");
        lock::write(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        assert!(text.contains(
            "name = \"@a/b\"\nversion = \"1.2.5\"\nsource = \"override\"\npath = \"fix/b\"\n"
        ));
        assert!(!text.contains("digest"));
        manifest("\"@a/b 1\": \"https://x.org/b.git\"");
        assert_eq!(lock::write(&dir).unwrap_err().code, "E017");
    }

    #[test]
    fn the_lock_marks_each_package_with_the_subgraphs_that_use_it() {
        let dir = world("kinds");
        std::fs::write(
            dir.join("package.yag"),
            "package: Package = Package(name=\"@a/app\", version=\"0.1.0\", dependencies=[\"@a/b 1.0.0\"], dev_dependencies=[\"@a/c 1.0.0\"])\n",
        )
        .unwrap();
        lock::write(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join("yan.lock")).unwrap();
        // c requires b, so b is in both subgraphs and c only in dev; one version of b.
        assert!(text.contains("name = \"@a/b\"\nversion = \"1.2.0\""));
        assert!(text.contains("kinds = [\"dep\", \"dev\"]"));
        assert!(text.contains("name = \"@a/c\"") && text.matches("kinds = [\"dev\"]").count() == 1);
    }
}
