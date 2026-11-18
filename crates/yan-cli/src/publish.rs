//! `yan publish --check` (19.4, R646): the manifest refusals that need no resolver, E016.

use std::path::Path;

use yan_diag::{Diagnostic, Span};
use yan_elab::Manifest;

fn refusal(message: String) -> Diagnostic {
    Diagnostic::error("E016", "package.yag", Span::default(), "", message)
}

fn major(version: &str) -> Option<u64> {
    version.split(['.', '-', '+']).next()?.parse().ok()
}

/// The pre-release dependencies of a package at 1.0 or later: `"@org/name x.y.z-pre"` (R643).
fn prereleases(m: &Manifest) -> Vec<&str> {
    if m.version.as_deref().and_then(major).is_none_or(|v| v < 1) {
        return Vec::new();
    }
    m.dependencies
        .iter()
        .map(String::as_str)
        .filter(|r| r.split_once(' ').is_some_and(|(_, v)| v.contains('-')))
        .collect()
}

pub fn refusals(m: &Manifest) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if m.has_overrides {
        out.push(refusal(
            "`overrides` is root-only and cannot be published".into(),
        ));
    }
    if !m.previews.is_empty() && m.version.as_deref().and_then(major).is_some_and(|v| v >= 1) {
        out.push(Diagnostic::error(
            "E002",
            "package.yag",
            Span::default(),
            "",
            "a package at 1.0 or later cannot enable previews",
        ));
    }
    for r in prereleases(m) {
        out.push(refusal(format!(
            "`{r}` is a pre-release dependency of a package at 1.0 or later"
        )));
    }
    out
}

pub fn check(root: &Path) -> Result<Vec<Diagnostic>, String> {
    let manifest = read(root)?;
    let mut out = refusals(&manifest);
    // A workspace publishes its members, each after the members it requires (RFC 0084 D7).
    for member in members_in_order(root) {
        let rel = member.strip_prefix(root).unwrap_or(&member);
        let mut found = refusals(&read(&member)?);
        for d in &mut found {
            d.file = rel.join("package.yag").display().to_string();
        }
        out.extend(found);
    }
    Ok(out)
}

fn read(root: &Path) -> Result<Manifest, String> {
    let path = root.join("package.yag");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (m, _) = yan_yag::parse("package.yag", &src, false);
    m.and_then(|m| yan_elab::manifest(&m))
        .ok_or_else(|| "package.yag declares no `package: Package = Package(...)`".into())
}

/// The directories of the workspace's members, each after every member it requires; members that
/// do not depend on each other keep their order in `members`.
pub fn members_in_order(root: &Path) -> Vec<std::path::PathBuf> {
    let packages = crate::graph::packages(root);
    let members: Vec<&yan_db::Package> = packages
        .iter()
        .filter(|p| p.role == yan_db::Role::Member)
        .collect();
    let needs = |m: &yan_db::Package| -> Vec<usize> {
        yan_db::dependency_roots(&packages, m)
            .into_iter()
            .filter(|(d, _)| d.kind == yan_elab::DepKind::Normal)
            .filter_map(|(_, found)| members.iter().position(|x| Some(*x) == found))
            .collect()
    };
    let needed: Vec<Vec<usize>> = members.iter().map(|m| needs(m)).collect();
    let mut done: Vec<usize> = Vec::new();
    while done.len() < members.len() {
        let next = (0..members.len())
            .find(|i| !done.contains(i) && needed[*i].iter().all(|d| done.contains(d)))
            // A cycle cannot hold: requirements name published versions (D6); keep what is left in order.
            .or_else(|| (0..members.len()).find(|i| !done.contains(i)));
        match next {
            Some(i) => done.push(i),
            None => break,
        }
    }
    done.into_iter().map(|i| members[i].root.clone()).collect()
}

