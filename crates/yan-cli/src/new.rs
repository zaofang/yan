//! `yan new` (18.1): write a package from a template. Templates are 0BSD (R352).

use std::path::{Path, PathBuf};

use clap::ValueEnum;

#[derive(Clone, Copy, ValueEnum)]
pub enum Template {
    /// A program reading standard input.
    Cli,
    /// Functions with contracts and examples, and no capabilities.
    Library,
    /// Public functions an agent may call, each holding only the capabilities its `uses` names.
    AgentTool,
}

impl Template {
    /// The template's files: path in the package, then text with `{name}` for the package name.
    fn files(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Template::Cli => &[
                ("package.yag", include_str!("../templates/cli/package.yag")),
                (
                    "src/main.yag",
                    include_str!("../templates/cli/src/main.yag"),
                ),
            ],
            Template::Library => &[
                (
                    "package.yag",
                    include_str!("../templates/library/package.yag"),
                ),
                (
                    "src/lib.yag",
                    include_str!("../templates/library/src/lib.yag"),
                ),
            ],
            Template::AgentTool => &[
                (
                    "package.yag",
                    include_str!("../templates/agent-tool/package.yag"),
                ),
                (
                    "src/main.yag",
                    include_str!("../templates/agent-tool/src/main.yag"),
                ),
            ],
        }
    }
}

/// Write the package `name` (`@org/pkg`, or `pkg` for `@local/pkg`) into `parent/pkg`, which
/// must not exist. Returns that directory.
pub fn create(parent: &Path, name: &str, template: Template) -> Result<PathBuf, String> {
    let (full, pkg) = match name.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((org, pkg)) if is_segment(org) => (name.to_owned(), pkg),
            _ => return Err(format!("`{name}` is not `@org/name` or `name`")),
        },
        None => (format!("@local/{name}"), name),
    };
    if !is_segment(pkg) {
        return Err(format!(
            "`{pkg}` is not a package name: lowercase ASCII letters, digits, `-` and `_`, starting with a letter"
        ));
    }
    let dir = parent.join(pkg);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    for (path, text) in template.files() {
        let file = dir.join(path);
        let parent = file.parent().expect("a file in the package");
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&file, text.replace("{name}", &full))
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(dir)
}

fn is_segment(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yan-new-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_template_checks_and_passes_its_tests() {
        for t in Template::value_variants() {
            let tag = t.to_possible_value().unwrap().get_name().to_owned();
            let parent = scratch(&tag);
            let dir = create(&parent, "@acme/demo", *t).unwrap();
            let manifest = std::fs::read_to_string(dir.join("package.yag")).unwrap();
            let (m, _) = yan_yag::parse("package.yag", &manifest, true);
            let m = yan_elab::manifest(&m.unwrap()).expect("a Package(...)");
            assert_eq!(m.name.as_deref(), Some("@acme/demo"));
            let report = test::run(&dir, None, false, None, None).unwrap();
            assert!(!report.failed(false), "{tag}: {:?}", report.lines());
            assert!(!report.tests.is_empty(), "{tag} has no tests");
            std::fs::remove_dir_all(&parent).unwrap();
        }
    }

    #[test]
    fn names() {
        let parent = scratch("names");
        let dir = create(&parent, "tool_1", Template::Cli).unwrap();
        assert_eq!(dir, parent.join("tool_1"));
        let manifest = std::fs::read_to_string(dir.join("package.yag")).unwrap();
        assert!(manifest.contains("name=\"@local/tool_1\""));
        for bad in ["Tool", "1tool", "@acme", "@Acme/x", "a/b", ""] {
            assert!(create(&parent, bad, Template::Cli).is_err(), "{bad}");
        }
        assert!(
            create(&parent, "tool_1", Template::Cli)
                .unwrap_err()
                .contains("already exists")
        );
        std::fs::remove_dir_all(&parent).unwrap();
    }
}
