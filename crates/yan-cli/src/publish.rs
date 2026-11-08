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
    for r in prereleases(m) {
        out.push(refusal(format!(
            "`{r}` is a pre-release dependency of a package at 1.0 or later"
        )));
    }
    out
}

pub fn check(root: &Path) -> Result<Vec<Diagnostic>, String> {
    let path = root.join("package.yag");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (m, _) = yan_yag::parse("package.yag", &src, false);
    let manifest = m
        .and_then(|m| yan_elab::manifest(&m))
        .ok_or("package.yag declares no `package: Package = Package(...)`")?;
    Ok(refusals(&manifest))
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
    fn clean_manifest_passes() {
        let m = manifest(
            "package: Package = Package(name=\"a\", version=\"1.0.0\", dependencies={\"x\": \"@o/x 1.2\"}, dev_dependencies={\"t\": \"@o/t 2.0.0-b.1\"})\n",
        );
        assert!(refusals(&m).is_empty());
    }
}
