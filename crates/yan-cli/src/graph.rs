//! The package graph the compiler reads (RFC 0079 D5): the workspace around a path, its members and
//! the packages `yan.lock` pins, found in the store, `vendor/` or an override.

use std::path::{Path, PathBuf};

use yan_db::{Package, Role, foreign_set};

use crate::lock::{manifest_at, records};

pub fn install() {
    yan_db::install_graph_builder(around);
}

fn package(root: PathBuf, workspace: &Path, role: Role) -> Option<Package> {
    let manifest = manifest_at(&root)?;
    let (modules, layout) = yan_db::scan_layout(&root);
    Some(Package {
        name: manifest.name.clone(),
        role,
        foreign: foreign_set(&root),
        layout,
        modules,
        deps: manifest.dependencies.clone(),
        dev_deps: manifest.dev_dependencies.clone(),
        tools: manifest.tools.clone(),
        sub_item_lint: missing_sub_item_docs(&root),
        workspace: workspace.to_owned(),
        manifest,
        root,
    })
}

/// The packages of the workspace that holds `path`: every `package.yag` above it, the members of the
/// outermost, and what its lock pins. Nothing for a script outside any package.
fn around(path: &Path) -> Vec<Package> {
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let Some(top) = dir
        .ancestors()
        .filter(|d| d.join("package.yag").is_file())
        .last()
    else {
        return Vec::new();
    };
    let Some(root) = package(top.to_owned(), top, Role::Root) else {
        return Vec::new();
    };
    let members: Vec<PathBuf> = root.manifest.members.iter().map(|m| top.join(m)).collect();
    let mut out = vec![root];
    let nested = dir
        .ancestors()
        .take_while(|d| *d != top)
        .filter(|d| d.join("package.yag").is_file())
        .map(Path::to_owned);
    for d in members.iter().cloned().chain(nested) {
        if out.iter().any(|p| p.root == d) {
            continue;
        }
        let role = if members.contains(&d) {
            Role::Member
        } else {
            Role::Nested
        };
        out.extend(package(d, top, role));
    }
    out.extend(locked(top));
    out
}

/// The packages `yan.lock` pins whose source is on disk: an override's directory, or the unpacked
/// archive for the digest.
fn locked(top: &Path) -> Vec<Package> {
    let store = crate::registry::configured_store(top);
    let roots = crate::store::roots(top, store.as_deref());
    records(top)
        .iter()
        .filter_map(|r| {
            let dir = match r
                .get("path")
                .and_then(toml::Value::as_str)
                .filter(|p| !p.starts_with("git+"))
            {
                Some(p) => top.join(p),
                None => crate::store::source(&roots, r.get("digest")?.as_str()?)
                    .ok()
                    .flatten()?,
            };
            // The lock names what was resolved; an archive that calls itself something else must
            // not drop out of the closure its dependents are checked against.
            let mut p = package(dir, top, Role::Locked)?;
            let field = |k: &str| r.get(k).and_then(toml::Value::as_str).map(str::to_owned);
            p.name = field("name").or(p.name);
            p.manifest.version = field("version").or(p.manifest.version);
            Some(p)
        })
        .collect()
}

/// The root of the workspace at `top`, its members and what its lock pins: the graph a locked build
/// checks (RFC 0079 D3), without the nested packages that only a module's position adds.
pub(crate) fn packages(top: &Path) -> Vec<Package> {
    let Some(root) = package(top.to_owned(), top, Role::Root) else {
        return Vec::new();
    };
    let mut out = vec![root.clone()];
    for m in &root.manifest.members {
        out.extend(package(top.join(m), top, Role::Member));
    }
    out.extend(locked(top));
    out
}

/// W861 is on when the nearest `yan.toml` sets `[lint] missing-sub-item-docs = true` (R848).
fn missing_sub_item_docs(root: &Path) -> bool {
    root.ancestors()
        .map(|d| d.join("yan.toml"))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("lint")?.get("missing-sub-item-docs")?.as_bool())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yan-graph-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn manifest(name: &str, extra: &str) -> String {
        format!(
            "package: Package = Package(name=\"{name}\", version=\"0.1.0\", yan=\"1.x\", platform=\"cli\"{extra})\n"
        )
    }

    #[test]
    fn a_workspace_has_a_root_its_members_and_their_foreign_sets() {
        let d = tmp("ws");
        std::fs::create_dir_all(d.join("a/ffi")).unwrap();
        std::fs::write(
            d.join("package.yag"),
            manifest("@t/root", ", members=[\"a\"]"),
        )
        .unwrap();
        std::fs::write(d.join("a/package.yag"), manifest("@t/a", "")).unwrap();
        std::fs::write(
            d.join("a/ffi/z.yn"),
            "unsafe extern \"C\" lib z\n  safe f() -> c_int\n",
        )
        .unwrap();
        let g = around(&d.join("a/m.yn"));
        let by = |n: &str| g.iter().find(|p| p.name.as_deref() == Some(n)).unwrap();
        assert_eq!(by("@t/root").role, Role::Root);
        assert_eq!(by("@t/a").role, Role::Member);
        assert_eq!(by("@t/a").foreign.modules, ["ffi/z.yn"]);
        assert!(by("@t/root").foreign.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_script_outside_any_package_has_no_graph() {
        let d = tmp("lone");
        assert!(around(&d.join("s.yn")).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