/// The foreign fields of the version's `.info` (R1034), computed from source, and the declared
/// ceiling: a version whose `.info` has no `capabilities` is unchecked against its closure (RFC 0079 D3).
pub fn info_fields(root: &Path) -> serde_json::Value {
    let set = yan_db::foreign_set(root);
    let mut v = serde_json::json!({
        "ffi": set.modules,
        "libs": set.libs.iter().map(|l| serde_json::json!({"name": l.name, "link": l.link})).collect::<Vec<_>>(),
    });
    if let Some(ceiling) = crate::lock::manifest_at(root).and_then(|m| m.capabilities) {
        v["capabilities"] = serde_json::json!(ceiling);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(src: &str) -> Manifest {
        let (m, _) = yan_yag::parse("package.yag", src, false);
        yan_elab::manifest(&m.unwrap()).unwrap()
    }

    #[test]
    fn overrides_refused() {
        let m = manifest(
            "package: Package = Package(name=\"a\", version=\"0.1.0\", overrides={\"@o/x\": \"x\"})\n",
        );
        assert_eq!(refusals(&m).len(), 1);
    }

    #[test]
    fn prerelease_refused_from_one_point_oh() {
        let dep = "dependencies={\"x\": \"@o/x 1.2.0-rc.1\"}";
        let at = |v: &str| {
            manifest(&format!(
                "package: Package = Package(name=\"a\", version=\"{v}\", {dep})\n"
            ))
        };
        assert_eq!(refusals(&at("1.0.0")).len(), 1);
        assert!(refusals(&at("0.9.0")).is_empty());
    }

    #[test]
    fn members_publish_after_the_members_they_require() {
        let d = std::env::temp_dir().join(format!("yan-publish-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let pkg = |name: &str, extra: &str| {
            format!("package: Package = Package(name=\"{name}\", version=\"1.0.0\"{extra})\n")
        };
        let write = |rel: &str, text: String| {
            std::fs::create_dir_all(d.join(rel)).unwrap();
            std::fs::write(d.join(rel).join("package.yag"), text).unwrap();
        };
        write("", pkg("@t/root", ", members=[\"c\", \"b\", \"a\"]"));
        write("a", pkg("@t/a", ""));
        write("b", pkg("@t/b", ", dependencies={\"a\": \"@t/a 1.0\"}"));
        write("c", pkg("@t/c", ", dependencies={\"b\": \"@t/b 1.0\"}"));
        let order: Vec<String> = members_in_order(&d)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(order, ["a", "b", "c"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn clean_manifest_passes() {
        let m = manifest(
            "package: Package = Package(name=\"a\", version=\"1.0.0\", dependencies={\"x\": \"@o/x 1.2\"}, dev_dependencies={\"t\": \"@o/t 2.0.0-b.1\"})\n",
        );
        assert!(refusals(&m).is_empty());
    }

    #[test]
    fn info_fields_list_modules_and_libs() {
        let d = std::env::temp_dir().join(format!("yan-publish-info-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("ffi")).unwrap();
        std::fs::write(d.join("package.yag"), "").unwrap();
        std::fs::write(
            d.join("ffi/z.yn"),
            "unsafe extern \"C\" lib zlib link \"z\"\n  safe f() -> c_int\n",
        )
        .unwrap();
        let v = info_fields(&d);
        assert_eq!(v["ffi"], serde_json::json!(["ffi/z.yn"]));
        assert_eq!(
            v["libs"],
            serde_json::json!([{"name": "zlib", "link": "z"}])
        );
        assert!(v.get("capabilities").is_none());
    }

    #[test]
    fn info_fields_publish_the_declared_ceiling() {
        let d = std::env::temp_dir().join(format!("yan-publish-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("package.yag"),
            "package: Package = Package(name=\"@t/a\", version=\"0.1.0\", capabilities=[\"io\", \"zlib\"])\n",
        )
        .unwrap();
        assert_eq!(
            info_fields(&d)["capabilities"],
            serde_json::json!(["io", "zlib"])
        );
    }
}
