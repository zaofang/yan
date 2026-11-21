//! `yan x PKG TOOL`: run a locked `tools` package's `bins` entry point under that package's own
//! ceiling (RFC 0100 D1).

use std::path::{Path, PathBuf};

use yan_db::{Package, Role};

use crate::graph::around_dir;

/// The source file of the module that `TOOL` names in `PKG`'s `bins`.
pub fn resolve(dir: &Path, pkg: &str, tool: &str) -> Result<PathBuf, String> {
    let packages = around_dir(dir);
    let root = packages
        .iter()
        .find(|p| p.role == Role::Root)
        .ok_or_else(|| format!("no package.yag at or above {}", dir.display()))?;
    let requirement_names = |p: &Package| -> Vec<String> {
        p.manifest
            .tools
            .iter()
            .filter_map(|r| r.split_whitespace().next().map(str::to_owned))
            .collect()
    };
    let declared = requirement_names(root);
    if !declared.iter().any(|n| n == pkg) {
        return Err(format!(
            "`{pkg}` is not in `tools` of {}; tools: {}",
            root.name.as_deref().unwrap_or("this package"),
            list(&declared)
        ));
    }
    let tool_pkg = packages
        .iter()
        .find(|p| p.name.as_deref() == Some(pkg) && p.role != Role::Root)
        .ok_or_else(|| format!("`{pkg}` is in `tools` but not locked; run `yan lock`"))?;
    let module = tool_pkg
        .manifest
        .bins
        .iter()
        .find(|(name, _)| name == tool)
        .map(|(_, m)| m)
        .ok_or_else(|| {
            let names: Vec<String> = tool_pkg
                .manifest
                .bins
                .iter()
                .map(|(n, _)| n.clone())
                .collect();
            format!("`{pkg}` has no tool `{tool}`; tools: {}", list(&names))
        })?;
    tool_pkg
        .modules
        .iter()
        .find(|m| m.path.join(".") == *module)
        .map(|m| m.file.clone())
        .ok_or_else(|| format!("`bins` of `{pkg}` names module `{module}`, which does not exist"))
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    }
}
